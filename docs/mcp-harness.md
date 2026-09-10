# Local MCP harness

The desktop, MCP and API share the product groups in
[`interface/catalog.json`](../interface/catalog.json). Use the same named
modeling operations, arguments and returned references in headless and live work.
The transport handles live submission, revision checks and acknowledgement.
See [the product interface](interface.md) for the complete contract and
[native command scripts](native-scripts.md) for reusable construction sequences.

## Choose the document owner

An unattached `nbcad-mcp` process owns an independent headless document. It does
not modify an open CAD window. This is the supported path for offline examples,
CI and independent repeatability checks.

For a running desktop, call `cad_list_sessions`, select its explicit session or
window/document identity, and call `cad_attach`. Ordinary modeling tools and
`cad_interface` execute requests then route mutations to that desktop's engine.
Callers do not issue a separate submit, wait or refresh after every edit.
`cad_interface` launch attaches the new desktop; acknowledged document transitions
update the binding, including replacement of a document within the same session.

The desktop remains the single live writer. Its published model is a read cache,
not shared memory. MCP never writes the live `model.json` back. Active-sketch
queries use the live owner so unfinished sketch data is available without
admitting half-finished history into the persisted project.

This routing and its lifecycle checks landed in
[#91](https://github.com/jackControls/noBS-CAD/pull/91), completing
[#11](https://github.com/jackControls/noBS-CAD/issues/11) and
[#15](https://github.com/jackControls/noBS-CAD/issues/15). An in-process transport
is an architectural option, not a prerequisite for that shared ownership contract.

## Identity and the lower-level protocol

Session data lives under `NBCAD_SESSION_DIR`, or the system temporary directory's
`nbcad-sessions` folder. Each UUID v4 session publishes `model.json`,
`active-sketch.json` when applicable, `focus.json`, and `heartbeat.json`.
Publications carry session/window/document identities and engine/published
generations. The active sketch and completed model have separate generation
fences. A heartbeat alone does not prove a new completed model is available.

`cad_list_sessions` projects one entry per live process/window pair, with retained
documents and an authoritative active document. Expiring process leases under
`_ui/processes` keep concurrent desktops independently discoverable. Inactive tabs
remain listed while their owner is alive. Closed and previous-run tabs do not.
Selectors passed to `cad_attach` are intersected before ambiguity is reported.

Diagnostic primitives remain available for transport tests and recovery:

- `cad_submit` queues one mutation with its expected engine generation and bound
  session/window/document identity. Stale generations and mismatched identities
  reject instead of overwriting another edit or wedging the next request.
- `cad_await_apply` waits for the applied/failed receipt and explicit publication
  fence. It distinguishes a completed model from an active-sketch-only update.
- `cad_session_status` in `document/session` observes the loaded completed model's
  `attached_generation` against the live `generation`. Missing or unequal
  generations are stale. It also reports identity, publication fences, heartbeat
  age, pending inbox operations and the latest receipt. Headless returns
  `attached:false` and `code:not_attached`, without an error.
- `cad_refresh` explicitly rereads the attached snapshot; `cad_detach` returns to
  headless operation. Neither is an extra step in ordinary attached modeling.

Status does not refresh a model or add replay operations. `stale:true` is expected
while an active sketch advances the live engine beyond its last completed model.
Heartbeat age describes publisher liveness separately from model freshness; a
matching revision can still have `heartbeat_stale:true`. Heartbeat-derived fields
come from one read, but inbox/receipt observations can change during the probe.

Attach, refresh, completed-model awaits and acknowledged document transitions
record the loaded publication fence only after a successful read/load. An
acknowledged identical model can advance its fence without recomputing geometry;
an active-sketch-only publication retains the earlier completed-model fence. Rust
script playback defers reconstruction until a snapshot query or completion; status
keeps reporting the older loaded generation while that cache remains deferred.

Targeted live windows are supported today. One stdio client still has one active
binding; concurrent scheduling across several documents remains
[#12](https://github.com/jackControls/noBS-CAD/issues/12). Do not infer a broker from
the ability to discover and attach to several windows.

## Grouping and disclosure

`cad_interface` catalog returns the shared product groups and operations. The
former `cad_ui`, `cad_view` and `cad_launch` aliases are retired. Native controls
can be inspected and operated by fresh opaque IDs. Hidden, disabled, stale and
modal-blocked controls reject.

Disclosure remains a discovery aid. Focus packs, soft TTL and LRU limits do not
prevent calls to undisclosed tools. Results can contain `_disclosure` hints;
`full_static` and `cad_list_all_tools` remain available. A timed worker sends
`notifications/tools/list_changed` without requiring a later client ping.
Logs use stderr so stdout stays valid stdio JSON-RPC.

## Repeatable construction and presentation

The readable `.nbcad.jsonc` sources under [examples/scripts](../examples/scripts)
run through one Rust interpreter from MCP, `cargo xtask run-script`, or the native
Scripts workspace. The same source supports maximum rate and paced presentation,
caption notes, camera targets and final checks. Stop on a failed operation; place
comprehensive geometry and assembly checks at the end.

The [bench](garden-bench.md) is built from sketches, features and physical mating
references. Imported geometry does not substitute for its editable construction
history. The older `cad_script` operation exports a forward trace of modeling
mutations and a restored baseline; it does not reconstruct parametric history
from an imported B-rep. Use authored native scripts for new teaching examples.

```sh
cargo xtask run-script FILE.nbcad.jsonc --server MCP --repeat 2 --out proof
cargo xtask run-script FILE.nbcad.jsonc --server MCP --session UUID --new --present --speed 2 --compare proof/run-1.json --out live-proof
```

Preserve the user's current document first. `--new` creates a design tab in the
specified window. Omit it only when that tab is already blank. Use `--desktop`
only when a separate window is intended. See the
[script interface review](script-interface-review.md) for the remaining draft
integration work; passing replay checks does not complete the teaching interface.

## Camera, assembly and exports

`cad_interface` view targets the attached or explicitly named session. It supports
isometric and orthographic orientations, fit, and timed focus on an active sketch,
body or component. Acknowledgement reports the completed camera transition; it is
not evidence that a screen capture contains the native renderer. Review actual
rendered frames when testing presentations.

Assembly inspection and joint operations use the shared engine path. Suppressing,
deleting or moving a joint does not require replacing its other fields. Motion
uses degrees and millimetres; inspect the solved assembly for diagnostics. STEP,
STL and 3MF are exchange/export products, while `.nbcad` preserves editable history.

`cargo xtask test-mcp controls --server MCP --session UUID --out controls.json`
exercises camera and joint controls in an explicitly selected disposable document.
The native live, drawing, playback and Scripts-workspace checks have separate
scoped drivers described in [interface.md](interface.md) and
[native-scripts.md](native-scripts.md). Keep tests tied to actual behavior and
failure recovery; do not add another exhaustive tool-count gate or CI matrix.

Existing `tutor_quest_pip_*` engine tests cover cam-bolt/clip exports and slicer
metadata without changing the headless document. The broader learning path,
capability lessons and flagship release evidence remain
[#16](https://github.com/jackControls/noBS-CAD/issues/16).
