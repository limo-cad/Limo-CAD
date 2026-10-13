---
type: Concept
title: MCP harness
description: Local headless and live-document MCP routing, engineering resources and replay boundaries.
status: stable
updated: 2026-09-20
---

# MCP harness

`mcp-server/` provides `limo-cad-mcp`: a **stdio** MCP server for local
automation and testing (no required cloud).

Canonical notes: [MCP harness](../../docs/mcp-harness.md).
Proposals: [proposed architecture](../../docs/proposed-architecture.md).

## Development branch

- Soft focus-scoped tools advertise `listChanged: true`; out-of-focus tools stay callable.
- An unattached MCP process owns a headless document using the shared Rust planner
  and native OCCT. It does not silently edit a visible desktop document.
- Discover live windows with `cad_list_sessions`, attach explicitly, submit
  changes through the owning UI's inbox and await their apply/publication receipt.
  Prefer MCP attach/new-project owners; keep session `model.json` untouched and start fresh rather than reuse a retired session id.
- `cad_interface` supplies guarded application, UI and script control; use its
  inspected controls and returned state rather than a second UI-specific model API.
- Prefer MCP tool **`cad_help`** (`search` → `get` / `topics`) before designing;
  optional prompt **`help_search`** frames a contextual query. Standard
  `resources/list` and `resources/read` expose the same offline Markdown bundle
  (`limo-cad://knowledge/...`, start at `index.md` when browsing full pages).
  Guidance is not an execution tool.

These notes describe the branch that bundles them. Older snapshots may predate
live routing or resource discovery; inspect the running server's capabilities.

`solid_import_step` imports a STEP file as a **reference solid** (dumb body).
`cad_script` dumps the successful **forward** MCP tool sequence for this
process — portable modeling ops only (session-control `cad_attach` /
`cad_refresh` / `cad_detach` are not recorded). Attach/refresh seed the trace
with `cad_load_project_model` + loaded `model_json` (refresh replaces the
baseline). We do **not** reverse-engineer feature history from STEP B-rep.
`cad_compare_solids` summarizes existing `solid_scene` bbox/mesh counts so a
rebuilt history can be checked against that imported reference.

See also the [live-control contract](../../docs/mcp-harness.md) and
[server documentation](../../mcp-server/README.md).
