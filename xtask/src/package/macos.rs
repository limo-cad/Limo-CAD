use super::{
    common::{self, Package},
    Options,
};
use anyhow::{ensure, Context, Result};
use std::{
    collections::{BTreeMap, VecDeque},
    env, fs,
    path::{Path, PathBuf},
};

const ENTRY_LIBRARIES: &[&str] = &[
    "TKDESTEP",
    "TKXSBase",
    "TKDE",
    "TKFillet",
    "TKHLR",
    "TKOffset",
    "TKBool",
    "TKBO",
    "TKShHealing",
    "TKPrim",
    "TKTopAlgo",
    "TKMesh",
    "TKBRep",
    "TKGeomAlgo",
    "TKGeomBase",
    "TKG3d",
    "TKG2d",
    "TKMath",
    "TKernel",
];
fn system(name: &str) -> bool {
    name.starts_with("/usr/lib/") || name.starts_with("/System/Library/")
}
fn dependencies(package: &Package, path: &Path) -> Result<Vec<String>> {
    let text = common::output(package.command("otool").arg("-L").arg(path))?;
    Ok(parse_dependencies(&text))
}
fn parse_dependencies(text: &str) -> Vec<String> {
    text.lines()
        .skip(1)
        .map(|v| v.trim().split(" (compatibility version").next().unwrap())
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
        .collect()
}
fn name(path: &Path) -> Result<&str> {
    path.file_name()
        .and_then(|v| v.to_str())
        .context("UTF-8 dylib name")
}
fn resolve(sdk: &Path, dependency: &str, parent: &Path) -> Result<PathBuf> {
    let candidates = if dependency.starts_with("@rpath/") {
        let basename = name(Path::new(dependency))?;
        vec![
            sdk.join("lib").join(basename),
            parent.parent().context("dylib parent")?.join(basename),
            Path::new("/opt/homebrew/opt/tbb/lib").join(basename),
            Path::new("/usr/local/opt/tbb/lib").join(basename),
        ]
    } else if let Some(relative) = dependency.strip_prefix("@loader_path/") {
        vec![parent.parent().context("dylib parent")?.join(relative)]
    } else {
        vec![dependency.into()]
    };
    candidates
        .into_iter()
        .find(|p| p.is_file())
        .with_context(|| {
            format!(
                "cannot resolve {dependency}, required by {}",
                parent.display()
            )
        })
}

