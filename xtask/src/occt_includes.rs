//! Verified package-relative fallbacks for MSVC quoted private-header includes.
//! An existing /I build/include/opencascade resolves ../Package/Header here.
//! The source, CMake recipe and completed Ninja objects remain unchanged.
use anyhow::{ensure, Context, Result};
use std::{collections::BTreeMap, fs, io::Write, path::Path};

pub(super) fn prepare(
    source: &Path,
    build: &Path,
    inventory: &BTreeMap<String, String>,
) -> Result<usize> {
    let source = std::path::absolute(source)?;
    let build = std::path::absolute(build)?;
    crate::package::ordinary_directory(&source)?;
    crate::package::ordinary_directory(&build.join("include/opencascade"))?;
    let mut planned = Vec::new();
    for (relative, fingerprint) in inventory {
        let Some(digest) = fingerprint.strip_prefix("file:") else {
            continue;
        };
        let parts = relative.split('/').collect::<Vec<_>>();
        let (package, header) = match parts.as_slice() {
            ["src", package, header] | ["src", _, _, package, header] => (*package, *header),
            _ => continue,
        };
        if !super::forwarding_header(Path::new(header)) {
            continue;
        }
        for name in [package, header] {
            ensure!(
                !name.is_empty()
                    && name != "."
                    && name != ".."
                    && name
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte)),
                "Unsafe OCCT private-header component: {relative}"
            );
        }
        let target = source.join(relative);
        super::require_windows_compiler_path(&target)?;
        crate::package::ordinary_file(&target)?;
        ensure!(
            crate::hash::file(&target)? == digest,
            "OCCT private-header source differs from verified inventory: {relative}"
        );
        let target_text = target
            .to_str()
            .context("non-Unicode OCCT private-header target")?
            .replace('\\', "/");
        ensure!(
            !target_text.contains(['"', '\r', '\n']),
            "Unsafe OCCT private-header include target"
        );
        let destination = build.join("include").join(package).join(header);
        super::require_windows_compiler_path(&destination)?;
        let bytes = format!("#include \"{target_text}\"\n").into_bytes();
        if present(&destination)? {
            crate::package::ordinary_file(&destination)?;
            ensure!(
                fs::metadata(&destination)?.len() == bytes.len() as u64
                    && fs::read(&destination)? == bytes,
                "Refusing to overwrite conflicting OCCT private-header fallback: {}",
                destination.display()
            );
        }
        let parent = destination.parent().context("private-header parent")?;
        if present(parent)? {
            crate::package::ordinary_directory(parent)?;
        } else {
            crate::package::ordinary_directory(parent.parent().context("include parent")?)?;
        }
        planned.push((destination, bytes));
    }
    // Complete preflight before additions; never rewrite an existing file.
    for (destination, bytes) in &planned {
        if !present(destination)? {
            let parent = destination.parent().context("private-header parent")?;
            fs::create_dir_all(parent)?;
            crate::package::ordinary_directory(parent)?;
            let mut staged = tempfile::NamedTempFile::new_in(parent)?;
            staged.write_all(bytes)?;
            staged.as_file_mut().sync_all()?;
            staged.persist_noclobber(destination)?;
        }
        crate::package::ordinary_file(destination)?;
        ensure!(
            fs::metadata(destination)?.len() == bytes.len() as u64
                && fs::read(destination)? == *bytes,
            "OCCT private-header fallback changed during preparation: {}",
            destination.display()
        );
    }
    Ok(planned.len())
}

fn present(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).context("inspect OCCT private-header fallback"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const HEADER: &str = "src/DataExchange/TKDESTEP/RWStepGeom/PrivateContext.pxx";
    fn fixture() -> (tempfile::TempDir, BTreeMap<String, String>) {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        fs::create_dir_all(source.join(HEADER).parent().unwrap()).unwrap();
        fs::create_dir_all(source.join("src/RWStepGeom")).unwrap();
        fs::write(source.join(HEADER), b"verified private context").unwrap();
        fs::create_dir_all(temp.path().join("build/include/opencascade")).unwrap();
        let inventory = BTreeMap::from([(
            HEADER.into(),
            format!("file:{}", crate::hash::file(&source.join(HEADER)).unwrap()),
        )]);
        (temp, inventory)
    }
    #[test]
    fn package_relative_private_headers_preserve_source_recipe_and_completed_objects() {
        let (temp, inventory) = fixture();
        let source = temp.path().join("source");
        let build = temp.path().join("build");
        fs::write(build.join("build.ninja"), b"configured recipe").unwrap();
        fs::write(build.join("completed.obj"), b"compiled object").unwrap();
        let flat = build.join("include/opencascade/PrivateContext.pxx");
        fs::write(&flat, b"original CMake forwarder").unwrap();
        assert_eq!(prepare(&source, &build, &inventory).unwrap(), 1);
        let fallback = build.join("include/RWStepGeom/PrivateContext.pxx");
        let expected = format!(
            "#include \"{}\"\n",
            source.join(HEADER).to_str().unwrap().replace('\\', "/")
        );
        assert_eq!(fs::read_to_string(&fallback).unwrap(), expected);
        assert_eq!(prepare(&source, &build, &inventory).unwrap(), 1);
        assert_eq!(fs::read(&flat).unwrap(), b"original CMake forwarder");
        assert_eq!(
            fs::read(build.join("build.ninja")).unwrap(),
            b"configured recipe"
        );
        assert_eq!(
            fs::read(build.join("completed.obj")).unwrap(),
            b"compiled object"
        );
        assert_eq!(
            fs::read(source.join(HEADER)).unwrap(),
            b"verified private context"
        );
    }
    #[test]
    fn package_relative_header_preflight_rejects_conflicts_and_source_drift_before_writes() {
        let (temp, mut inventory) = fixture();
        let source = temp.path().join("source");
        let build = temp.path().join("build");
        let extra = "src/RWStepGeom/AFirst.pxx";
        fs::write(source.join(extra), b"another verified context").unwrap();
        inventory.insert(
            extra.into(),
            format!("file:{}", crate::hash::file(&source.join(extra)).unwrap()),
        );
        fs::create_dir(build.join("include/RWStepGeom")).unwrap();
        let fallback = build.join("include/RWStepGeom/PrivateContext.pxx");
        fs::write(&fallback, b"conflicting content").unwrap();
        assert!(prepare(&source, &build, &inventory)
            .unwrap_err()
            .to_string()
            .contains("conflicting"));
        assert!(!build.join("include/RWStepGeom/AFirst.pxx").exists());
        assert_eq!(fs::read(&fallback).unwrap(), b"conflicting content");
        fs::remove_file(&fallback).unwrap();
        fs::write(source.join(HEADER), b"unverified source change").unwrap();
        assert!(prepare(&source, &build, &inventory)
            .unwrap_err()
            .to_string()
            .contains("inventory"));
        assert!(!build.join("include/RWStepGeom/AFirst.pxx").exists());
        inventory.insert("src/../unsafe.pxx".into(), "file:untrusted".into());
        assert!(prepare(&source, &build, &inventory)
            .unwrap_err()
            .to_string()
            .contains("Unsafe"));
    }
}
