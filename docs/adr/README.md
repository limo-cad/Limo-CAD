# Architecture Decision Records

Lightweight ADRs for cross-cutting recommendations and decisions. Each record
states its own status. Long design prose lives in `docs/mcp-harness.md` and
`docs/proposed-architecture.md`.

| ADR | Title | Status |
|-----|-------|--------|
| [0001](0001-rust-shared-engine.md) | Rust for the shared engine and platform-neutral features | Historical proposal |
| [0002](0002-bevy-viewport.md) | Native Bevy viewport boundary | Shell/composition superseded by the Bevy interface |
| [0003](0003-bevy-interface.md) | One Bevy application interface | Implemented |
| [0003](0003-3mf-export.md) | 3MF (+ STL) with materials/colors | Implemented for native export with per-body appearance |
| [0005](0005-main-goals.md) | High-level product directions | Accepted |
| [0006](0006-dynamic-mcp-focus.md) | Focus-scoped MCP + UI co-link | **Accepted** (2026-09-20); multi-window **deferred** |
| [0007](0007-limo-name.md) | Limo product name and localized presentation | **Accepted** (2026-10-02); rename implementation pending |

Treat proposed and deferred ADRs as reviewable direction, not permission to
start them ahead of reliable mechanical CAD and local MCP fundamentals.
