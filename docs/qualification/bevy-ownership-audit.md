# Bounded Bevy ownership audit (#333)

This is a source inventory, not an FPS, latency, memory-pressure or deployed
application qualification. It covers the native browser/history/CAM snapshots,
presented input/window projections, MCP response/request ownership and committed
body render consumers inspected on 2026-10-10, with the bounded native/export
follow-up below reviewed on 2026-10-11. This is not an exhaustive allocation graph.
Previously integrated shared scene, pose, metadata and immutable drawing work is
not reimplemented here.

## Avoidable ownership removed

- `controller/browser.rs::synchronize` previously cloned all of `DocumentDto`
  at each aggregate revision. Commit `3181443d` retains only the browser's title,
  units and hierarchy. A short `with_document` read compares these inputs; an
  unchanged projection reuses its Arc. Owned rows are necessary after the engine
  guard ends. Cost scales with the browser hierarchy, excluding feature history.
  Owner replacement clears the predecessor before considering reuse. Hiding the
  panel intentionally preserves this bounded projection and collapse state.
- `controller/history/panel.rs::synchronize` and `history/drag.rs::pointer`
  previously retained full `DocumentDto` values. Commit `db1aa006` owns only
  `FeatureTree`: ordered IDs, names, kinds, suppression/status and rollback cursor.
  The panel reuses unchanged history across aggregate revisions; exact owner and
  revision receipts remain independent of Arc identity. Drag startup shares the
  rendered Arc only at its exact current receipt, otherwise takes a narrow
  snapshot under the publisher-to-engine lock order. Drag previews need stable
  order/cursor inputs until drop or cancellation. Comparison/copy cost scales with
  feature count and names/status text; browser hierarchy is excluded.
- `controller/workbench/cam_view.rs::synchronize` previously copied the entire
  CAM document on a selection-only preview-key change. Commit `4d7cb0ac` reuses
  the retained document at the same owner/revision. Selected setup/operation and
  warnings are still recomputed; job key, cancellation and generation remain
  selection-sensitive. A changed revision or owner requires a new snapshot.
- `session_bridge.rs` engine envelopes and save/export/edit text consumers now
  transfer owned JSON payloads and strings instead of cloning the complete model
  response (`0c587b12`). Required edit/cancel baselines remain owned. Envelope,
  null/error and missing-text contracts are unchanged.
- `native_viewport/platform.rs::apply_model_state` retires transient preview and
  CAM stock inputs on document replacement before successor rendering
  (`479665c7`). Existing readers can retain immutable old snapshots; clearing
  stock advances its revision so downstream CPU/GPU refresh paths see retirement.
- MCP engine envelopes, successful structured responses, image data and final
  JSON-RPC responses transfer their owned values (`08e6ca5c`, `f184efdd`). Text is
  serialized before the structured payload moves. Incoming stdio tool arguments
  and names also move into dispatch (`1ea5a165`); error-response ID copying stays
  outside this change. Missing/null/error and notification contracts are retained.
- MCP `load_snapshot_model` serializes the borrowed model string instead of
  cloning it into a temporary JSON Value (`8352889c`). The retained model text
  still supplies attach/refresh and script baselines; JSON string encoding and
  generation, recompute, cancellation and publication fences are unchanged.
  `crates/sketch/src/host.rs` also serializes `cam_document_ref()` synchronously
  under the same manager read instead of cloning the complete CAM document.
  Mutable CAM editor and import baselines retain their required owned copies.
- Deferred pointer and lesson-preview guards consume one narrow presented
  owner/modal/canvas projection (`b498e6a4`, `94df968d`), without retaining surface
  text. Window fallback consumes paired presented bounds/scale (`676751ff`),
  preserving actual Window/current UiScale precedence and avoiding a second read.
- Mechanism admission and drag continuation, history drag ticks and general
  editor input now use the same atomic presented owner/modal/viewport projection
  (`eabccdf6`, `e9f43173`). The general editor captures it before mechanism
  delegation, at the previous snapshot point. Camera, epoch, receipt, capture,
  modal, bounds and cached press-owner checks retain their original order and
  meaning. Validated canvas names are unique. Strict affected desktop Clippy
  passed at `0c2e30b4`; repeating pure gesture tests would not measure this
  allocation removal, so those earlier passes remain retained.
- File shortcut admission checks presented modal emptiness through the existing
  atomic pointer-context projection (`cf5cae14`). The shortcut entry runs for
  unhandled native input, including cursor moves; it formerly copied the whole
  presented frame before filtering keyboard events. The same presented frame,
  missing/poisoned-frame behavior, modal and awaiting guards and their ordering
  remain authoritative. Richer frame consumers are unchanged.
