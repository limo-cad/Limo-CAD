# Agentic guidance index

Committed operating docs for humans and coding agents working on noBS CAD.
**Do not** add root `AGENTS.md` / `.cursor/rules` to git (project policy).

| Doc | Purpose |
|-----|---------|
| [STEERABLE_MCP.md](STEERABLE_MCP.md) | Soft disclosure invariants |
| [INSTALL_MCP.md](INSTALL_MCP.md) | Hardened `xtask install-mcp` client wiring |
| [MAINTENANCE.md](MAINTENANCE.md) | Build, OCCT, test, PR checklist |
| [UI_OVERLAYS.md](UI_OVERLAYS.md) | React/Tauri flyout, clipping, and hit-test invariant |
| [../mcp-harness.md](../mcp-harness.md) | Public as-built MCP notes |
| [../../mcp-server/README.md](../../mcp-server/README.md) | Tool surface and build |

## Code truth

| Path | Owns |
|------|------|
| `mcp-server/src/disclosure.rs` | Focus packs, soft TTL, tags |
| `mcp-server/src/session.rs` | Headless session dirs, attach |
| `mcp-server/src/lib.rs` | Tool registry, RPC, goldens |
| `crates/export/` | 3MF/STL writers, material catalog |
