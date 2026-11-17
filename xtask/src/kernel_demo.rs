//! The Limo kernel's browser demo: one self-contained page.
//!
//! The page template and the WebAssembly glue live with the kernel
//! (`crates/kernel/examples/web.html` and `web.rs`). This command compiles
//! the glue, compresses the module and embeds it in the template.
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::io::Write as _;
use std::path::PathBuf;

pub fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let mut out: Option<PathBuf> = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--out" if out.is_none() => {
                out = Some(PathBuf::from(
                    args.next().context("--out needs a file path")?,
                ));
            }
            "--help" | "-h" => {
                println!(
                    "cargo xtask kernel-demo [--out FILE.html] (default: target/kernel-demo/limo-kernel-demo.html)\nRequires the wasm32-unknown-unknown Rust target."
                );
                return Ok(());
            }
            _ => bail!("Unknown or duplicate kernel-demo option {argument}"),
        }
    }
    let root = crate::build_tools::root();
    let build = root.join("target").join("kernel-demo");
    // Whole-program optimisation keeps the module small without slowing the
    // modelling down; these settings stay out of the workspace profile.
    let mut cargo = crate::build_tools::cargo();
    cargo
        .args([
            "build",
            "--release",
            "--locked",
            "-p",
            "limo-cad-kernel",
            "--example",
            "web",
        ])
        .args([
            "--no-default-features",
            "--target",
            "wasm32-unknown-unknown",
            "--target-dir",
        ])
        .arg(&build)
        .env("CARGO_PROFILE_RELEASE_LTO", "fat")
        .env("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", "1")
        .env("CARGO_PROFILE_RELEASE_PANIC", "abort")
        .env("CARGO_PROFILE_RELEASE_STRIP", "true");
    crate::build_tools::run(&mut cargo).context(
        "Build the kernel for the browser; install the target with cargo xtask bootstrap --wasm",
    )?;
    let module = build
        .join("wasm32-unknown-unknown")
        .join("release")
        .join("examples")
        .join("web.wasm");
    let wasm = std::fs::read(&module).with_context(|| format!("Read {}", module.display()))?;
    let mut packer = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    packer.write_all(&wasm)?;
    let packed = packer.finish()?;
    let template = root
        .join("crates")
        .join("kernel")
        .join("examples")
        .join("web.html");
    let page = std::fs::read_to_string(&template)
        .with_context(|| format!("Read {}", template.display()))?;
    for marker in ["__KERNEL_GZIP_BASE64__", "__KERNEL_BYTES__"] {
        if !page.contains(marker) {
            bail!("{} has no {marker} placeholder", template.display());
        }
    }
    let page = page
        .replace("__KERNEL_GZIP_BASE64__", &STANDARD.encode(&packed))
        .replace("__KERNEL_BYTES__", &wasm.len().to_string());
    let out = out.unwrap_or_else(|| build.join("limo-kernel-demo.html"));
    if let Some(folder) = out.parent() {
        std::fs::create_dir_all(folder)?;
    }
    std::fs::write(&out, &page).with_context(|| format!("Write {}", out.display()))?;
    println!(
        "{}\n{} KB page: {} KB of WebAssembly, {} KB compressed",
        out.display(),
        page.len() / 1024,
        wasm.len() / 1024,
        packed.len() / 1024
    );
    Ok(())
}
