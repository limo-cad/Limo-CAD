# Projected support-face boundary in a sketch

A sketch created on a planar body face receives **that face's boundary edges as
projected geometry**. The boundary can close a region the user drew against it,
so a semicircle whose two endpoints rest on the face edge becomes a selectable
profile for Extrude, Revolve, Sweep and Loft instead of staying an open
chain.

This is the behavior a user expects from "sketch on this face": the face they
picked has edges, and those edges are part of the sketch.

## What is projected

| Property | Value |
|----------|-------|
| Source | The support face's own edges (`FaceDto::edge_keys`), not every coplanar edge of the body |
| Fallback | Scenes that publish no boundary keys (some imported or assembly bodies) project every coplanar edge, the same set the snap midpoints use |
| Shape | The body edge's tessellated polyline in sketch coordinates, plus the exact circle when the edge is circular |
| Lifetime | Saved at the sketch's history stage; refreshed from stable `EdgeId`s only when that stage's scene is available |
| Identity | Reserved `id` range above every authored entity id (`PROJECTED_EDGE_ID_BASE`) |

`ProjectedEdgeDto` is published in the session and persisted as
`ProjectSketchV2.support_boundary` in schema 9 (introduced in schema 8). Replay needs this snapshot
before planning any solid consumer. A missing `support_boundary` means a
legacy authored-only sketch: opening or recomputing it must not insert
remainder regions and reinterpret saved numeric profile indices. New face
sketches opt into boundary profiles, including the surrounding region when it
shares authored edges.

## Profile rules

Segments derived from projected edges join the same segment graph the authored
curves build (`profile_catalog_item` in `crates/sketch/src/manager.rs`), which
is what lets the projection seal a loop. Two rules keep authored regions
selectable in new sketches:

1. **A face bounded only by projections is not a profile.** The support
   boundary is a real face of the planar subdivision, but the user never drew
   it. `extract_bounded_faces` reports `authored_edges` per face and the
   catalog drops faces with zero.
2. **Nesting only sees emitted profiles.** Because the bare face outline is
   never emitted, a rectangle drawn on a face stays at nesting depth 0. Adding
   the outline as ordinary sketch geometry would have made it the depth-zero
   outer loop and turned every drawn shape inside it into a hole — a shape that
   can no longer be extruded.

Provenance survives noding through the reserved id range: a piece shared by
authored geometry and a projected edge keeps the smaller authored id
(`node_segments_impl` dedupe), so it still counts as authored.

The kernel still receives analytic curves. A projected straight edge becomes a
line, a projected circular edge becomes one arc (or a full circle), and
anything longer stays an explicit polyline (`projected_profile_curve`).
Exact authored contacts on circular carriers are inserted into the subdivision
samples before noding, so endpoints between display vertices still close a region.

### Persistent region identities

Profile `index` is now a persistent ID, not its position in a sorted array.
Schema 9 saves a directed source-curve cycle for each discovered region, plus
tombstones for removed regions. Dimension edits, translations, catalog
reordering and save/reopen preserve unambiguous identities. Split/merge changes
get new IDs; existing consumers report a missing profile instead of choosing
another region. Ambiguous cycles require unchanged geometry and are otherwise
rejected conservatively. Undo can restore an old cycle, but branching after Undo
cannot recycle its entity IDs. Hole parent IDs are remapped with their profiles.
Projected sources use stable body-edge IDs in a separate namespace from authored
entities, so renumbered discovery slots neither break nor steal saved selections.
Schema 1–8 projects bootstrap their current catalog without changing old indices;
legacy face sketches lacking boundary intent remain authored-only.

## Snapping

Drawing against the boundary snaps onto it. `SketchSession::nearest_projected_edge_point`
returns the closest point on the finite projected edge, and the
acquisition is reported as `SnapTarget::ProjectedEdge { edge, position }`.

