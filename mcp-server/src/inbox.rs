//! Atomic publication of the desktop's numeric JSON inbox entries.
//!
//! Readers need no lock: only a fully written, synced, closed payload receives
//! a numeric `.json` name. The OS lock serializes publishers across processes
//! and is released on exit, including a crash. Keep its file in place: deleting
//! it would allow two publishers to lock different files with the same name.

use std::fs::{self, File, TryLockError};
#[cfg(test)]
use std::io::Write;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Bounded wait for another publisher of the same inbox. Production always
/// uses it; contention stress tests share a budget of their own instead.
pub(crate) const PUBLISH_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) fn sequences(dir: &Path) -> io::Result<Vec<u64>> {
    sequences_in(&limo_cad_session_storage::root(), dir)
}

fn sequences_in(registry: &Path, dir: &Path) -> io::Result<Vec<u64>> {
    let entries = match limo_cad_session_storage::read_dir_from(registry, dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut seqs = Vec::new();
    for entry in entries {
        let entry = entry?;
        let kind = match entry.file_type() {
            Ok(kind) => kind,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if !kind.is_file() {
            continue;
        }
        if let Some(seq) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.strip_suffix(".json"))
            .and_then(|seq| seq.parse::<u64>().ok())
        {
            seqs.push(seq);
        }
    }
    seqs.sort_unstable();
    Ok(seqs)
}

pub(crate) fn next_sequence(registry: &Path, inbox: &Path) -> io::Result<u64> {
    let mut max = 0;

    for dir in [
        inbox.to_path_buf(),
        inbox.join("applied"),
        inbox.join("failed"),
    ] {
        for seq in sequences_in(registry, &dir)? {
            max = max.max(seq);
        }
    }
    max.checked_add(1)
        .ok_or_else(|| io::Error::other("inbox sequence exhausted"))
}

fn lock_publishers(inbox: &Path, timeout: Duration) -> io::Result<File> {
    let lock = limo_cad_session_storage::private_options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(inbox.join(".publish.lock"))?;
    let deadline = Instant::now() + timeout;
    loop {
        match lock.try_lock() {
            Ok(()) => return Ok(lock),
            Err(TryLockError::Error(error)) => return Err(error),
            Err(TryLockError::WouldBlock) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(io::Error::new(
                        ErrorKind::TimedOut,
                        "timed out waiting for inbox publisher",
                    ));
                }
                std::thread::sleep(remaining.min(Duration::from_millis(1)));
            }
        }
    }
}

struct StagedFile(PathBuf);

impl Drop for StagedFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

#[cfg(test)]
pub(crate) fn publish(registry: &Path, inbox: &Path, content: &[u8]) -> io::Result<u64> {
    publish_with(registry, inbox, |file| file.write_all(content))
}

