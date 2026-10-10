# Agentic guidance index

- [Eval goldens](EVALS.md) — modeling E1–E5 and help H1–H8+ wire checks (`cad_help` + resources).
- [How humans find help today](HUMAN_HELP.md) — index/taxonomy/MCP/Pages doors; Scripts deep-link; prompts gap.
- Design package VERSION / `design_v*.limo.jsonc` naming: Help id `concepts.design-version-scripts`.
- Geometry naming (bodies / faces / scripts / STEP): Help id `concepts.geometry-naming`.
- Shared reference geometry (followers / named parents): Help id `concepts.shared-reference-geometry`.

Committed operating docs for humans and coding agents working on Limo CAD.
Prefer leaving root `AGENTS.md` / `.cursor/rules` out of git (project policy).

| Doc | Purpose |
|-----|---------|
| [jsonc-workflow.md](jsonc-workflow.md) | Collections, fast rebuilds, and `export_script` versus `cad_script` |
| [PLAN-jsonc-roundtrip-collections.md](PLAN-jsonc-roundtrip-collections.md) | Constraints and non-goals for includes and script export |
| [HUMAN_HELP.md](HUMAN_HELP.md) | Human browse path (pre–Tauri Help) |
| [STEERABLE_MCP.md](STEERABLE_MCP.md) | Soft disclosure invariants |
| [INSTALL_MCP.md](INSTALL_MCP.md) | Standalone development-server installer |
| [../DEVELOPMENT.md](../DEVELOPMENT.md) | Canonical build, SDK and test setup |
| [MAINTENANCE.md](MAINTENANCE.md) | Native maintenance and review notes |
| [UI_OVERLAYS.md](UI_OVERLAYS.md) | React/Tauri flyout, clipping, and hit-test invariant |
| [../mcp-harness.md](../mcp-harness.md) | Public as-built MCP notes |
| [../../mcp-server/README.md](../../mcp-server/README.md) | Tool surface and current boundaries |

## Code truth

| Path | Owns |
|------|------|
| `mcp-server/src/disclosure.rs` | Focus packs, soft TTL, tags |
| `mcp-server/src/session.rs` | Headless session dirs, attach |
| `mcp-server/src/lib.rs` | Tool registry, RPC, goldens |
| `crates/export/` | 3MF/STL writers, material catalog |
