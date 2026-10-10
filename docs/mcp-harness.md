# Local MCP harness

The desktop, MCP and API share the product groups in
[`interface/catalog.json`](../interface/catalog.json). Use the same named
modeling operations, arguments and returned references in headless and live work.
The transport handles live submission, revision checks and acknowledgement.
See [the product interface](interface.md) for the complete contract and
[native command scripts](native-scripts.md) for reusable construction sequences.

## Choose the document owner

For manual native UI work, use `cargo xtask cad-call --installed --interactive
--out RESPONSE.json` to reconnect after a source edit or committed checkpoint.
This explicit mode verifies the canonical runtime manifest, its recorded clean
source identity, enabled native computer control and every installed payload hash.
It never builds, deploys or closes a desktop. It qualifies only the installed
revision, not the current checkout. Ordinary managed commands still prepare the
current source. Rebuild xtask with `--features native-control-harness` when using
the Windows control harness.

Discover a fresh active session, require `build_pair.status=matched`, and observe
before each individual guarded `cad_computer_control` input. Read-only captures
verify the visible result; `input_sent` alone proves only insertion. Preserve other
open documents and follow the active session after document replacement or
Undo/Redo. Windows may deny a foreground request; never send input without a
fresh observation confirming that CAD owns the foreground.

`cad_route` is the single live broker tool. It addresses multiple desktop
documents through one stdio server with three actions: `submit` sends one request,
`status` polls a submitted request's ticket, and `batch` sends a bounded ordered
group through the same submission and receipt path.

Live `cad_interface` inspection reports `build_pair` with the actual compiled
desktop and MCP identities. Require `status:matched` before qualifying a paired
build. `different` means the clean revisions or release identities differ;
`unknown` means identity reporting is unavailable; `unverified_modified` means
source changes prevent the commit alone from proving equality. Compatible
desktop control remains available so existing live work can be preserved.

`cad_route` with `action: "batch"` sends 1–16 literal modeling operations or live
engine queries to one explicit route, with its current `base_generation`. It
validates every operation name and argument envelope before publication, then submits and
awaits each normal receipt in order. A failure, owner replacement, intervening
edit or the batch's bounded deadline prevents later submissions. Successful
operations keep their own Undo entries. Every submitted call retains a
`cad_route` ticket; poll a pending ticket with `action: "status"` before deciding
whether to retry. Never repeat the whole batch after partial completion.
Use `include_values:false` when only application receipts are needed. The
broker does not attach or recompute geometry in the MCP process.

The batch request takes `route`, `base_generation` and `calls`, where each call
contains a literal tool `name` and optional `arguments` object. Optional
`timeout_ms` defaults to 30,000 and ranges from 1 to 30,000; `include_values`
defaults to `true`. The 256 KiB request limit includes the complete broker
envelope. Unlike nonblocking `submit`, `batch` waits within its deadline and
returns each operation's receipt, the next generation and any unsubmitted calls.
Tickets reach the caller in the final response. If the connection is lost before
that response, completed operations can remain applied without a known ticket;
inspect the live document before continuing and never resubmit the whole batch.
Use `submit` and `status` when each operation needs a separately retained ticket.

This is a normal MCP tool call. The supported `2025-06-18` protocol removed
JSON-RPC transport batching. There are no scripts, bindings or generated-ID
references in a CAD batch. Return to the agent to inspect results and plan the
next group. UI controls require fresh rendered targets and remain individual
`cad_interface` interactions.
Take the process instance, window, document and session identities from
`cad_list_sessions`. Submit an operation with `action: "submit"`, an explicit
`route`, the tool `name` and its `arguments`. Submission returns a `ticket`
immediately; poll `action: "status"` with that complete ticket. It leaves the
server's attached or headless document untouched and never loads another model
into the broker. Routes intersect all supplied identities and reject ambiguity,
closed documents and expired process leases.

Modeling mutations require the current `base_generation`. The existing numeric
inbox orders writes to each document; concurrent writes with the same generation
can submit, but only the first applicable mutation commits. Later conflicts are
reported in their receipts. Do not retry a submitted mutation before checking its
original ticket. Native read queries and `cad_interface` UI actions use the
existing control queue, with process, window and document fences checked before
dispatch. UI actions other than inspect/capture also require `base_generation`.
Pending work stays with its original document across tab changes, replacement,
close and process exit. Completed receipts remain readable. A route does not
activate an inactive tab; its owner must activate it before queued writes apply.
Control submission requires a recent heartbeat, so activate the document before
submitting UI actions or live read queries.
Use the existing explicit attachment workflow for scripts and offline tools.

The application always exposes local stdio MCP. A normal launch opens a CAD
window. Use `limo-cad --headless` for an independent worker without a window; the
standalone developer executable `limo-cad-mcp` is already headless. An unattached
worker owns an independent document and does not modify an open CAD window.
Use this path for offline examples, CI and independent repeatability checks.

Closing stdin ends a headless worker. For a visible CAD app it only disconnects
the transport, leaving the window and its documents open. Replay and package
checks use a headless worker so their owned process can finish on stdin EOF,
including when that worker targets a separate live desktop.

