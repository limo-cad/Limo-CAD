//! Explicit Windows dependency assembly from the probed FreeType prefix.
use anyhow::{ensure, Context, Result};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
};

fn present(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => {
            Err(error).with_context(|| format!("Inspect SDK runtime path {}", path.display()))
        }
    }
}

struct Input {
    source: PathBuf,
    relative: Option<PathBuf>,
    bytes: Vec<u8>,
}
impl Input {
    fn read(source: PathBuf, relative: Option<PathBuf>) -> Result<Self> {
        crate::package::ordinary_file(&source)?;
        let mut bytes = Vec::new();
        fs::File::open(&source)?
            .take(64 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() <= 64 * 1024 * 1024,
            "SDK dependency exceeds 64 MiB: {}",
            source.display()
        );
        Ok(Self {
            source,
            relative,
            bytes,
        })
    }
    fn recheck(&self) -> Result<()> {
        ensure!(
            Self::read(self.source.clone(), None)?.bytes == self.bytes,
            "SDK runtime dependency changed during assembly: {}",
            self.source.display()
        );
        Ok(())
    }
}

pub(super) struct Plan {
    inputs: Vec<Input>,
    machine: u16,
}
impl Plan {
    pub(super) fn from_compiler_identity(identity: &str) -> Result<Self> {
        let library = identity
            .lines()
            .find_map(|line| line.strip_prefix("FREETYPE_LIBRARY_RELEASE="))
            .filter(|value| !value.is_empty())
            .context("FreeType probe missing FREETYPE_LIBRARY_RELEASE")?;
        let library_digest_prefix = format!("input={library}:");
        let expected_library_digest = identity
            .lines()
            .find_map(|line| line.strip_prefix(&library_digest_prefix))
            .context("FreeType compiler identity omits its exact import-library digest")?;
        let library = PathBuf::from(library);
        ensure!(
            library.is_absolute(),
            "FreeType runtime assembly requires the exact absolute probed library"
        );
        let lib = library.parent().context("FreeType library directory")?;
        ensure!(
            lib.file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("lib")),
            "FreeType runtime assembly requires a reviewed prefix/lib layout: {}",
            library.display()
        );
        let root = lib.parent().context("FreeType prefix")?;
        crate::package::ordinary_directory(root)?;
        let bin = root.join("bin");
        crate::package::ordinary_directory(&bin)?;
        let mut files = BTreeMap::new();
        for entry in fs::read_dir(&bin)? {
            let entry = entry?;
            if entry
                .path()
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"))
            {
                let name = entry
                    .file_name()
                    .to_str()
                    .context("non-Unicode dependency DLL name")?
                    .to_ascii_lowercase();
                ensure!(
                    files.insert(name, entry.path()).is_none(),
                    "Ambiguous case-insensitive DLL names in FreeType prefix"
                );
            }
        }
        let library_input = Input::read(library.clone(), None)?;
        ensure!(
            crate::hash::reader(library_input.bytes.as_slice())? == expected_library_digest,
            "Probed FreeType library changed before runtime assembly"
        );
        let mut inputs = vec![library_input];
        let mut pending = vec!["freetype.dll".to_owned()];
        let mut seen = BTreeSet::new();
        let mut ports = BTreeSet::new();
        let mut machine = None;
        while let Some(name) = pending.pop() {
            if !seen.insert(name.clone()) {
                continue;
            }
            let port = crate::windows_runtime::dependency_port(&name)
                .with_context(|| format!("Unreviewed FreeType runtime dependency {name}; no arbitrary DLL or OCCT replacement is allowed"))?;
            ports.insert(port);
            let source = files.get(&name).with_context(|| {
                format!(
                    "Missing {name} in the exact probed FreeType bin {}",
                    bin.display()
                )
            })?;
            let input = Input::read(source.clone(), Some(PathBuf::from("bin").join(&name)))?;
            let image = crate::windows_runtime::inspect_bytes(&input.bytes)?;
            if let Some(expected) = machine {
                ensure!(
                    image.machine == expected,
                    "FreeType dependency architecture mismatch: {name}"
                );
            } else {
                machine = Some(image.machine);
            }
            for dependency in image.imports {
                if !crate::windows_runtime::is_system_import(&dependency) {
                    pending.push(dependency.to_ascii_lowercase());
                }
            }
            inputs.push(input);
        }
        for port in ports {
            let relative = PathBuf::from("share").join(port).join("copyright");
            let input = Input::read(root.join(&relative), Some(relative))?;
            ensure!(
                !input.bytes.is_empty(),
                "FreeType dependency copyright notice is empty: {port}"
            );
            inputs.push(input);
        }
        Ok(Self {
            inputs,
            machine: machine.context("Missing FreeType image architecture")?,
        })
    }

    /// Called only with the builder's exact owned-prefix lock retained.
    pub(super) fn apply(&self, prefix: &Path, key: &str) -> Result<()> {
        crate::occt_cache::verify_owner(prefix, key)?;
        crate::package::ordinary_directory(prefix)?;
        let bin = crate::package::runtime_bin(prefix)?;
        let mut existing_dlls = BTreeMap::new();
        for entry in fs::read_dir(&bin)? {
            let entry = entry?;
            if entry
                .path()
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"))
            {
                let name = entry
                    .file_name()
                    .to_str()
                    .context("non-Unicode SDK DLL name")?
                    .to_ascii_lowercase();
                ensure!(
                    existing_dlls.insert(name, entry.path()).is_none(),
                    "Ambiguous case-insensitive DLL names in SDK runtime"
                );
            }
        }
        let kernel = crate::windows_runtime::inspect_imports(
            existing_dlls
                .get("tkernel.dll")
                .context("Missing installed TKernel.dll")?,
        )?;
        ensure!(
            kernel.machine == self.machine,
            "FreeType runtime architecture differs from the installed OCCT SDK"
        );
        for input in &self.inputs {
            input.recheck()?;
        }
        // Validate every existing destination before making even a missing-file
        // addition. An interrupted repair leaves the old receipt invalid rather
        // than certifying a partial closure; normal resume retains Ninja outputs.
        for input in &self.inputs {
            if let Some(relative) = &input.relative {
                let destination = destination(prefix, &bin, relative, &existing_dlls);
                if present(&destination)? {
                    crate::package::ordinary_file(&destination)?;
                    ensure!(
                        fs::metadata(&destination)?.len() == input.bytes.len() as u64,
                        "Refusing to overwrite conflicting SDK runtime content: {}",
                        destination.display()
                    );
                    ensure!(
                        fs::read(&destination)? == input.bytes,
                        "Refusing to overwrite conflicting SDK runtime content: {}",
                        destination.display()
                    );
                }
                let mut parent = destination.parent().context("runtime destination parent")?;
                while !present(parent)? {
                    parent = parent.parent().context("runtime destination ancestor")?;
                }
                crate::package::ordinary_directory(parent)?;
            }
        }
        for input in &self.inputs {
            if let Some(relative) = &input.relative {
                let destination = destination(prefix, &bin, relative, &existing_dlls);
                if !present(&destination)? {
                    let parent = destination.parent().context("runtime destination parent")?;
                    fs::create_dir_all(parent)?;
                    crate::package::ordinary_directory(parent)?;
                    let mut staged = tempfile::NamedTempFile::new_in(parent)?;
                    use std::io::Write;
                    staged.write_all(&input.bytes)?;
                    staged.as_file_mut().sync_all()?;
                    staged.persist_noclobber(&destination)?;
                }
            }
        }
        for input in &self.inputs {
            input.recheck()?;
            if let Some(relative) = &input.relative {
                let destination = destination(prefix, &bin, relative, &existing_dlls);
                crate::package::ordinary_file(&destination)?;
                ensure!(
                    fs::metadata(&destination)?.len() == input.bytes.len() as u64
                        && fs::read(&destination)? == input.bytes,
                    "SDK runtime destination changed during assembly: {}",
                    destination.display()
                );
            }
        }
        verify(prefix)
    }
}

