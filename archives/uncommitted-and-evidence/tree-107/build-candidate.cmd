@echo off
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if errorlevel 1 exit /b %errorlevel%
set OCCT_ROOT=D:\limo-cad-maintenance\checked-occt-sdk
set CARGO_TARGET_DIR=D:\limo-startup-target-e4015de
set CARGO_BUILD_JOBS=1
set NBCAD_BUILD_REVISION=e4015de638b55823299d4a195e20a10ecbe9fae7
cd /d C:\Users\jeffg\.t3\worktrees\noBS-CAD\t3code-028ff4c7
cargo build --locked --manifest-path src-tauri\Cargo.toml --features custom-protocol --bin nbcad --config "profile.dev.opt-level=0" --config "profile.dev.package.'*'.opt-level=0" --config "profile.dev.package.'*'.debug=2"