- Field commit and requested-focus checks (`452ba807`, `0320098d`) borrow
  the exact presented owner/top modal instead of copying full surface text.
  Their missing/poisoned frame, pending presentation, queued/IME and owned text
  behavior stays unchanged; the requested-focus regression passed at `115bc19a`
  in `signed-zero-focus-115bc19a-results.json`. Bambu panel review's separate
  `9e16fade` follow-up compares the retained full model by borrow while keeping
  actual worker request payloads owned; its focused source check passed once
  in the export receipt below.
- Active sketch availability, history dispatch and fallback publication focus
  (`8d4acaea`) read the active-session flags directly instead of building and
  serializing sketch geometry. Public/editor DTOs and the existing poisoned-lock
  behavior remain unchanged. Wheel and busy Save/Escape routing (`d20d62b1`) use
  narrow presented projections at each original read position. Layout warning
  labels (`fd85a556`) borrow assembly intent and own only requested strings,
  preserving order, duplicates, first match and unknown-ID fallbacks.
- Native body mutation identities (`1671a574`) provide conservative dependency
  invalidation and a checked kernel owner. Exact private commit proofs let the
  renderer reuse same-document mesh handles and edge metadata across unchanged
  bodies (`5daa6160`). Proofs require the complete error-free committed body set
  and exact Scene Arc; they are neither serialized nor derived from model input.
  Failed attempts clear proof. Replacement/thaw uses a new kernel owner. Missing
  proof or assets falls back to rebuilding; foreign-document assets stay owned
  by that document. Kernel tessellation and DTO construction remain unchanged.

## Required ownership retained

- `workbench/cam.rs::synchronize` retains `Editor.cam` as the draft's complete
  baseline. `Draft::apply` and record/duplicate/delete helpers produce a complete
  validated CAM replacement while preserving untouched setups, tools and
  operations. These mutable copy-on-apply values are not read-only waste. The
  baseline scales with the complete CAM document; independent CAM revision
  tracking is not claimed by the selection-only reuse change.
- `workbench/cam/central.rs::reduce` captures a receipt-bound project baseline
  when opening the tool library. `central/model.rs::import` clones that baseline
  to replace/add one tool and preserve unrelated project records and identities.
  Publishing also needs the selected project tool. Narrowing the saved baseline
  to tools alone would change the existing whole-document import contract.
  `0bd892bc` retires this project snapshot when the library becomes inactive or
  its owner changes; active drafts and background readers keep their required
  lifetime. Its focused source regression passed once (receipt below).
- `cam_view.rs::State.document` is immutable owned input for background preview
  jobs and stock simulation, after the UI/engine read ends. New input costs scale
  with the full CAM document. Selection-only reuse avoids repeated allocation;
  it does not establish independent CAM revision tracking. Inactive cleanup
  drops the UI's strong reference and cancels work. A running worker or reader
  may legitimately retain its immutable predecessor until it finishes.
- Drawing paper and annotation synchronization use `shared_drawing_snapshot`;
  their retained Arc is necessary for editor/render readers. Editing or document
  replacement must publish a successor, leaving existing readers immutable.
- Hole-callout preparation now uses that immutable snapshot under its exact
  document receipt (`2f466b60`), avoiding an initial full drawing copy. Creating
  the edited annotation draft still needs one owned copy of drawing intent.
  Owner/revision, cancellation and worker commit guards are unchanged. This is
  a reviewed allocation removal, not a measured latency improvement.

## Evidence and remaining acceptance

Browser/history/CAM production-path regressions passed at clean `ff6b635c`,
including the specifically affected existing history drag/rollback case. The CAM
regression uses the same guarded input acquisition as `synchronize`, without
starting simulation threads. Owned desktop envelopes, replacement-stock, arc
overshoot and guarded exposure receipts passed at clean `2cda40b5`. Native
generation and MCP response transfer checks passed at clean `f184efdd` (four new
filters). Incoming stdio and all five presented pointer/lesson/window and body
Assets reuse checks passed at clean `42ec4d2f`. Source
review, formatting and diff checks passed; no runtime latency or physical
GPU-memory qualification follows.

At clean `8352889c`, strict MCP Clippy and the specifically affected existing
`read_only_snapshot_attach_refresh_detach` and
`advertised_document_contract_round_trips_empty_and_populated_documents` checks
passed once. The receipt is
`D:/limo-cad-maintenance/backlog-autonomous-20261010/mcp-read-borrows-8352889c-results.json`.
These cover real snapshot attachment/refresh/detachment and populated CAM wire
behavior; they do not qualify deployed UI or measure allocation/latency savings.
The File shortcut projection received source review and diff/format checks;
no additional GUI or gesture pass is claimed for that caller change.

