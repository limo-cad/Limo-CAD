//! Qualify checked matrix allocation/copy behavior in the actual OCCT runtime.
use anyhow::{ensure, Context, Result};
use std::{env, fs, path::Path, process::Command};

pub fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    ensure!(
        args.next().as_deref() == Some("--prefix"),
        "use verify-occt-storage --prefix SDK"
    );
    let prefix = args.next().context("missing SDK path")?;
    ensure!(
        args.next().is_none(),
        "unexpected storage verification argument"
    );
    verify(Path::new(&prefix))
}

pub fn verify(prefix: &Path) -> Result<()> {
    let sdk = crate::build_tools::sdk::resolve(
        &[prefix.to_path_buf()],
        env::consts::OS,
        env::consts::ARCH,
        None,
    )
    .map_err(anyhow::Error::msg)?;
    verify_header(&sdk.include)?;
    let staging = tempfile::tempdir()?;
    let source = staging.path().join("source");
    fs::create_dir(&source)?;
    for (name, content) in [
        (
            "CMakeLists.txt",
            include_bytes!("../../native/occt-overlay/CMakeLists.txt").as_slice(),
        ),
        (
            "math-double-tab-check.cxx",
            include_bytes!("../../native/occt-overlay/math-double-tab-check.cxx").as_slice(),
        ),
    ] {
        fs::write(source.join(name), content)?;
    }
    let build = staging.path().join("build");
    let mut configure = Command::new("cmake");
    configure
        .arg("-S")
        .arg(&source)
        .arg("-B")
        .arg(&build)
        .args(["-G", "Ninja", "-DCMAKE_BUILD_TYPE=Release"])
        .arg(format!("-DOCCT_INCLUDE={}", sdk.include.display()))
        .arg(format!("-DOCCT_LIB={}", sdk.lib.display()));
    let mut compile = Command::new("cmake");
    compile.arg("--build").arg(&build).args(["--parallel", "1"]);
    #[cfg(windows)]
    select_msvc(&mut configure, &mut compile)?;
    crate::build_tools::run(&mut configure)?;
    crate::build_tools::run(&mut compile)?;
    let mut probe = Command::new(build.join(if cfg!(windows) {
        "limo_occt_storage_check.exe"
    } else {
        "limo_occt_storage_check"
    }));
    let variable = if cfg!(windows) {
        "PATH"
    } else if cfg!(target_os = "macos") {
        "DYLD_LIBRARY_PATH"
    } else {
        "LD_LIBRARY_PATH"
    };
    let mut search = vec![
        prefix.join("bin"),
        sdk.lib.parent().context("OCCT library parent")?.join("bin"),
        sdk.lib,
    ];
    if let Some(existing) = env::var_os(variable) {
        search.extend(env::split_paths(&existing));
    }
    probe.env(variable, env::join_paths(search)?);
    crate::build_tools::run(&mut probe)
}

#[cfg(windows)]
fn select_msvc(configure: &mut Command, compile: &mut Command) -> Result<()> {
    let target = format!("{}-pc-windows-msvc", env::consts::ARCH);
    let compiler = find_msvc_tools::find_tool(&target, "cl.exe")
        .with_context(|| format!("MSVC C++ compiler and Windows SDK required for {target}"))?;
    configure.arg(format!(
        "-DCMAKE_CXX_COMPILER:FILEPATH={}",
        compiler.path().display()
    ));
    for (name, value) in compiler.env() {
        configure.env(name, value);
        compile.env(name, value);
    }
    Ok(())
}

pub fn verify_header(include: &Path) -> Result<()> {
    let expected = include_str!("../../native/occt-overlay/opencascade/math_DoubleTab.lxx")
        .replace("\r\n", "\n");
    let installed = fs::read_to_string(include.join("math_DoubleTab.lxx"))?
        .replace("\r\n", "\n")
        .replace(
            "#include \"Standard_OutOfRange.hxx\"",
            "#include <Standard_OutOfRange.hxx>",
        );
    ensure!(installed == expected, "OCCT SDK lacks the reviewed checked matrix header; rebuild with cargo xtask build-occt or the pinned vcpkg overlay");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    #[ignore = "requires installed MSVC, Windows SDK, CMake and Ninja"]
    fn windows_probe_selects_native_msvc_over_ambient_cxx() {
        let staging = tempfile::tempdir().unwrap();
        fs::write(
            staging.path().join("CMakeLists.txt"),
            "cmake_minimum_required(VERSION 3.20)\nproject(compiler_probe LANGUAGES CXX)\nadd_executable(compiler_probe main.cpp)\n",
        )
        .unwrap();
        fs::write(
            staging.path().join("main.cpp"),
            "#include <windows.h>\n#include <array>\n#ifndef _MSC_VER\n#error MSVC compiler required\n#endif\nint main() { return GetCurrentProcessId() && std::array<int, 1>{0}[0] == 0 ? 0 : 1; }\n",
        )
        .unwrap();
        let build = staging.path().join("build");
        let mut configure = Command::new("cmake");
        configure
            .arg("-S")
            .arg(staging.path())
            .arg("-B")
            .arg(&build)
            .args(["-G", "Ninja", "-DCMAKE_BUILD_TYPE=Release"])
            .env("CXX", "must-not-select-ambient-CXX.exe");
        let mut compile = Command::new("cmake");
        compile.arg("--build").arg(&build);
        select_msvc(&mut configure, &mut compile).unwrap();
        crate::build_tools::run(&mut configure).unwrap();
        crate::build_tools::run(&mut compile).unwrap();
        crate::build_tools::run(&mut Command::new(build.join("compiler_probe.exe"))).unwrap();
    }

    #[test]
    fn checked_header_accepts_vcpkg_formatting_and_rejects_changed_storage() {
        let staging = tempfile::tempdir().unwrap();
        let header = staging.path().join("math_DoubleTab.lxx");
        let reviewed = include_str!("../../native/occt-overlay/opencascade/math_DoubleTab.lxx")
            .replace("\r\n", "\n");
        fs::write(&header, &reviewed).unwrap();
        verify_header(staging.path()).unwrap();
        let packaged = reviewed
            .replace(
                "#include <Standard_OutOfRange.hxx>",
                "#include \"Standard_OutOfRange.hxx\"",
            )
            .replace('\n', "\r\n");
        fs::write(&header, packaged).unwrap();
        verify_header(staging.path()).unwrap();
        fs::write(
            &header,
            reviewed.replace("const std::size_t count", "const int count"),
        )
        .unwrap();
        assert!(verify_header(staging.path()).is_err());
        fs::write(&header, "old storage header").unwrap();
        assert!(verify_header(staging.path()).is_err());
    }
}
