# Product interface and executable examples

`interface/catalog.json` owns the workspace and command grouping used by the
ribbon and MCP/API. There is no separate ribbon configuration or API grouping
list. Each operation belongs to exactly one product group; tests reject missing,
duplicate, and stale entries. MCP disclosure packs are internal advertisement
policy, not a second public command taxonomy.

Use `cad_interface` with `action: catalog` to discover groups and typed operations.
Use `action: execute`, `group`, `operation`, and `arguments` to drive them. The
same call returns the engine result headlessly or in an attached desktop;
generation checks, submission, publication, and acknowledgement are internal.
Named MCP operation tools use that same route when attached, so existing
clients also make one call per operation. A successful launch binds that new
desktop automatically; attach explicitly only when choosing an existing document.
File opens and tab transitions also update that binding, so subsequent operation
calls follow the active document without a separate attach step.
An outdated desktop is rejected before submission. No uncertain mutation is
automatically retried.

CAM operations use the same Program, Simulate and Output groups as the ribbon.
`cam_set_document` replaces the entire result of `cam_get_document`; preserve
unedited post settings, generation stamps and associative height/linking intent.
Load warnings are accepted but recomputed by the engine. `cam_simulate_setup`
accepts modeled `stock_mesh`, finished-model `target`, an inclusive
`through_operation_id`, and optional step/time prefixes (zero means uncut stock).
`cam_simulate_gcode` accepts NC text and the same stock/target inputs with a
step prefix. `cam_post_events` returns neutral post events through
`cam-output/advanced`, retaining the normal freshness and verification checks.
Read-result `_disclosure` metadata is accepted on document round trips but is
not persisted. Partial-time playback frames omit comparison verdicts; request
the complete scope for verification.
Neither simulation nor post export constitutes whole-machine safety certification.

`action: launch` starts an explicitly
supplied desktop executable (or
`LIMO_CAD_DESKTOP_BIN`). It reports `ready` only after the new process publishes a
fresh session and that session acknowledges a UI inspection. `starting` is not
permission to launch a duplicate: inspect the existing process/session first.

`cad_interface` inspects the running application's actual controls. Results are grouped
by ribbon workspace/panel, project tabs, browser, viewport, sketch palette,
appearance, comments, timeline, drawing inspector, dialogs, and portal menus.
Only currently rendered controls appear; disabled controls remain explicitly
disabled. Missing labels are reported. This inventory is not a claim that every
product feature is implemented or behaviorally tested.

Inspect immediately before acting. Use the returned opaque control ID with
`click`, `double_click`, `context_menu`, `set_value`, or `key`. IDs expire on the
next inspection and reject changed documents, recycled controls, hidden or
disabled targets, and controls blocked by modal dialogs. Text changes and
clicks use normal input, change, focus, and blur handlers.

For geometry interaction use `action: viewport`, `canvas: viewport | drawing`,
and a `move`, `click`, `double_click`, or atomic `drag` gesture. `point` and drag
`to` are application-window CSS pixels; inspect `ui.canvases` for bounds. The
3D viewport also accepts a `world` point. These operations route through the
same canvas handlers as user input. They do not bypass feature validation.

`action: window` accepts `foreground`, `background`, `inspect`, or `close`. Returned
window flags describe observed OS state; foreground requests are subject to OS
focus policy. Native wake events advance camera animation, operation playback,
and keepalives while browser timers are throttled. Hidden execution reports
`presented: false`; a successful operation is not proof of rendered pixels.

The native Bevy host also supports `action: capture` with an absolute PNG `path`.
It captures the application's own rendered window, including controls and open
dialogs, without reading the desktop or other applications. The reply waits for
GPU readback and file encoding and returns the path and pixel dimensions. Existing
files require explicit `overwrite: true`. Restore a minimized window with
`action: window, mode: foreground` first. The legacy web shell does not implement
this capture action. Native background control retains the last window layout
while reporting `presented: false`.

`close` requests application exit through the same unsaved-work guard as the
title-bar close button, Alt+F4, File → Exit, and native macOS Quit. Its reply
acknowledges the request, not process termination. Inspect and use the normal
Save / Don't Save / Cancel controls if a prompt appears. A save without a known
path opens the native picker; save to an explicit path first for unattended
replay. All live control replies, including confirmation clicks, are written
before shutdown. There is no MCP force-kill or implicit discard mode.