#[cfg(test)]
fn publish_with(
    registry: &Path,
    inbox: &Path,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<u64> {
    publish_with_timeout(registry, inbox, PUBLISH_TIMEOUT, write)
}

pub(crate) fn publish_with_timeout(
    registry: &Path,
    inbox: &Path,
    timeout: Duration,
    write: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<u64> {
    limo_cad_session_storage::create_dir_all_from(registry, inbox)?;
    let _lock = lock_publishers(inbox, timeout)?;
    let seq = next_sequence(registry, inbox)?;

    let stage_path = inbox.join(".publish.tmp");
    match fs::remove_file(&stage_path) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let staged = StagedFile(stage_path);
    {
        let mut file = limo_cad_session_storage::private_options()
            .create_new(true)
            .write(true)
            .open(&staged.0)?;
        write(&mut file)?;
        file.sync_all()?;
    }

    fs::hard_link(&staged.0, inbox.join(format!("{seq}.json")))?;

    Ok(seq)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    };

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "limo-cad-inbox-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
            }
            #[cfg(not(unix))]
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn paused_partial_write_is_never_visible_to_reader() {
        let dir = TestDir::new();
        let inbox = dir.0.clone();
        let (paused_tx, paused_rx) = mpsc::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            publish_with(&inbox, &inbox, |file| {
                file.write_all(b"{\"name\":")?;
                paused_tx.send(()).unwrap();
                resume_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                file.write_all(b"\"solid_fillet\",\"base_generation\":170}")
            })
        });
        paused_rx.recv_timeout(Duration::from_secs(5)).unwrap();

        let pending_during_write = sequences_in(&dir.0, &dir.0).unwrap();
        resume_tx.send(()).unwrap();
        assert_eq!(writer.join().unwrap().unwrap(), 1);
        assert!(
            pending_during_write.is_empty(),
            "partial command was published"
        );
        assert_eq!(sequences_in(&dir.0, &dir.0).unwrap(), vec![1]);
        assert_eq!(
            fs::read_to_string(dir.0.join("1.json")).unwrap(),
            r#"{"name":"solid_fillet","base_generation":170}"#
        );
        assert!(!dir.0.join(".publish.tmp").exists());
    }

    #[test]
    fn failed_write_and_abandoned_stage_leave_no_command() {
        let dir = TestDir::new();
        let error = publish_with(&dir.0, &dir.0, |file| {
            file.write_all(b"{\"name\":")?;
            Err(io::Error::other("injected write failure"))
        })
        .unwrap_err();
        assert!(error.to_string().contains("injected write failure"));
        assert!(sequences_in(&dir.0, &dir.0).unwrap().is_empty());
        assert!(!dir.0.join(".publish.tmp").exists());
        fs::write(dir.0.join(".publish.tmp"), "abandoned partial payload").unwrap();
        assert_eq!(publish(&dir.0, &dir.0, b"complete").unwrap(), 1);
        assert_eq!(fs::read(dir.0.join("1.json")).unwrap(), b"complete");
    }

    #[test]
    fn abandoned_published_hardlink_never_changes_prior_command_or_receipt() {
        for archive in ["", "applied", "failed"] {
            let dir = TestDir::new();
            let destination = dir.0.join(archive);
            fs::create_dir_all(&destination).unwrap();
            let previous = destination.join("9.json");
            let stage = dir.0.join(".publish.tmp");
            fs::write(&stage, b"previous published command").unwrap();
            fs::hard_link(&stage, &previous).unwrap();

            assert_eq!(publish(&dir.0, &dir.0, b"next command").unwrap(), 10);
            assert_eq!(fs::read(previous).unwrap(), b"previous published command");
            assert_eq!(fs::read(dir.0.join("10.json")).unwrap(), b"next command");
        }
    }

    #[test]
    fn publishers_do_not_reuse_sequences_while_reader_archives() {
        let dir = TestDir::new();
        fs::create_dir(dir.0.join("applied")).unwrap();
        fs::create_dir(dir.0.join("failed")).unwrap();
        fs::write(dir.0.join("applied/7.json"), "legacy applied").unwrap();
        fs::write(dir.0.join("failed/9.json"), "legacy failed").unwrap();
        const WRITERS: usize = 8;
        const EACH: usize = 16;

        let deadline = Instant::now() + Duration::from_secs(120);
        let (published_tx, published_rx) = mpsc::channel();
        std::thread::scope(|scope| {
            let inbox = &dir.0;
            let reader = scope.spawn(move || {
                let mut observed = Vec::new();

                for _ in 0..WRITERS * EACH {
                    let remaining = deadline
                        .checked_duration_since(Instant::now())
                        .expect("reader exceeded the publication stress deadline");
                    published_rx
                        .recv_timeout(remaining)
                        .expect("reader did not receive every command");
                    for seq in sequences_in(inbox, inbox).unwrap() {
                        let src = inbox.join(format!("{seq}.json"));
                        let body = fs::read_to_string(&src).unwrap();
                        assert!(
                            body.starts_with("writer-") && body.ends_with("-complete"),
                            "reader observed incomplete payload: {body:?}"
                        );
                        fs::rename(src, inbox.join(format!("applied/{seq}.json"))).unwrap();
                        observed.push((seq, body));
                    }
                }
                observed
            });
            let writers: Vec<_> = (0..WRITERS)
                .map(|writer| {
                    let inbox = &dir.0;
                    let published_tx = published_tx.clone();
                    scope.spawn(move || {
                        (0..EACH)
                            .map(|index| {
                                let payload = format!("writer-{writer}-{index}-complete");
                                let remaining = deadline
                                    .checked_duration_since(Instant::now())
                                    .expect("writer exceeded the publication stress deadline");
                                let seq = publish_with_timeout(inbox, inbox, remaining, |file| {
                                    file.write_all(payload.as_bytes())
                                })
                                .unwrap();
                                published_tx.send(()).unwrap();
                                (seq, payload)
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();

            drop(published_tx);
            let mut expected: Vec<_> = writers
                .into_iter()
                .flat_map(|writer| writer.join().unwrap())
                .collect();
            expected.sort();
            let observed = reader.join().unwrap();
            assert_eq!(observed, expected);
            assert_eq!(
                observed.iter().map(|(seq, _)| *seq).collect::<Vec<_>>(),
                (10..10 + (WRITERS * EACH) as u64).collect::<Vec<_>>()
            );
            assert!(sequences_in(&dir.0, &dir.0).unwrap().is_empty());
        });
    }

    #[test]
    fn publisher_lock_wait_is_bounded_and_released_on_drop() {
        let dir = TestDir::new();
        let owner = lock_publishers(&dir.0, Duration::from_secs(1)).unwrap();
        let error = lock_publishers(&dir.0, Duration::from_millis(20)).unwrap_err();
        assert_eq!(error.kind(), ErrorKind::TimedOut);
        drop(owner);
        assert_eq!(publish(&dir.0, &dir.0, b"complete").unwrap(), 1);
    }

    #[test]
    fn publication_timeout_does_not_write_or_consume_a_sequence() {
        let dir = TestDir::new();
        let owner = lock_publishers(&dir.0, Duration::from_secs(1)).unwrap();
        let mut wrote = false;
        let error = publish_with_timeout(&dir.0, &dir.0, Duration::from_millis(20), |file| {
            wrote = true;
            file.write_all(b"must not be published")
        })
        .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::TimedOut);
        assert!(!wrote, "a timed-out publisher must not call its writer");
        assert!(sequences_in(&dir.0, &dir.0).unwrap().is_empty());
        assert!(!dir.0.join(".publish.tmp").exists());
        drop(owner);
        assert_eq!(publish(&dir.0, &dir.0, b"complete").unwrap(), 1);
        assert_eq!(fs::read(dir.0.join("1.json")).unwrap(), b"complete");
    }

    #[test]
    fn publication_never_overwrites_an_existing_destination() {
        let dir = TestDir::new();
        let error = publish_with(&dir.0, &dir.0, |file| {
            file.write_all(b"new command")?;

            fs::write(dir.0.join("1.json"), b"already published")
        })
        .unwrap_err();
        assert_eq!(error.kind(), ErrorKind::AlreadyExists);
        assert_eq!(
            fs::read(dir.0.join("1.json")).unwrap(),
            b"already published"
        );
        assert!(!dir.0.join(".publish.tmp").exists());
        assert_eq!(publish(&dir.0, &dir.0, b"next command").unwrap(), 2);
    }

    #[test]
    fn sequence_exhaustion_or_unreadable_archive_cannot_reuse_a_number() {
        let dir = TestDir::new();
        fs::write(dir.0.join(format!("{}.json", u64::MAX)), b"last command").unwrap();
        assert!(publish(&dir.0, &dir.0, b"new command")
            .unwrap_err()
            .to_string()
            .contains("exhausted"));
        assert_eq!(
            fs::read(dir.0.join(format!("{}.json", u64::MAX))).unwrap(),
            b"last command"
        );
        let other = TestDir::new();
        fs::write(other.0.join("applied"), b"not a readable archive directory").unwrap();
        assert!(publish(&other.0, &other.0, b"new command").is_err());
        assert!(sequences_in(&other.0, &other.0).unwrap().is_empty());
    }

    #[test]
    #[ignore = "subprocess fixture invoked by killed_publisher_releases_os_lock"]
    fn inbox_lock_child() {
        let inbox = PathBuf::from(std::env::var_os("LIMO_CAD_INBOX_LOCK_TEST_DIR").unwrap());
        let _lock = lock_publishers(&inbox, Duration::from_secs(1)).unwrap();
        fs::write(inbox.join(".publish.tmp"), b"abandoned partial payload").unwrap();
        fs::write(inbox.join("child-ready"), b"ready").unwrap();
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    #[test]
    fn killed_publisher_releases_os_lock() {
        struct Child(std::process::Child);
        impl Drop for Child {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let dir = TestDir::new();
        let mut child = Child(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "inbox_lock_child"])
                .env("LIMO_CAD_INBOX_LOCK_TEST_DIR", &dir.0)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while !dir.0.join("child-ready").exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "lock holder exited before readiness"
            );
            assert!(
                Instant::now() < deadline,
                "lock holder did not become ready"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(
            lock_publishers(&dir.0, Duration::from_millis(20))
                .unwrap_err()
                .kind(),
            ErrorKind::TimedOut
        );
        assert!(sequences_in(&dir.0, &dir.0).unwrap().is_empty());
        drop(child);
        assert_eq!(publish(&dir.0, &dir.0, b"complete").unwrap(), 1);
        assert_eq!(fs::read(dir.0.join("1.json")).unwrap(), b"complete");
    }
}
