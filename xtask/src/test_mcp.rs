use anyhow::{bail, Context, Result};
use std::{path::Path, process::Command};

/// Browser boundary fixtures retain their existing drivers. Native command
/// examples use the shared Rust interpreter and commented script files.
pub fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let suite = args.next().unwrap_or_else(|| "contracts".into());
    if suite == "playback" {
        return crate::playback_test::run(&args.collect::<Vec<_>>()).map_err(anyhow::Error::msg);
    }
    if suite == "scripts-workspace" {
        return crate::playback_test::run_workspace(&args.collect::<Vec<_>>())
            .map_err(anyhow::Error::msg);
    }
    if suite == "garden-bench" {
        return crate::replay::run(
            ["--recipe".to_owned(), "garden-bench".to_owned()]
                .into_iter()
                .chain(args),
        );
    }
    let script = match suite.as_str() {
        "contracts" => "contracts.mjs",
        "live" => "live.mjs",
        "controls" => "controls.mjs",
        "exit" => "exit.mjs",
        "bench" => "bench.mjs",
        "garden-bench" => "garden-bench.mjs",
        "drawing" => "drawing.mjs",
        _ => bail!("Unknown MCP suite '{suite}'; use contracts, live, controls, playback, scripts-workspace, exit, bench, garden-bench, or drawing"),
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
