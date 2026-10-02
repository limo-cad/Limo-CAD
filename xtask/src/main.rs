//! Repo maintenance tasks for noBS CAD.
//!
//! ```text
//! cargo run -p xtask -- install-mcp --dry-run
//! cargo run -p xtask -- install-mcp --clients cursor,vscode --no-build
//! ```

mod install_mcp;
mod native_assembly_test;
mod native_body_appearance_test;
mod native_body_test;
mod native_build_test;
mod native_cam_geometry_test;
mod native_cam_nc_test;
mod native_cam_test;
mod native_drawing_annotations_test;
mod native_drawing_authoring_test;
mod native_drawing_editor_test;
mod native_drawing_hole_test;
mod native_drawing_navigation_test;
mod native_drawing_section_test;
mod native_drawing_test;
mod native_exchange_test;
mod native_fixture;
mod native_hole_test;
mod native_inspect_test;
mod native_joint_test;
mod native_lessons_test;
mod native_lifecycle_test;
mod native_mechanism_test;
mod native_move_test;
mod native_planes_test;
mod native_platform_test;
mod native_preferences_test;
mod native_profile_export_test;
mod native_refine_test;
mod native_sketch_test;
mod native_studies_test;
mod native_support_test;
mod native_switching_test;
mod native_thread_test;
mod native_view_test;
mod package;
mod package_mcp;
mod playback_test;
mod project_archive;
mod release_tooling;
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
        "verify-linux-recipe-handler" => package::verify_recipe_handler(args),
        "version" => release_tooling::version::run(args),
        "check-release-tag" => release_tooling::tag::run(args),
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
  version       Read VERSION; --check verifies all carriers and release notes;
                --sync updates carriers without changing historical release notes.
  check-release-tag TAG SHA
                Require a v<VERSION> tag on a commit already merged into main.
  package       Build and audit the host desktop package entirely through Rust.
                Use --help for prerequisites and optional Windows target selection.
  verify-linux-recipe-handler
                Verify the owned packaged recipe association; used by Linux package checks.
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
  test-mcp      Run contracts (default), live, controls, native-lifecycle, native-sketch, native-support, native-build, native-refine, native-body, native-pattern, native-view, native-thread, native-planes, playback, scripts-workspace, exit, bench, garden-bench, or drawing. Additional
                arguments pass directly to the selected MCP test/demo driver.
                Example: cargo xtask test-mcp live --server PATH --desktop PATH
                Native sketch: test-mcp native-sketch --server CAD_BINARY --session BLANK_DOCUMENT_UUID --out ABSOLUTE_PATH
                Native solid: test-mcp native-build --server CAD_BINARY --session BLANK_DOCUMENT_UUID --out ABSOLUTE_PATH
                Native lifecycle: test-mcp native-lifecycle --server CAD_BINARY --session BLANK_DOCUMENT_UUID --out ABSOLUTE_PATH
                Native drawing/lessons: test-mcp native-drawing (or native-lessons) with the same blank-session arguments.
                Native annotation preservation: test-mcp native-drawing-annotations with the same blank-session arguments.
                Native drawing editor: test-mcp native-drawing-editor with the same blank-session arguments.
                Native note/dimension authoring: test-mcp native-drawing-authoring with the same blank-session arguments.
                Native drilled-solid hole notes: test-mcp native-drawing-hole with the same blank-session arguments.
                Disposable Linux hole-note circle pick and leader drag: test-mcp native-drawing-hole-platform --desktop-input --server PATH --out ABSOLUTE_EMPTY_ROOT under Xvfb.
                Native manufacturing profile DXF: test-mcp native-profile-export with the same blank-session arguments.
                Disposable Linux paper input: test-mcp native-drawing-platform --desktop-input --server PATH --out ABSOLUTE_EMPTY_ROOT under Xvfb.
                Disposable Linux CAM row/WCS input: test-mcp native-cam-platform --desktop-input --server PATH --out ABSOLUTE_EMPTY_ROOT under Xvfb.
                Disposable Linux CAM geometry/linking input: test-mcp native-cam-geometry-platform with the same owned-window arguments.
                Disposable Linux chamfer picking/placement/drag: test-mcp native-chamfer-platform with the same owned-window arguments.
                Disposable Linux revision-cloud placement/drag: test-mcp native-cloud-platform with the same owned-window arguments.
                Disposable Linux drawing output and menu captures: test-mcp native-drawing-output-platform with the same owned-window arguments (does not drive a save dialog).
                Native drawing navigation: test-mcp native-drawing-navigation with --desktop-input (Windows OS gestures) or --mcp-only and the same isolated blank-session arguments.
                Native body appearance: test-mcp native-body-appearance with the same blank-session arguments.
                Native exchange: test-mcp native-exchange with the same blank-session arguments.
                Native CAM geometry: test-mcp native-cam-geometry with the same blank-session arguments.
                Native imported NC: test-mcp native-cam-nc with the same blank-session arguments and isolated NBCAD_CONFIG_DIR.
                Native application preferences: test-mcp native-preferences with the same blank-session arguments and isolated NBCAD_CONFIG_DIR.
                Disposable switching timings: test-mcp switching-measurement; see docs/native-switching-measurement.md for matched archives and receipt limits.
                Both save editable models and window PNGs for visual review.
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
