//! Small process receipts for an exclusively owned fixture child. No request
//! arguments, model contents, or session contents belong in these records.
use serde_json::{json, Value};
use std::{
    fs::{self, File},
    io::{self, Write},
    path::Path,
    process::{Child, ExitStatus},
    time::{Duration, Instant},
};

#[cfg(any(target_os = "macos", test))]
mod macos_sample;

pub(super) const DISCONNECTED_EXIT_GRACE: Duration = Duration::from_millis(250);
const MAX_RECORDS: usize = 16;
const MAX_RECORD_BYTES: usize = 4096;

pub(super) fn bounded_text(text: &str, limit: usize) -> String {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

pub(super) enum Observation {
    Exited(ExitStatus),
    Running,
    Unavailable(String),
}
impl Observation {
    pub(super) fn from_wait(result: io::Result<Option<ExitStatus>>) -> Self {
        match result {
            Ok(Some(status)) => Self::Exited(status),
            Ok(None) => Self::Running,
            Err(error) => Self::Unavailable(bounded_text(&error.to_string(), 256)),
        }
    }
    pub(super) fn describe(&self) -> String {
        match self {
            Self::Exited(status) => format!("exited ({status}, code={:?})", status.code()),
            Self::Running => "still running at observation deadline".into(),
            Self::Unavailable(error) => format!("exit status unavailable: {error}"),
        }
    }
    fn value(&self) -> Value {
        match self {
            Self::Exited(status) => json!({"state":"exited","status":status.to_string(),
                "code":status.code(),"success":status.success()}),
            Self::Running => json!({"state":"running"}),
            Self::Unavailable(error) => json!({"state":"unavailable","error":error}),
        }
    }
}

/// EOF can precede process exit. Observe briefly without sending another request
/// or closing stdin; a live process with closed stdout remains a distinct state.
pub(super) fn observe(child: &mut Child, timeout: Duration) -> Observation {
    let deadline = Instant::now() + timeout;
    loop {
        let observation = Observation::from_wait(child.try_wait());
        if !matches!(observation, Observation::Running) {
            return observation;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return observation;
        }
        std::thread::sleep(remaining.min(Duration::from_millis(10)));
    }
}

pub(super) struct Evidence {
    file: File,
    records: usize,
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    directory: std::path::PathBuf,
}
impl Evidence {
    pub(super) fn create(directory: &Path) -> io::Result<Self> {
        Ok(Self {
            file: fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join("host-process.jsonl"))?,
            records: 0,
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            directory: directory.to_owned(),
        })
    }

    /// Capture bounded thread evidence only for this still-owned fixture child.
    pub(super) fn capture_threads(&self, child: &mut Child) -> io::Result<()> {
        #[cfg(target_os = "linux")]
        let pid = child.id();
        #[cfg(target_os = "linux")]
        fs::write(
            self.directory.join("host-threads-before-cleanup.json"),
            serde_json::to_vec_pretty(&thread_waits(Path::new("/proc"), pid))?,
        )?;
        #[cfg(target_os = "macos")]
        macos_sample::capture(&self.directory, child);
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let _ = child;
        Ok(())
    }
    pub(super) fn record(
        &mut self,
        event: &str,
        pid: u32,
        request: Option<(&str, u64)>,
        observed: &Observation,
        cleanup: Option<(&Observation, bool, Option<&str>)>,
    ) -> io::Result<()> {
        // Keep one slot for cleanup even if an unbounded replay caller continues
        // asking for replies after transport EOF. The final status must survive.
        let limit = if cleanup.is_some() {
            MAX_RECORDS
        } else {
            MAX_RECORDS - 1
        };
        if self.records >= limit {
            return Ok(());
        }
        let mut record = json!({"event":event,"pid":pid,"observed":observed.value()});
        if let Some((method, id)) = request {
            record["request"] = json!({"method":bounded_text(method,128),"id":id,
                "method_truncated":method.len()>128});
        }
        if let Some((after, kill_attempted, kill_error)) = cleanup {
            record["cleanup"] = json!({"after":after.value(),"kill_attempted":kill_attempted,
                "kill_error":kill_error.map(|error| bounded_text(error,256))});
        }
        let bytes = serde_json::to_vec(&record)?;
        if bytes.len() + 1 > MAX_RECORD_BYTES {
            return Err(io::Error::other("Owned process receipt exceeds byte limit"));
        }
        // Count attempted writes as well, so a failed filesystem cannot cause an
        // unbounded log on repeated cleanup. Process receipts use at most 64 KiB
        // per child; the separate thread snapshot has its own field/count bounds.
        self.records += 1;
        self.file.write_all(&bytes)?;
        self.file.write_all(b"\n")?;
        self.file.flush()
    }
}

#[cfg(any(target_os = "linux", test))]
fn thread_waits(proc_root: &Path, pid: u32) -> Value {
    use std::io::Read;
    let tasks = proc_root.join(pid.to_string()).join("task");
    let entries = match fs::read_dir(&tasks) {
        Ok(entries) => entries,
        Err(error) => return json!({"pid":pid,"error":bounded_text(&error.to_string(),256)}),
    };
    let mut tids: Vec<u32> = entries
        .take(512)
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
        .collect();
    tids.sort_unstable();
    let truncated = tids.len() > 128;
    tids.truncate(128);
    let threads: Vec<_> = tids
        .into_iter()
        .map(|tid| {
            let mut thread = json!({"tid":tid});
            for field in ["comm", "wchan", "syscall"] {
                let path = tasks.join(tid.to_string()).join(field);
                let read = (|| -> io::Result<String> {
                    if !fs::symlink_metadata(&path)?.is_file() {
                        return Err(io::Error::other("Not a regular proc field"));
                    }
                    let mut bytes = Vec::new();
                    File::open(path)?.take(512).read_to_end(&mut bytes)?;
                    Ok(bounded_text(&String::from_utf8_lossy(&bytes), 512))
                })();
                thread[field] = match read {
                    Ok(value) => json!(value),
                    Err(error) => json!({"error":bounded_text(&error.to_string(),256)}),
                };
            }
            thread
        })
        .collect();
    json!({"pid":pid,"threads":threads,"truncated":truncated,
        "limits":{"directory_entries":512,"threads":128,"bytes_per_field":512},
        "fields":["comm","wchan","syscall"]})
}

#[cfg(test)]
mod thread_tests {
    use super::*;

    #[test]
    fn wait_state_evidence_is_owned_bounded_and_excludes_environment() {
        let root = std::env::temp_dir().join(format!(
            "limo-thread-evidence-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        let task = root.join("7/task/7");
        fs::create_dir_all(&task).unwrap();
        fs::write(task.join("comm"), "cad-native-kernel").unwrap();
        fs::write(task.join("wchan"), "futex_wait_queue").unwrap();
        fs::write(task.join("syscall"), "x".repeat(4096)).unwrap();
        fs::write(task.join("environ"), "secret environment").unwrap();
        fs::create_dir_all(root.join("7/task/not-a-tid")).unwrap();
        fs::create_dir_all(root.join("8/task/8")).unwrap();
        let evidence = thread_waits(&root, 7);
        let threads = evidence["threads"].as_array().unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0]["tid"], 7);
        assert_eq!(threads[0]["wchan"], "futex_wait_queue");
        assert_eq!(threads[0]["syscall"].as_str().unwrap().len(), 512);
        assert!(!evidence.to_string().contains("secret environment"));
        assert!(threads[0].get("environ").is_none());
    }
}
