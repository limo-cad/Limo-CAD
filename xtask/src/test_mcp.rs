use anyhow::{bail, Context, Result};
use std::{path::Path, process::Command};

/// One supported test/demo entry point. The browser and stdio drivers remain
/// JavaScript because they exercise the product's browser and MCP boundaries.
pub fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let suite = args.next().unwrap_or_else(|| "contracts".into());
    let script = match suite.as_str() {
        "contracts" => "contracts.mjs",
        "live" => "live.mjs",
        "controls" => "controls.mjs",
        "exit" => "exit.mjs",
        "bench" => "bench.mjs",
        "garden-bench" => "garden-bench.mjs",
        "drawing" => "drawing.mjs",
        _ => bail!("Unknown MCP suite '{suite}'; use contracts, live, controls, exit, bench, garden-bench, or drawing"),
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let status = Command::new("node")
        .arg(root.join("xtask/mcp").join(script))
        .args(args)
        .current_dir(root)
        .status()
        .context("Run MCP suite (Node.js and npm ci are required)")?;
    if !status.success() {
        bail!("MCP {suite} suite failed ({status})");
    }
    Ok(())
}