`cargo xtask test-mcp exit --server PATH --desktop PATH [--out REPORT]` launches
only disposable windows and verifies foreground/background exit, the File menu,
cancel/discard, save followed by fresh-process reopen, and Windows native close.
Native macOS menu/shortcut delivery still needs a macOS smoke run.

`action: file` supports absolute `.limo` paths for `open` and `save`, and an
explicit `name` for `rename`, using the normal project pipeline. Replacing an
existing file requires `overwrite: true`; opening over unsaved work requires
`discard_changes: true`. This avoids OS file-picker automation. Follow returned
`active_session_id` after a document transition. OS print, driver setup, and
other external dialogs are not DOM controls.

## Ordered plans

Use one client to execute a plan sequentially. Await each `cad_interface` response before
the next operation. The UI has one presentation lane and acknowledges changed
model state after publication. Failed operations stop the supplied golden;
never replay an uncertain mutation automatically. Independently submitting
commands from competing clients is not a transactional plan.

Set `pace_ms` with `cad_interface` (0–2000). The same calls support fast checks and
visible demonstrations: zero is the default and adds no presentation delay,
while 500 ms deliberately paces a lesson. When visible, button touches and
operation groups animate, with viewport feedback for sketch and solid changes.
Feedback cleanup runs independently of execution. Camera changes animate.
Reduced-motion
preferences suppress the highlight animation.

## Checks that grow with the product

The shared command catalog is checked by `cargo test --locked -p limo-cad-interface`.
Bevy control and revision guards live in the native interface Rust tests.

Run the native golden against a newly launched disposable document:

```powershell
cargo xtask test-mcp live --server <limo-cad-mcp.exe> --desktop <Limo-CAD.exe> --part --drawing --idle --pace 0 --save <new-absolute-path.limo> --out <report.json>
```

The executable and native libraries must be available (development builds may
need the OCCT bin directory on PATH). Use `--pace 500` for a live demonstration.
The runner speaks MCP stdio only. It checks launch readiness, foreground and
background camera control, optional 35-second idle recovery, native sketch/UI
mode agreement, the real Extrude dialog and resulting solid, optional drawing
placement, save/open, overwrite refusal, and continued control after open.
Reports contain calls, responses, and timings; assertion failures stop the plan.

The live file test also creates a second empty document, opens different saved
models into that same session, and closes its active tab while the first remains.
Subsequent document/solid reads must follow the acknowledged model and surviving
tab without an explicit refresh or attach. Same-session controls reconcile changed
snapshots; identical snapshots avoid geometry replay. Delivered native control
requests retain their originating window/session ownership until reply or expiry,
independently of whether the source tab remains resident.

Snapshot export tickets are separate from the engine's edit revision. Repeated
publication and adaptive grid changes during camera movement do not create model
edits. A real model mutation still advances the revision and rejects stale queued
commands. `assembly_document` reads the live engine through the existing query
channel, avoiding a geometry rebuild just to retrieve component occurrences.

Name sketches and datum planes when creating them. For solid history operations,
use the timeline context menu's **Rename feature**, or call
`solid_rename_feature {"feature_id": 2, "name": "Front post / 630 mm stock"}`
through `document/history`. Renaming retains the evaluated geometry and updates
both the timeline and saved feature definition. Undo and Redo restore the name;
recompute and save/reopen preserve it. Names must contain 1 to 256 characters
without control characters. Source sketch and datum names are reference identities
and are not changed by this command.

This complements the native part/assembly model goldens and camera/joint
controls test. Those tests validate geometry and persistence; the live UI
golden validates the connection between engine state and interactive UI. A
passing command-dispatch inventory alone is not full feature coverage. Expand
goldens with real feature workflows when adding capabilities, rather than
introducing a mirrored list of expected tools or controls.

## Bench and feature workshop

```powershell
cargo xtask test-mcp bench --server <limo-cad-mcp.exe> --workshop all --out <report-directory>
```