Acquired endpoints, Point-tool placements and circle/arc centers receive a
`ReferenceOnEdge` relation. It retains one sliding degree of freedom on the
edge interior, respects finite endpoints, and uses the exact carrier for
circles/arcs. The cached external geometry follows history-stage-safe refreshes,
including the targets in Undo/Redo snapshots. A missing edge fails closed in
the solver and profile catalog. Ctrl/Cmd suppresses new inferred attachments;
it does not remove an existing relation during a drag. Center-rectangle and slot
virtual centers are still positional picks, not persistent external anchors.

## Refresh and invalidation

- `begin_sketch_with_options` and history-stage-safe `edit_sketch` install the projection through
  `install_support_references`, next to the support-edge snap midpoints. A
  sketch that no longer sits on a face clears it.
- `prepare_load_project` restores saved boundaries before planning any solid
  consumers. `commit_solid` refreshes them only at the appropriate history stage.
- A sketch whose history stage is masked by a later topology writer keeps its
  previous projection, matching how datum-hosted sketches keep their frozen
  basis. This applies to editing too: a pocket's output is never projected back
  into its own input sketch.
- A support face that disappears is already reported as a broken reference on
  the sketch feature; the projection is simply empty.

## Viewport

Projected geometry is drawn only for the **active** sketch, in its own color
(`--cad-projected`, purple in both themes, matching the "projected geometry"
convention in mainstream CAD). It is never a pick, hover, grip or constraint
target for ordinary entity selection; creation tools can acquire it as a reference.

The Sketch Palette **Projected Geometries** row toggles it. Browser
(`src/components/viewport/Viewport.tsx`) and native (`draw_projected_edges` in
`desktop/src/native_viewport/platform.rs`) renderers both honor the toggle;
the native side receives it as `hide_projected_geometry` on the presentation
payload, inverted so an older payload keeps the reference geometry visible.
Both renderers follow the supplied polyline, including clockwise circular
edges on reversed face bases; neither infers a positive sweep from endpoints.

## Arc direction and generated points

The first deliberate angular move (past a 2° jitter threshold) selects CW or
CCW until the next arc. Autofill is negative for CW and positive for CCW.
Typing a magnitude preserves an autofilled minus; an explicit `+` or `-`
sets direction. Typed angles remain authoritative after further pointer moves.
Driving angle formulas remain editable even though labels show the included
unsigned magnitude. Explicit positive signs are accepted by the shared formula
parser as well as the pointer-entry state. Reference labels retain parentheses.

Schema 8 also records which points creation/modify tools generated as handles.
Deleting their last owner removes unused handles, but preserves independent
Point-tool placements and points still shared or constrained. Trim rebinds
first and cleans up afterwards. Legacy points without ownership metadata are
preserved conservatively. Undo/redo and save/reopen retain ownership.

## Shared tool behavior

- Rectangle, circle, both arc modes, slot and chamfer preview/commit resolve
  the same raw expressions and acquisitions in the engine. Invalid/incomplete,
  zero or negative lengths cannot quietly fall back to a free cursor pick.
  The frontend tessellates the resolved curves; the initial arc-direction latch
  stays in pointer interaction state. Polygon/scale and the incremental spline
  rubber-band retain their existing presentation previews.
- Copy, mirror and both patterns share a topology-aware transform path. Shared
  points are copied once, internal connections remain connected, generated
  handles keep their ownership, and reflections retain major/full arc sweeps.
- A common mutation gate serializes tool submits, rejects duplicate Enter/click,
  and prevents a late reply from clearing the next tool or replacing another
  sketch. A rejected operation leaves the tool available for retry.
- Signed offset values/formulas are stored without stripping their sign. The
  cursor chooses a side before numeric entry; that side is latched while typing.
  Editing the sign can cross the source. Line-offset side metadata persists in
  schema 9 and is removed with its constraint so deleted offsets cannot break
  save/reopen; legacy distance dimensions retain their old semantics.
- Native model-edge strokes use a capped 0.0001 mm camera lift and no global
  depth bias. The former one-pixel lift and bias leaked hidden edges through
  thin solids at some zoom levels; the GPU regression checks this explicitly.
  An edge whose planar neighbour rises towards the camera (both faces of an
  inside corner do) is lifted further by the depth its own stroke width spans
  across that face, bounded to 2.5 px and 1 % of the body radius, so concave
  edges read like convex ones without lifting silhouette edges at all.