Further source inspection confirms exact closed-document retirement, cold-owner
cleanup after worker errors, weak scene stamps and document-specific mesh/entity
removal. Existing close/low-memory regressions were not rerun. The actual kernel
still meshes every live body after solid recompute. The new renderer can preserve
unchanged body uploads/derived edges only through the authoritative identities
and exact scene proofs described above. Body IDs, pointers and topology
signatures alone are not geometry revisions. Both new native/Assets checks
passed; this does not qualify deployed rendering or physical GPU memory.

## Detached native work and final publication

The later bounded investigation supersedes the earlier background-export
no-defect finding: the existing receipt fences were correct, but expensive native
projection, mesh/ZIP preparation and archive parsing still held live locks.
The following changes preserve those fences while separating owned computation:

- Cold paper preparation captures selected independent B-reps plus coherent
  scene/assembly and resolved sheet intent. Native projection runs on the worker;
  exact source keys reject retired results. Finished projection reuse is bounded
  to matching selected geometry and intent, never body IDs alone. Pending/error
  painting retains hidden paper/clip parents while retiring stale children.
- `PreparedDrawingExport` captures selected native shapes and drawing intent
  under a short engine read. File SVG/DXF/PDF and native Print release publisher
  and engine locks before HLR, output encoding, font work and `Page::prepare`.
  Files validate the original owner/revision and I/O guard immediately before
  atomic publication. Print revalidates before its separate native dialog.
  PDF page and content use the parser's 96 DPI; physical page size is covered by
  the retained independent page oracle, not inferred from a filename.
- Bambu template inspection captures exact source context before detached
  parsing; post-write template inspection also runs after the actual atomic
  write. The initiating busy control is admitted once. PREVIEW owns selected
  native shapes, poses, appearances, component structure and validated intent;
  computation grants no later adoption authority. The original receipt and
  callback token/generation remain required.
- Prepared Bambu WRITE (`2f31f813`, `5527b23b`) separates native mesh/ZIP build
  from registration/publication. A real dialog-operation lease is revoked by
  Cancel, replacement and relevant choices, including changes without a model
  revision. Final publisher -> lease -> engine proof/registration -> atomic
  write preserves private host/session, exact model and selected native geometry
  identities. Detached CAD-layout source (`8c8c9ff0`) uses the same captured
  meshes and bed. Its refusal-before-consent and explicit-consent/cancellation
  source checks passed at `9e16fade`, recorded in the export receipt. The authentic
  template has four plates; positive single-plate ResolvedScene/native service
  acceptance remains separate in #316.
- Portable STL/3MF (`ecfe0aef`, integrated as `fd774bfe`) captures selected genuine
  B-reps and coherent units, solved placements, names and required appearance,
  component and bed intent. Existing writers and layout analysis run detached.
  A distinct Files export-job lease covers actual picker/direct enqueue and
  Cancel/replacement/transitions/drop. Final exact publisher -> job lease ->
  native owner/model/geometry proof -> atomic write is required.
- Exact STEP (`d493f04e`) uses that export-job lifecycle and independent selected
  B-reps, preserving AP242, thread metadata, units and occurrence transfer.
  Occurrences retain their original priority over requested body IDs. It adds
  no display-mesh prerequisite. OCCT's mutable process-global STEP parameters
  require one native exchange mutex around reader/writer construction, transfer
  and serialization; independent shapes alone cannot isolate that state.
  Detached export takes no live document locks, but concurrent STEP exchanges
  serialize and an actual import may wait for that gate.

Deep native copies above are required isolation boundaries, not avoidable
read-only duplication. Editable drafts, saved/recovery baselines, selected output
bytes and in-flight worker inputs still need owned lifetimes. No lock is retained
across a user event. Cancellation before final publication grants no file write;
errors after an actual write remain explicit about the file already written.