When an agent starts CAD normally, its stdio tools automatically bind to that
process's visible active document once it is ready. Startup requests that need a
document report not ready until it is published; they never create invisible
headless work. Explicit `cad_attach` can select a different document. After
`cad_detach`, a desktop transport requires an explicit target rather than
silently selecting another document or starting headless work.

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
[#91](https://github.com/limo-cad/Limo-CAD/pull/91), completing
[#11](https://github.com/limo-cad/Limo-CAD/issues/11) and
[#15](https://github.com/limo-cad/Limo-CAD/issues/15). An in-process transport
is an architectural option, not a prerequisite for that shared ownership contract.

## Identity and the lower-level protocol

Session data lives under `LIMO_CAD_SESSION_DIR` when explicitly configured. The
shared Rust transport otherwise uses the system temporary directory's
`limo-cad-sessions` folder on Windows and `limo-cad-sessions-<effective-user-id>` on
Unix. Desktop and MCP must be updated together for Unix default discovery;
older registries are not moved or deleted. An explicit override must select a
dedicated registry owned by the current user that is already private on Unix.

New Unix registry directories use owner-only `0700`, and new snapshot, receipt
and inbox files use `0600` at creation. Existing roots with group/other access
are rejected for reads and writes with an actionable configuration error; the
transport never chmods an existing directory. Descendants may retain older
user-owned, non-writable legacy modes inside that private root. Foreign-owned
and symlink directories/files are rejected; non-regular payloads such as FIFOs
cannot block snapshot readers. Registry ancestors must belong to the current
user or root and be protected from other users; a trusted sticky temporary
directory is accepted. Trusted OS directory aliases are accepted only when
their destination ancestry is protected too. These checks reject overrides
under foreign-owned or publicly writable non-sticky parents before publication.
Each UUID v4 session publishes `model.json`,
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

Ordinary attached tools use one selected document per stdio client. `cad_route`
addresses other live documents without changing that binding or loading their
models. Independent `submit`/`status` tickets let documents progress between
polls; `batch` orders a bounded group within one explicit route.

## Grouping and disclosure

`cad_interface` catalog returns the shared product groups and operations. The
former `cad_ui`, `cad_view` and `cad_launch` aliases are retired. Native controls
can be inspected and operated by fresh opaque IDs. Hidden, disabled, stale and
modal-blocked controls reject.

Long field values in `inspect` are bounded to 4,096 UTF-16 code units. A truncated
value includes `value_truncated`, `value_length` and `value_start`; textareas also
report their original `selection` offsets. The excerpt follows the caret so
chapter navigation stays useful without returning the entire multi-megabyte
recipe on every click. Inspection never shortens or edits the actual source.

Disclosure remains a discovery aid. Focus packs, soft TTL and LRU limits do not
prevent calls to undisclosed tools. Results can contain `_disclosure` hints;
`full_static` and `cad_list_all_tools` remain available. A timed worker sends
`notifications/tools/list_changed` without requiring a later client ping.
Logs use stderr so stdout stays valid stdio JSON-RPC.

The `cam` focus pack exposes CAM document inspection, editing, generation,
posting and remaining-stock simulation. These operations use the same native
CAM engine and freshness checks as the Manufacture workspace.

## MCP design iteration

Drive working designs through individual MCP operations or `cad_interface`
`execute`, inspecting between feature edits. Save the `.limo` project rather
than maintaining a presentation copy. Review configurations support list,
upsert, rename, delete, recall, clear, and project roundtrip through
`document/appearance`. Attached `cad_interface` `inspect` returns `view_state`
for camera/visibility/offset capture. Headless clients supply the camera.
The native stdio regression in `mcp-server/tests/named_views.rs` constructs and
restores a design through actual MCP requests without script playback.

## Optional repeatable construction and presentation

The readable `.limo.jsonc` sources under [examples/scripts](../examples/scripts)
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
cargo xtask run-script FILE.limo.jsonc --server CAD_EXECUTABLE --server-arg --headless --repeat 2 --out proof
cargo xtask run-script FILE.limo.jsonc --server CAD_EXECUTABLE --server-arg --headless --session UUID --new --present --speed 2 --compare proof/run-1.json --out live-proof
```

These examples use the packaged application. A standalone `limo-cad-mcp` needs no
`--server-arg`. [Developer replay setup](DEVELOPMENT.md#replay-a-recipe) covers
executable paths, AppImage arguments and bounded initialization.

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
STL and 3MF are exchange/export products, while `.limo` preserves editable history.

The following regression driver uses the standalone `limo-cad-mcp` developer server
as `MCP`; see the [developer setup](DEVELOPMENT.md).
`cargo xtask test-mcp controls --server MCP --session UUID --out controls.json`
exercises camera and joint controls in an explicitly selected disposable document.
The native live, drawing, playback and Scripts-workspace checks have separate
scoped drivers described in [interface.md](interface.md) and
[native-scripts.md](native-scripts.md). Keep tests tied to actual behavior and
failure recovery; do not add another exhaustive tool-count gate or CI matrix.

Existing `tutor_quest_pip_*` engine tests cover cam-bolt/clip exports and slicer
metadata without changing the headless document. The broader learning path,
capability lessons and flagship release evidence remain
[#16](https://github.com/limo-cad/Limo-CAD/issues/16).
