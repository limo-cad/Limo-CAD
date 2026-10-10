//! Winit dlopens XKB, so DT_NEEDED discovery alone misses this runtime closure.
use super::common::{self, Package};
use anyhow::{ensure, Context, Result};
use regex::Regex;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

const SONAME: &str = "libxkbcommon-x11.so.0";
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Runtime {
    pub soname: String,
    pub package: String,
    pub version: String,
    pub sha256: String,
    pub copyright: String,
    pub common_licenses: Vec<String>,
}
fn platform(name: &str) -> bool {
    matches!(
        name,
        "libc.so.6" | "libm.so.6" | "libpthread.so.0" | "libdl.so.2" | "librt.so.1"
    ) || name.starts_with("ld-linux")
        || name.starts_with("linux-vdso")
}
fn parse_ldd(text: &str) -> Result<BTreeMap<String, PathBuf>> {
    let linked = Regex::new(r"^(\S+)\s+=>\s+(/\S+)\s+\(0x[\da-fA-F]+\)$")?;
    let direct = Regex::new(r"^(/\S+|linux-vdso\S+)\s+\(0x[\da-fA-F]+\)$")?;
    let mut result = BTreeMap::new();
    for line in text.lines().map(str::trim).filter(|v| !v.is_empty()) {
        if let Some(c) = linked.captures(line) {
            let name = c[1].to_owned();
            let path = PathBuf::from(&c[2]);
            ensure!(
                Path::new(&name).file_name().and_then(|v| v.to_str()) == Some(name.as_str()),
                "invalid XKB SONAME {name}"
            );
            if let Some(old) = result.insert(name.clone(), path.clone()) {
                ensure!(old == path, "conflicting XKB dependency paths for {name}");
            }
        } else if let Some(c) = direct.captures(line) {
            ensure!(
                platform(
                    Path::new(&c[1])
                        .file_name()
                        .context("loader name")?
                        .to_str()
                        .context("loader UTF-8")?
                ),
                "cannot resolve XKB dependency: {line}"
            );
        } else {
            anyhow::bail!("cannot resolve XKB dependency: {line}");
        }
    }
    ensure!(!result.is_empty(), "empty XKB dependency report");
    Ok(result)
}
fn needed(package: &Package, path: &Path, names: &BTreeSet<String>) -> Result<()> {
    let text = common::output(package.command("patchelf").arg("--print-needed").arg(path))?;
    for name in text.lines().filter(|v| !v.is_empty()) {
        ensure!(
            platform(name) || names.contains(name),
            "{} needs missing bundled XKB dependency {name}",
            path.display()
        );
    }
    Ok(())
}
fn owner(package: &Package, path: &Path) -> Result<String> {
    let text = path.to_str().context("XKB library UTF-8 path")?;
    for candidate in [text.to_owned(), text.replacen("/usr/lib/", "/lib/", 1)] {
        if let Ok(output) =
            common::output(package.command("dpkg-query").args(["--search", &candidate]))
        {
            if let Some((name, _)) = output
                .lines()
                .next()
                .and_then(|line| line.split_once(": /"))
            {
                return Ok(name.into());
            }
        }
    }
    anyhow::bail!("no Ubuntu package owns XKB runtime {}", path.display())
}
pub(super) fn stage(package: &Package, licenses: &Path) -> Result<Vec<Runtime>> {
    let libdir = common::output(
        package
            .command("pkg-config")
            .args(["--variable=libdir", "xkbcommon-x11"]),
    )?;
    let seed = Path::new(&libdir).join(SONAME).canonicalize()?;
    let mut closure = parse_ldd(&common::output(
        package
            .command("ldd")
            .arg(&seed)
            .env("LD_LIBRARY_PATH", "")
            .env("LD_PRELOAD", "")
            .env("LD_AUDIT", ""),
    )?)?;
    closure.insert(SONAME.into(), seed);
    closure.retain(|name, _| !platform(name));
    let names = closure.keys().cloned().collect();
    for path in closure.values() {
        needed(package, path, &names)?;
    }
    let stage = common::fresh_child(licenses, "xkb")?;
    fs::create_dir(stage.join("lib"))?;
    fs::create_dir(stage.join("licenses"))?;
    let common_pattern = Regex::new(r"/usr/share/common-licenses/([A-Za-z0-9.+-]+)")?;
    let mut manifest = Vec::new();
    for (soname, path) in closure {
        let source = path.canonicalize()?;
        let owner = owner(package, &source)?;
        let plain = owner.split(':').next().context("Ubuntu package name")?;
        let copyright_source = Path::new("/usr/share/doc").join(plain).join("copyright");
        let copyright = format!("{plain}-copyright.txt");
        fs::copy(&source, stage.join("lib").join(&soname))?;
        fs::copy(&copyright_source, stage.join("licenses").join(&copyright))?;
        let text = fs::read_to_string(&copyright_source)?;
        let common_licenses: BTreeSet<_> = common_pattern
            .captures_iter(&text)
            .map(|c| c[1].trim_end_matches(['.', ',']).to_owned())
            .collect();
        for name in &common_licenses {
            fs::copy(
                Path::new("/usr/share/common-licenses").join(name),
                stage.join("licenses").join(name),
            )?;
        }
        let version = common::output(package.command("dpkg-query").args([
            "--show",
            "--showformat=${Version}",
            &owner,
        ]))?;
        manifest.push(Runtime {
            soname,
            package: owner,
            version,
            sha256: common::sha256(&source)?,
            copyright,
            common_licenses: common_licenses.into_iter().collect(),
        });
    }
    fs::write(
        stage.join("licenses/runtime.json"),
        format!("{}\n", serde_json::to_string_pretty(&manifest)?),
    )?;
    Ok(manifest)
}
pub(super) fn verify(package: &Package, app: &Path, manifest: &[Runtime]) -> Result<()> {
    let library_root = app.join("usr/lib").canonicalize()?;
    let names = manifest.iter().map(|entry| entry.soname.clone()).collect();
    for entry in manifest {
        let path = library_root.join(&entry.soname).canonicalize()?;
        ensure!(
            path.starts_with(&library_root),
            "XKB runtime escapes AppImage: {}",
            entry.soname
        );
        needed(package, &path, &names)?;
    }
    let closure = parse_ldd(&common::output(
        package
            .command("ldd")
            .arg(library_root.join(SONAME))
            .env("LD_LIBRARY_PATH", &library_root)
            .env("LD_PRELOAD", "")
            .env("LD_AUDIT", ""),
    )?)?;
    for (name, path) in closure {
        if !platform(&name) {
            ensure!(
                path.canonicalize()?.starts_with(&library_root),
                "AppImage XKB falls back to host dependency {name}: {}",
                path.display()
            );
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closure_parser_keeps_xcb_and_rejects_missing_or_conflicting_libraries() {
        let text = "linux-vdso.so.1 (0x00007fff)\nlibxkbcommon.so.0 => /lib/libxkbcommon.so.0 (0x00100000)\nlibxcb-xkb.so.1 => /lib/libxcb-xkb.so.1 (0x00200000)\nlibc.so.6 => /lib/libc.so.6 (0x00300000)\n/lib64/ld-linux-x86-64.so.2 (0x00400000)";
        let result = parse_ldd(text).unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result["libxcb-xkb.so.1"], Path::new("/lib/libxcb-xkb.so.1"));
        for bad in [
            "libxcb-xkb.so.1 => not found",
            "not a dynamic executable",
            "",
            "libxcb.so.1 => /lib/a (0x1)\nlibxcb.so.1 => /lib/b (0x2)",
        ] {
            assert!(parse_ldd(bad).is_err());
        }
    }
}