The source checkpoint at `1c0dc76e` passed six exact focused checks once plus
strict Windows desktop/native/MCP checks, retaining three earlier passes without
repetition: [detached drawing receipt](detached-drawing-output-20261011.json).
The CAM retirement receipt is
`D:/limo-cad-maintenance/backlog-autonomous-20261010/cam-retirement-0bd892bc-results.json`.
At clean `9e16fade`, strict Windows desktop/native-engine/OCCT/export checks
and exactly three new focused filters passed once: real native Bambu WRITE
operation ownership, portable STL/3MF job ownership and the borrowed panel review
contract. [Export ownership receipt](detached-export-ownership-20261011.json)
retains the exact artifact and the original no-consent fixture failure. It also
retains the `9cdf863d` Print-to-File DOCINFOW option check without repetition;
that check does not qualify the OS dialog/spooler or physical print. The real
four-plate template is not positive single-plate ResolvedScene qualification.
STEP `d493f04e`, native drawing-export `27ec6551` and desktop/CI repair `4382a5a8`
are integrated and qualified at clean `fd85a556`: strict changed-package Windows
checks and their three new regressions passed once. The specifically affected
real sketch-history and busy Save regressions also passed once after the narrow
read changes. [STEP/drawing receipt](detached-step-drawing-20261011.json) records
both artifacts and the original strict-lint failure with its assertion-preserving
correction. Earlier passing export/drawing/Print-to-File and generic pointer
filters were retained. This is combined-source qualification of the stated
paths, with no new live-pair, GPU, memory-pressure or latency claim.

### Selected-body section detachment: source checks passed, live acceptance pending

Native section capture (`b11a727b`, `f9ce64e1`) copies the selected genuine B-rep
under the live engine guard after the existing request/source checks. This is a
required owned isolation copy limited to that body. Detached native
section/probe/cutaway computation releases that guard. The finished report keeps
private host/session ownership, a Weak scene stamp, captured geometry/metadata
revisions and the selected body's actual kernel identity through presentation.

Desktop `3013d119` captures under the original ordered UI receipt and admits its
DispatchGuard once; section computation and CPU raster/cutaway preparation run
outside publisher/engine guards. Before adding assets or adopting the report,
the original owner/revision, visible overlay generation and private native proof
must still match. Close, draft changes, edits and document/kernel replacement
reject prepared output. The worker retains its existing single pending slot.
Genuinely admitted attached MCP section queries also release process-lease and
publisher guards during computation/serialization, then reacquire the original
guards and validate native proof before retaining/delivering the original ticket's
outcome. Liveness publication failures retain that outcome for delivery-only
retry; they do not dispatch section work again. No-ticket compatibility and
other query paths retain their prior routing.

The ordered UI and attached-ticket regressions passed once at `3013d119`;
the native section regression passed at `67cb9e15` after correcting its
BoundaryContact-versus-NoIntersection fixture expectation. Strict Windows checks
and exact receipts are recorded in
`docs/qualification/detached-section-interference-20261011.json`.
The one prior human exact180 section pass is selectively marked needs_recheck;
its original receipt is retained. These source checks do not qualify live UI,
memory, FPS or latency. Capture
and deep copy still briefly hold live guards; their cost depends on the selected
geometry. Required edit/cancel and in-flight presentation ownership is unchanged.

### Static interference detachment: source checks passed, live acceptance pending

The bounded static interference seam captures the authoritative solved
occurrence poses and scene coherently. It copies genuine B-reps only for the
candidate bodies used by the unchanged exact distance/Common algorithm, sharing
one copied definition across repeated occurrences. Private finished proof uses
host/session ownership, a Weak scene stamp, geometry/metadata revisions, exact
current solved-pose equality and native identities for every visible requested
source body, including bodies outside candidate pairs. A zero-pair report cannot
skip native source invalidation; absent non-candidate identities are retained as
absence rather than adding a new shape-presence admission requirement.

Native `2045da06`/`b52d9a5b` and desktop `67cb9e15` release live
publisher/engine guards after original ordered
capture, then checks the original owner/revision, enabled Inspect tab and static
report generation plus native proof before adoption. Clearance changes, source
refresh, another Check, hide/reopen and tab changes retire that generation.
Genuinely admitted MCP static queries release both process-lease and publisher
guards for computation/serialization; original proof, ticket retention and
liveness/delivery-only retry remain required. Swept motion at that historical
checkpoint remained guarded; the following qualified motion work supersedes
that disposition. The three new native/UI/attached-ticket
regressions passed once at `67cb9e15`, including independent overlap/clearance,
zero-pair native replacement, unchanged-render-stamp pose changes, hide/reopen
and original-ticket delivery-only retry. This does not qualify live UI, memory
or deployed performance.

The source gate for #333 now rests on its explicit ownership, independent
invalidation, asset/history retirement and stale human/agent fence criteria.
The final concrete CSV repair is qualified below. Knowledge resources are
static bundled data. Shared Arcs and source tests do not establish universal
process memory containment, physical GPU release or FPS/stutter results; those
are limits rather than additional closure gates. Broad UI and manual MCP
mutation coverage remain paused, with existing targeted passes retained.