fn stage(package: &Package, options: &Options) -> Result<Vec<String>> {
    let mut candidates = Vec::new();
    if let Some(root) = &options.occt_root {
        ensure!(
            root.join("lib/libTKernel.dylib").is_file(),
            "explicit OCCT SDK is missing lib/libTKernel.dylib: {}",
            root.display()
        );
        candidates.push(root.clone());
    } else {
        if let Some(root) = env::var_os("OCCT_ROOT") {
            candidates.push(root.into());
        }
        candidates.extend(
            [
                "/opt/homebrew/opt/opencascade",
                "/usr/local/opt/opencascade",
                "/opt/opencascade",
            ]
            .into_iter()
            .map(PathBuf::from),
        );
    }
    let sdk = candidates
        .into_iter()
        .find(|p| p.join("lib/libTKernel.dylib").is_file())
        .context("OCCT 7.9 SDK missing; set OCCT_ROOT")?;
    crate::occt_storage::verify(&sdk)?;
    let mut queue: VecDeque<_> = ENTRY_LIBRARIES
        .iter()
        .map(|library| sdk.join("lib").join(format!("lib{library}.dylib")))
        .collect();
    let mut libraries = BTreeMap::new();
    while let Some(source) = queue.pop_front() {
        ensure!(
            source.is_file(),
            "required OCCT library missing: {}",
            source.display()
        );
        let ids = common::output(package.command("otool").arg("-D").arg(&source))?;
        let id = ids
            .lines()
            .nth(1)
            .map(str::trim)
            .context("dylib has no install ID")?;
        let output_name = name(Path::new(id))?.to_owned();
        let resolved_source = source.canonicalize()?;
        if let Some(previous) = libraries.get(&output_name) {
            ensure!(
                previous == &resolved_source,
                "conflicting dylib install IDs: {output_name}"
            );
            continue;
        }
        libraries.insert(output_name, resolved_source);
        for dependency in dependencies(package, &source)? {
            if !system(&dependency) && dependency != id {
                queue.push_back(resolve(&sdk, &dependency, &source)?);
            }
        }
    }
    let stage = common::fresh_child(&package.desktop, "occt-libs")?;
    let licenses = stage.join("licenses");
    fs::create_dir(&licenses)?;
    for (source, destination) in [
        ("LICENSE_LGPL_21.txt", "OCCT-LGPL-2.1.txt"),
        ("OCCT_LGPL_EXCEPTION.txt", "OCCT_LGPL_EXCEPTION.txt"),
    ] {
        fs::copy(
            sdk.join("share/doc/opencascade").join(source),
            licenses.join(destination),
        )?;
    }
    for (output_name, source) in &libraries {
        let destination = stage.join(output_name);
        fs::copy(source, &destination)?;
        common::executable(&destination)?;
        common::run(
            package
                .command("install_name_tool")
                .args(["-id", &format!("@rpath/{output_name}")])
                .arg(&destination),
        )?;
        for dependency in dependencies(package, &destination)? {
            if !system(&dependency) && dependency != format!("@rpath/{output_name}") {
                let linked_name = name(Path::new(&dependency))?;
                ensure!(
                    libraries.contains_key(linked_name),
                    "unbundled dylib dependency {dependency}"
                );
                common::run(
                    package
                        .command("install_name_tool")
                        .args(["-change", &dependency, &format!("@rpath/{linked_name}")])
                        .arg(&destination),
                )?;
            }
        }
    }
    for library in ENTRY_LIBRARIES {
        let link = format!("lib{library}.dylib");
        let versioned = libraries
            .keys()
            .find(|name| name.starts_with(&format!("lib{library}.")))
            .with_context(|| format!("missing staged ABI library {library}"))?;
        if versioned != &link {
            common::symlink(Path::new(versioned), &stage.join(link))?;
        }
    }
    let names: Vec<_> = libraries.keys().cloned().collect();
    fs::write(
        stage.join("libraries.json"),
        format!("{}\n", serde_json::to_string_pretty(&names)?),
    )?;
    for library in &names {
        for dependency in dependencies(package, &stage.join(library))? {
            ensure!(
                system(&dependency) || dependency.starts_with("@rpath/"),
                "{library} retains non-portable dependency {dependency}"
            );
        }
    }
    println!(
        "Staged {} OCCT/TBB dylibs from {}",
        names.len(),
        sdk.display()
    );
    Ok(names)
}

