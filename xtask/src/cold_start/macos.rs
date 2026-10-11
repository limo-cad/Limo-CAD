use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::DirBuilderExt,
    path::{Component, Path, PathBuf},
    process::Command,
};

const HELPER: &str = include_str!("metal-cache.m");
const PROBE: &str = include_str!("probe.m");

fn output(command: &mut Command) -> Result<String> {
    let result = command
        .output()
        .with_context(|| format!("start {command:?}"))?;
    ensure!(
        result.status.success(),
        "{command:?} failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    Ok(String::from_utf8(result.stdout)?)
}

fn sign(app: &Path) -> Result<()> {
    output(
        Command::new("codesign")
            .args(["--force", "--sign", "-", "--options=0"])
            .arg(app),
    )?;
    output(
        Command::new("codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(app),
    )?;
    Ok(())
}

fn plist(path: &Path, value: &Value) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    output(Command::new("plutil").args(["-convert", "xml1"]).arg(path))?;
    Ok(())
}

fn private_directory(path: &Path) -> Result<()> {
    fs::DirBuilder::new().mode(0o700).create(path)?;
    Ok(())
}

fn compile(source: &Path, binary: &Path, dylib: bool) -> Result<()> {
    let mut command = Command::new("xcrun");
    command.args(["clang", "-O2", "-fobjc-arc", "-Wall", "-Wextra", "-Werror"]);
    if dylib {
        command.arg("-dynamiclib");
    }
    command.args(["-framework", "Foundation", "-framework", "Metal"]);
    output(command.arg(source).arg("-o").arg(binary))?;
    sign(binary)
}

pub(super) fn prepare(source: &Path, requested_out: &Path, verify: bool) -> Result<()> {
    ensure!(requested_out.is_absolute(), "--out must be absolute");
    let parent = requested_out
        .parent()
        .context("output parent")?
        .canonicalize()?;
    let name = requested_out.file_name().context("output directory name")?;
    ensure!(
        matches!(
            requested_out.components().next_back(),
            Some(Component::Normal(_))
        ),
        "invalid output directory"
    );
    let out = parent.join(name);
    ensure!(
        fs::symlink_metadata(&out).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
        "output already exists or cannot be inspected; use a new directory"
    );
    let source = source.canonicalize()?;
    ensure!(
        !out.starts_with(&source),
        "output cannot be inside the source app"
    );
    ensure!(
        source.extension().is_some_and(|v| v == "app"),
        "--app must be a packaged .app"
    );
    output(
        Command::new("codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&source),
    )?;
    let source_info = source.join("Contents/Info.plist");
    let mut info: Value = serde_json::from_str(&output(
        Command::new("plutil")
            .args(["-convert", "json", "-o", "-"])
            .arg(&source_info),
    )?)?;
    let executable = info["CFBundleExecutable"]
        .as_str()
        .context("CFBundleExecutable")?
        .to_owned();
    let mut components = Path::new(&executable).components();
    ensure!(
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none(),
        "bundle executable must be a single filename"
    );
    let relative_exe = Path::new("Contents/MacOS").join(executable);
    crate::package::ordinary_file(&source.join(&relative_exe))?;
    crate::package::ordinary_file(&source_info)?;
    let source_sections = sections(&source.join(&relative_exe))?;
    let source_hash = crate::hash::file(&source.join(&relative_exe))?;

    private_directory(&out)?;
    let tools = out.join("tools");
    private_directory(&tools)?;
    let profile = out.join("profile");
    private_directory(&profile)?;
    private_directory(&profile.join("config"))?;
    private_directory(&profile.join("sessions"))?;
    let caches = out.join("cache-runs");
    private_directory(&caches)?;
    let app = out.join("Limo CAD Cold Start.app");
    output(Command::new("ditto").arg(&source).arg(&app))?;
    let framework = app.join("Contents/Frameworks");
    fs::create_dir_all(&framework)?;
    let helper = framework.join("libLimoColdCache.dylib");
    ensure!(
        !helper.exists(),
        "input app already contains a cold-start helper"
    );
    let helper_source = tools.join("metal-cache.m");
    fs::write(&helper_source, HELPER)?;
    compile(&helper_source, &helper, true)?;

    // A unique bundle ID also isolates the OS's app preferences and running instance.
    let identity = tempfile::Builder::new()
        .prefix("identity-")
        .tempdir_in(&out)?;
    let suffix = identity
        .path()
        .file_name()
        .context("test identity")?
        .to_string_lossy();
    info["CFBundleIdentifier"] = json!(format!("org.limocad.desktop.coldtest.{suffix}"));
    info["CFBundleName"] = json!("Limo CAD Cold Start");
    info["CFBundleDisplayName"] = json!("Limo CAD Cold Start");
    let environment = json!({
        "LIMO_CAD_CONFIG_DIR": profile.join("config"),
        "LIMO_CAD_SESSION_DIR": profile.join("sessions"),
        "LIMO_CAD_COLD_CACHE_ROOT": caches,
        "DYLD_INSERT_LIBRARIES": helper,
    });
    // Do not retain production environment overrides or claim production URL handlers.
    info["LSEnvironment"] = environment.clone();
    info.as_object_mut()
        .context("bundle plist dictionary")?
        .remove("CFBundleURLTypes");
    plist(&app.join("Contents/Info.plist"), &info)?;
    sign(&app)?;
    ensure!(
        source_sections == sections(&app.join(&relative_exe))?,
        "compiled release sections changed"
    );
    ensure!(
        source_hash == crate::hash::file(&source.join(&relative_exe))?,
        "source release changed during preparation"
    );
    verify_resources(&source, &app, &relative_exe)?;
    let gpu_verification = if verify {
        Some(verify_gpu(&tools, &helper, &suffix)?)
    } else {
        None
    };
    ensure!(
        fs::read_dir(&caches)?.next().is_none(),
        "user launch cache is not empty"
    );
    for name in ["config", "sessions"] {
        ensure!(
            fs::read_dir(profile.join(name))?.next().is_none(),
            "test profile is not empty"
        );
    }
    let receipt = json!({
        "source_app": source,
        "app": app,
        "source_executable_sha256": source_hash,
        "test_executable_sha256": crate::hash::file(&app.join(relative_exe))?,
        "compiled_release_sections_unchanged": true,
        "compiled_section_hashes": source_sections,
        "source_resources_unchanged": true,
        "test_helper_sha256": crate::hash::file(&helper)?,
        "launch_environment": environment,
        "empty_profile": true,
        "cad_app_not_launched": true,
        "signature": "ad-hoc; deep strict verification passed",
        "gpu_verification": gpu_verification,
        "method": "Test-only private Metal cache-path override before GPU initialization; fresh cache per top-level launch; verified child processes share that launch's cache",
        "limits": "Cold Metal shader compilation caches only; host hardware/load and OS/filesystem memory caches are retained. Keep this local test app in the prepared directory."
    });
    fs::write(
        out.join("cold-test-ready.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    fs::write(out.join("START HERE.txt"), "Double-click Limo CAD Cold Start.app in Finder.\nEvery launch uses a new empty Metal shader cache. The release code and shaders are unchanged.\nThis separate profile starts empty. Existing settings, documents, recovery and caches are preserved.\nThe test-only helper uses a private Metal interface and stops if cache isolation cannot be verified.\nOS/filesystem caches and host hardware/load are retained. Keep this app in its prepared folder.\n")?;
    println!(
        "Cold-start test app: {}\nReceipt: {}",
        app.display(),
        out.join("cold-test-ready.json").display()
    );
    Ok(())
}

fn verify_resources(source: &Path, copy: &Path, executable: &Path) -> Result<()> {
    let mut pending = vec![source.to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            let relative = path.strip_prefix(source)?;
            let target = copy.join(relative);
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                ensure!(
                    fs::read_link(&path)? == fs::read_link(&target)?,
                    "resource symlink changed: {}",
                    relative.display()
                );
            } else if kind.is_dir() {
                pending.push(path);
            } else if relative != executable
                && relative != Path::new("Contents/Info.plist")
                && !relative.starts_with("Contents/_CodeSignature")
            {
                ensure!(
                    crate::hash::file(&path)? == crate::hash::file(&target)?,
                    "release resource changed: {}",
                    relative.display()
                );
            }
        }
    }
    Ok(())
}

// Ad-hoc signing changes the executable's signature. Compare the loaded sections,
// including embedded shaders and constants, rather than its signing blob.
fn sections(path: &Path) -> Result<BTreeMap<String, String>> {
    let data = fs::read(path)?;
    let u32_at = |start: usize| -> Result<u32> {
        Ok(u32::from_le_bytes(
            data.get(start..start + 4)
                .context("truncated Mach-O")?
                .try_into()?,
        ))
    };
    ensure!(
        u32_at(0)? == 0xfeedfacf,
        "cold-start preparation currently requires a thin 64-bit macOS executable"
    );
    let mut cursor = 32;
    let mut result = BTreeMap::new();
    for _ in 0..u32_at(16)? {
        let command = u32_at(cursor)?;
        let length = u32_at(cursor + 4)? as usize;
        ensure!(
            length >= 8
                && cursor
                    .checked_add(length)
                    .is_some_and(|end| end <= data.len()),
            "invalid Mach-O load command"
        );
        if command == 0x19 {
            ensure!(length >= 72, "truncated Mach-O segment");
            let count = u32_at(cursor + 64)? as usize;
            ensure!(count <= (length - 72) / 80, "truncated Mach-O sections");
            for index in 0..count {
                let section = cursor + 72 + index * 80;
                if matches!(u32_at(section + 64)? & 0xff, 1 | 0xc | 0x12) {
                    continue;
                }
                let name = |start: usize| -> Result<&str> {
                    let bytes = &data[start..start + 16];
                    Ok(std::str::from_utf8(
                        &bytes[..bytes.iter().position(|v| *v == 0).unwrap_or(16)],
                    )?)
                };
                let length = usize::try_from(u64::from_le_bytes(
                    data[section + 40..section + 48].try_into()?,
                ))?;
                let start = u32_at(section + 48)? as usize;
                let end = start
                    .checked_add(length)
                    .context("Mach-O section overflow")?;
                let bytes = data
                    .get(start..end)
                    .context("truncated Mach-O section data")?;
                let key = format!("{}/{}", name(section + 16)?, name(section)?);
                ensure!(
                    result
                        .insert(key, crate::hash::hex(&Sha256::digest(bytes)))
                        .is_none(),
                    "duplicate Mach-O section"
                );
            }
        }
        cursor += length;
    }
    ensure!(
        result.keys().any(|key| key == "__TEXT/__text"),
        "executable has no compiled text section"
    );
    Ok(result)
}

fn verify_gpu(tools: &Path, helper: &Path, suffix: &str) -> Result<Value> {
    let caches = tools.join("verification-caches");
    private_directory(&caches)?;
    let probe = tools.join("Metal Cold Cache Check.app");
    fs::create_dir_all(probe.join("Contents/MacOS"))?;
    let source = tools.join("probe.m");
    fs::write(&source, PROBE)?;
    compile(&source, &probe.join("Contents/MacOS/cache-probe"), false)?;
    plist(
        &probe.join("Contents/Info.plist"),
        &json!({
            "CFBundleIdentifier": format!("org.limocad.cold-cache-check.{suffix}"),
            "CFBundleName": "Metal Cold Cache Check", "CFBundleExecutable": "cache-probe",
            "CFBundlePackageType": "APPL", "CFBundleVersion": "1", "LSUIElement": true,
            "LSEnvironment": {"LIMO_CAD_COLD_CACHE_ROOT": caches, "DYLD_INSERT_LIBRARIES": helper}
        }),
    )?;
    sign(&probe)?;
    let mut receipts = Vec::new();
    for number in [1, 2] {
        let stdout = tools.join(format!("gpu-probe-{number}.json"));
        let stderr = tools.join(format!("gpu-probe-{number}.stderr"));
        output(
            Command::new("open")
                .args(["-W", "-n", "-a"])
                .arg(&probe)
                .arg("--stdout")
                .arg(&stdout)
                .arg("--stderr")
                .arg(&stderr),
        )?;
        let receipt: Value = serde_json::from_slice(&fs::read(&stdout)?)?;
        ensure!(
            receipt["gpu_result"] == json!([7, 8, 9, 10]),
            "GPU probe failed"
        );
        let cache = PathBuf::from(receipt["cache_root"].as_str().context("probe cache root")?);
        ensure!(
            cache.parent() == Some(caches.as_path()),
            "probe used an existing cache"
        );
        let setup: Value = serde_json::from_slice(&fs::read(
            cache.join(format!("cache-setup-{}.json", receipt["process_id"])),
        )?)?;
        ensure!(
            setup["empty_before_gpu_initialization"] == true,
            "probe did not start with an empty cache"
        );
        let mut populated = [false, false];
        let mut pending = vec![cache.clone()];
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(directory)? {
                let entry = entry?;
                let kind = entry.file_type()?;
                ensure!(!kind.is_symlink(), "probe cache is redirected");
                if kind.is_dir() {
                    pending.push(entry.path());
                } else if kind.is_file() && entry.metadata()?.len() > 0 {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with("functions") && name.ends_with(".data") {
                        populated[0] = true;
                    }
                    if name.starts_with("libraries") && name.ends_with(".data") {
                        populated[1] = true;
                    }
                }
            }
        }
        ensure!(
            populated == [true, true],
            "private shader/library caches were not populated"
        );
        receipts.push(receipt);
    }
    ensure!(
        receipts[0]["cache_root"] != receipts[1]["cache_root"],
        "separate launches reused a cache"
    );
    Ok(
        json!({"normal_mac_launches": receipts, "distinct_empty_caches_and_gpu_execution_verified": true}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image() -> Vec<u8> {
        // One loaded text section, followed by an unrelated signing tail.
        let mut bytes = vec![0u8; 186];
        bytes[..4].copy_from_slice(&0xfeedfacfu32.to_le_bytes());
        bytes[16..20].copy_from_slice(&1u32.to_le_bytes());
        bytes[32..36].copy_from_slice(&0x19u32.to_le_bytes());
        bytes[36..40].copy_from_slice(&152u32.to_le_bytes());
        bytes[96..100].copy_from_slice(&1u32.to_le_bytes());
        bytes[104..110].copy_from_slice(b"__text");
        bytes[120..126].copy_from_slice(b"__TEXT");
        bytes[144..152].copy_from_slice(&2u64.to_le_bytes());
        bytes[152..156].copy_from_slice(&184u32.to_le_bytes());
        bytes[184..186].copy_from_slice(&[0xaa, 0xbb]);
        bytes
    }

    #[test]
    fn signing_tail_is_ignored_but_loaded_release_code_changes_are_detected() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("release");
        let mut bytes = image();
        fs::write(&file, &bytes).unwrap();
        let original = sections(&file).unwrap();
        bytes.extend_from_slice(b"different signing data");
        fs::write(&file, &bytes).unwrap();
        assert_eq!(sections(&file).unwrap(), original);
        bytes[184] ^= 1;
        fs::write(&file, &bytes).unwrap();
        assert_ne!(sections(&file).unwrap(), original);
    }

    #[test]
    fn malformed_executables_fail_before_they_can_be_repackaged() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("release");
        for malformed in [
            image()[..185].to_vec(),
            {
                let mut bytes = image();
                bytes[36..40].copy_from_slice(&7u32.to_le_bytes());
                bytes
            },
            {
                let mut bytes = image();
                bytes[96..100].copy_from_slice(&u32::MAX.to_le_bytes());
                bytes
            },
        ] {
            fs::write(&file, malformed).unwrap();
            assert!(sections(&file).is_err());
        }
    }
}
