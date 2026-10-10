use super::{
    common::{self, Package},
    Options,
};
use anyhow::{bail, ensure, Context, Result};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

#[path = "provenance.rs"]
mod provenance;

pub(super) struct Target {
    pub triple: &'static str,
    pub arch: &'static str,
    pub triplet: &'static str,
}
pub(super) fn target(arch: &str, selected: Option<&str>) -> Result<Target> {
    match selected.unwrap_or(match arch {
        "x86_64" => "x86_64-pc-windows-msvc",
        "aarch64" => "aarch64-pc-windows-msvc",
        _ => "unsupported",
    }) {
        "x86_64-pc-windows-msvc" => Ok(Target {
            triple: "x86_64-pc-windows-msvc",
            arch: "x64",
            triplet: "x64-windows",
        }),
        "aarch64-pc-windows-msvc" => Ok(Target {
            triple: "aarch64-pc-windows-msvc",
            arch: "arm64",
            triplet: "arm64-windows",
        }),
        other => bail!("unsupported Windows target {other}"),
    }
}
pub(super) fn build(package: &Package, options: &Options) -> Result<()> {
    let target = target(env::consts::ARCH, options.target.as_deref())?;
    let source = provenance::read(&package.root)?;
    ensure!(
        !options.computer_control || !source.modified,
        "Packaged computer control requires a clean source tree; commit build inputs first"
    );
    let sdk = options
        .occt_root
        .clone()
        .or_else(|| env::var_os("OCCT_ROOT").map(PathBuf::from))
        .unwrap_or_else(|| package.root.join("vcpkg_installed").join(target.triplet))
        .canonicalize()
        .context("resolve Windows OCCT SDK")?;
    let target_arch = if target.arch == "x64" {
        "x86_64"
    } else {
        "aarch64"
    };
    if target_arch == env::consts::ARCH {
        crate::occt_storage::verify(&sdk)?;
    } else {
        let layout = crate::build_tools::sdk::resolve(
            std::slice::from_ref(&sdk),
            "windows",
            target_arch,
            None,
        )
        .map_err(anyhow::Error::msg)?;
        crate::occt_storage::verify_header(&layout.include)?;
        println!("Cross-built packages require checked-runtime qualification on the target before publication.");
    }
    runtime_bin(&sdk)?;
    common::run(&mut build_command(package, options, &target, &sdk, &source))?;
    ensure!(
        provenance::read(&package.root)? == source,
        "Source revision or modified state changed while building the Windows package"
    );
    let release = package.target.join(target.triple).join("release");
    stage(
        package,
        &target,
        &release.join("limo-cad.exe"),
        &sdk,
        &release.join("bundle/portable"),
        &source,
        options.computer_control,
    )
}

fn build_command(
    package: &Package,
    options: &Options,
    target: &Target,
    sdk: &Path,
    source: &provenance::Source,
) -> std::process::Command {
    let mut command = package.cargo();
    command
        .args(["--target", target.triple])
        .env("LIMO_CAD_BUILD_REVISION", &source.revision)
        .env("OCCT_ROOT", sdk)
        .env("VCPKG_TARGET_TRIPLET", target.triplet);
    if options.computer_control {
        command.args(["--features", "native-computer-control"]);
    }
    command
}