pub(super) fn build(package: &Package, options: &Options) -> Result<()> {
    let identity = env::var("APPLE_SIGNING_IDENTITY")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "-".into());
    let production = identity != "-";
    if production {
        for key in ["APPLE_API_KEY_PATH", "APPLE_API_KEY", "APPLE_API_ISSUER"] {
            ensure!(
                env::var(key).is_ok_and(|v| !v.is_empty()),
                "{key} required for production notarization"
            );
        }
    }
    let libraries = stage(package, options)?;
    let stage = package.desktop.join("occt-libs");
    let mut cargo = package.cargo();
    cargo.env("LIMO_CAD_OCCT_LIB_DIR", &stage);
    if let Some(sdk) = &options.occt_root {
        cargo.env("OCCT_ROOT", sdk);
    }
    common::run(&mut cargo)?;
    let bundle = package.target.join("release/bundle");
    let app = common::fresh_child(&bundle.join("macos"), "Limo CAD.app")?;
    let contents = app.join("Contents");
    for directory in ["MacOS", "Resources", "Frameworks"] {
        fs::create_dir_all(contents.join(directory))?;
    }
    let executable = contents.join("MacOS/limo-cad");
    fs::copy(package.target.join("release/limo-cad"), &executable)?;
    common::executable(&executable)?;
    fs::copy(
        package.desktop.join("icons/icon.icns"),
        contents.join("Resources/icon.icns"),
    )?;
    package.notices(&contents.join("Resources/licenses"))?;
    common::copy_tree(
        &stage.join("licenses"),
        &contents.join("Resources/licenses"),
    )?;
    for library in &libraries {
        fs::copy(
            stage.join(library),
            contents.join("Frameworks").join(library),
        )?;
    }
    fs::write(contents.join("Info.plist"), format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\n<key>CFBundleIdentifier</key><string>org.limocad.desktop</string>\n<key>CFBundleName</key><string>Limo CAD</string>\n<key>CFBundleDisplayName</key><string>Limo CAD</string>\n<key>CFBundleExecutable</key><string>limo-cad</string>\n<key>CFBundleIconFile</key><string>icon.icns</string>\n<key>CFBundlePackageType</key><string>APPL</string>\n<key>CFBundleShortVersionString</key><string>{}</string>\n<key>CFBundleVersion</key><string>{}</string>\n<key>LSMinimumSystemVersion</key><string>12.0</string>\n<key>NSHighResolutionCapable</key><true/>\n<key>CFBundleURLTypes</key><array><dict><key>CFBundleURLName</key><string>Limo CAD recipe</string><key>CFBundleURLSchemes</key><array><string>limo-cad</string><string>nbcad</string></array></dict></array>\n</dict></plist>\n", package.version, package.version))?;
    let commands = common::output(package.command("otool").arg("-l").arg(&executable))?;
    if !commands.contains("@executable_path/../Frameworks") {
        common::run(
            package
                .command("install_name_tool")
                .args(["-add_rpath", "@executable_path/../Frameworks"])
                .arg(&executable),
        )?;
    }
    for dependency in dependencies(package, &executable)? {
        if system(&dependency) {
            continue;
        }
        let linked_name = name(Path::new(&dependency))?;
        ensure!(
            libraries.iter().any(|v| v == linked_name),
            "unbundled executable dependency {dependency}"
        );
        if !dependency.starts_with("@rpath/") {
            common::run(
                package
                    .command("install_name_tool")
                    .args(["-change", &dependency, &format!("@rpath/{linked_name}")])
                    .arg(&executable),
            )?;
        }
    }
    let sign = |path: &Path| -> Result<()> {
        let mut command = package.command("codesign");
        command.args(["--force", "--sign", &identity]);
        if production {
            command.args(["--options", "runtime", "--timestamp"]);
        }
        common::run(command.arg(path))
    };
    for library in &libraries {
        sign(&contents.join("Frameworks").join(library))?;
    }
    sign(&app)?;
    common::run(
        package
            .command("codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&app),
    )?;
    let notarize = |path: &Path| -> Result<()> {
        common::run(
            package
                .command("xcrun")
                .args(["notarytool", "submit"])
                .arg(path)
                .arg("--key")
                .arg(env::var("APPLE_API_KEY_PATH")?)
                .arg("--key-id")
                .arg(env::var("APPLE_API_KEY")?)
                .arg("--issuer")
                .arg(env::var("APPLE_API_ISSUER")?)
                .arg("--wait"),
        )
    };
    if production {
        let temp = tempfile::tempdir_in(bundle.join("macos"))?;
        let archive = temp.path().join("notarization.zip");
        common::run(
            package
                .command("ditto")
                .args(["-c", "-k", "--keepParent"])
                .arg(&app)
                .arg(&archive),
        )?;
        notarize(&archive)?;
        common::run(
            package
                .command("xcrun")
                .args(["stapler", "staple"])
                .arg(&app),
        )?;
    }
    let output = bundle.join("dmg");
    fs::create_dir_all(&output)?;
    let arch = if env::consts::ARCH == "aarch64" {
        "aarch64"
    } else {
        "x64"
    };
    let dmg = output.join(format!("Limo.CAD_{}_{arch}.dmg", package.version));
    {
        let temp = tempfile::tempdir()?;
        common::copy_tree(&app, &temp.path().join("Limo CAD.app"))?;
        common::symlink(
            Path::new("/Applications"),
            &temp.path().join("Applications"),
        )?;
        common::run(
            package
                .command("hdiutil")
                .args(["create", "-volname", "Limo CAD", "-srcfolder"])
                .arg(temp.path())
                .args(["-ov", "-format", "UDZO"])
                .arg(&dmg),
        )?;
    }
    if production {
        sign(&dmg)?;
        notarize(&dmg)?;
        common::run(
            package
                .command("xcrun")
                .args(["stapler", "staple"])
                .arg(&dmg),
        )?;
    }
    common::run(package.command("hdiutil").arg("verify").arg(&dmg))?;
    common::checksum(&dmg)?;
    println!("Native app: {}", app.display());
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dependency_parser_retains_spaces_and_system_paths() {
        let result = parse_dependencies("/some path/libTK.dylib:\n\t@rpath/libTK.7.9.dylib (compatibility version 7.9.0, current version 7.9.0)\n\t/System/Library/Frameworks/Foundation.framework/Versions/C/Foundation (compatibility version 300.0.0, current version 1.0.0)\n");
        assert_eq!(result[0], "@rpath/libTK.7.9.dylib");
        assert!(system(&result[1]));
    }
}
