//! Run transport/resource regressions without an OCCT SDK or Cargo target.
//! rustc --edition=2021 --test scripts/ci/test-mcp-boundaries.rs -o mcp-boundaries
//! ./mcp-boundaries
#![allow(dead_code)] // Transport entry points are consumed by the application.

#[path = "../../xtask/src/replay/bounded_log.rs"]
mod bounded_log;
#[path = "../../mcp-server/src/stdio_input.rs"]
mod input_pipe;
#[path = "../../mcp-server/src/desktop_process_reaper.rs"]
mod reaper;
