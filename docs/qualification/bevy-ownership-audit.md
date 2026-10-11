# Bounded Bevy ownership audit (#333)

This is a source inventory, not an FPS, latency, memory-pressure or deployed
application qualification. It covers the native browser/history/CAM snapshots,
presented input/window projections, MCP response/request ownership and committed
body render consumers inspected on 2026-10-10. Other UI/MCP call sites still require audit.
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
  baseline scales with the complete CAM document; independent CAM revisions or
  shared editor baselines remain potential future work, not completed here.
- `workbench/cam/central.rs::reduce` captures a receipt-bound project baseline
  when opening the tool library. `central/model.rs::import` clones that baseline
  to replace/add one tool and preserve unrelated project records and identities.
  Publishing also needs the selected project tool. Narrowing the saved baseline
  to tools alone would change the existing whole-document import contract.
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

Further source inspection confirms exact closed-document retirement, cold-owner
cleanup after worker errors, weak scene stamps and document-specific mesh/entity
removal. Existing close/low-memory regressions were not rerun. The actual kernel
still meshes every live body after solid recompute. The new renderer can preserve
unchanged body uploads/derived edges only through the authoritative identities
and exact scene proofs described above. Body IDs, pointers and topology
signatures alone are not geometry revisions. Both new native/Assets checks
passed; this does not qualify deployed rendering or physical GPU memory.

This does not close #333. Remaining work includes broader menu/window/MCP and
document-cache behavior under memory pressure and deployed render acceptance.
The bounded drawing/mesh background-export inspection found an existing exact
publisher-to-engine receipt fence, single pending job, final owner/revision check
and atomic publication; no additional defect was established there. Knowledge
resources are static bundled data. Other consumers remain unaudited. A shared
Arc alone does not prove worker retirement or reduced stutter across the application.
