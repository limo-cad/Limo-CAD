# Agentic evals (draft)

Tiny golden set for harness regression. Expand later; keep hermetic.

## Modeling (E1–E5)

| ID | Task | Pass |
|----|------|------|
| E1 | `cad_help` search/get first, then `resources/read` `limo-cad://knowledge/...` when the full page is needed | Search selects a page; full-page markdown is non-empty via true MCP resources |
| E2 | `fillet-basics` recipe `mode:fast` headless | All recipe checks pass |
| E3 | After E2, edit stock extrude 12→18 mm via `solid_edit_extrude` | `solid_extrude_definitions` distance 18 |
| E4 | Replay E2 twice independently (fresh MCP processes) | Matching checks_completed + body presence |
| E5 | One-step: `cad_new_project` → sketch rectangle → extrude → `solid_scene` | Stable body id; no invented tools |

Repo-reproducible coverage (from repository root):

```bash
cargo test -p limo-cad-help
cargo test --manifest-path mcp-server/Cargo.toml -- --nocapture
cargo xtask install-mcp --dry-run
```

Record: server revision, elapsed_ms, pass/fail/skip, tool error strings. Prefer honest SKIP over inventing geometry APIs.

## Help MCP wire (H1–H8 core; H9–H46 corpus; ops merged)

In-process BM25 unit tests live in `crates/help`. MCP unit coverage for `cad_help`
(direct tool calls and `cad_interface` execute) lives in `limo-cad-mcp`.

```bash
cargo test -p limo-cad-help
cargo test --manifest-path mcp-server/Cargo.toml cad_help -- --nocapture
cargo xtask knowledge check
```

Optional stdio wire checks against a freshly built `limo-cad-mcp` binary
(`cargo xtask install-mcp`) should exercise the same H1–H8 expectations below.

| ID | Call | Pass |
|----|------|------|
| H1 | search `clearance fit` | top/any id contains `fits-clearances` |
| H2 | search `draft angle` | DFM / draft hit |
| H3 | search `cad_help tenacity` | `agent-mcp-workflow` |
| H4 | get `machine-design.concepts.fits-clearances` | body non-empty; related_recipes includes a known recipe |
| H5 | get path-like id | `isError` / allowlist rejection |
| H6 | search `limit=100` | ≤10 hits (clamped) |
| H7 | topics | total > 0, page ≤50 |
| H8 | search `fillet` | some hit `related_recipes` includes `fillet-basics` |
| H35 | search `interference fit shaft hole` | top/any `fits-clearances` |
| H36 | search `datum sketch plane coordinate system MCP` | `datum-sketch-plane-choice` |
| H37 | search `inspect between mutates solid_scene` | `agent-mcp-workflow` |
| H38 | search `hole wizard vs modeled hole` | `hole-wizard-vs-modeled` |
| H39 | search `solid_edit fillet edit history` | `agent-mcp-workflow` |
| H40 | search `export preflight 3MF vs STL` | `export-print` |
| H41 | search `unit systems mm default soft focus` | `agent-mcp-workflow` |
| H42 | search `cad_list_all_tools soft disclosure focus packs` | `agent-mcp-workflow` |
| H43 | search `cad_attach headless sessions` | `agent-mcp-workflow` |
| H44 | search `driving dimensions solid_edit parametric` | `agent-mcp-workflow` |
| H45 | search `face_id edge_id topology solid_scene` | `agent-mcp-workflow` |
| H46 | search `power screws lead screws pitch backdrive` | `power-screws-lead-screws` |

Caps **confirmed** 2026-09-19 (H1–H8 core); corpus through **H46** (2026-09-20 purge retune): search default 5 / max 10, snippet ~280, get 12 KiB, topics page 50. Locked in `docs/machine-design-help-search.md`. Retune only from honest FAIL notes — keep goldens honest; prefer stronger queries over softer expectations.
