use anyhow::{ensure, Context, Result};
use std::{
    env,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// Reject redirected runtime paths, including redirects in their parent directories.
pub(crate) fn ordinary_directory(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        let metadata = fs::symlink_metadata(ancestor)?;
        ensure!(
            metadata.is_dir() && !redirected(&metadata),
            "Runtime directory is redirected or not a directory: {}",
            ancestor.display()
        );
    }
    Ok(())
}

pub(crate) fn ordinary_file(path: &Path) -> Result<()> {
    ordinary_directory(path.parent().context("runtime file parent")?)?;
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && !redirected(&metadata),
        "Runtime file is redirected or not a file: {}",
        path.display()
    );
    Ok(())
}

fn redirected(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

pub(super) struct Package {
    pub root: PathBuf,
    pub desktop: PathBuf,
    pub target: PathBuf,
    pub version: String,
}
impl Package {
    pub fn new() -> Result<Self> {
        let root = super::super::release_tooling::root().canonicalize()?;
        let desktop = root.join("desktop");
        let target = env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .map(|p| if p.is_absolute() { p } else { root.join(p) })
            .unwrap_or_else(|| desktop.join("target"));
        let version = super::super::release_tooling::version::read(&root)?;
        let manifest =
            fs::read_to_string(desktop.join("Cargo.toml"))?.parse::<toml_edit::DocumentMut>()?;
        ensure!(
            manifest["package"]["version"].as_str() == Some(&version),
            "native Cargo version disagrees with VERSION; run cargo xtask version --sync"
        );
        fs::create_dir_all(&target)?;
        let target = target.canonicalize()?;
        Ok(Self {
            root,
            desktop,
            target,
            version,
        })
    }
    pub fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut command = Command::new(program);
        command.current_dir(&self.root);
        command
    }
    pub fn cargo(&self) -> Command {
        let mut command = crate::build_tools::cargo();
        command.current_dir(&self.root);
        command.args([
            "build",
            "--manifest-path",
            "desktop/Cargo.toml",
            "--locked",
            "--release",
            "--bin",
            "limo-cad",
        ]);
        command
    }
    pub fn notices(&self, directory: &Path) -> Result<()> {
        fs::create_dir_all(directory)?;
        fs::copy(
            self.root.join("LICENSE"),
            directory.join("Limo-CAD-LICENSE.txt"),
        )?;
        fs::copy(
            self.root.join("THIRD_PARTY_NOTICES.md"),
            directory.join("THIRD_PARTY_NOTICES.md"),
        )?;
        Ok(())
    }
}
pub(super) fn run(command: &mut Command) -> Result<()> {
    let status = command
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| format!("start {command:?}"))?;
    ensure!(status.success(), "{command:?} failed ({status})");
    Ok(())
}
pub(super) fn output(command: &mut Command) -> Result<String> {
    let result = command
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("start {command:?}"))?;
    ensure!(
        result.status.success(),
        "{command:?} failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(String::from_utf8(result.stdout)?.trim().to_owned())
}
pub(super) fn sha256(path: &Path) -> Result<String> {
    crate::hash::file(path)
}
pub(super) fn checksum(path: &Path) -> Result<()> {
    let name = path
        .file_name()
        .context("artifact name")?
        .to_str()
        .context("UTF-8 artifact name")?;
    let sidecar = path.with_file_name(format!("{name}.sha256"));
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().context("artifact parent")?)?;
    writeln!(temporary, "{}  {name}", sha256(path)?)?;
    temporary.persist(&sidecar)?;
    println!(
        "Verified artifact: {}\nChecksum: {}",
        path.display(),
        sidecar.display()
    );
    Ok(())
}
/// Only delete an ordinary generated child of a resolved owning directory.
pub(super) fn fresh_child(owner: &Path, name: &str) -> Result<PathBuf> {
    let mut components = Path::new(name).components();
    ensure!(
        matches!(components.next(), Some(std::path::Component::Normal(_)))
            && components.next().is_none(),
        "unsafe generated directory name"
    );
    fs::create_dir_all(owner)?;
    let owner = owner.canonicalize()?;
    let child = owner.join(name);
    if child.exists() || fs::symlink_metadata(&child).is_ok() {
        let metadata = fs::symlink_metadata(&child)?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            ensure!(
                metadata.file_attributes() & 0x400 == 0,
                "generated directory is a reparse point: {}",
                child.display()
            );
        }
        ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "generated path is not an ordinary directory: {}",
            child.display()
        );
        ensure!(
            child.canonicalize()?.parent() == Some(owner.as_path()),
            "generated directory leaves its owner"
        );
        fs::remove_dir_all(&child)?;
    }
    fs::create_dir(&child)?;
    Ok(child)
}
pub(super) fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            symlink(&fs::read_link(entry.path())?, &target)?;
        } else if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), target)?;
        } else {
            anyhow::bail!("unsupported staged file {}", entry.path().display());
        }
    }
    Ok(())
}
pub(super) fn executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    #[cfg(not(unix))]
    {
        ensure!(path.is_file(), "executable is missing: {}", path.display());
    }
    Ok(())
}
pub(super) fn symlink(source: &Path, destination: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(source, destination)?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (source, destination);
        anyhow::bail!("Unix package symlinks require a Unix host")
    }
}
pub(super) fn zip_directory(source: &Path, destination: &Path) -> Result<()> {
    fn files(directory: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            ensure!(!kind.is_symlink(), "portable ZIP must not contain links");
            if kind.is_dir() {
                files(&entry.path(), out)?;
            } else if kind.is_file() {
                out.push(entry.path());
            }
        }
        Ok(())
    }
    let mut paths = Vec::new();
    files(source, &mut paths)?;
    paths.sort();
    let parent = source.parent().context("portable package parent")?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(destination.parent().context("ZIP parent")?)?;
    let mut writer = zip::ZipWriter::new(temporary.as_file_mut());
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    for path in paths {
        let name = path
            .strip_prefix(parent)?
            .to_str()
            .context("UTF-8 ZIP path")?
            .replace('\\', "/");
        writer.start_file(name, options)?;
        std::io::copy(&mut File::open(path)?, &mut writer)?;
    }
    writer.finish()?.flush()?;
    temporary.persist(destination)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    #[test]
    fn owned_directory_guard_and_portable_archive_checksum() {
        let temp = tempfile::tempdir().unwrap();
        assert!(fresh_child(temp.path(), "../escape").is_err());
        let source = fresh_child(temp.path(), "Limo-CAD-fixture").unwrap();
        fs::write(source.join("Limo-CAD.exe"), b"fixture").unwrap();
        fs::create_dir(source.join("licenses")).unwrap();
        fs::write(source.join("licenses/LICENSE.txt"), b"license").unwrap();
        let zip = temp.path().join("fixture.zip");
        zip_directory(&source, &zip).unwrap();
        let hash = sha256(&zip).unwrap();
        zip_directory(&source, &zip).unwrap();
        assert_eq!(sha256(&zip).unwrap(), hash);
        let mut archive = zip::ZipArchive::new(File::open(&zip).unwrap()).unwrap();
        let mut content = String::new();
        archive
            .by_name("Limo-CAD-fixture/Limo-CAD.exe")
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();
        assert_eq!(content, "fixture");
        assert!(archive
            .by_name("Limo-CAD-fixture/licenses/LICENSE.txt")
            .is_ok());
        checksum(&zip).unwrap();
        assert_eq!(
            fs::read_to_string(temp.path().join("fixture.zip.sha256")).unwrap(),
            format!("{hash}  fixture.zip\n")
        );
    }
    #[cfg(unix)]
    #[test]
    fn generated_directory_must_not_follow_a_link() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        symlink(outside.path(), &temp.path().join("linked")).unwrap();
        assert!(fresh_child(temp.path(), "linked").is_err());
        assert!(outside.path().is_dir());
    }
}