pub(crate) fn runtime_bin(sdk: &Path) -> Result<PathBuf> {
    [
        "bin",
        "win64/vc17/bin",
        "win64/vc16/bin",
        "win64/vc15/bin",
        "win64/vc14/bin",
    ]
    .iter()
    .map(|name| sdk.join(name))
    .find(|path| path.join("TKernel.dll").is_file())
    .context("TKernel.dll missing from OCCT SDK")
}
fn stage(
    package: &Package,
    target: &Target,
    executable: &Path,
    sdk: &Path,
    output: &Path,
    source: &provenance::Source,
    computer_control: bool,
) -> Result<()> {
    ensure!(executable.is_file(), "Cargo did not produce limo-cad.exe");
    let name = format!(
        "Limo-CAD-{}-windows-{}{}",
        package.version,
        target.arch,
        if computer_control {
            "-computer-control"
        } else {
            ""
        }
    );
    let directory = common::fresh_child(output, &name)?;
    let count = stage_runtime(
        &package.root,
        executable,
        sdk,
        &runtime_bin(sdk)?,
        &directory,
    )?;
    let commit = source.stamp();
    let capability = if computer_control {
        "Guarded native mouse/keyboard computer control is ENABLED in this opt-in build."
    } else {
        "Native mouse/keyboard computer control is not included in this default build."
    };
    fs::write(
        directory.join("package-manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "version": package.version,
            "target": target.triple,
            "source_revision": source.revision,
            "source_modified": source.modified,
            "native_computer_control": computer_control,
            "executable_sha256": common::sha256(&directory.join("Limo-CAD.exe"))?,
        }))?,
    )?;
    fs::write(directory.join("README.txt"), format!("Limo CAD {} - Windows {} portable build\n\nRun Limo-CAD.exe directly; no installation is required.\n\nLocal stdio MCP is always available. A normal launch opens the CAD window.\n{capability}\nUse args [\"--headless\"] for an agent worker without an extra window.\nKeep the DLLs beside the executable; no separate server or OCCT SDK is required.\n\nSystem requirements:\n- Windows 10 version 1803 or newer, or Windows 11\n- Microsoft Visual C++ v14 {} Redistributable\n  https://aka.ms/vc14/vc_redist.{}.exe\n- A graphics adapter and driver accepted by wgpu's DX12 or Vulkan backend\n\nThe Visual C++ runtime is intentionally not bundled. Install the centrally\nserviced Microsoft Redistributable for security and servicing updates.\n\nSource: https://github.com/limo-cad/Limo-CAD\nSource commit: {commit}\n", package.version, target.arch, target.arch, target.arch))?;
    let zip = output.join(format!("{name}.zip"));
    common::zip_directory(&directory, &zip)?;
    common::checksum(&zip)?;
    println!("Packaged {count} runtime DLLs");
    Ok(())
}

