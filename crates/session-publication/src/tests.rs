use super::*;
use limo_cad_project_file::{ProjectSnapshot, SnapshotIntent};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
};
static ENVIRONMENT: Mutex<()> = Mutex::new(());
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Root {
    path: PathBuf,
    previous: Option<std::ffi::OsString>,
}
impl Root {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "limo-publication-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        limo_cad_session_storage::create_registry(&path).unwrap();
        let previous = std::env::var_os("LIMO_CAD_SESSION_DIR");
        std::env::set_var("LIMO_CAD_SESSION_DIR", &path);
        Self { path, previous }
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        if let Some(value) = self.previous.take() {
            std::env::set_var("LIMO_CAD_SESSION_DIR", value);
        } else {
            std::env::remove_var("LIMO_CAD_SESSION_DIR");
        }
        assert_eq!(self.path.parent(), Some(std::env::temp_dir().as_path()));
        fs::remove_dir_all(&self.path).unwrap();
    }
}
fn publication(sequence: u64, payload: u8) -> ProjectPublication {
    ProjectPublication::new(
        PublicationReceipt {
            process_instance_id: "process".into(),
            session_id: "session".into(),
            window_id: "main".into(),
            document_id: "doc".into(),
            epoch: 1,
            engine_revision: sequence,
            export_sequence: sequence,
        },
        ProjectSnapshot::new(
            r#"{"format":"limo-cad-project","schema_version":15}"#.into(),
            vec![SnapshotIntent {
                name: "intent/future.cbor".into(),
                payload: vec![0, 255, payload],
            }],
        )
        .unwrap(),
    )
    .unwrap()
}
#[test]
fn repeated_commits_bound_files_and_exact_retry_is_idempotent() {
    let _lock = ENVIRONMENT.lock().unwrap();
    let root = Root::new();
    let mut previous = None;
    for seq in 1..=8 {
        let next = publication(seq, seq as u8);
        commit(&root.path, previous.as_ref(), &next).unwrap();
        commit(&root.path, previous.as_ref(), &next).unwrap();
        assert_eq!(read_committed(&root.path).unwrap(), next);
        previous = Some(next.receipt().clone());
    }
    assert_eq!(fs::read_dir(&root.path).unwrap().count(), 3);
    assert!(commit(&root.path, previous.as_ref(), &publication(8, 99)).is_err());
    assert_eq!(read_committed(&root.path).unwrap(), publication(8, 8));
}
#[test]
fn failure_after_slot_write_retains_previous_commit_and_retry_succeeds() {
    let _lock = ENVIRONMENT.lock().unwrap();
    let root = Root::new();
    let old = publication(1, 1);
    let next = publication(2, 2);
    commit(&root.path, None, &old).unwrap();
    let prior = fs::read(root.path.join(COMMIT)).unwrap();
    assert!(
        commit_with_hook(&root.path, Some(old.receipt()), &next, || Err(
            io::Error::other("injected before commit")
        ))
        .is_err()
    );
    assert_eq!(fs::read(root.path.join(COMMIT)).unwrap(), prior);
    assert_eq!(read_committed(&root.path).unwrap(), old);
    commit(&root.path, Some(old.receipt()), &next).unwrap();
    assert_eq!(read_committed(&root.path).unwrap(), next);
}
#[test]
fn stale_or_foreign_writer_cannot_replace_publication() {
    let _lock = ENVIRONMENT.lock().unwrap();
    let root = Root::new();
    let old = publication(2, 2);
    commit(&root.path, None, &old).unwrap();
    assert!(commit(&root.path, None, &publication(3, 3)).is_err());
    assert!(commit(&root.path, Some(old.receipt()), &publication(1, 1)).is_err());
    let mut owner = publication(3, 3).receipt().clone();
    owner.epoch += 1;
    let foreign = ProjectPublication::new(owner, old.snapshot().clone()).unwrap();
    assert!(commit(&root.path, Some(old.receipt()), &foreign).is_err());
    assert_eq!(read_committed(&root.path).unwrap(), old);
}
#[test]
fn reader_retries_when_two_commits_reuse_its_selected_slot() {
    let _lock = ENVIRONMENT.lock().unwrap();
    let root = Root::new();
    let old = publication(1, 1);
    commit(&root.path, None, &old).unwrap();
    let mut first = true;
    let result = read_with_hook(&root.path, || {
        if first {
            first = false;
            let second = publication(2, 2);
            commit(&root.path, Some(old.receipt()), &second)?;
            commit(&root.path, Some(second.receipt()), &publication(3, 3))?;
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(result, publication(3, 3));
}
#[test]
fn constantly_changing_reader_is_bounded() {
    let _lock = ENVIRONMENT.lock().unwrap();
    let root = Root::new();
    let mut current = publication(1, 1);
    commit(&root.path, None, &current).unwrap();
    let mut attempts = 0;
    let error = read_with_hook(&root.path, || {
        attempts += 1;
        let next = publication(current.receipt().export_sequence + 1, 2);
        commit(&root.path, Some(current.receipt()), &next)?;
        current = next;
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::WouldBlock);
    assert_eq!(attempts, 4);
}
#[test]
fn corrupt_commit_or_capsule_never_falls_back_to_previous_slot() {
    let _lock = ENVIRONMENT.lock().unwrap();
    let root = Root::new();
    let old = publication(1, 1);
    let next = publication(2, 2);
    commit(&root.path, None, &old).unwrap();
    commit(&root.path, Some(old.receipt()), &next).unwrap();
    let record = read_commit(&root.path).unwrap();
    atomic_write(&root.path.join(SLOTS[record.slot as usize]), &[0x80]).unwrap();
    assert!(read_committed(&root.path).is_err());
    atomic_write(&root.path.join(COMMIT), &[0x80]).unwrap();
    assert!(read_committed(&root.path).is_err());
    assert!(commit(&root.path, Some(next.receipt()), &publication(3, 3)).is_err());
}
#[test]
fn slot_write_failure_preserves_commit() {
    let _lock = ENVIRONMENT.lock().unwrap();
    let root = Root::new();
    let old = publication(1, 1);
    commit(&root.path, None, &old).unwrap();
    fs::create_dir(root.path.join(SLOTS[1])).unwrap();
    assert!(commit(&root.path, Some(old.receipt()), &publication(2, 2)).is_err());
    assert_eq!(read_committed(&root.path).unwrap(), old);
}
