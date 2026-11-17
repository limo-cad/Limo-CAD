//! Bounded composite publication transport. The caller must serialize ALL
//! writers per session directory (including retries) under its owner lease.
//! This module does not provide a cross-process writer lock or authenticate
//! the publisher. Readers must check the returned owner against their live route.
//! Two payload slots preserve the last commit if preparation fails. Only the
//! commit-record rename publishes a new generation. This is process-failure
//! consistency, not a guarantee about power-loss durability on every filesystem.
use limo_cad_project_file::{ProjectPublication, PublicationReceipt, MAX_PUBLICATION_BYTES};
use limo_cad_session_storage::{atomic_write, read_bounded};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Cursor, ErrorKind},
    path::Path,
};

const COMMIT: &str = "publication-commit.cbor";
const SLOTS: [&str; 2] = ["publication-0.cbor", "publication-1.cbor"];
const COMMIT_LIMIT: usize = 4096;
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Commit {
    format: String,
    version: u32,
    slot: u8,
    receipt: PublicationReceipt,
}
fn invalid(error: impl std::fmt::Display) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, error.to_string())
}
fn read_commit(directory: &Path) -> io::Result<Commit> {
    let bytes = read_bounded(directory.join(COMMIT), COMMIT_LIMIT)?;
    if bytes.first().is_none_or(|byte| byte >> 5 != 5) {
        return Err(invalid("invalid publication commit map"));
    }
    let mut reader = Cursor::new(&bytes);
    let commit: Commit =
        ciborium::de::from_reader_with_recursion_limit(&mut reader, 16).map_err(invalid)?;
    if reader.position() != bytes.len() as u64
        || commit.format != "limo-cad-publication-commit"
        || commit.version != 1
        || commit.slot > 1
    {
        return Err(invalid(
            "invalid publication commit version, slot or trailing bytes",
        ));
    }
    Ok(commit)
}

/// Read a coherent committed pair. A reused slot or changed commit causes a
/// bounded retry; malformed stable data returns an error, never a model fallback.
pub fn read_committed(directory: &Path) -> io::Result<ProjectPublication> {
    read_with_hook(directory, || Ok(()))
}
fn read_with_hook(
    directory: &Path,
    mut after_commit_read: impl FnMut() -> io::Result<()>,
) -> io::Result<ProjectPublication> {
    for _ in 0..4 {
        let before = read_commit(directory)?;
        after_commit_read()?;
        let bytes = read_bounded(
            directory.join(SLOTS[before.slot as usize]),
            MAX_PUBLICATION_BYTES,
        );
        let after = read_commit(directory)?;
        if before != after {
            continue;
        }
        return ProjectPublication::decode_for(&bytes?, &before.receipt).map_err(invalid);
    }
    Err(io::Error::new(
        ErrorKind::WouldBlock,
        "publication changed during every read attempt",
    ))
}

/// Commit under an externally held single-writer owner lease. `previous` must
/// be the writer's last acknowledged receipt (None only for a fresh directory).
/// An exact retry of an already committed payload is idempotent. Different
/// content cannot reuse a receipt; ownership cannot change within a directory.
pub fn commit(
    directory: &Path,
    previous: Option<&PublicationReceipt>,
    next: &ProjectPublication,
) -> io::Result<()> {
    commit_with_hook(directory, previous, next, || Ok(()))
}
fn commit_with_hook(
    directory: &Path,
    previous: Option<&PublicationReceipt>,
    next: &ProjectPublication,
    before_commit: impl FnOnce() -> io::Result<()>,
) -> io::Result<()> {
    let current = match read_commit(directory) {
        Ok(value) => Some(value),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if current
        .as_ref()
        .is_some_and(|value| &value.receipt == next.receipt())
    {
        if read_committed(directory)? == *next {
            return Ok(());
        }
        return Err(invalid(
            "publication receipt cannot be reused for different content",
        ));
    }
    if current.as_ref().map(|value| &value.receipt) != previous {
        return Err(invalid(
            "publication commit changed since the writer's receipt",
        ));
    }
    if let Some(old) = previous {
        let new = next.receipt();
        if old.process_instance_id != new.process_instance_id
            || old.session_id != new.session_id
            || old.window_id != new.window_id
            || old.document_id != new.document_id
            || old.epoch != new.epoch
            || new.engine_revision < old.engine_revision
            || new.export_sequence <= old.export_sequence
        {
            return Err(invalid(
                "publication owner changed or counters moved backwards",
            ));
        }
    }
    let slot = current.as_ref().map_or(0, |value| 1 - value.slot);
    let payload = next.encode_cbor().map_err(invalid)?;
    let record = Commit {
        format: "limo-cad-publication-commit".into(),
        version: 1,
        slot,
        receipt: next.receipt().clone(),
    };
    let mut encoded = Vec::new();
    ciborium::into_writer(&record, &mut encoded).map_err(invalid)?;
    if encoded.len() > COMMIT_LIMIT {
        return Err(invalid("publication commit exceeds size limit"));
    }
    atomic_write(&directory.join(SLOTS[slot as usize]), &payload)?;
    before_commit()?;
    atomic_write(&directory.join(COMMIT), &encoded)
}

#[cfg(test)]
mod tests;