Add `--session <UUID> --pace 500` to drive an empty live document. The
runner can launch one with `--desktop <Limo-CAD.exe>` and save the resulting
assembly with `--save <new-absolute-path.limo>`. The same
MCP plan runs headlessly in CI and through the live inbox for demonstrations.
Mutation routing comes from the server catalog's shared `mutates` metadata.
Active-sketch inspection, expression evaluation, and previews query the live
engine through the existing control channel. They do not read the completed
model snapshot, which intentionally excludes a sketch still being edited.
The native product drivers live in `xtask/src/mcp_scenarios` and use the shared Rust `replay::Client` transport. Browser contract fixtures remain under `xtask/mcp`.

The workshop exercises sketch operations and mutations in the product's solid
build, refine, repeat, and body groups. Adding an operation fails coverage until an
example successfully calls it. Geometry checks cover feature edits, body
counts, meshes, sketch dimensions, and replay errors. Successful invocation
coverage does not mean every parameter combination or UI dialog is tested.

After isolated workshop cases, the plan builds a garden bench from seven native
sketch/extrude parts, reused as seventeen occurrences with sixteen rigid joints.
It asserts solved occurrence positions, clean diagnostics, and native history.
Back supports attach through face connectors; rigid joints reject nonzero
motion coordinates instead of silently ignoring them. Reports include call arguments, timings, coverage,
and a model checkpoint. Workshop resets are explicit and require an empty
document at the start. Do not run it over user work.

For a readable authored design, run `cargo xtask run-script FILE.limo.jsonc --server <limo-cad-mcp>`.
This Rust runner interprets the commented command file through the shared interface;
the recipe-library layer carries the complete bench and focused feature lessons.
Add `--repeat 2` for independent headless determinism checks, or use
`--session <UUID> --present` to narrate and frame the construction in an existing window.
See `native-scripts.md` for the format and playback controls.
The [crown garden bench](garden-bench.md) adds crowned/slotted pickets,
rounded armrests, rear-post clearance pockets, reusable components and
driving-dimension edits with native history restore checks. The simple `bench`
fixture above is not a faithful reconstruction of any external reference.

