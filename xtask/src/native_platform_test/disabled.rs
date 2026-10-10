//! Keep non-input xtask commands available without compiling the Windows input harness.
use anyhow::{bail, Result};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) struct Driver {
    pub(crate) pid: u32,
    pub(crate) helper: PathBuf,
}

pub(super) fn unavailable<T>() -> Result<T> {
    bail!("Windows OS-input harness is disabled. Build xtask with --features native-control-harness; the CAD executable also needs native-computer-control. No OS input was sent.")
}

impl Driver {
    pub(crate) fn new(_: u32, _: &Path) -> Result<Self> {
        unavailable()
    }

    pub(crate) fn source(&self) -> &'static str {
        "Windows OS-input harness disabled"
    }

    pub(crate) fn event(&self, _: &str) -> Result<()> {
        unavailable()
    }

    pub(crate) fn invoke(&self, _: &str, _: Option<&str>) -> Result<String> {
        unavailable()
    }

    pub(crate) fn clipboard_read(&self) -> Result<String> {
        unavailable()
    }

    pub(crate) fn clipboard_write(&self, _: &str) -> Result<()> {
        unavailable()
    }

    pub(crate) fn complete_dialog(&self, _: &str, _: &str) -> Result<()> {
        unavailable()
    }

    pub(crate) fn command(&self, _: &str) -> Result<Command> {
        unavailable()
    }
}
