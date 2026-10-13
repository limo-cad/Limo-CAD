//! Atomic payload installation, independent of the maintenance CLI test graph.
use anyhow::{Context, Result};
#[cfg(windows)]
use std::time::{Duration, Instant};
use std::{fs, io::Write, path::Path};

/// Replace a payload whose owner directory and existing destination have already
/// been validated by the caller as ordinary paths within the permitted owner.
/// This helper does not create directories or authorize destinations; native
/// deployment retains those checks before calling this shared implementation.
pub fn replace_file(source: &Path, destination: &Path) -> Result<()> {
    let owner = destination.parent().context("payload owner")?;
    if destination.exists() {
        // Loaded SDK DLLs can reject replacement even when the staged payload
        // is already installed. Compare complete contents before any write;
        // changed payloads still follow the ordinary replacement path below.
        if crate::hash::file(source)? == crate::hash::file(destination)? {
            return Ok(());
        }
    }
    let mut temporary = tempfile::NamedTempFile::new_in(owner)?;
    std::io::copy(&mut fs::File::open(source)?, &mut temporary)?;
    temporary.flush()?;
    temporary.as_file().sync_all()?;
    #[cfg(windows)]
    let persisted = {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match temporary.persist(destination) {
                Ok(_) => break Ok(()),
                Err(error) => {
                    if matches!(error.error.raw_os_error(), Some(5 | 32 | 33)) {
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        if !remaining.is_zero() {
                            std::thread::sleep(remaining.min(Duration::from_millis(100)));
                            if Instant::now() < deadline {
                                temporary = error.file;
                                continue;
                            }
                        }
                    }
                    break Err(error.error);
                }
            }
        }
    };
    #[cfg(not(windows))]
    let persisted = temporary
        .persist(destination)
        .map(|_| ())
        .map_err(|error| error.error);
    persisted.with_context(|| {
        format!(
            "Cannot replace {}; check file permissions or close any process using this payload (--restart stops only the canonical CAD/MCP runtime)",
            destination.display()
        )
    })
}

#[cfg(all(test, windows))]
mod replacement_tests {
    use super::replace_file;
    use std::{fs, os::windows::fs::OpenOptionsExt};

    #[test]
    fn identical_payload_can_remain_installed_under_a_replacement_lock() {
        let scratch = tempfile::tempdir().expect("scratch directory");
        let source = scratch.path().join("staged.dll");
        let destination = scratch.path().join("installed.dll");
        fs::write(&source, b"same payload").expect("staged payload");
        fs::write(&destination, b"same payload").expect("installed payload");
        // FILE_SHARE_READ allows hashing but denies writes and delete/rename.
        let _lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&destination)
            .expect("replacement-denying handle");

        replace_file(&source, &destination).expect("identical payload needs no replacement");
        assert_eq!(fs::read(&destination).unwrap(), b"same payload");
    }

    #[test]
    fn changed_same_length_payload_is_rejected_and_preserved_under_a_replacement_lock() {
        let scratch = tempfile::tempdir().expect("scratch directory");
        let source = scratch.path().join("staged.dll");
        let destination = scratch.path().join("installed.dll");
        fs::write(&source, b"new payload").expect("staged payload");
        fs::write(&destination, b"old payload").expect("installed payload");
        let _lock = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&destination)
            .expect("replacement-denying handle");

        let error = replace_file(&source, &destination).expect_err("changed payload stays guarded");
        assert!(error
            .to_string()
            .contains("--restart stops only the canonical"));
        assert_eq!(fs::read(&destination).unwrap(), b"old payload");
    }
}
