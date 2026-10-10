# Proposed architecture (not yet accepted product commits)

This document collects **aspirational / proposed** implementation ideas.
They are useful for discussion and issues. They are **not** promises that
`main` already behaves this way.

Accepted product directions (without binding IPC/UI architecture) live in
[goals.md](goals.md). Factual MCP notes live in [mcp-harness.md](mcp-harness.md).

## Status key

| Status | Meaning |
|--------|---------|
| Proposed | Open for prototyping; may change |
| Deferred | Interesting later; not near-term P0 |

---

## 1. Focus-scoped MCP tools — Proposed

**Problem today:** `limo-cad-mcp` advertises `tools.listChanged: false` and returns
a large static tool list (~100 tools), which floods agent context.

**Proposal:** when modeling **focus** changes (document / sketch / solid /
modify / history / print):

1. Advertise `tools.listChanged: true`.
2. Add/remove tools for that focus.
3. Send `notifications/tools/list_changed` (exact MCP notification name).
4. Keep a tiny always-on spine (status, set focus, safe cancel).

**Invariant:** keep a fully **local** automation path. Today that is **stdio**.
Stdio is the current supported transport, not an irreversible forever decision —
offline/local behavior is the invariant; internal IPC can evolve with evidence.

Spec reference: [MCP tools / listChanged](https://modelcontextprotocol.io/specification/2025-06-18/server/tools).

**Branch note (`feat/3mf-print-export`):** soft disclosure prototype is implemented;
see [mcp-harness.md](mcp-harness.md). `main` still uses a static list until merge.

---

## 2. Co-link MCP ↔ one active UI document — Proposed (first milestone)

**Problem today:** MCP owns an independent document from the visible UI
(fork of truth). Same Rust planner crates; separate instances.

**Proposal:** attach MCP to **one** live UI/engine session
(`list_sessions` / `attach` with `document_id`, optional `window_id`).

**v1 rules sketch:** explicit attach; writer lock or clear conflict errors;
headless MCP without UI remains valid for CI goldens.

This is the useful first automation milestone. Prototype before treating it as
required product behavior.

**Branch note:** `cad_list_sessions` / `cad_attach` / `cad_refresh` /
`cad_detach` implement **read-only snapshot** load from headless session dirs
(require valid `model.json`; no MCP writeback). Not a live shared UI document.

---

## 3. Multi-window / multi-document MCP broker — Deferred

Multiple open windows may matter later. A stdio **broker** that routes by
`window_id` / `document_id` is one option; one MCP process per document is
another (especially for CI).

**Not a P0 product requirement** until real use cases justify it. Co-link to
one active document comes first.

---

## 4. 3MF export with materials/colors — Proposed target

**Accepted direction:** additive manufacturing / 3MF with useful appearance
metadata ([goals.md](goals.md)).

**Proposed implementation sketch:**

- Rust export from OCCT tessellation → 3MF primary, STL fallback
- UI + MCP export tools when ready
- v1 appearance may be per-body color + named material
- Golden fixtures: colored cube → 3MF

Describe as a **target with testable scope**, not current functionality.
(No 3MF writer on `main` today.)

**Branch note (`feat/3mf-print-export`):** native 3MF/STL export + MCP print pack
(portable model for standard/Bambu/Orca; Prusa/Cura metadata — not a sliced
G-code project).

---

## 5. Rust crate roles (factual guidance for proposals)

When proposing engine work, keep these boundaries clear:

| Crate | Role |
|-------|------|
| `limo-cad-core`, `limo-cad-sketch`, `limo-cad-solid` | Host-neutral model logic (document, sketches, features, history, planning) |
| `limo-cad-occt` | Native geometry adapter (OCCT) |
| `limo-cad-wasm` | Browser adapter path (WASM host + OpenCascade.js for solids in the browser build) |

UI (React/Three.js/Tauri) displays and commands; geometry truth stays in the
Rust model + kernel adapters.

---

## 6. Shared agent / editor guidance files — Policy (open to revisit)

**Current project policy:** editor- and agent-specific steering files stay
**internal**. Root and nested `AGENTS.md`, plus `.cursor/` content, remain
gitignored. Public documentation of MCP tools and architecture is welcome in
`docs/` and `mcp-server/README.md`.

**Later option:** maintainers may revisit a short shared cross-tool guidance
file (commonly named `AGENTS.md`) for build/test commands and contribution
norms. Relevant references if that discussion reopens:

- [AGENTS.md open format](https://github.com/agentsmd/agents.md)
- [Cursor Docs — Rules / AGENTS.md](https://cursor.com/docs/rules)
- [GitHub Copilot — repository / agent instructions](https://docs.github.com/en/copilot/how-tos/copilot-on-github/customize-copilot/add-custom-instructions/add-repository-instructions)
- [Microsoft ISE — AGENTS.md and skills](https://devblogs.microsoft.com/ise/ai-assisted-development-agents-skills-copilot-cli/)

---

## 7. Education / quests — Deferred product layer

Tutor-style loops that reuse golden MCP scenarios are attractive later. Keep
them out of the top-level committed goals until the CAD foundation and local
automation path are stronger.
