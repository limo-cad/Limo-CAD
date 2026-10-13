use crate::{error, failed, ProjectFileError, MAX_FILE_BYTES};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub fn read_binary_file(path: &Path) -> Result<Vec<u8>, ProjectFileError> {
    read_bounded(path, MAX_FILE_BYTES)
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, ProjectFileError> {
    let metadata = fs::metadata(path).map_err(|source| failed("could not read file", source))?;
    if !metadata.is_file() {
        return Err(error("the selected path is not a regular file"));
    }
    if metadata.len() > limit {
        return Err(error("file is larger than the 256 MB safety limit"));
    }
    let file = File::open(path).map_err(|source| failed("could not open file", source))?;
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| failed("could not read file", source))?;
    if bytes.len() as u64 > limit {
        return Err(error("file grew beyond the 256 MB safety limit"));
    }
    Ok(bytes)
}

struct PendingSave {
    path: PathBuf,
    file: Option<File>,
}
impl Drop for PendingSave {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = fs::remove_file(&self.path);
    }
}

impl PendingSave {
    fn create(parent: &Path) -> Result<Self, ProjectFileError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..32 {
            let serial = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(".limo-save-{}-{serial}.tmp", std::process::id()));
            match OpenOptions::new().create_new(true).write(true).open(&path) {
                Ok(file) => {
                    return Ok(Self {
                        path,
                        file: Some(file),
                    })
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(source) => return Err(failed("could not create temporary save file", source)),
            }
        }
        Err(error("could not reserve an unused temporary save file"))
    }
}

/// Flush a private temporary file, close its handle, then atomically replace
/// the target on the same filesystem. Concurrent writers cannot truncate one
/// another's temporary content; the final target is one whole completed save.
pub fn write_binary_file_atomic(path: &Path, bytes: &[u8]) -> Result<(), ProjectFileError> {
    write_bounded(path, bytes, MAX_FILE_BYTES)
}

/// Publish a new completed file only if the destination still does not exist.
/// An earlier `exists()` check cannot grant permission to replace a file that
/// another writer creates while this save is being prepared.
pub fn write_binary_file_new(path: &Path, bytes: &[u8]) -> Result<(), ProjectFileError> {
    write_bounded_mode(path, bytes, MAX_FILE_BYTES, false)
}

fn write_bounded(path: &Path, bytes: &[u8], limit: u64) -> Result<(), ProjectFileError> {
    write_bounded_mode(path, bytes, limit, true)
}

fn write_bounded_mode(
    path: &Path,
    bytes: &[u8],
    limit: u64,
    replace: bool,
) -> Result<(), ProjectFileError> {
    if bytes.len() as u64 > limit {
        return Err(error("file is larger than the 256 MB safety limit"));
    }
    if path.file_name().is_none() {
        return Err(error("save path has no valid file name"));
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut pending = PendingSave::create(parent)?;
    let file = pending.file.as_mut().unwrap();
    file.write_all(bytes)
        .map_err(|source| failed("could not write save file", source))?;
    file.sync_all()
        .map_err(|source| failed("could not flush save file", source))?;
    drop(pending.file.take());
    if replace {
        fs::rename(&pending.path, path)
            .map_err(|source| failed("could not replace save file", source))?;
    } else {
        fs::hard_link(&pending.path, path).map_err(|source| {
            failed(
                "could not create a new save file without replacing existing work",
                source,
            )
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "limo-cad-project-file-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn concurrent_saves_publish_whole_payloads_and_leave_no_temporary_files() {
        let directory = Directory::new();
        let target = directory.0.join("part.limo");
        fs::write(&target, b"previous").unwrap();
        let barrier = Arc::new(Barrier::new(8));
        let threads: Vec<_> = (0..8)
            .map(|value| {
                let barrier = barrier.clone();
                let target = target.clone();
                std::thread::spawn(move || {
                    let bytes = vec![value; 32 * 1024];
                    barrier.wait();
                    write_binary_file_atomic(&target, &bytes).unwrap();
                })
            })
            .collect();
        let results: Vec<_> = threads.into_iter().map(|thread| thread.join()).collect();
        for result in results {
            result.unwrap();
        }
        let bytes = read_binary_file(&target).unwrap();
        assert_eq!(bytes.len(), 32 * 1024);
        assert!(bytes.iter().all(|byte| *byte == bytes[0]));
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 1);
    }

    #[test]
    fn simultaneous_new_saves_have_one_winner_and_never_replace_its_contents() {
        let directory = Directory::new();
        let target = directory.0.join("part.limo");
        let barrier = Arc::new(Barrier::new(8));
        let workers: Vec<_> = (0_u8..8)
            .map(|value| {
                let target = target.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let result = write_binary_file_new(&target, &vec![value; 32 * 1024]);
                    (value, result)
                })
            })
            .collect();
        let completed: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        let winners: Vec<_> = completed
            .iter()
            .filter(|(_, result)| result.is_ok())
            .collect();
        assert_eq!(winners.len(), 1);
        assert_eq!(fs::read(&target).unwrap(), vec![winners[0].0; 32 * 1024]);
        assert!(write_binary_file_new(&target, b"later unexpected replacement").is_err());
        assert_eq!(fs::read(&target).unwrap(), vec![winners[0].0; 32 * 1024]);
        assert_eq!(
            fs::read_dir(&directory.0).unwrap().count(),
            1,
            "Only the completed destination remains"
        );
    }

    #[test]
    fn failures_preserve_destination_and_clean_only_the_owned_temp() {
        let directory = Directory::new();
        let target = directory.0.join("part.limo");
        fs::write(&target, b"previous").unwrap();
        assert!(write_bounded(&target, b"too large", 1).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"previous");
        assert!(read_bounded(&target, 1).is_err());
        let folder = directory.0.join("folder.limo");
        fs::create_dir(&folder).unwrap();
        assert!(write_binary_file_atomic(&folder, b"new").is_err());
        assert!(folder.is_dir());
        assert!(read_binary_file(&folder).is_err());
        assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 2);
    }
}
