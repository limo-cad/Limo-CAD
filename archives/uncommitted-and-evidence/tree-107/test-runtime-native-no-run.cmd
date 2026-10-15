@echo off
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if errorlevel 1 exit /b %errorlevel%
set OCCT_ROOT=D:\limo-cad-maintenance\checked-occt-sdk
set CARGO_TARGET_DIR=D:\limo-startup-target-optimized
set CARGO_BUILD_JOBS=1
set NBCAD_BUILD_REVISION=5f9b6d75970b1d7a31137294deb4535dda8465b5
cd /d C:\Users\jeffg\.t3\worktrees\noBS-CAD\t3code-028ff4c7
set PATH=D:\limo-cad-maintenance\checked-occt-sdk\bin;%PATH%
cargo test --locked --manifest-path src-tauri\Cargo.toml --features custom-protocol --lib native_viewport:: --no-run

