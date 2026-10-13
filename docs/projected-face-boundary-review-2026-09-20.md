# Review: projected-face-boundary

## Extended hardening — 2026-09-20

This section supersedes the earlier follow-up's schema and coverage notes.
This section records implementation and local validation before PR publication.

Implemented the follow-up test and tool-improvement pass:

- Persistent profile identities preserve regions through reorder, dimension
  edits and reopen; split/merge ambiguity fails closed, including real solid
  consumers. Projected sources use actual body-edge IDs rather than transient
  discovery slots. Entity IDs cannot be recycled by branching after Undo.
- A sliding `ReferenceOnEdge` constraint connects acquired endpoints, authored
  points and circle/arc centers to finite support edges. Circular carriers remain
  analytic, missing edges fail closed, and refreshed targets survive Undo/Redo.
- Shared engine resolution for rectangle/circle/arc/slot/chamfer previews and
  commits, with strict typed-input validation. Ctrl-created rectangle corners
  remain independent, and virtual rectangle centers cannot constrain other points.
- A shared asynchronous mutation guard prevents double submission and stale
  completion across cancellation, tool switches and sketch switches; rejection
  retains a clean retry path.
- One topology-aware transform path for copy, mirror and patterns, retaining
  shared endpoints, internal relations, generated-handle ownership and arc spans.
- Signed line/circle/arc offsets preserve expressions and can cross the source
  when edited; numeric entry latches the cursor-selected side. Deleting offsets
  prunes their saved metadata, including through Undo/Redo and reopen.
- Real native GPU tests uncovered thin-wall hidden-edge leakage. Removed the
  global edge depth bias and capped the camera lift at 0.0001 mm. Both normal
  directions, themes, grazing views and three occlusion zoom levels are covered.
- CI now builds fresh WASM for sketch smoke interactions, runs the broader
  matrix on main/manual execution, explicitly compiles native integration tests,
  and offers a separate opt-in native GPU workflow with screenshot artifacts.

Project schema is **9**, accepting schemas 1–8. Older readers must reject the
new format rather than silently discard region identity, external attachment
or signed-offset side metadata. Existing authored-only legacy sketches retain
their profile meaning. Additional native tests cover completed Extrude/Revolve/
Sweep/Loft histories, Rib's separate centerline semantics, holes/concave supports,
small circular-edge regions, rollback, upstream edits and save/reopen.

The broader browser pass also updated obsolete fixtures: menu-only commands,
polygon submenu selection, dimension-overlaid trim clicks, adaptive stroke
weights and the new geometric right-angle mark. Assertions still exercise the
underlying engine mutations and pointer/context-menu behavior.
The final signed-angle browser sweep also caught explicit `+45` being rejected
by the formula parser; unary plus is now supported and covered directly.

### Remaining architectural boundaries

Face-hosted sketch bases remain frozen. External-edge attachments follow a
resolved history-stage refresh, but this pass does not add automatic intermediate
kernel checkpoints when later features mask that stage. Virtual slot/rectangle
centers remain positional picks. Projected edges are not newly exposed as axes,
paths or Rib centerlines. Native cameras are perspective-only; no orthographic
GPU test is claimed. CI wiring is local and has not been run on GitHub.

### Extended verification

- Full Rust workspace tests passed, including 464 sketch/solid tests. Existing
  ignored tests retain their opt-in status; they are not counted as passes.
- The native kernel suite passed (61 tests), and the eight completed-history/
  pocket integration tests passed again after the final profile-key change.
- Desktop library tests passed: 110 passed, three ignored. The focused native
  GPU test was run separately and passed: 12 normal/theme/view screenshots and
  three pixel-exact hidden-edge comparisons, at zoom 0.7, 1 and 2.5. Proofs are
  in `docs/qa/native-boundary/` (local, ignored build evidence). This used the
  production Bevy offscreen renderer, not a packaged Tauri-app visual smoke test.
