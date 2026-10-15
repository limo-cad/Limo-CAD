//! Repo maintenance tasks for noBS CAD.
//!
//! ```text
//! cargo run -p xtask -- install-mcp --dry-run
//! cargo run -p xtask -- install-mcp --clients cursor,vscode --no-build
//! ```

mod install_mcp;
mod native_assembly_test;
mod native_body_test;
mod native_build_test;
mod native_fixture;
mod native_hole_test;
mod native_inspect_test;
mod native_joint_test;
mod native_mechanism_test;
mod native_move_test;
mod native_planes_test;
mod native_refine_test;
mod native_sketch_test;
mod native_studies_test;
mod native_support_test;
mod native_thread_test;
mod native_view_test;
mod package;
mod package_mcp;
mod playback_test;
mod project_archive;
mod replay;
mod test_mcp;

use anyhow::{bail, Result};
use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        print_usage();
        bail!("missing command");
    };

    match command.as_str() {
        "package" => package::run(args),
        "run-script" => replay::run(args),
        "cad-call" => replay::call(args),
        "verify-package-mcp" => package_mcp::run(args),
        "test-mcp" => test_mcp::run(args),
        "install-mcp" => {
            let options = install_mcp::Options::parse(args)?;
            install_mcp::run(options)
        }
        "help" | "--help" | "-h" => {
            print_usage();
            Ok(())
        }
        other => {
            print_usage();
            bail!("unknown command '{other}'");
        }
    }
}

fn print_usage() {
    eprintln!(
        "\
noBS CAD xtask

Usage:
  cargo xtask package
  cargo run -p xtask -- install-mcp --dry-run
  cargo run -p xtask -- install-mcp --clients LIST [--no-build] [--binary PATH]

Commands:
  package       Build the host desktop package using the existing platform bundler.
                Use --help for prerequisites and optional Windows target selection.
  run-script    Run a .nbcad.jsonc file or --recipe ID using the Rust MCP client. Use --server PATH,
                plus --server-arg --headless for packaged workers without a window. Repeat --server-arg for literal arguments.
                --init-timeout-seconds N bounds startup only (default: 30); modeling waits remain unbounded.
                --session UUID --new --present to replay in an existing window.
                --repeat 2 verifies independent headless runs are deterministic.
                Use run-script --help for all options.
  cad-call      Send one MCP command from Rust (--tool NAME --args JSON).
                Accepts the same server arguments and initialization timeout; use cad-call --help.
  verify-package-mcp
                Verify a packaged executable over stdio without launching a GUI:
                --server PATH --server-arg --headless [--out REPORT.json]
                Repeat --server-arg for additional executable arguments.
                --timeout-seconds N bounds each request (default: 120).
                --desktop also checks default stdio in one owned GUI, save, disconnect and guarded exit.
  test-mcp      Run contracts (default), live, controls, native-sketch, native-support, native-build, native-refine, native-body, native-pattern, native-view, native-thread, native-planes, playback, scripts-workspace, exit, bench, garden-bench, or drawing. Additional
                arguments pass directly to the selected MCP test/demo driver.
                Example: cargo xtask test-mcp live --server PATH --desktop PATH
                Native sketch: test-mcp native-sketch --server CAD_BINARY --session BLANK_DOCUMENT_UUID --out ABSOLUTE_PATH
                Native solid: test-mcp native-build --server CAD_BINARY --session BLANK_DOCUMENT_UUID --out ABSOLUTE_PATH
  install-mcp   Detect installed agent clients and upsert the local nbcad-mcp
                stdio server into each client's user config (Cursor, VS Code,
                Claude, OpenCode).

Options for install-mcp:
  --dry-run           Discover/print only — zero build, copy, or config write
  --no-build          Do not cargo-build the MCP server (use existing binary)
  --binary PATH       Explicit path to nbcad-mcp (skips default discovery)
  --clients LIST      Required for writes. Comma-separated:
                      cursor,vscode,claude,opencode
  --server-name NAME  Config key (default: nobs-cad)

Docs: docs/agentic/INSTALL_MCP.md
"
    );
}