For complementary mechanism examples, FreeCAD maintains an
[assembly example](https://github.com/FreeCAD/FreeCAD/blob/main/data/examples/AssemblyExample.FCStd)
and documents a vise and crank-slider in its
[Assembly workbench guide](https://github.com/FreeCAD/FreeCAD-documentation/blob/main/wiki/Assembly_Workbench.md).
The vise would exercise sliding and screw motion; the crank-slider would
exercise coupled joints. Treat these as design references for new native MCP
plans. An imported shape would not prove native sketch history, and `.FCStd`
is not a Limo CAD project format. No external model is vendored here.

## Direct drawing operations

`drawing_create_sheet`, `drawing_select_sheet`, and `drawing_delete_sheet` belong
to `drawing/sheet`; `drawing_add_view` and `drawing_projection` belong to
`drawing/views`; `drawing_add_note` belongs to `drawing/annotate`.
`drawing_document` reads the persisted drawing. The editor uses the same Rust
commands for these edits, including validation, ID allocation and returning
changed released sheets to draft. Live edits select the drawing workspace and
participate in the drawing editor's undo history.

A view supplies its name, kind, direction/up vectors, paper position and scale.
The engine assigns IDs; body filters, hidden/tangent edges and parent alignment
are optional. Projection returns exact OCCT linework, bounds, topology anchors
and circular references from the current completed model. Headless and attached
operation calls use the same arguments and results.

```powershell
cargo xtask test-mcp drawing --server <limo-cad-mcp.exe> --out <report.json>
cargo xtask test-mcp drawing --server <limo-cad-mcp.exe> --desktop <Limo-CAD.exe> --save <new-absolute-path.limo> --out <live-report.json>
```

The example builds native stock, three views and a fabrication note, checks exact
projection dimensions and failed-edit atomicity, then restores the drawing in a
fresh server. Live mode also verifies the drawing canvas and native save/reopen.
This is a focused example, not a comprehensive feature-coverage requirement.
Specialized annotations, derived-view ergonomics, editing/deletion of individual
views/annotations, and drawing exports remain tracked in #93.

All native stdio example/test drivers share the Rust `replay::Client`. Browser-only fixtures still use JavaScript. The part-design
examples in #89 retain their distinct geometry checks and lessons.

### Associative linear dimensions

The drawing dimensions group exposes drawing_add_linear_dimension. Choose two
current anchors returned by drawing_projection, retaining body_id, edge_id,
edge_key and endpoint and copying model_point into fallback_point. Specify the
sheet/view IDs, aligned/horizontal/vertical mode and signed paper-mm offset.
Precision, prefix/suffix and tolerance/basic/reference/fit presentation are
optional. The stored value comes from model topology, not an entered dimension
label. A stale/excluded edge, reused numeric ID with another topology key,
missing view or invalid precision rejects without consuming an annotation ID.

The editor's existing two-point dimension tool uses the same atomic engine
command. Existing drawing history and release invalidation remain in effect.
The drawing golden now dimensions a 6 mm extrusion, edits it to 8 mm, verifies
current topology and tolerance metadata, rejects invalid edits, and restores
through a fresh process. With --desktop and --save it also checks native file
save/reopen. This is the linear-dimension slice of #93; specialized annotations,
complete manufacturing sheets and export operations remain to implement.

## Inspection and reference naming

The Assembly browser's selected-instance inspector provides **Remove instance**.
Plain Delete invokes the same operation only from the focused, selected component
tree row. MCP uses `assembly_remove_occurrence {"occurrence_id": 5}` in
`assembly/joints`; native and WebAssembly hosts share its typed engine operation.
Removal preserves the reusable definition, source geometry, other placements
and allocation counters, and has one native Undo/Redo entry.

Removal rejects a grounded instance, children, referenced joints or contact
sets, saved-view occurrence offsets, drawing selections or annotations, and print
handoff or height bindings. The error identifies the dependent item to release,
move, rebind or remove first. The last instance of a definition must remain or be
hidden: compatibility promotion otherwise recreates an instance of retained source
geometry. Removal never cascades into dependent authored work.

`assembly_interference_check` belongs to `assembly/inspect`, matching the existing
Assembly browser’s Inspect panel. It uses the same exact retained-BRep query as the native
engine, with solved occurrence transforms. An empty occurrence filter checks all
visible occurrences; a nonempty filter checks pairs within that set. A single
selected occurrence therefore has no other occurrence to compare. Broad-phase
culling avoids distant pairs. Touching is distinguished from positive overlap
volume, and a configurable nonnegative clearance threshold admits nearby pairs.
Unsolved assemblies, failed geometry, and missing/hidden requested occurrences
reject instead of returning a misleading clean result. The existing Inspect panel and MCP use one shared native implementation and the
same product group; the disabled Model-ribbon placeholder is not another API path.

`sketch_begin` and construction-plane creation accept optional meaningful `name`
values. Names survive native replay, history and references. Empty/control-character
or duplicate names reject before allocating history IDs. Automatic sketch names
skip an explicitly named SketchN. Headless and live arguments use the same shared
encoder, including datum names; there is no second conversion implementation.

## Review configurations through MCP

Use `cad_interface` `execute` in `document/appearance` for `named_views`,
`upsert_named_view`, `rename_named_view`, `delete_named_view`,
`recall_named_view`, and `clear_named_view`. The individual tool names are also
callable. Prefer per-view operations over replacing the entire list with
`set_named_views`. Invalid configurations or names reject atomically.

`upsert_named_view` accepts `{name, camera: {position, target, up},
visible_body_ids, part_offsets?: [{body_id, translation}]}`.
`rename_named_view` accepts `{name, new_name}`; delete and recall accept `{name}`.
Attached `cad_interface` `inspect` returns `view_state` with current camera,
visibility and offsets. Copy those three fields plus a name into upsert.
Camera is null when no modeling viewport is available; supply explicit camera
coordinates headlessly. Do not pass the extra inspection fields to upsert.

Saved views live in the project and survive save/load. Offsets affect display
only. Metadata edits clear the active configuration; recall to display it again.
Clear preserves current visibility. Use existing feature-edit tools to iterate
on the working project, then save with desktop `action: file`, `command: save`,
and an absolute `.limo` `path` (explicit `overwrite: true` to replace a file), or
headless `cad_project_model`/`cad_load_project_model`. Scripts are optional for
explicitly requested teaching and replay.

Attached `cad_interface` `action: history` with `command: undo` or `redo` uses the
same document history controller as Ctrl/Cmd-Z and the native Edit menu, including
sketch, solid, drawing, and assembly history. Inspect returns `state.history`
availability. Unavailable commands and commands blocked by a dialog reject;
this action operates on document history rather than a focused text field.