## Tests

- `cargo test -p limo-cad-solid profile::tests` — derived segments seal loops,
  projected-only faces report zero authored edges, and overlapping authored
  geometry keeps its provenance.
- `cargo test -p limo-cad-sketch --test projected_face_boundary` — a face sketch
  projects its four boundary edges; a semicircle drawn against the boundary
  becomes a depth-0 profile; a rectangle drawn inside a face stays the only
  profile; projections survive save/reload through the recompute; and geometry
  snaps exactly onto the projected boundary.
- `cargo test -p limo-cad-occt --features native-occt --test projected_boundary_history --test pocket_profile_edges`
  — completed pockets save/reopen with their consumers; upstream depth edits,
  sketch reopening and rollback preserve profile meaning; legacy corner
  rectangles keep index 0; circular samples retain both face orientations.
- `cargo test -p limo-cad-sketch --test creation_regressions --test native_projection_contract`
  — acquired arc endpoints, formula/literal edit data, reference dimensions,
  full turns, generated-point ownership, trim, undo/redo and native drawing.
- `npm run test:arc-sweep` — initial-direction latch, jitter, explicit signs,
  full turns, resolved-outline tessellation and mutation-gate ownership.
- `cargo test -p limo-cad-sketch --test creation_intent --test transform_lifecycle --test profile_identity --test reference_edges`
  — preview/commit lock matrices, invalid inputs, Ctrl behavior, connected
  transformed geometry, identity changes and sliding external-edge relations.
- `cargo test -p limo-cad-sketch --lib projected_identity_uses_body_edges_not_transient_catalog_positions`
  — persistent profile keys use stable body-edge IDs, not temporary discovery
  slots; renumbering keeps selections and replacement edges cannot inherit them.
- `npm run e2e:creation-snap` — rectangle/circle midpoint and boundary previews
  agree with committed geometry, including Ctrl suppression.
- `npm run test:viewport-theme` — the projected color token matches both
  themes, stays legible, and is distinct from authored sketch geometry.
- `node scripts/run-e2e.mjs e2e-face-boundary-profile.mjs` — browser-engine
  check of the picker behavior (requires `cargo xtask build-wasm`).
- `npm run e2e:sketch-smoke` — creation snapping, arc entry, duplicate submits,
  failed-operation retry, late replies, signed offset drift and face profiles.
- `npm run e2e:sketch-regression` — the broader arc, point, slot, spline,
  constraints, dimensions, modify-tool and adaptive-grid interaction suites.
- `LIMO_CAD_PREVIEW_PROOF_DIR=/absolute/output/path cargo test --manifest-path desktop/Cargo.toml --lib native_sketch_boundary_visual_matrix -- --ignored`
  — real native GPU screenshots for both face normals, both themes, face/close/
  grazing views, and hidden-edge comparisons at three zoom levels. Requires a
  functioning GPU; it is not a browser screenshot test. Current native cameras
  are perspective-only, so no orthographic coverage is claimed.

CI runs the browser smoke suite with a fresh WASM build on the frontend PR path;
the broader matrix runs on main/manual runs. Native kernel integration targets
are explicit in the Windows/Linux MCP workflow. Native GPU coverage is a separate
manual macOS workflow, with screenshots retained as artifacts. These workflows
must still be exercised on GitHub after publishing the branch.

## Deferred

- Projected edges as reference geometry for Revolve axes, Sweep paths or Rib
  centerlines. Rib consumes authored centerlines, not closed region profiles.
- Re-projecting when a support face moves. A face-hosted sketch keeps its
  frozen basis. The new sliding relation follows edge geometry only when the
  manager resolves that sketch's history stage. This does not introduce staged
  kernel replay/checkpoints for every upstream edit; later consuming features
  still mask that stage and retain its saved boundary.
