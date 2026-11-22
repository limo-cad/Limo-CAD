# crates/ index

Host-neutral Rust CAD crates (shared by UI / WASM / MCP where applicable).

| Crate | Role |
|-------|------|
| [`build-info/`](build-info/) | Compiled desktop/MCP identity; kept outside the geometry engine dependency graph |
| [`cam/`](cam/) | Host-neutral 3-axis CAM intent, validation, motion planning, volumetric stock simulation, and posts |
| [`kernel-session/`](kernel-session/) | The [Limo kernel](https://github.com/limo-cad/limo-kernel) behind the solid contract. The kernel is Limo CAD's own exact B-rep kernel in pure Rust, kept in its own repository and taken here at one pinned commit (`limo-cad-kernel` in the root `Cargo.toml`); it is opt-in with `LIMO_CAD_KERNEL=limo`. This crate runs a recompute plan's jobs on it, resuming from the longest unchanged prefix, and returns the evaluated scene. It is the one place where the kernel meets the application's types; its `lab` example runs every bundled recipe on both kernels and compares the solids |
| [`interface/`](interface/) | Shared product catalog and rendered-control ownership, inspection, input and focus semantics |
| [`print/`](print/) | Reading scanned 2D prints for agents: plate outline calibration, millimetre crops with a grid and hole overlay, ring scores at model holes, hole-symbol detection with coverage matching |
| [`project-file/`](project-file/) | Project container compatibility, bounded reads, and atomic saves shared by native hosts |
| Other folder names under this directory | Geometry / document / history primitives |

MCP tool surface lives in [../mcp-server/](../mcp-server/), not here.
Agentic docs: [../docs/agentic/INDEX.md](../docs/agentic/INDEX.md).
