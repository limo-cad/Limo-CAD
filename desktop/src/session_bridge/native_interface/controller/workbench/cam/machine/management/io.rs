use std::path::{Path, PathBuf};

pub(super) fn load(config: &Path) -> Result<crate::cam_posts::Catalog, String> {
    crate::cam_posts::list(config)
}
pub(super) fn import(config: &Path, source: &Path) -> Result<crate::cam_posts::Catalog, String> {
    crate::cam_posts::import(config, source)
}
pub(super) fn open_folder(config: &Path) -> Result<PathBuf, String> {
    let path = crate::cam_posts::directory(config)?;
    #[cfg(target_os = "windows")]
    let mut command = std::process::Command::new("explorer");
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut command = std::process::Command::new("xdg-open");
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command
        .arg(&path)
        .spawn()
        .map_err(|error| format!("Could not open post folder: {error}"))?;
    Ok(path)
}