- Focused MCP drawing and CAM project-persistence regressions passed.
- Full frontend regression tests, desktop frontend build, TypeScript checks,
  all Rust formatting checks and `git diff --check` passed. The three edited/new
  CI workflows parse as YAML; they have not been exercised remotely.
- All 14 browser suites passed against freshly rebuilt WASM: creation snapping,
  tool lifecycle, center-arc input, face-boundary profiles, arc endpoints,
  point extension, slot, spline, M1b, M1c, M1c2, M1d, reference dimensions and
  adaptive grid. The isolated review-owned server was stopped by the runner.

The existing OCCT deprecated-header, Vite bundling and xtask unused-mut warnings
remain. No unrelated warning cleanup was included.

## Implementation follow-up — 2026-09-20

The eight findings below have been addressed in this worktree. The original
review is retained as the record of the pre-fix behavior.

| Finding | Fix |
|---|---|
| 1. Missing boundary profiles on reload | Schema 8 saves the sketch's history-stage boundary before replay plans are built. Reopening an earlier sketch cannot replace it with the downstream cut result. Saved midpoints are recovered for snapping. |
| 2. Existing profile indices retarget | Sketches without saved boundary intent remain authored-only, preserving old numeric profile selections through reopen/edit/rollback. New face sketches support the additional enclosed regions and remainder. |
| 3. Clockwise acquisitions reversed | Stored endpoint angles, point acquisitions and eligibility checks are reversed together. Typed-angle/radius picks cannot acquire an unrelated cursor point. |
| 4. Native pocket test fails to compile | Updated the request to `angle_text` / `sweep_rad` and exercised the native feature target. |
| 5. Deleting an arc deletes authored points | Persist explicit generated-handle ownership. Delete only affected, now-unused generated points; retain authored/adopted/shared/constrained points. Creation and modify tools use this metadata, including copy/mirror/pattern points. Trim cleanup runs after rebinding and preserves unrelated previously detached points. |
| 6. Preview/commit snapping differs | Rectangle and circle previews use the shared acquisition policy. Also fixed center-rectangle preview scaling and redundant grid/corner snapping after acquisition. |
| 7. Native circular projections take the wrong direction | Draw the same directed samples used by browser snapping; retain analytic carriers for solid profile generation. |
| 8. Angle editing drops formulas | Keep parameter ID/name/expression and signed literal edit value. Only display text uses the unsigned included angle. Reference formatting, angle parameter kind and full-circle measurement are preserved. |

The agreed center-arc interaction is implemented: the first deliberate angular
movement (2° jitter threshold) latches CW/CCW; autofill carries that sign.
Replacing an autofilled magnitude retains a minus sign, while explicit `+` or
`-` sets direction. Typed angles remain authoritative through pointer drift.

Compatibility: schema 1–7 projects still load. Old readers reject schema 8 rather
than silently dropping the boundary or ownership data. Existing face sketches
do not automatically gain new regions; newly created face sketches do.

The Slot/Spline browser runners now accept the isolated test-server override.
The Slot test compares committed width with its live snapped preview instead
of assuming a fixed grid spacing despite the adaptive grid.

### Fix verification

- `cargo test -p limo-cad-sketch -p limo-cad-solid -p limo-cad-occt --features limo-cad-occt/native-occt --quiet`:
  510 tests passed (453 sketch/solid, 57 native kernel/integration).
- Full frontend regression suite, TypeScript checking, workspace/MCP formatting
  checks and `git diff --check` passed.
- Desktop test targets compile with the native kernel. The OCCT headers emit
  an existing deprecated-`sprintf` warning; no new build warning remains.
- MCP drawing migration and CAM project-persistence regression tests passed.
- Fresh WebAssembly build and binding generation completed. Browser interaction
  suites passed: `e2e-creation-snap` (12 rectangle/circle cases),
  `e2e-center-arc-input`, `e2e-face-boundary-profile`, `e2e-arc-endpoints`,
  `e2e-point-extension`, `e2e-slot`, and `e2e-spline`.
- The isolated review-owned browser server was stopped after verification.

