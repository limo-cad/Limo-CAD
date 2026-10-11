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
}
impl Evidence {
    pub(super) fn create(directory: &Path) -> io::Result<Self> {
        Ok(Self {
            file: fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join("host-process.jsonl"))?,
            records: 0,
        })
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
        // unbounded log on repeated cleanup. At most 64 KiB is written per child.
        self.records += 1;
        self.file.write_all(&bytes)?;
        self.file.write_all(b"\n")?;
        self.file.flush()
    }
}
