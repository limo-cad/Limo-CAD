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

`action: launch` starts an explicitly
supplied desktop executable (or
`NBCAD_DESKTOP_BIN`). It reports `ready` only after the new process publishes a
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

`action: file` supports absolute `.nbcad` paths for `open` and `save`, and an
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

Run `cargo xtask test-mcp contracts` for browser behavior contracts: actual
DOM discovery, grouping, disabled/hidden/stale/modal guards, field events,
tree gestures, atomic drags, serialization, and recovery after failure. It
derives enabled ribbon commands from product configuration and checks that
they dispatch an action. It does not maintain copied tool counts. The desktop
frontend CI job runs this test with Playwright Chromium.

Run the native golden against a newly launched disposable document:

```powershell
cargo xtask test-mcp live --server <nbcad-mcp.exe> --desktop <nbcad.exe> --part --drawing --idle --pace 0 --save <new-absolute-path.nbcad> --out <report.json>
```

The executable and native libraries must be available (development builds may
need the OCCT bin directory on PATH). Use `--pace 500` for a live demonstration.
The runner speaks MCP stdio only. It checks launch readiness, foreground and
minimized camera control, optional 35-second idle recovery, native sketch/UI
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

This complements the native part/assembly model goldens and camera/joint
controls test. Those tests validate geometry and persistence; the live UI
golden validates the connection between engine state and interactive UI. A
passing command-dispatch inventory alone is not full feature coverage. Expand
goldens with real feature workflows when adding capabilities, rather than
introducing a mirrored list of expected tools or controls.

## Bench and feature workshop

```powershell
cargo xtask test-mcp bench --server <nbcad-mcp.exe> --workshop all --out <report-directory>
```

Add `--session <UUID> --pace 500` to drive an empty live document. The
runner can launch one with `--desktop <nbcad.exe>` and save the resulting
assembly with `--save <new-absolute-path.nbcad>`. The same
MCP plan runs headlessly in CI and through the live inbox for demonstrations.
Mutation routing comes from the server catalog's shared `mutates` metadata.
Active-sketch inspection, expression evaluation, and previews query the live
engine through the existing control channel. They do not read the completed
model snapshot, which intentionally excludes a sketch still being edited.
The product drivers live in `xtask/mcp` and use the shared `mcp-server/client.mjs` transport.

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

For a richer editable design, run `cargo xtask test-mcp garden-bench`.
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
is not a noBS-CAD project format. No external model is vendored here.

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
cargo xtask test-mcp drawing --server <nbcad-mcp.exe> --out <report.json>
cargo xtask test-mcp drawing --server <nbcad-mcp.exe> --desktop <nbcad.exe> --save <new-absolute-path.nbcad> --out <live-report.json>
```

The example builds native stock, three views and a fabrication note, checks exact
projection dimensions and failed-edit atomicity, then restores the drawing in a
fresh server. Live mode also verifies the drawing canvas and native save/reopen.
This is a focused example, not a comprehensive feature-coverage requirement.
Specialized annotations, derived-view ergonomics, editing/deletion of individual
views/annotations, and drawing exports remain tracked in #93.

All stdio example/test drivers share `mcp-server/client.mjs`. The part-design
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
