//! Native package builders. There are no legacy-script fallbacks.
mod common;
mod linux;
mod macos;
mod recipe_handler;
mod windows;
mod xkb;

pub(crate) use common::{ordinary_directory, ordinary_file};
pub(crate) use windows::{runtime_bin, stage_runtime};

use anyhow::{bail, ensure, Context, Result};
use std::{env, path::PathBuf};

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Options {
    target: Option<String>,
    bundle: Option<String>,
    occt_root: Option<PathBuf>,
    stage_licenses: bool,
    computer_control: bool,
    help: bool,
}
impl Options {
    fn parse(mut args: impl Iterator<Item = String>) -> Result<Self> {
        let mut result = Self::default();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--target" if result.target.is_none() => {
                    result.target = Some(args.next().context("--target requires a Rust target")?)
                }
                "--bundle" if result.bundle.is_none() => {
                    result.bundle = Some(
                        args.next()
                            .context("--bundle requires portable, deb, appimage or dmg")?,
                    )
                }
                "--occt-root" if result.occt_root.is_none() => {
                    result.occt_root = Some(
                        args.next()
                            .context("--occt-root requires an SDK directory")?
                            .into(),
                    )
                }
                "--stage-licenses" if !result.stage_licenses => result.stage_licenses = true,
                "--computer-control" if !result.computer_control => result.computer_control = true,
                "--help" | "-h" if !result.help => result.help = true,
                _ => bail!(
                    "unknown or duplicate package option {arg}; use cargo xtask package --help"
                ),
            }
        }
        Ok(result)
    }
    fn validate(&self, os: &str, arch: &str) -> Result<()> {
        ensure!(
            os == "windows" || !self.computer_control,
            "--computer-control is only supported for Windows packages"
        );
        ensure!(
            os == "windows" || self.target.is_none(),
            "--target is only supported on Windows"
        );
        match os {
            "windows" => {
                windows::target(arch, self.target.as_deref())?;
                ensure!(
                    self.bundle.as_deref().is_none_or(|v| v == "portable"),
                    "Windows supports --bundle portable"
                );
                ensure!(
                    !self.stage_licenses,
                    "--stage-licenses is only supported on Linux"
                );
            }
            "linux" => {
                ensure!(arch == "x86_64", "Linux packages currently support x86_64");
                ensure!(
                    self.bundle
                        .as_deref()
                        .is_none_or(|v| matches!(v, "deb" | "appimage")),
                    "Linux supports --bundle deb or appimage"
                );
            }
            "macos" => {
                ensure!(
                    matches!(arch, "x86_64" | "aarch64"),
                    "unsupported macOS architecture {arch}"
                );
                ensure!(
                    self.bundle.as_deref().is_none_or(|v| v == "dmg"),
                    "macOS supports --bundle dmg"
                );
                ensure!(
                    !self.stage_licenses,
                    "--stage-licenses is only supported on Linux"
                );
            }
            _ => bail!("native packaging is supported on Windows, Linux and macOS"),
        }
        Ok(())
    }
}
pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let options = Options::parse(args)?;
    if options.help {
        println!("Native Rust packaging; no Node, npm or PowerShell bundler.\nUsage: cargo xtask package [--bundle portable|deb|appimage|dmg] [--target WINDOWS_TARGET]\n       [--occt-root SDK_DIRECTORY] [--stage-licenses] [--computer-control]\nDefaults: Windows portable ZIP, Linux DEB + AppImage, macOS app + DMG.\nWindows targets: x86_64-pc-windows-msvc, aarch64-pc-windows-msvc.\nWindows --computer-control enables guarded OS input for packaged keyboard qualification; default builds leave it disabled.\nInstall the host SDK and packaging/signing tools in docs/DEVELOPMENT.md.\nOCCT_ROOT, CARGO_TARGET_DIR and existing signing variables remain supported.");
        return Ok(());
    }
    options.validate(env::consts::OS, env::consts::ARCH)?;
    let context = common::Package::new()?;
    match env::consts::OS {
        "windows" => windows::build(&context, &options),
        "linux" => linux::build(&context, &options),
        "macos" => macos::build(&context, &options),
        _ => unreachable!(),
    }
}
pub fn verify_recipe_handler(args: impl Iterator<Item = String>) -> Result<()> {
    recipe_handler::run(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retired_bundlers_and_native_node_toolchains_cannot_return_silently() {
        let root = crate::release_tooling::root();
        for file in [
            "scripts/bundle-linux.mjs",
            "scripts/bundle-macos.mjs",
            "scripts/bundle-windows-portable.ps1",
            "scripts/stage-occt-macos.mjs",
            "scripts/desktop-package.mjs",
            "scripts/native-linux-package.mjs",
            "scripts/linux-xkb-runtime.mjs",
            "scripts/sync-version.mjs",
            "scripts/ci/check-release-tag.mjs",
            "scripts/capture-bevy-ui.mjs",
        ] {
            assert!(
                !root.join(file).exists(),
                "retired implementation returned: {file}"
            );
        }
        let forbidden = regex::Regex::new(r"setup-node|\bnpm\b|\bnode\b").unwrap();
        for file in [
            ".github/workflows/desktop-packages.yml",
            ".github/workflows/version-guard.yml",
            ".github/workflows/native-host-tests.yml",
            ".github/workflows/native-visual.yml",
            ".github/workflows/windows-native-ime.yml",
            "scripts/docker/ubuntu-26.04.Dockerfile",
            "scripts/docker/appimage-ubuntu-22.04.Dockerfile",
            "scripts/verify-linux-viewport.sh",
        ] {
            let text = std::fs::read_to_string(root.join(file)).unwrap();
            assert!(
                !forbidden.is_match(&text),
                "native toolchain depends on Node/npm again: {file}"
            );
        }
        assert!(!root.join("package.json").exists());
        assert!(!root.join("package-lock.json").exists());
    }
    #[test]
    fn dispatch_rejects_wrong_targets_and_bundles_before_any_build() {
        let parse = |args: &[&str]| Options::parse(args.iter().map(|v| (*v).into()));
        for args in [
            vec!["--target"],
            vec!["--bundle"],
            vec!["--bundle", "deb", "--bundle", "deb"],
            vec!["--release"],
            vec!["--computer-control", "--computer-control"],
        ] {
            assert!(parse(&args).is_err());
        }
        assert!(parse(&["--target", "aarch64-pc-windows-msvc"])
            .unwrap()
            .validate("linux", "x86_64")
            .is_err());
        assert!(parse(&["--bundle", "deb"])
            .unwrap()
            .validate("windows", "x86_64")
            .is_err());
        assert!(parse(&["--target", "x86_64-unknown-linux-gnu"])
            .unwrap()
            .validate("windows", "x86_64")
            .is_err());
        assert!(Options::default().validate("linux", "aarch64").is_err());
        for (os, arch) in [
            ("windows", "x86_64"),
            ("windows", "aarch64"),
            ("linux", "x86_64"),
            ("macos", "aarch64"),
        ] {
            Options::default().validate(os, arch).unwrap();
        }
        assert!(parse(&["--help"]).unwrap().help);
        let controlled = parse(&["--computer-control"]).unwrap();
        for arch in ["x86_64", "aarch64"] {
            controlled.validate("windows", arch).unwrap();
        }
        for (os, arch) in [("linux", "x86_64"), ("macos", "aarch64")] {
            assert!(controlled.validate(os, arch).is_err());
        }
    }
}
