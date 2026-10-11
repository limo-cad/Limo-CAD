# Bounded Bevy ownership audit (#333)

This is a source inventory, not an FPS, latency, memory-pressure or deployed
application qualification. It covers the native browser/history and CAM snapshot
consumers inspected on 2026-10-10. Other UI/MCP call sites still require audit.
Previously integrated shared scene, pose, metadata and immutable drawing work is
not reimplemented here.

## Avoidable ownership removed

- `controller/browser.rs::synchronize` previously cloned all of `DocumentDto`
  at each aggregate revision. Commit `f3c0357e` retains only the browser's title,
  units and hierarchy. A short `with_document` read compares these inputs; an
  unchanged projection reuses its Arc. Owned rows are necessary after the engine
  guard ends. Cost scales with the browser hierarchy, excluding feature history.
  Owner replacement clears the predecessor before considering reuse. Hiding the
  panel intentionally preserves this bounded projection and collapse state.
- `controller/history/panel.rs::synchronize` and `history/drag.rs::pointer`
  previously retained full `DocumentDto` values. Commit `a9a24bc1` owns only
  `FeatureTree`: ordered IDs, names, kinds, suppression/status and rollback cursor.
  The panel reuses unchanged history across aggregate revisions; exact owner and
  revision receipts remain independent of Arc identity. Drag startup shares the
  rendered Arc only at its exact current receipt, otherwise takes a narrow
  snapshot under the publisher-to-engine lock order. Drag previews need stable
  order/cursor inputs until drop or cancellation. Comparison/copy cost scales with
  feature count and names/status text; browser hierarchy is excluded.
- `controller/workbench/cam_view.rs::synchronize` previously copied the entire
  CAM document on a selection-only preview-key change. Commit `a85c78d3` reuses
  the retained document at the same owner/revision. Selected setup/operation and
  warnings are still recomputed; job key, cancellation and generation remain
  selection-sensitive. A changed revision or owner requires a new snapshot.

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

## Evidence and remaining acceptance

The three commits add production-path regressions for relevant invalidation,
owner replacement, retained-reader retirement and read-only model preservation.
The CAM regression exercises the same guarded input acquisition used by
`synchronize`, without starting simulation threads. Formatting and diff checks
passed in the isolated source worktree. No builds, tests or live UI acceptance
were run by the source worker; retained CI/manual receipts belong in the primary
status/coverage records.

This does not close #333. Remaining work includes broader menu/window/MCP and
background-export lifetime/lock-order inspection, document-cache behavior under
memory pressure, and finer render invalidation. Stable per-body kernel revisions
or dirty-body identities are needed before claiming correct per-body incremental
updates. A shared Arc alone does not prove inactive-document eviction, worker
retirement or reduced stutter across the application.