fn destination(
    prefix: &Path,
    bin: &Path,
    relative: &Path,
    existing_dlls: &BTreeMap<String, PathBuf>,
) -> PathBuf {
    relative.strip_prefix("bin").map_or_else(
        |_| prefix.join(relative),
        |name| {
            existing_dlls
                .get(&name.to_string_lossy().to_ascii_lowercase())
                .cloned()
                .unwrap_or_else(|| bin.join(name))
        },
    )
}

pub(super) fn verify(prefix: &Path) -> Result<()> {
    let bin = crate::package::runtime_bin(prefix)?;
    crate::windows_runtime::verify_directory(&bin)?;
    for entry in fs::read_dir(bin)? {
        let entry = entry?;
        if let Some(port) = entry
            .file_name()
            .to_str()
            .and_then(crate::windows_runtime::dependency_port)
        {
            let notice = prefix.join("share").join(port).join("copyright");
            crate::package::ordinary_file(&notice)?;
            ensure!(
                fs::metadata(notice)?.len() > 0,
                "SDK dependency notice is empty: {port}"
            );
        }
    }
    Ok(())
}

/// Return whether explicit repair added closure files. Qualification must
/// succeed before the caller atomically republishes the existing cache key.
pub(super) fn verify_or_repair(
    prefix: &Path,
    key: &str,
    identity: &str,
    repair: bool,
) -> Result<bool> {
    if let Err(error) = verify(prefix) {
        ensure!(repair,
            "Installed SDK runtime closure is incomplete: {error:#}. Use explicit --repair-runtime to assemble missing dependencies from the exact probed FreeType prefix; existing SDK/cache content is preserved");
        Plan::from_compiler_identity(identity)?.apply(prefix, key)?;
        return Ok(true);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    const KEY: &str = "owned-runtime-fixture";
    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, String) {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("probed-freetype");
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::create_dir(root.join("lib")).unwrap();
        let library = root.join("lib/freetype.lib");
        fs::write(&library, b"probed import library").unwrap();
        for (name, imports) in [
            (
                "freetype.dll",
                vec!["z.dll", "bz2.dll", "libpng16.dll", "brotlidec.dll"],
            ),
            ("z.dll", vec!["KERNEL32.dll"]),
            ("bz2.dll", vec!["KERNEL32.dll"]),
            ("libpng16.dll", vec!["z.dll"]),
            ("brotlidec.dll", vec!["brotlicommon.dll"]),
            ("brotlicommon.dll", vec!["KERNEL32.dll"]),
        ] {
            fs::write(
                root.join("bin").join(name),
                crate::windows_runtime::fixture_image(0x8664, &imports),
            )
            .unwrap();
        }
        for port in ["freetype", "zlib", "bzip2", "libpng", "brotli"] {
            fs::create_dir_all(root.join("share").join(port)).unwrap();
            fs::write(
                root.join("share").join(port).join("copyright"),
                format!("selected {port} notice"),
            )
            .unwrap();
        }
        let prefix = temporary.path().join("owned-sdk");
        let lock = crate::occt_cache::prepare_locked(&prefix, KEY).unwrap();
        fs::create_dir(prefix.join("bin")).unwrap();
        for (name, imports) in [
            ("TKernel.dll", vec!["KERNEL32.dll"]),
            ("TKService.dll", vec!["TKernel.dll", "freetype.dll"]),
        ] {
            fs::write(
                prefix.join("bin").join(name),
                crate::windows_runtime::fixture_image(0x8664, &imports),
            )
            .unwrap();
        }
        drop(lock);
        let identity = format!(
            "FREETYPE_LIBRARY_RELEASE={}\ninput={}:{}\n",
            library.display(),
            library.display(),
            crate::hash::file(&library).unwrap()
        );
        (temporary, root, prefix, identity)
    }

    #[test]
    fn explicit_runtime_assembly_preserves_owner_and_adds_exact_transitive_files_and_notices() {
        let (_temporary, root, prefix, identity) = fixture();
        let plan = Plan::from_compiler_identity(&identity).unwrap();
        let _lock = crate::occt_cache::prepare_locked(&prefix, KEY).unwrap();
        let kernel = fs::read(prefix.join("bin/TKernel.dll")).unwrap();
        assert!(verify(&prefix).is_err());
        assert!(plan.apply(&prefix, "another-owner").is_err());
        assert!(!prefix.join("bin/freetype.dll").exists());
        fs::write(
            prefix.join("bin/FreeType.DLL"),
            fs::read(root.join("bin/freetype.dll")).unwrap(),
        )
        .unwrap();
        plan.apply(&prefix, KEY).unwrap();
        plan.apply(&prefix, KEY).unwrap();
        verify(&prefix).unwrap();
        assert_eq!(fs::read(prefix.join("bin/TKernel.dll")).unwrap(), kernel);
        assert_eq!(
            fs::read(prefix.join("bin/FreeType.DLL")).unwrap(),
            fs::read(root.join("bin/freetype.dll")).unwrap()
        );
        assert!(prefix.join("bin/brotlicommon.dll").is_file());
        assert_eq!(
            fs::read_dir(prefix.join("bin"))
                .unwrap()
                .filter_map(|entry| entry.ok())
                .filter(|entry| entry.file_name().eq_ignore_ascii_case("freetype.dll"))
                .count(),
            1
        );
        for port in ["freetype", "zlib", "bzip2", "libpng", "brotli"] {
            assert_eq!(
                fs::read(prefix.join("share").join(port).join("copyright")).unwrap(),
                fs::read(root.join("share").join(port).join("copyright")).unwrap()
            );
        }
        crate::occt_cache::publish(&prefix, KEY).unwrap();
        assert!(crate::occt_cache::complete(&prefix, KEY).unwrap());
    }

    #[test]
    fn runtime_assembly_rejects_missing_notices_architecture_conflicts_and_source_drift_before_writes(
    ) {
        let (_temporary, root, prefix, identity) = fixture();
        let notice = root.join("share/brotli/copyright");
        let original = fs::read(&notice).unwrap();
        fs::remove_file(&notice).unwrap();
        assert!(Plan::from_compiler_identity(&identity).is_err());
        assert!(!prefix.join("bin/freetype.dll").exists());
        fs::write(&notice, original).unwrap();
        let changed = root.join("bin/z.dll");
        let original = fs::read(&changed).unwrap();
        fs::write(
            &changed,
            crate::windows_runtime::fixture_image(0x14c, &["KERNEL32.dll"]),
        )
        .unwrap();
        assert!(Plan::from_compiler_identity(&identity)
            .err()
            .unwrap()
            .to_string()
            .contains("architecture"));
        fs::write(&changed, &original).unwrap();
        let freetype = root.join("bin/freetype.dll");
        let original_freetype = fs::read(&freetype).unwrap();
        fs::write(
            &freetype,
            crate::windows_runtime::fixture_image(0x8664, &["TKMath.dll"]),
        )
        .unwrap();
        assert!(Plan::from_compiler_identity(&identity).is_err());
        assert!(!prefix.join("bin/freetype.dll").exists());
        fs::write(&freetype, original_freetype).unwrap();
        let library = root.join("lib/freetype.lib");
        let original_library = fs::read(&library).unwrap();
        fs::write(&library, b"drifted import library").unwrap();
        assert!(Plan::from_compiler_identity(&identity).is_err());
        fs::write(&library, original_library).unwrap();
        let plan = Plan::from_compiler_identity(&identity).unwrap();
        fs::write(&changed, b"changed source").unwrap();
        assert!(plan.apply(&prefix, KEY).is_err());
        assert!(!prefix.join("bin/freetype.dll").exists());
        fs::write(&changed, original).unwrap();
        fs::write(prefix.join("bin/bz2.dll"), b"conflicting existing runtime").unwrap();
        assert!(plan.apply(&prefix, KEY).is_err());
        assert!(!prefix.join("bin/freetype.dll").exists());
        assert_eq!(
            fs::read(prefix.join("bin/bz2.dll")).unwrap(),
            b"conflicting existing runtime"
        );
    }

    #[test]
    fn complete_legacy_sdk_requires_explicit_runtime_repair_and_inventory_republication() {
        let (_temporary, _root, prefix, identity) = fixture();
        let _lock = crate::occt_cache::prepare_locked(&prefix, KEY).unwrap();
        crate::occt_cache::publish(&prefix, KEY).unwrap();
        let receipt_path = prefix.join(".limo-sdk-complete.json");
        let receipt = fs::read(&receipt_path).unwrap();
        assert!(crate::occt_cache::complete(&prefix, KEY).unwrap());
        assert!(verify_or_repair(&prefix, KEY, &identity, false)
            .unwrap_err()
            .to_string()
            .contains("--repair-runtime"));
        assert_eq!(fs::read(&receipt_path).unwrap(), receipt);
        assert!(!prefix.join("bin/freetype.dll").exists());
        assert!(verify_or_repair(&prefix, KEY, &identity, true).unwrap());
        assert_eq!(fs::read(&receipt_path).unwrap(), receipt);
        assert!(!crate::occt_cache::complete(&prefix, KEY).unwrap());
        // Runtime completion does not itself assert ABI qualification or publish.
        crate::occt_cache::publish(&prefix, KEY).unwrap();
        assert!(crate::occt_cache::complete(&prefix, KEY).unwrap());
        assert!(!verify_or_repair(&prefix, KEY, &identity, false).unwrap());
    }
}
