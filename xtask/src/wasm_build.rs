//! One host-independent entry point for the browser's Rust engine bundle.
use anyhow::{bail, Context, Result};
use std::process::Command;

pub fn smoke(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("Use cargo xtask smoke-wasm (requires Chrome and wasm-pack)");
    }
    let root = crate::build_tools::root();
    crate::build_tools::require_tool("wasm-pack")?;
    let status = Command::new("wasm-pack")
        .current_dir(root)
        .args(["test", "--headless", "--chrome", "crates/wasm", "--locked"])
        .status()
        .context("Run browser WASM smoke tests; install wasm-pack and Chrome")?;
    if !status.success() {
        bail!("Browser WASM smoke tests failed ({status})");
    }
    Ok(())
}

pub fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let mut profile = None;
    for argument in args {
        match argument.as_str() {
            "--dev" | "--release" if profile.is_none() => profile = Some(argument),
            "--help" | "-h" => {
                println!("cargo xtask build-wasm [--dev|--release] (default: release)\nRequires wasm-pack and the wasm32-unknown-unknown Rust target.");
                return Ok(());
            }
            _ => bail!("Unknown or duplicate build-wasm option {argument}"),
        }
    }
    let root = crate::build_tools::root();
    crate::build_tools::require_tool("wasm-pack")?;
    let status = Command::new("wasm-pack")
        .current_dir(root)
        .args([
            "build",
            "crates/wasm",
            "--target",
            "web",
            "--out-dir",
            "../../web/engine",
            "--out-name",
            "limo_cad_wasm",
            "--no-pack",
        ])
        .arg(profile.as_deref().unwrap_or("--release"))
        .args(["--", "--locked"])
        .status()
        .context("Run wasm-pack; install it with cargo xtask bootstrap --wasm")?;
    if !status.success() {
        bail!("Browser engine build failed ({status})");
    }
    Ok(())
}