/// Stage the same executable, OCCT runtime and notices for packages and local installs.
pub(crate) fn stage_runtime(
    root: &Path,
    executable: &Path,
    sdk: &Path,
    bin: &Path,
    directory: &Path,
) -> Result<usize> {
    common::ordinary_file(executable)?;
    common::ordinary_directory(sdk)?;
    common::ordinary_directory(bin)?;
    fs::create_dir_all(directory)?;
    common::ordinary_directory(directory)?;
    fs::copy(executable, directory.join("Limo-CAD.exe"))?;
    let mut count = 0;
    for entry in fs::read_dir(bin)? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("dll"))
        {
            common::ordinary_file(&entry.path())?;
            fs::copy(entry.path(), directory.join(entry.file_name()))?;
            count += 1;
        }
    }
    ensure!(count > 0, "SDK contains no runtime DLLs");
    for name in ["TKernel.dll", "TKDESTEP.dll", "TKFillet.dll", "TKHLR.dll"] {
        ensure!(
            directory.join(name).is_file(),
            "required OCCT runtime library missing: {name}"
        );
    }
    let licenses = directory.join("licenses");
    fs::create_dir_all(&licenses)?;
    common::ordinary_directory(&licenses)?;
    common::ordinary_file(&root.join("LICENSE"))?;
    common::ordinary_file(&root.join("THIRD_PARTY_NOTICES.md"))?;
    fs::copy(root.join("LICENSE"), licenses.join("Limo-CAD-LICENSE.txt"))?;
    fs::copy(
        root.join("THIRD_PARTY_NOTICES.md"),
        licenses.join("THIRD_PARTY_NOTICES.md"),
    )?;
    fn copyrights(source: &Path, licenses: &Path) -> Result<()> {
        common::ordinary_directory(source)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                copyrights(&entry.path(), licenses)?;
            } else if entry.file_name() == "copyright" {
                let path = entry.path();
                common::ordinary_file(&path)?;
                let port = path
                    .parent()
                    .and_then(Path::file_name)
                    .context("vcpkg port name")?
                    .to_string_lossy();
                fs::copy(&path, licenses.join(format!("vcpkg-{port}.txt")))?;
            }
        }
        Ok(())
    }
    copyrights(&sdk.join("share"), &licenses)?;
    ensure!(
        licenses.join("vcpkg-opencascade.txt").is_file(),
        "vcpkg OpenCASCADE license notice missing"
    );
    Ok(count)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn both_windows_packages_preserve_the_opt_in_control_feature_and_target() {
        let root = crate::release_tooling::root().to_path_buf();
        let package = Package {
            desktop: root.join("desktop"),
            target: root.join("desktop/target"),
            root,
            version: "0.2.2".into(),
        };
        let source = provenance::Source {
            revision: "test-revision".into(),
            modified: false,
        };
        for arch in ["x86_64", "aarch64"] {
            let target = super::target(arch, None).unwrap();
            for computer_control in [false, true] {
                let options = Options {
                    computer_control,
                    ..Options::default()
                };
                let command = build_command(&package, &options, &target, &package.root, &source);
                let args: Vec<_> = command.get_args().map(|a| a.to_string_lossy()).collect();
                assert!(args.windows(2).any(|a| a == ["--target", target.triple]));
                assert_eq!(
                    args.windows(2)
                        .any(|a| a == ["--features", "native-computer-control"]),
                    computer_control
                );
                assert!(args.iter().any(|a| a == "--release"));
                assert!(args.iter().any(|a| a == "--locked"));
            }
        }
    }

    #[test]
    fn windows_package_requires_runtime_and_license_closure() {
        let temp = tempfile::tempdir().unwrap();
        let resolved = temp.path().canonicalize().unwrap();
        let root = resolved.as_path();
        fs::write(root.join("LICENSE"), "license").unwrap();
        fs::write(root.join("THIRD_PARTY_NOTICES.md"), "notices").unwrap();
        let sdk = root.join("sdk");
        let bin = sdk.join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(sdk.join("share/opencascade")).unwrap();
        fs::write(sdk.join("share/opencascade/copyright"), "OCCT").unwrap();
        for name in ["TKernel.dll", "TKDESTEP.dll", "TKFillet.dll"] {
            fs::write(bin.join(name), "DLL").unwrap();
        }
        let exe = root.join("limo-cad.exe");
        fs::write(&exe, "exe").unwrap();
        let package = Package {
            root: root.to_owned(),
            desktop: root.into(),
            target: root.into(),
            version: "0.3.0-rc.1".into(),
        };
        let target = target("x86_64", None).unwrap();
        let output = root.join("output");
        let source = provenance::Source {
            revision: "123456789abcdef0123456789abcdef0123456789a".into(),
            modified: true,
        };
        assert!(stage(&package, &target, &exe, &sdk, &output, &source, false).is_err());
        fs::write(bin.join("TKHLR.dll"), "DLL").unwrap();
        stage(&package, &target, &exe, &sdk, &output, &source, false).unwrap();
        let readme =
            fs::read_to_string(output.join("Limo-CAD-0.3.0-rc.1-windows-x64/README.txt")).unwrap();
        assert!(readme.contains(&format!("Source commit: {}\n", source.stamp())));
        assert!(!readme.contains("local working tree"));
        let mut archive = zip::ZipArchive::new(
            fs::File::open(output.join("Limo-CAD-0.3.0-rc.1-windows-x64.zip")).unwrap(),
        )
        .unwrap();
        assert!(archive
            .by_name("Limo-CAD-0.3.0-rc.1-windows-x64/licenses/vcpkg-opencascade.txt")
            .is_ok());
        assert!(archive
            .by_name("Limo-CAD-0.3.0-rc.1-windows-x64/TKHLR.dll")
            .is_ok());
        drop(archive);
        for arch in ["x86_64", "aarch64"] {
            let target = super::target(arch, None).unwrap();
            for control in [false, true] {
                stage(&package, &target, &exe, &sdk, &output, &source, control).unwrap();
                let name = format!(
                    "Limo-CAD-0.3.0-rc.1-windows-{}{}",
                    target.arch,
                    if control { "-computer-control" } else { "" }
                );
                let mut zip = zip::ZipArchive::new(
                    fs::File::open(output.join(format!("{name}.zip"))).unwrap(),
                )
                .unwrap();
                let manifest: serde_json::Value = serde_json::from_reader(
                    zip.by_name(&format!("{name}/package-manifest.json"))
                        .unwrap(),
                )
                .unwrap();
                assert_eq!(manifest["native_computer_control"], control);
                assert_eq!(manifest["target"], target.triple);
                assert_eq!(manifest["source_revision"], source.revision);
                assert_eq!(manifest["source_modified"], source.modified);
                assert_eq!(manifest["executable_sha256"], common::sha256(&exe).unwrap());
                let text = fs::read_to_string(output.join(&name).join("README.txt")).unwrap();
                assert!(text.contains(if control {
                    "ENABLED in this opt-in build"
                } else {
                    "not included in this default build"
                }));
            }
        }
    }
}