### Swept collision and contact-aware playback: source checks passed

The next bounded native and desktop slices detach unchanged swept and contact
algorithms from live document guards. They own required authored Assembly sampler
intent, share the immutable Scene input and copy only genuine selected native
sources. Swept work retains visible first-sample definitions; contacts retain
enabled pair definitions even when hidden. No all-frame array, complete model,
SketchManager or history clone is captured. Authored Assembly cloning and
per-frame solver work remain intentional scaling-dependent boundaries.

Private finished proof retains exact owner/session, Weak Scene, independent
revisions, authored input equality and all dependent native identities including
absent noncandidate shapes. This catches raw study/contact/placement/native
replacement even without a render stamp. Existing UI receipt/one admission and
MCP original ticket/completion/liveness rules remain authoritative. Swept-only
inputs retire their own generation without invalidating static reports; late
playback results require current serial, selected study and enabled Motion tab.
Pause, Stop, Seek, draft/hide/tab and source replacement therefore retire stale
preview adoption. Cancellation and original source models remain intact.

Three new source regressions passed once at `f2c47dd9`, and three at `1472b342`.
The original fixture compiler failure remains recorded before zero executed
regressions. Physical oracles independently cover five/full versus three/early
swept samples and a 0.5 mm contact stop at time 0.3/Z3.5 with zero overlap.
All older passing filters remain recorded without reruns. The combined direct
Bevy merge `39ca1a1e` receives strict Windows compilation checks after incoming
section-worker replacement, not another execution of those passed cases.
[Source and exact receipts](detached-swept-contact-20261011.json) distinguish each
revision and retain separately authored macOS renderer/section evidence.

Mechanism startup also removes a full assembly wire round trip (`1fb5c668`):
the original receipt now wraps a borrowed eligibility callback, which returns
one bool before drag setup. `can_drag_occurrence` still clones required authored
projection/graph inputs; this is not a no-copy or constant-time claim.

These are isolated production worker/native/ticket checks, not visible human
input, physical GPU, deployed responsiveness or memory containment. Every
existing UI/MCP leaf and receipt remains intact, broad coverage stays paused,
and the incoming renderer/section rechecks remain visible. At that checkpoint
motion CSV remained the identified source gate; the qualification below completes
that concrete repair.
Broader human cutover/display/drawing/slicer acceptance remains separate; a
universal benchmark or exhaustive unrelated service graph is not added to the
original issue criteria. Main and Jack's PRs remain untouched; all work is
retained in PR124/both Bevy aliases.

### Motion CSV and original #333 acceptance disposition

Native `37a76655` and desktop `e224021b` capture the required authored Assembly
and immutable Scene under the original admission. The existing pure sampler
and 14-column CSV formatter run outside engine/publisher/process-lease guards;
there is no native geometry dependency or added B-rep copy. Finished private
proof drops the strong Scene input and retains Weak Scene identity, original
host/session, independent revisions and exact authored intent. Required owned
assembly/solver inputs and output bytes remain scaling-dependent boundaries.

The actual Motion state owns a private revocable file-operation authority from
picker creation through atomic publication. Cancel, changed draft, hide/tab,
replacement, retired picker and State drop revoke it. Final lock order is
publisher -> operation lease -> engine source proof -> atomic write. Detached
computation holds no file lease. Admitted MCP transfers CSV ownership rather
than cloning the wire output; private source validation and original ticket
completion/liveness rules still retain the result for delivery-only retry.

Strict changed-package Windows compilation and exactly three new regressions
passed once. A genuine repeated stock/slider has independent times
0/0.25/0.5/0.75/1 and Z positions 5/3.75/2.5/1.25/0, speed 5 after its initial
zero and the existing finite-difference acceleration 20 at the first step.
Actual worker/MCP reentry, overwrite, cancellation/retirement, raw study,
same-model Scene replacement, owner changes and unchanged model checks pass.
[CSV receipt and original seven criteria](detached-motion-csv-20261011.json)
map this evidence plus retained foundation/asset/history/input and detached
drawing/export/section/static/swept/contact evidence. Historical passes are
identified as retained, not fresh executions. No concrete listed source
acceptance gap remains; closure is scoped to that original contract after push.

The kernel still tessellates the whole live scene after solid recompute.
Authoritative body generations allow checked reuse of unchanged renderer assets;
body IDs, pointers and topology alone do not. Legitimate in-flight readers retain
their owned input until completion. Source qualification is not a claim of
universal memory containment, GPU release, timing, human routing or full-app
completion. Existing deployment, affected renderer/section and other open-issue
acceptance remains tracked without repeating passing cases or extending #333.
