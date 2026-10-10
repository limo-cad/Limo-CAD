# crates/ index

Host-neutral Rust CAD crates (shared by UI / WASM / MCP where applicable).

| Crate | Role |
|-------|------|
| [`build-info/`](build-info/) | Compiled desktop/MCP identity; kept outside the geometry engine dependency graph |
| [`cam/`](cam/) | Host-neutral 3-axis CAM intent, validation, motion planning, volumetric stock simulation, and posts |
| [`interface/`](interface/) | Shared product catalog and rendered-control ownership, inspection, input and focus semantics |
| [`print/`](print/) | Reading scanned 2D prints for agents: plate outline calibration, millimetre crops with a grid and hole overlay, ring scores at model holes, hole-symbol detection with coverage matching |
| [`project-file/`](project-file/) | Project container compatibility, bounded reads, and atomic saves shared by native hosts |
| Other folder names under this directory | Geometry / document / history primitives |

MCP tool surface lives in [../mcp-server/](../mcp-server/), not here.
Agentic docs: [../docs/agentic/INDEX.md](../docs/agentic/INDEX.md).
