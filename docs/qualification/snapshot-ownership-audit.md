# Native snapshot ownership audit

Source review for [#333](https://github.com/limo-cad/Limo-CAD/issues/333), based on
the historical `278025a52247c7316c8553fd2a390f9179150733` census and the
bounded source follow-up through clean `fd85a556` on 2026-10-11. Scoped STEP,
native/attached drawing and affected history/input qualification is recorded
below. The finite section/static-interference follow-up at `67cb9e15` is recorded
in the later isolation section and its exact qualification receipt.
This is a concrete inventory
of production snapshot APIs and their desktop consumers, shared host/MCP read
contracts, and the document caches they feed. It is not a census of every clone
in the repository, a benchmark, or live UI qualification. Test fixtures and
JSON-schema construction are excluded from production copy counts.

The narrow active-sketch browser-name follow-up below is based on
`96b642b9fb481daa77351e7eedccd917ee112fc1`; its source review is separate from
runtime or performance qualification.

## Copy inventory

Each entry records the consumer, owned data and scale, owner/lifetime,
invalidation boundary, and remaining disposition. Paths under
`desktop/src/session_bridge/native_interface/` are abbreviated as `native/`.

1. **Coherent viewport frames**: `crates/native-engine/src/host.rs`,
   `NativeEngineHost::viewport_frame` and `solid_scene_snapshot`.
   `SolidSceneDto` is shared by `Arc`; ordinary reads do not duplicate its mesh
   or topology buffers. A metadata-cache miss owns sketch DTOs, datum planes,
   profiles and appearances, scaling with authored entities. Placement misses
   own body/occurrence pose vectors, scaling with placements. Each warm project
   owns its cache; frame consumers share those allocations. Metadata and
   placement invalidate separately (see below). This caching is implemented.

2. **Synchronous geometry consumers**: `native_editor/support.rs`, native
   menu body checks, `files/io.rs::capture`, `files/bambu.rs`, named views,
   print-intent controls, and `controller/section_review.rs`. Their
   `solid_scene_snapshot` calls retain an Arc for the local computation;
   collecting body IDs, choices, errors or summaries allocates those results,
   not tessellation copies. Selected export intent retains IDs/settings in an
   Arc across dialogs, not the scene/kernel. Native feature edit/cancel
   snapshots can retain a coherent viewport frame until the edit ends. These
   intentional holders can outlive one frame; a weak cache stamp alone cannot
   prove every reader has released its Arc.

3. **Retained browser/history panels: repaired**: browser synchronization owns
   only title, units and hierarchy (`3181443d`); history synchronization owns
   `Arc<FeatureTree>` (`db1aa006`). Short manager reads compare their consumed
   metadata and reuse the projection when unchanged. Owner replacement retires
   the prior holder before reuse. Costs scale with their respective hierarchy
   or history; neither panel retains the complete DocumentDto or geometry.

4. **History and feature edit boundaries**: history drag shares the current
   FeatureTree only at an exact owner/revision receipt; otherwise it captures
   that narrow tree under the publisher-to-engine guard. Drag needs stable
   ordering/rollback inputs until drop or cancellation. `native/feature.rs`
   `Snapshot::capture` retains document metadata, parameters/source occurrences,
   decoded assembly intent/solution and its shared viewport for Apply/Cancel.
   Required edit/recovery state stays owned across events; a borrowed guard
   must not replace that lifetime.

5. **Undo/Redo decision reads: repaired**: `native/history.rs`
   `apply_native_history_guarded` previously copied a whole DocumentDto to read
   rollback position, feature count and latest feature ID. One `with_document`
   callback now returns those three scalars coherently. Its engine guard ends
   before history dispatch. Existing owner/revision/control checks, empty
   history errors, DeleteLatest identity, owned Undo model tickets and
   transactional restore/history snapshots remain unchanged. Owned complete
   model strings/history tickets are required for recovery across mutations.

6. **Drawing paper and editors**:
   `native/controller/workbench/drawing_paper_view.rs::paint` owns one
   shared DrawingDocumentDto Arc per owner/revision receipt; annotation
   `runtime.rs` and `drawing_editor.rs` share the authoritative Arc, including
   guarded fallback when no matching paper publication exists.
   Ordinary reads share that Arc without copying authored data. Its retained
   allocation scales with sheets/views/annotations/templates; successor edits
   may require an owned mutation result. Native projected line buffers are
   separate derived output. Editor retirement releases the applicable Arc;
   dirty draft protection must run before transitions. Paper artwork retains
   one selected-sheet DTO after successful rendering, keyed by geometry revision,
   sheet content and units; a rejected frame moves that same snapshot into its
   error receipt. Cost scales with that sheet's authored data. A selected
   annotation or sheet/view draft owns the edited record/fields for Reset/Apply.
   `drawing_authoring/hole/submit.rs` creates a changed drawing document for the
   existing whole-document setter; this is an owned mutation result, not an
   inspection snapshot.

7. **Native print preparation: repaired**: `native/controller/files/printing.rs`
   `prepare` formerly copied every drawing sheet/template only to find the
   active sheet ID/name. Its short `with_drawing` callback now returns that ID
   and one name String. No active-sheet fallback was added: missing selection
   still returns `Select a drawing sheet to print`. SVG export and the immutable
   `native_print::Page` remain owned output, sized to the exported page. They
   cross worker/native dialog boundaries, with the existing repeated document
   receipt checks. The drawing callback ends before export or title lookup
   reacquires the engine lock.

   `files/drawing_output.rs::capture` formerly serialized and decoded the whole
   drawing document for SVG/DXF export selection. Its `with_drawing` callback
   now returns only the selected sheet ID/name and requested format under the
   existing owner/revision fence. Missing active-sheet selection still returns
   `Select a drawing sheet to export`. The owned output intent survives the
   picker; delayed export owns its SVG/DXF/PDF payload. Prepared selected native
   copies move HLR/encoding/Page work outside live engine and publisher guards;
   final original receipt and I/O validation precede atomic publication. Native
   Print uses the same detached export/Page seam with separate native-dialog
   ownership. Document validation remains at existing setters/loaders.

8. **CAM intent consumers**: `native/controller/workbench/cam.rs` owns
   CamDocumentDto on editor owner/revision refresh; `cam_view.rs` owns an Arc
   per owner/revision/selection key and clears it when inactive, cancelling
   pending work. `cam/central.rs` owns the project CAM intent when opening the
   library. Inactive/replaced-owner retirement (`0bd892bc`) clears the project
   baseline and receipt/draft; active library data and required readers retain
   their bounded lifetimes. These copies scale with
   tools/setups/operations and regeneration fingerprint stamps. Derived toolpath
   motion and NativeViewportFrame geometry are not stored in this DTO.
   `cam_view.rs` previously discarded the DTO returned by `cam_snapshot`
   when reading its warning. Its warning-only accessor now reads the same
   setup validation under a short engine guard without copying CAM intent.
   The existing owner/revision fence still covers that read. The unchanged
   compatibility `cam_snapshot` accessor still returns owned CAM intent.
   The actual `cam_view.rs` worker retains the existing Arc document so voxel
   work can run after the engine guard ends. Preserve that worker snapshot.

   `cam_export.rs` previously copied the whole CAM document to derive its
   retained Post NC/event-stream draft. The new `with_cam` callback selects the
   same setup/operation/default and calls the unchanged `Draft::new`, returning
   only owned posting fields/configuration and summary Strings. It preserves
   `Choose a CAM setup`/`Choose a setup to post` errors, the exact receipt fence,
   and worker/export snapshots. Summary construction scans current setup/tool
   metadata under the engine guard; it is not constant-time. The callback does
   not reenter the host/bridge or retain a borrow across user events. Full CAM
   editor/library snapshots remain intentional lifetime boundaries; CAM
   selection-only reuse and inactive/replaced library retirement have focused
   source passes, while deployed memory-pressure acceptance remains open.

9. **Borrowed menus and actions**: `native/controller/browser.rs::action_node`,
   history validation/rename/edit preparation, file export readiness, sketch
   edit validation, `feature/editing.rs`, and `workbench/ribbon_menu.rs` use
   `with_document`/`with_drawing`/`with_cam` callbacks. They return only necessary
   IDs, names/choice lists, booleans or history indices before updating
   ECS or entering another engine method. The callbacks inspected here do not
   reenter the engine. Ribbon sheet tabs clone at most six names; datum/profile
   choices scale with the displayed metadata. These are owned UI outputs,
   not copies of the source drawing/model geometry.

10. **MCP and transport**: `crates/sketch/src/host.rs::handle_read_only`
    serializes `solid_scene_ref`, `drawing_document_ref` and
    `assembly_document_ref` directly. `mcp-server/src/lib.rs` summary,
    section/projection, preflight and mesh export paths borrow source scene
    and assembly structure; generated query/export payloads are owned
    transport results. `document` still derives an owned DocumentDto, and
    `cam_document` now serializes `cam_document_ref()` under the same manager
    read (`8352889c`), without a full temporary CAM clone. DocumentDto assembly
    remains an owned metadata/wire boundary, rather than evaluated mesh data.
    Owned tessellation/export meshes, JSON/base64 and saved model
    strings scale with the requested output and must remain valid after the
    call. The compatibility API `NativeEngineHost::viewport_snapshot`
    explicitly returns owned geometry/metadata. No ordinary desktop menu or
    navigation consumer was found using that compatibility method; current
    desktop matches are fixtures/tests. `native_viewport/ui_lab.rs` is an
    opt-in capture fixture and is excluded from normal UI copy claims.

## Native UI entrypoint census

The historical finite scan at `278025a5` covered the production desktop
calls to `document_snapshot`, `drawing_snapshot`, `cam_document_snapshot`,
`viewport_snapshot`, `solid_scene_snapshot`, `viewport_frame`, `cam_snapshot`,
borrowed document/drawing callbacks and the CAM warning accessor. It also covers
multiline literal `engine_call` reads for document/drawing/CAM/assembly intent,
assembly solution, active sketch, named views, print intent and project model
export. It found 73 matching source blocks in 34 files before the then-current
borrowed CAM/drawing reductions. Blocks can contain multiple calls; this is not an allocation count.
Tests, replay/capture fixtures and schema construction were excluded. Dynamic
dispatch, aliases and derived-output consumers were inspected by the groups
below, rather than assumed absent because they lack a literal API name.

The exact source excerpts remain outside the repository at
`D:/limo-cad-maintenance/backlog-autonomous-20261010/issue333-native-entrypoint-census-278-source.json`.
The covered native entrypoint groups and their lifetime dispositions follow.
Unresolved projection suggestions from that historical census are inspection
leads, not proof of a remaining current defect. The repaired paths are updated
below and in the later export-isolation section:

- **Host/publication/save/history bridge**: `native_interface.rs`,
  `native/workspace.rs`, `native/publication.rs` and `native/history.rs`.
  Published model text/active-sketch payloads and archive/history tickets are
  owned across broker/write/mutation/recovery boundaries. Publication uses the
  existing reservation/owner fence and releases engine reads before publishing.
  Published snapshot reads are not instantaneous live-engine snapshots. The
  model-frame alias shares `viewport_frame` Arcs. Active-sketch presence and
  Undo/Redo availability now use the borrowed session scalars (`8d4acaea`),
  removing the historical whole-sketch serialization from history decisions
  and fallback publication focus. Public/editor sketch DTOs remain owned where
  their full content is required. Original owner/history ordering and the
  poisoned host-lock behavior are preserved.

- **Sketch and mechanism input**: `native_editor/mod.rs`, `support.rs` and
  `mechanism.rs`, plus the `active` helper consumers in `palette.rs`,
  `annotations.rs` and `interaction.rs`. Palette/annotation/hover snapshots own
  sketch DTOs keyed by editor stamp; they survive pointer/paint events and share
  retained geometry. A mechanism drag validates `can_drag_occurrence` from a
  decoded assembly under the receipt fence, then owns scalar pose/camera state.
  That predicate is another projection candidate; do not hold a host guard
  across the drag. Browser sketch-name lookup now borrows the manager's active
  session name under the existing engine guard and owns only that optional
  string. It no longer serializes/parses a complete SketchDto for the name;
  browser receipt/revision refresh and retained document lifetimes are unchanged.

- **Browser/history/feature ingress**: `native/feature.rs`,
  `native/feature/editing.rs`, `native/controller.rs`, `controller/browser.rs`,
  `controller/history.rs`, `controller/history/panel.rs` and
  `controller/history/drag.rs`. Borrowed menu/history validation returns owned
  metadata. Browser/panel snapshots, drag intent and feature edit/cancel
  snapshots retain exact receipts across events; shared frames carry geometry.
  Clean browser/history use the narrow projections above; edit/recovery
  snapshots still require their owned lifetime.

- **Assembly/named-view/section ingress**: `controller/assembly.rs`,
  `controller/named_views.rs`, `named_views/panel.rs` and `section_review.rs`.
  Assembly state retains an Arc per exact receipt; joints/studies/inspection
  borrow that retained intent. Named-view choices own IDs/names/placements and
  section/body selection shares scene Arcs. Named-view/export diagnostics now
  borrow assembly intent only to own the requested occurrence labels (`fd85a556`).
  Section query/output buffers are derived owned results, not copied source
  scenes, and stale worker results remain owner/revision fenced.

- **File/export/print ingress**: `controller/files/io.rs`, `files/bambu.rs`,
  `files/drawing_output.rs`, `files/panel.rs`, `files/printing.rs`,
  `controller/print_intent.rs`, `print_intent/heights.rs` and
  `workbench/ribbon_menu.rs`. Menus borrow selection metadata or share scene
  Arcs. Print/SVG/DXF selection now retains only sheet identity/name/format.
  Archive, STEP, print, Bambu and drawing workers must own their selected intent
  and requested output across dialogs/writes. Print/Bambu compound summaries
  still decode model/print/assembly intent; their scalar/output consumers need
  individual projection review rather than blanket removal of export snapshots.

- **Drawing workbench ingress**: `workbench/drawing_paper_view.rs`,
  `drawing_editor.rs`, `drawing_authoring/runtime.rs` and
  `drawing_authoring/hole/submit.rs`. Paper, sheet artwork and editor drafts use
  the retained document/selected-sheet lifetimes described above. The hole
  submission creates owned changed intent for the setter. Shared paper Arcs
  avoid duplicate editor captures when their receipts match; fallback captures
  remain owned until their editor retires.

- **CAM workbench ingress**: `workbench/cam.rs`, `cam/central.rs`, `cam_view.rs`
  and `cam_export.rs`. Editor/library and background voxel work retain owned
  intent with receipt/generation fences. Posting now projects an owned draft;
  warning-only synchronization no longer copies CAM intent. Prepared program
  bytes/events/warnings are owned output. The compatibility `cam_snapshot`
  method has no ordinary desktop production caller in this census.

Direct feature/forms contexts borrow retained metadata; generated mesh/HUD/line
buffers and operation/transport outputs have their own owned lifetimes. The
opt-in script-preview service owns produced scenes and bounded frame/cache
payloads in a separate worker context; it does not read the live UI document
through these native entrypoints. No script/recipe was executed for this audit.
This closes the bounded native ingress inventory, not the full application
allocation graph, service/cache architecture or memory-pressure acceptance.

## Cache invalidation and lifetime matrix

- **Authoritative warm project / viewport cache:** native engine mutex protects
  each manager/kernel/cache. Ordinary `handle_read_only` calls return before
  invalidation. Drawing/CAM/print intent changes, document title and non-sketch
  feature rename retain viewport metadata; a renamed sketch invalidates it.
  Assembly, named-view and visibility commands invalidate placement only.
  Authored sketch/solid changes invalidate metadata; recompute publishes new
  geometry on commit. The execute path conservatively invalidates metadata
  before preparation; that does not prove a stale-data defect or justify a new
  caching architecture. Warm tab activation keeps the project cache. Cold
  thaw constructs and verifies a replacement before publishing it.

- **Installed frame / model resource:**
  `desktop/src/native_viewport/platform.rs::ModelResource` strongly retains the
  active shared document/pose vectors; replacing the frame releases the prior
  active holder. `ModelInstanceState` retains identity/visibility layout per
  document, not another full scene. Camera, HUD, annotations and preview mesh
  revisions are separate from geometry. Camera Fit uses installed geometry.

- **Body meshes / local edges:** the document-indexed `BodyMeshCache` owns mesh
  handles, and `ModelEdgeCache` owns local bounds/edge-side/face-boundary data.
  Both store weak scene identity stamps. Occurrence/visibility changes can
  reuse body mesh handles. Private authoritative body-generation proofs now
  preserve unchanged same-document meshes/edge Arcs across successor scenes;
  missing/error/stale/isolated proof or evicted asset falls back to rebuild. Close/eviction drops only
  the addressed document entities/cache/instance state. Weak stamps do not
  pin CPU scenes, but handles and derived boundaries intentionally retain
  assets until that retirement.

- **Drawing derived source/raster cache:** `workbench/drawing_edges.rs::EdgeCache`
  retains current and previous source/raster results. SourceKey separates
  document/geometry revisions from an Arc layout containing owner epoch,
  sheet, views and relevant styles. Refresh replaces layout only when those
  inputs differ. Only committed adjacent sheet-selection revisions can retag
  reusable source; edits/Undo/replacement miss the exact key. Document eviction
  clears matching current/previous/error entries and paper geometry. Retaining
  one previous result is deliberate rollback/error presentation, not a claim
  of unlimited sheet caching. Cold native projection now owns selected shape
  copies and coherent source intent on its worker; finished native projection
  reuse also checks selected geometry and resolved intent exactly. Those native
  copies are required to prevent concurrent mutation of live OCCT shapes.

- **Editors / background work:** drawing/CAM clean inactive editor snapshots
  retire on the matching document policy; dirty drafts remain protected until
  Apply/Reset or explicit document close. Workers carry receipts and immutable
  payloads, validate owner/revision/control before application and reject stale
  completions. Pending native print SVG/page storage is local to the operation.
  Source correctness does not establish that every live dialog cancellation
  releases at the expected time. CAM process-service cache budgets and
  cancellation rules are documented in [CAM_SIM_PLAYBACK](../CAM_SIM_PLAYBACK.md);
  their complete global lock/lifetime graph remains outside this bounded audit.

- **Cold retention / saves:** workspace inactive-LRU/memory policy excludes the
  active tab, unfinished sketches and in-flight saves. Engine eviction replaces
  the warm manager/kernel/cache with model text, body IDs/errors, sketch recovery
  and named-view metadata, not a scene Arc. File/archive/history receipts remain
  in the workspace/publisher. Cold restoration validates IDs/errors/sketches
  before swapping. This structural policy plus weak cache stamps does not
  imply total-process memory containment or prove no temporary reader retains
  an old scene.

## Lock order and ownership

1. **UI/MCP/worker document operations:** publisher mutex -> owner/session/epoch
   validation -> engine mutex. `with_native_document_receipt` retains the outer
   publisher fence while its callback runs. `with_document`, `with_drawing` and
   `with_cam` hold only the inner engine guard for the synchronous borrowed read. Never call
   another engine method or publisher-locking bridge method inside them.
2. **Print/drawing-output/background exports:** coherent selected native
   copies and intent are captured under the original receipt, then release both
   publisher and engine before HLR, mesh/ZIP/STEP serialization or Page work.
   Final original owner/revision and actual job/I/O validation remain separate.
   History/CAM posting callbacks return owned scalar/draft inputs before another
   engine call; their existing mutation/history transaction fences remain.
   Print result storage is taken independently before receipt reentry; no slot
   guard is held across it. Long work never holds the export-operation lease.
3. **Frame/cache synchronization:** capture a coherent Arc frame under the
   engine mutex, release it, then change ECS/cache state. Pointer/camera reads
   use retained frame/geometry and do not borrow an engine guard across events.
   Owned edit snapshots carry exact receipts instead of retaining mutex guards.
4. **Read-only routing:** `engine_call` checks immutable host dispatch before
   mutation invalidation; native inbox uses the registered read-only flag to
   preserve model revision and draft ownership. Read-only JSON allocation is
   not a model mutation. This is a reviewed path-specific order, not proof of
   the full application/service lock graph.

## Follow-up export isolation and remaining qualification

The earlier borrowed selection reductions did not remove the long live holds
inside native HLR, mesh/ZIP and STEP output work. The bounded subsequent repairs
are documented in [the Bevy ownership audit](bevy-ownership-audit.md#detached-native-work-and-final-publication):

- Cold paper has coherent selected-shape/scene/assembly/sheet worker snapshots,
  exact stale source rejection and bounded finished-projection reuse. Paper
  errors retain hidden parent identity while retiring stale children.
- Drawing File SVG/DXF/PDF and native Print capture selected native shapes and
  intent briefly, then prepare output/Page outside live guards. Final file or
  dialog owner/revision checks remain separate from computation.
- Bambu inspection/previews own exact context and selected shapes outside the
  publisher. WRITE additionally needs the current dialog-operation lease,
  exact native owner/model/geometry registration and final atomic write.
- STL/3MF use a distinct Files export-job lease and selected native copies plus
  coherent units/placements/name/appearance/structure/bed intent. Original
  writers and layout checks run outside document locks.
- STEP uses that job lifecycle and copies the effective exact body selection;
  AP242/thread/units/occurrences remain unchanged. A process-global STEP exchange
  mutex protects OCCT initialization/parameters/transfer/serialization, without
  granting document or file authority. Concurrent STEP exchanges serialize.

Copies that enable native background isolation, editable Apply/Cancel drafts,
recovery/history, output bytes and genuinely running immutable readers are
required. Final job locking is publisher -> operation lease -> native validation
and atomic write, never long meshing/HLR/ZIP under that lease. UI revocation has
no inverse lease-to-publisher callback. Bambu metadata preparation after an actual
write cannot retroactively claim the file was not written.

### Selected-body section computation: source checks passed, live acceptance pending

`NativeEngineHost::prepare_section_review` captures one selected genuine B-rep
after the existing request/source validation (`b11a727b`, `f9ce64e1`). The owned
copy is required for native thread isolation. Capture remains a live engine
read with geometry-dependent copy cost; native section/probe/cutaway computation
then runs detached. Private finished provenance retains host/session ownership,
a Weak scene stamp, geometry/metadata revisions and actual selected-body kernel
identity. The weak stamp alone does not pin a retired scene's buffers.

Desktop `3013d119` uses the existing single ordered worker slot and one original
dispatch admission. UI receipt capture ends before computation and CPU raster/
cutaway preparation. Adoption checks the original owner/revision, visible panel
generation and opaque native proof before adding image assets or adopting the
report. Close, changed draft, edits and document/kernel replacement reject
prepared data. Attached MCP queries with genuine native tickets release both
process-lease and publisher guards for computation/serialization, then recheck
private native proof under the original completion guards. The genuine outcome
is retained before fallible liveness publication; delivery-only retry does not
run section work again. No-ticket compatibility and other query routing remain
unchanged. Required editor/recovery snapshots retain their existing lifetimes.

The ordered UI and attached-ticket filters passed once at `3013d119`; the native
section filter passed at `67cb9e15` after correcting its boundary-versus-outside
fixture assertion. Strict Windows source checks and exact artifacts are in
`docs/qualification/detached-section-interference-20261011.json`.
The original human exact180 section pass is selectively marked needs_recheck
with its receipt retained. These focused source checks do not establish live UI behavior,
whole-application ownership, memory containment, FPS or latency.

### Static interference computation: source checks passed, live acceptance pending

Static interference captures coherent solved occurrence poses and the current
scene, then deep-copies only candidate native bodies for exact distance/Common
work. Repeated occurrences share a copied body definition. The immutable scene
is a legitimate in-flight broadphase input; finished provenance keeps only its
Weak stamp. Capture/copy and final validation retain their bounded live guard
scope, with geometry/pose-dependent cost.

Private source proof includes exact host/session ownership, scene/revision
stamps, current solved poses and native identities for all visible request-selected
body sources, not just candidate pairs. Empty reports still reject native source
changes; absent non-candidate shapes retain absence without a new admission gate.
Native `2045da06`/`b52d9a5b` and desktop `67cb9e15` separate computation
from original ordered UI and genuine
MCP-ticket guards. UI adoption additionally checks the enabled Inspect tab and
static generation; clearance/source/new Check/hide-reopen/tab changes retire it.
MCP retains the original once-started outcome before liveness publication and
retries delivery only. Swept native motion keeps its existing guarded path.
The native and two desktop regressions passed once at `67cb9e15`; the same
receipt records genuine repeated-body overlap/clearance, zero-pair replacement,
pose changes without render-stamp changes and original worker/ticket rejection.
Required
draft/recovery/output ownership and earlier evidence remain intact; source review
does not qualify deployed responsiveness or memory/performance behavior.

Keep #333 open. Historical census counts/excerpts remain historical, not an
updated whole-repository clone count. Per-body upload reuse is now implemented
using private checked kernel owner/body generations and exact committed scene
proof; body IDs/topology/pointers still cannot authorize it. The wider service
lock graph, memory pressure and actual deployed background-holder/render behavior
remain acceptance work.

Browser/history/CAM, native identities/body Assets and earlier transport/input
passes are retained in [the Bevy ownership audit](bevy-ownership-audit.md).
The `1c0dc76e` drawing/PDF/inspection/preview checkpoint passed six exact filters
once with strict Windows source checks, retaining three earlier passes:
[receipt](detached-drawing-output-20261011.json). This includes independent PDF
physical dimensions and actual detached native lock reentry; it does not qualify
successful native Bambu service/toolpaths or physical OS printing.
[The newer export receipt](detached-export-ownership-20261011.json) records
strict Windows source checks and exactly three focused passes at `9e16fade`:
Bambu WRITE, portable STL/3MF and borrowed panel review. The existing `9cdf863d`
Print-to-File option check is retained without repetition; it is not an OS
print acceptance. The panel comparison preserves exact choices/model equality
and real worker-owned payloads, removing only a repeated temporary model copy.
The authentic four-plate template does not positively qualify single-plate
ResolvedScene layout. At clean `fd85a556`, integrated STEP `d493f04e`, native
drawing-export `27ec6551` and desktop `4382a5a8` passed their three new regressions
once, plus strict Windows desktop/native-engine/sketch/OCCT/export checks.
[The STEP/drawing receipt](detached-step-drawing-20261011.json) records exact
artifacts, stale-source/lock/ticket/job cases and the initial strict-lint failure
corrected without weakening the independent bounds oracle.

The same source also removes full frame copies from sidebar wheel and busy
Save/Escape routing at each original separate read (`d20d62b1`), and borrows
assembly data solely for requested named-view/export warning labels (`fd85a556`).
The specifically affected real history and busy Save checks passed once;
unrelated earlier passes and the generic pointer projection check were retained.
No mirror test was added for label formatting. Installed EB and retained UI
passes remain separate from these source revisions. This census update ran no
additional tests or live UI/MCP actions. No deployed, universal latency/FPS,
memory-pressure, physical GPU release or final #333 closure is claimed.
