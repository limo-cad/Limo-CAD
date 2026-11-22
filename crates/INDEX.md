# crates/ index

Host-neutral Rust CAD crates (shared by UI / WASM / MCP where applicable).

| Crate | Role |
|-------|------|
| [`build-info/`](build-info/) | Compiled desktop/MCP identity; kept outside the geometry engine dependency graph |
| [`cam/`](cam/) | Host-neutral 3-axis CAM intent, validation, motion planning, volumetric stock simulation, and posts |
| [`kernel/`](kernel/) | Limo CAD's own exact B-rep kernel in pure Rust: extrude, revolve, sweep, loft, Booleans, holes, threads, fillets, chamfers, shell and patterns, on analytic and free-form (spline) surfaces, with names that survive history edits and edge marks that carry a project from one kernel to the other, and the services around them (drawings, sections, measurement, mass properties, interference, STEP in and out). Opt-in with `LIMO_CAD_KERNEL=limo`; also builds for the browser. Depends on nothing: no other crate here, none from outside |
| [`kernel-session/`](kernel-session/) | The Limo kernel behind the solid contract: runs a recompute plan's jobs on `kernel/`, resuming from the longest unchanged prefix, and returns the evaluated scene. The one place where the kernel meets the application's types; its `lab` example runs every bundled recipe on both kernels and compares the solids |
| [`interface/`](interface/) | Shared product catalog and rendered-control ownership, inspection, input and focus semantics |
| [`print/`](print/) | Reading scanned 2D prints for agents: plate outline calibration, millimetre crops with a grid and hole overlay, ring scores at model holes, hole-symbol detection with coverage matching |
| [`project-file/`](project-file/) | Project container compatibility, bounded reads, and atomic saves shared by native hosts |
| Other folder names under this directory | Geometry / document / history primitives |

MCP tool surface lives in [../mcp-server/](../mcp-server/), not here.
Agentic docs: [../docs/agentic/INDEX.md](../docs/agentic/INDEX.md).
