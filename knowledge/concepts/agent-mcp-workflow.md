---
type: Concept
title: MCP workflow — help, focus, inspect, edit, sessions
description: Preferred cad_help loop, soft focus, inspect between writes, topology ids, solid_edit_*, mm units, headless vs attach.
status: draft
updated: 2026-09-20
topics: mcp, workflow, modeling, sessions
keywords: cad_help, solid_scene, cad_list_all_tools, solid_edit, topology id, headless, cad_attach, mm, geometry naming, body name
related_recipes: fillet-basics, mounting-plate, angle-bracket
---

# MCP workflow — help, focus, inspect, edit, sessions

Humans and MCP share one OKF help corpus (`cad_help` + `limo-cad://knowledge/…`
resources = same embeds). Soft focus steers the advertised tool list; every
tool stays callable.

## Help first

| Step | Tool |
|------|------|
| Discover | `cad_help` `search` (default 5 / max 10 hits, ~280-char snippets) |
| Open | `cad_help` `get` with a returned **id** |
| Full page | `resources/read` on the selected `limo-cad://knowledge/...` URI |
| Browse labels | `cad_help` `topics` (page size 50; alpha labels, not a curated map) |
| Curated browse map | [index](../index.md) + [machine-design taxonomy](../machine-design/taxonomy.md) (seeded vs planned ids) |

Caps locked 2026-09-19. Sharper queries beat dumping pages. Preferred order:
`search` → `get` → optional `resources/read` → datasheets → web. For “what
exists?”, prefer **taxonomy** / **index** over paging `topics`. Standards
URLs in pages are citations.

## Soft focus

Default disclosure is **dynamic**: spine ∪ active ∪ soft packs (TTL / LRU).
Out-of-focus tools stay **callable**. Hard errors are missing ids, unfinished
sketches, or kernel failure — soft focus is guidance.

| Mode | Behavior |
|------|----------|
| `dynamic` (default) | Narrow advertised list for the phase |
| `full_static` | Advertise everything (clients that need it) |

| Pack | Phase |
|------|--------|
| `document` | Name, project load/export, session metadata |
| `sketch` | Sketch create / constraints / dimensions |
| `solid` | Extrude, revolve, sweep, loft, rib |
| `modify` | Fillet, chamfer, hole |
| `body_ops` | Shell, patterns, combine, split, STEP import |
| `datums` | Construction planes / datum features |
| `history` | Rollback, delete, reorder |
| `inspect` | Read-only solid/sketch catalogs |
| `print` | 3MF/STL/STEP export, materials |
| `cam` | Tool library, toolpaths, post, sim |
| `assembly` | Components, joints, interference |

Golden path: `cad_list_focus_areas` → `cad_set_focus` for the phase → mutate.
When planning or the short list looks thin, call **`cad_list_all_tools`**
(schemas + focus tags).

## Inspect → mutate → inspect

| Tool | Between writes |
|------|----------------|
| `solid_scene` | Bodies, Body/Face/Edge ids, meshes, feature errors |
| Document / sketch status | Sketch state, focus, expected features |
| `assembly_interference_check` | Multi-body overlap at solved poses |

Preferred loop: note blank vs existing doc → write → `solid_scene` → read
feature fields → keep ids for the next op → next write. Pair visual claims with
[validate before show](validate-before-show.md); mesh export claims with
[adversarial mesh audit](adversarial-mesh-audit.md).

## Topology ids

Pass **Body / Face / Edge ids** from inspect into fillet, chamfer, hole, and
body ops. Copy ids from the payload; keep face/edge ids on the same body.
After rebuilds, Booleans, or `solid_edit_*`, re-read ids before the next
topology consume.

## Edit with `solid_edit_*`

When a dimension or edge set needs a change, edit the feature in place.

| Intent | Prefer |
|--------|--------|
| Extrude depth | `solid_edit_extrude` |
| Fillet / chamfer set or radius | `solid_edit_fillet` / `solid_edit_chamfer` |
| Hole size / points / role | `solid_edit_hole` + definitions |
| Revolve / sweep / loft / rib / shell | matching `solid_edit_*` |

Read `solid_*_definitions` before and after. Use a blank document when the
feature type itself is wrong and needs a different construction path.

## Units and drivers

- Default project units: **mm** (document/recipe may say otherwise).
- Include units in the plan (“extrude 12 mm”). Convert inch vendor data once in
  a VERIFY table ([research before commit](research-before-commit.md)).
- Prefer named **driving** dimensions editable via `solid_edit_*`. Treat
  driven/reference dims as readbacks.
- Keep expression trees shallow; re-inspect after changing a driver.

## Headless vs attach

| Goal | Mode |
|------|------|
| Goldens, coupons, recipes, CI | **Headless** |
| Drive the user's open window | **Attach** after `cad_list_sessions` |
| Pull latest UI export into MCP | `cad_refresh` while attached |
| End live binding | `cad_detach` |

Snapshot bridge (`LIMO_CAD_SESSION_DIR`): UUID v4 session ids; desktop publishes
`<uuid>/{model.json,…}`; attach needs valid `model.json`; refresh is explicit;
MCP edits stay in memory (session files are read-side). Other session
strategies remain available as the product evolves.

## Drive the design through MCP

The saved `.limo` project is the working design. Use typed MCP operations
for construction, feature edits, review configurations, verification and saving
from start to finish. Continue editing the current feature history; ordinary
design iteration does not require a parallel presentation script or a blank
document rebuild.

Use `cad_interface` `execute` with the discovered group and operation, or
individual modeling tools. Attached execution submits and waits for application
internally. Inspect between writes and re-read topology after feature edits.

Save one review configuration with `upsert_named_view`; use
`rename_named_view`, `delete_named_view`, `recall_named_view`, and
`clear_named_view` in `document/appearance`. Attached `cad_interface`
`inspect` returns `view_state`: copy its camera, visible body IDs and
part offsets plus a name into `upsert_named_view`. A null camera means the
modeling viewport is unavailable; headless clients supply a camera directly.
Display offsets do not move native solids. View metadata edits clear the active
configuration, and clear preserves visibility.

Save via desktop `cad_interface` with `action: file`, `command: save`, and
an absolute `.limo` `path` (explicit `overwrite: true` to replace a file), or retain
the headless `cad_project_model` string and restore it with
`cad_load_project_model`. Inspect the restored model before handoff.

Use desktop `cad_interface` `action: history` with `command: undo` or `redo`
for document history, and inspect `state.history` for availability. Check live
UI replies for `status: applied` before treating an action as complete.

Recipes and JSONC scripts remain optional for explicitly requested teaching or
replay on a deliberately blank document. They are not the source of truth for
an interactively edited design. See [agent workflow](../../docs/agentic/jsonc-workflow.md)
and [optional recipe versioning](design-version-scripts.md).

## Export format (AM)

Prefer **3MF** for print packages when available; STL as fallback. Keep
`.limo` for history and STEP for CAD interchange. Preflight and slicer
evidence: [export and print](export-print.md).

## Fits and clearances (quick pointer)

Role-based clearances and fit coupons beat a single global XY offset — see
[fits & clearances](../machine-design/concepts/fits-clearances.md) and
[fit coupons map](../machine-design/concepts/fit-coupons-recipes-map.md).

Related: [MCP harness](mcp-harness.md), [research before commit](research-before-commit.md),
[validate before show](validate-before-show.md),
[design VERSION / JSONC scripts](design-version-scripts.md),
[geometry naming](geometry-naming.md),
[STEERABLE_MCP](../../docs/agentic/STEERABLE_MCP.md).