No commit, push or PR change was made. The pre-existing global native edge lift
and depth-bias changes were not altered. Native GPU occlusion/visual coverage
remains unperformed; mathematical and renderer-contract checks are not claimed
as GPU screenshots. Moving a support face's frozen sketch basis remains the
existing documented limitation, separate from replaying saved boundaries.

## Original review

Reviewed 2026-09-20 at `126b75bc984e141ad6283cfeb9106500727d4986`, against merge base `0d644ab657b5f989da5e5a46cdc6c75f2894c51f` (17 commits, 42 changed files). The worktree was clean when reviewed. Product source was not modified.

## Verdict

Changes are needed before merging. The intended face-boundary closure works in the covered rectangular-face examples, but the implementation is not comprehensive across persistence, existing profile references, acquired arc endpoints, and both renderers. Eight actionable findings follow.

## Findings

### 1. [P1] Restore projected profiles before planning dependent solid features

[manager.rs:4274](https://github.com/limo-cad/Limo-CAD/blob/126b75bc984e141ad6283cfeb9106500727d4986/crates/sketch/src/manager.rs#L4274), [session.rs:487](https://github.com/limo-cad/Limo-CAD/blob/126b75bc984e141ad6283cfeb9106500727d4986/crates/sketch/src/session.rs#L487)

Projections are discarded on serialization and initialized empty on load. `prepare_load_project` builds the complete profile catalog and replay plan before any kernel commit can reconstruct them. Consequently, a feature using a boundary-closed profile cannot be replayed. The new refresh also skips a sketch whenever a later topology-writing feature exists, so a consuming extrude/cut prevents recovery even after the first commit; there is no previous projection to retain in a newly loaded session.

Reproduced with the real native kernel: extrude a 25 × 25 mm base, sketch a radius-5 semicircle against its top edge, cut a 5 mm pocket, export the model, and prepare loading into a fresh manager. Initial pocket creation succeeds. Reload produces only the base job and reports `Broken history reference: profile 1 was not found in 'Sketch2'` for the cut. The added reload test stops before creating a consuming feature, so it misses this failure.

Required: reconstruct references at the appropriate history stage before planning consumers, or persist sufficient stable reference/profile data to bootstrap replay. Cover a completed solid feature, not only a finished sketch, through save/reopen and upstream recompute.

### 2. [P1] Preserve existing profile identities when adding implicit boundaries

[manager.rs:5736](https://github.com/limo-cad/Limo-CAD/blob/126b75bc984e141ad6283cfeb9106500727d4986/crates/sketch/src/manager.rs#L5736), [manager.rs:600](https://github.com/limo-cad/Limo-CAD/blob/126b75bc984e141ad6283cfeb9106500727d4986/crates/sketch/src/manager.rs#L600)

Adding projected edges changes the bounded-face list, but existing solid definitions still select profiles by their numeric index. Filtering out projected-only faces does not preserve those indices: the remainder of a support face shares authored edges with a closed shape that touches its boundary, so it is retained and can sort before the original profile.

Baseline comparison using the real native kernel: a 5 × 5 mm rectangle in the upper-left corner of a 25 × 25 mm face is the sole profile, index 0 / area 25, on the merge base. This branch returns index 0 / area 600 and index 1 / area 25. Existing sketches acquire this changed catalog when reopened for editing or refreshed at their history stage; their downstream `profile_indices: [0]` are not remapped. A later recompute can therefore operate on the opposite, much larger region without a missing-reference error.

Required: preserve/remap authored-profile identity for existing features, or explicitly migrate/opt existing sketches into the new behavior. Add a legacy-project regression with a consuming feature and an edit/recompute cycle. This is separate from finding 1: merely restoring projections earlier would not preserve old selections.

### 3. [P1] Swap endpoint acquisitions together with clockwise arc angles

[session.rs:3097](https://github.com/limo-cad/Limo-CAD/blob/126b75bc984e141ad6283cfeb9106500727d4986/crates/sketch/src/session.rs#L3097)

Clockwise travel swaps the stored start/end angles, but the stored start is still attached using the original `start_target`, and the stored end using `sweep_target`. Those targets identify existing sketch points, not just coordinates, so the solver binds each acquired point to the wrong end and distorts the requested arc.

Reproduction: fix points at (5, 0) and (0, -5), then create a clockwise quarter arc about (0, 0), starting at the first point and ending at the second, with no radius lock. The operation returns success, but the endpoint relations are reversed and the solved angles are approximately `43.982297` and `-1.570796` radians. The profile code interprets such a multi-turn span as a full circle. The existing direction tests use free picks and therefore do not exercise this interaction.

Required: map the acquisitions and their eligibility checks to the corresponding stored endpoint after reversing the sweep. Test existing free/fixed points and connected lines, not only standalone arcs.

### 4. [P1] Update the native pocket test to the final request shape

[pocket_profile_edges.rs:135](https://github.com/limo-cad/Limo-CAD/blob/126b75bc984e141ad6283cfeb9106500727d4986/crates/occt/tests/pocket_profile_edges.rs#L135)

The new feature-gated test initializes `ArcCenterRequest.clockwise`, which was removed by a later commit. `cargo test -p limo-cad-occt --features native-occt --test pocket_profile_edges --no-run` fails with E0560; the compiler lists `angle_text` and `sweep_rad` as the available missing fields. Native-OCCT test builds including this integration target cannot complete. The ordinary sketch/solid suite does not compile it.

Required: update this call and run the native-feature integration test against the final branch, not only an earlier commit.

### 5. [P2] Do not treat every acquired point as an arc-owned endpoint

[sketch.rs:183](https://github.com/limo-cad/Limo-CAD/blob/126b75bc984e141ad6283cfeb9106500727d4986/crates/sketch/src/sketch.rs#L183)

The deletion cascade treats every `ArcEndpointCoincident` point without another surviving relation/entity as owned by the deleted arc. That relation also represents acquisition of a pre-existing user point; it contains no ownership information.

Reproduction: create an independent point at (5, 0), create a counter-clockwise arc starting on that point, then delete only the arc. The branch deletes the original point as well as the new endpoint. The same sequence on the merge base preserves the independently authored point. A point does not become disposable merely because it currently has no other sketch relation.

Required: distinguish generated endpoint handles from acquired/authored points before cascading deletion. Preserve acquired points and cover deletion used by modify tools as well as the direct Delete command.

### 6. [P2] Use the shared snap policy in rectangle and circle previews too

[Viewport.tsx:7793](https://github.com/limo-cad/Limo-CAD/blob/126b75bc984e141ad6283cfeb9106500727d4986/src/components/viewport/Viewport.tsx#L7793), [Viewport.tsx:7814](https://github.com/limo-cad/Limo-CAD/blob/126b75bc984e141ad6283cfeb9106500727d4986/src/components/viewport/Viewport.tsx#L7814)

Rectangle/circle commits now use `acquireToolSnap`, enabling midpoints and projected boundaries, but their live preview paths still call `snapCursorInfo(p, false, ...)` at lines 7478 and 7514. The shared-policy refactor therefore leaves preview and commit using different reference sets.

Reproduced with real browser pointer input on a face-hosted rectangle: hovering five pixels from the top boundary shows a grid marker and previews the corner at y = 7.283192 mm; clicking at that same location commits the corner at the projected boundary y = 7.5 mm. The shape changes on commit without a matching snap indication. Circles retain the same mismatched call pattern.

Required: route all preview paths through the tool policy and test hover/preview/commit agreement for rectangles and circles, in addition to center arcs.

### 7. [P2] Preserve projected circular-edge direction in the native renderer

[platform.rs:5498](https://github.com/limo-cad/Limo-CAD/blob/126b75bc984e141ad6283cfeb9106500727d4986/src-tauri/src/native_viewport/platform.rs#L5498)

The native projection renderer reconstructs every partial circular edge as a positive counter-clockwise sweep using only its first/last sample. Body-edge samples can project clockwise, particularly when the sketch basis normal is reversed. Forcing a positive sweep draws the complementary arc. Browser drawing, snapping, and profile extraction instead follow the actual sample polyline, so desktop users see a reference boundary different from the one used for geometry.

Real-kernel reproduction: extrude a semicircular profile and start a sketch on its bottom planar cap. The projected samples travel -π, while the native renderer's exact angle rule produces +π. A corresponding top-facing pocket example is a passing control. This was verified from kernel-generated coordinates and the renderer's calculation, not a native GPU screenshot.

Required: recover direction from intermediate samples/the analytic orientation, or draw the supplied polyline. Cover both face normals and non-semicircular partial arcs.

### 8. [P2] Keep driving-angle parameter metadata in dimension DTOs

[dims.rs:856](https://github.com/limo-cad/Limo-CAD/blob/126b75bc984e141ad6283cfeb9106500727d4986/crates/sketch/src/session/dims.rs#L856)

The `ArcAngle` display special case always emits null parameter ID, name, and expression, including for driving dimensions backed by formulas. The dimension editor uses `param_expression` to prepopulate the editable formula, falling back to the numeric value when it is absent. Opening and accepting such an angle therefore replaces the expression with a literal and can sever parameter dependencies.

Reproduction: create an arc with typed angle `=45*2`. It gets a driving 90° dimension, but its DTO has no expression instead of `45*2`; the editor consequently initializes with `90`. Showing an unsigned measured angle does not require discarding its driving parameter metadata.

Required: keep the binding/expression for driving dimensions and separate display formatting from edit data. Also retain reference-dimension formatting when the angle is switched to reference mode.

## Intention and scope

The original intention is sound: a face-hosted sketch should be able to use its support boundary to close a drawn region, while a projected-only outline should not swallow interior authored profiles. The authored-edge provenance and rectangular-face happy-path tests directly address that requirement.

The branch also changes center-arc radius/sweep entry, adds a persistent constraint kind, broadens snapping for other creation tools, changes common deletion behavior, and changes native model-edge rendering globally. These are related follow-ups, but they materially enlarge the regression surface beyond projection alone. I found no new dependency, unrelated workflow change, or remaining temporary diagnostic overlay. The native icon-decoder fix and frontend/native icon-contract test are relevant and useful.

The broad default gizmo depth bias also affects grids, and the one-pixel camera-space lift affects all ordinary body-edge strokes. Those changes need explicit native visual coverage for thin/occluded geometry, perspective views, and multiple zoom levels; the added kernel-edge existence test cannot establish their visual correctness. I have not claimed a specific occlusion defect without that verification.

## Verification

Passed:

- `cargo test -p limo-cad-sketch -p limo-cad-solid --quiet`: 441 tests, zero failures.
- `npm run test:frontend` and `npx tsc --noEmit`.
- `cargo check --manifest-path desktop/Cargo.toml --tests --locked --offline`.
- Fresh WebAssembly engine build and matching wasm-bindgen generation.
- Browser suites: `e2e-face-boundary-profile`, `e2e-center-arc-input`, `e2e-arc-endpoints`, and `e2e-point-extension`.
- `git diff --check origin/main...HEAD`.
- Merge-base controls for independently authored-point preservation and the original corner-rectangle profile index.

Additional probes:

- Six targeted Rust checks reproduce findings 1, 2, 3, 5, 7, and 8; a top-facing circular-edge control passes.
- One pointer-driven browser check reproduces finding 6.
- The native pocket integration-test compile reproduces finding 4.

The original Rust probes, baseline comparison and browser probe ran in an
external temporary review harness, not committed tests. Their failures were
intentional assertions of the missing behavior. Permanent regression coverage
was added during the follow-up passes above. The review-owned browser server
was stopped after testing.

Not performed: a full native GPU visual/occlusion pass, the complete release E2E suite, or the full native-OCCT integration suite (the new target does not compile). Existing passing tests should not be taken as covering the failing combinations above.
