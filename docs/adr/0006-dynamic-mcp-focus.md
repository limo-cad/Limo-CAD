# ADR 0006 — Focus-scoped MCP and UI co-link

- Status: **Accepted** (2026-09-20 refresh; originally Proposed 2026-07-27)
- Date: 2026-07-27
- Accepted refresh: 2026-09-20 (align with live `disclosure.rs` / stdio)
- Related: [docs/proposed-architecture.md](../proposed-architecture.md),
  [docs/mcp-harness.md](../mcp-harness.md),
  [docs/agentic/STEERABLE_MCP.md](../agentic/STEERABLE_MCP.md)
- Tracking (discussion): focus [#10](https://github.com/limo-cad/Limo-CAD/issues/10),
  co-link [#11](https://github.com/limo-cad/Limo-CAD/issues/11);
  multi-window [#12](https://github.com/limo-cad/Limo-CAD/issues/12) is
  **deferred** (not P0)

## Context

Today `limo-cad-mcp` speaks MCP over **stdio** (good: local, offline). Early
drafts assumed a large static tool list with `tools.listChanged: false`. Live
server now advertises dynamic tools and soft focus packs.

MCP supports dynamic tools via `tools.listChanged` and
`notifications/tools/list_changed`
([spec](https://modelcontextprotocol.io/specification/2025-06-18/server/tools)).

## Decision

### A. Focus-scoped tools

1. MCP is a serious local automation surface; agents prefer it for automation/tests.
2. **stdio** is the **current** supported local transport. Offline/local is the
   invariant; internal IPC may evolve with evidence.
3. Explicit **focus** packs (live `FocusPack::ALL`, 11):
   `document | assembly | sketch | solid | modify | body_ops | datums |
   history | inspect | print | cam`.
4. Soft disclosure: `tools/list` prefers tools for current focus (+ tiny
   always-on spine). Out-of-focus tools stay **callable** (guidance, not a jail).
5. On focus change: update advertised set, `tools.listChanged: true`, send
   `notifications/tools/list_changed` (throttled).
6. Keep granular tools; optional goal-level tools in the right focus.
7. Print focus eventually includes **3MF** with useful materials/colors (target).

### B. Session attach (not live UI LWW co-link)

1. Useful milestone shipped: `cad_list_sessions` / `cad_attach` / `cad_refresh` /
   `cad_detach` against session `model.json` export (see STEERABLE_MCP).
2. This is **not** a live UI co-link / last-writer-wins writeback into the
   running viewport — attach is explicit snapshot/session ownership.
3. Headless MCP without UI remains valid for CI goldens.

### C. Multi-window broker — deferred

Not a P0 product requirement. Revisit if real use cases justify routing by
`window_id` / `document_id`.

## Consequences

- Focus tools and session attach are product behavior; do not document
  `listChanged: false` as current.
- Long design prose stays in `docs/proposed-architecture.md` / `docs/mcp-harness.md`
  / `docs/agentic/STEERABLE_MCP.md`.
- Tests cover focus list snapshots, list_changed emission, and attach failure
  modes for missing/invalid `model.json`.

## Acceptance sketch

- [x] `initialize` → `tools.listChanged: true`
- [x] Notification name is exactly `notifications/tools/list_changed`
- [x] Default / soft-focus advertised set is smaller than full_static (spine + pack)
- [ ] Live UI co-link with MCP op immediately visible in viewport (LWW) — **not** claimed; session attach only
- [x] Stdio still works offline; docs do not claim irreversible IPC forever
- [x] Multi-window not required for the first attach milestone
