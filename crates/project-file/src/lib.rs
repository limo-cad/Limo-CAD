//! The .limo container and file boundary, shared by desktop hosts.
//! This crate does not own documents, dialogs, engine transactions or IPC.
//! A caller keeps its document lease through save/load and handles dirty state
//! only after the filesystem operation and authoritative engine load succeed.

mod archive;
mod files;

pub use archive::{
    ProjectArchive, SaveMetadata, CONTAINER_VERSION, LEGACY_FORMAT, PREVIOUS_FORMAT,
    PROJECT_EXTENSION, PROJECT_FORMAT,
};
pub use files::{read_binary_file, write_binary_file_atomic, write_binary_file_new};

pub const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_EXPANDED_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug)]
pub struct ProjectFileError(String);

impl std::fmt::Display for ProjectFileError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}
impl std::error::Error for ProjectFileError {}

fn error(message: impl Into<String>) -> ProjectFileError {
    ProjectFileError(message.into())
}
fn failed(stage: &str, source: impl std::fmt::Display) -> ProjectFileError {
    error(format!("{stage}: {source}"))
}
