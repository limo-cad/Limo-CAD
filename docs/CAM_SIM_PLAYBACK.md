# CAM stock views and explicit playback

This describes CAM prediction and playback. The NC interpreter retains its
separate block-boundary playback contract.

## Presentation limits

The 65,536-triangle presentation cap is separate from material removal and
verification. The extractor merges exactly extruded strips while retaining
local rim and tip detail. If a surface still exceeds the cap, it builds a
complete, conservatively occupied display-only coarse grid and warns that
small features can be obscured. It never stride-skips triangles, increases
retained-mesh budgets or coarsens cutting/verification. Warnings follow the
actual displayed frame, including reused air-move surfaces.

Completed and upcoming CAM lines now use separate native depth-priority groups, both slightly away from the near-plane extreme. Bevy's line pass writes depth: submitting blue last alone did not protect an overlapping plunge/retract at equal depth. The completed plunge remains blue after the tool enters horizontal motion. Setup selection now has an explicit selected style and `aria-pressed`, distinct from the active setup and Stock & WCS row. Stale buffer sample/presentation failures after a seek cannot close the replacement request.

Regression checks cover a 0.109 mm, roughly seven-million-cell deep two-hole job, complete buffered sample progression, UI selection of both Drill and in-control Contour, and completion after one Play action. These are synthetic reproducible jobs, not native playback performance measurements.

## Interaction contract

- Selecting a setup shows its **incoming stock**, including remaining material from a rest-source setup. This is a zero-move request, not a full simulation followed by a rewind. Empty setups can show incoming stock too. No cutter is borrowed from the first operation.
- Selecting a toolpath shows stock **after that operation and all preceding operations**. The cutter stays at that operation's final retract before and after loading. Selection does not open an animation clock.
- Right-click a toolpath → **Simulate toolpath**. Its playback starts after the preceding operations, with their already-cut stock.
- Right-click a setup → **Simulate whole setup**. Playback starts at incoming stock and includes every prepared physical move: rapid, feed, circular/helical, dwell, and tool changes between sections. No machine/ATC motion is invented.
- **CAM Sim** and **NC Sim** live in the **Simulate** ribbon tab. The workspace and its caches stay mounted when switching between Program, Simulate and Output.
- The always-visible viewport bar offers **Model / Stock / Compare**. Model shows the original CAD bodies without a stock surface or envelope; Stock shows the selection's remaining stock; Compare overlays the target as a faint reference. Compare replaces the former X-Ray button. **Paths** independently hides path lines and the parked cutter; the moving cutter stays visible during playback. Hide Paths in Model mode for an unobstructed CAD view.
- View choices persist across setup/operation selection and ribbon/workspace changes within the active project. New/opened projects start with Stock and Paths enabled. Editor dialogs temporarily show the model for geometry picking without changing the chosen mode. Browser eye-hidden bodies remain hidden. Modeled raw-stock bodies also stay hidden in Model mode, including before a stock frame exists.
- These are presentation changes only: no new planner/simulation request, stock extraction, generation invalidation, or stock-buffer discard. Native stock visibility uses the retained mesh handle. A shared freshness policy prevents showing the wrong selected stage and treats fully removed stock as a valid empty surface, not permission to resurrect the target model.
- Playback provides play/pause, return to start, previous/next motion, scrubbing, speed, and close. Space toggles playback and left/right arrows step when focus is not in an editor/control. Closing returns to the selection's stock view. The orbit camera stays independent of playback.
- During playback, the travelled centerline turns blue progressively, including the partial current move. Upcoming cutting moves remain green and rapids remain dotted amber; travelled rapids retain their dots. Rewinding restores the earlier color boundary. Overlapping/peck strokes put the travelled portion on top. This is motion progress, not a stock-removal or verification verdict.

## Progressive path presentation

The physical simulation timeline is tessellated once per timeline/scope into retained line segments with start/end times. The small native presentation update carries only an opaque path identity, playback time, and the same model-space physical tip used by the animated cutter. Bevy splits the active segment into blue and upcoming portions at that tip; on a circular/helical move this is the true interpolated curve position, not a point inside its chord. Dotted rapids preserve their original spacing as the color advances. Timing payloads are validated one-for-one against the existing bounded line budget; no extra stock calculation, timeline copy, or complete path upload occurs on clock ticks.

The color boundary follows the animation clock independently of buffered stock snapshots. Timeline identity matching prevents a delayed cursor from coloring another operation's path. A single-operation scope excludes earlier operations; static selection and closed playback have no progressive coloring. `scripts/test-cam-path-progress.ts`, native `path_progress` tests, and the CAM playback interaction test cover clock-only reuse, WCS, arc planes, partial moves, rapid dots, rewind, scope, and stale/closed cursors.

## Computation and cache ownership

All cutting, stock reconstruction, and verification stay in Rust. JavaScript handles controls, request scheduling, and continuous cutter-pose interpolation; native stock buffers still go directly from Rust to Bevy, without a triangle-buffer JSON round trip.

Ready-to-present result caching precedes planning and surface extraction. A hit returns the existing stock mesh, timeline, measurements, and comparison; it does not remesh stock or hidden comparison layers. The key contains the complete CAM document and normalized request. Target meshes can be omitted only with the host's unchanged opaque geometry key. Host identities are window/module-namespaced and include scene, setup, quality, and tolerance. Changing only the active setup preserves machining-input identity; real document or scene changes invalidate it.

Retention is bounded and on demand. Playback prepares only a small look-ahead window; there is no all-stage or full-movie mesh preload:

| Layer | Retention budget | Purpose |
| --- | --- | --- |
| Ready simulation results | 8 entries / 64 MiB estimated payload | Repeat selections skip planning, cutting, extraction, and comparison |
| Operation checkpoints | 4 entries / 32 MiB estimated payload | Extend a previously prepared prefix instead of recutting earlier operations |
| Forward playback checkpoints | 4 entries / 16 MiB estimated payload | Incremental frames; backward seeks restore an earlier stock bitset |
| Native prepared playback | One session; scope-start/completed/last-display bitsets plus a temporary partial sweep | Retain the physical timeline once; skip planning, keys, full-result copies and comparison on samples |
| Ready playback frames | Client: 8 frames / 24 MiB estimated geometry; native hard cap: 24 frames / 48 MiB unique geometry | Worker prepares ahead; display selects only a ready past/current frame by handle |
| Native display meshes | 4 entries / 32 MiB estimated CPU/main-world/GPU payload | Reuse uploaded geometry across recent stage selections |

These are retained-cache budgets, not a bound on total process memory or temporary allocations. Existing target-verification caches remain separate. Operation checkpoint capture is bounded **during** the run, not only on insertion afterward. If a requested checkpoint was not retained, the kernel replays from an available earlier state; it never presents that earlier stock as the requested finished stage. Eviction, input changes, and first-time views can still require computation.

Native CAM animation uses a prepared Rust playback session and a bounded producer/consumer buffer. Opening sends the source inputs once. Samples send only session ID and physical time; returned frames contain measurements and a handle, not mesh buffers or the full timeline. Preparation never publishes future stock. The client presents the newest ready snapshot at or before the cutter time, and limits its clock to the prepared horizon on an underrun. Orbit/input processing does not wait on the cutting worker. Snapshot spacing adapts to measured preparation cost (0.125–1 second of wall-time spacing, scaled by playback speed); the cutter is interpolated on its independent animation clock. This is bounded look-ahead, not a guarantee of real-time throughput for every model. Browser/WASM testing retains a bounded compatibility path, but does not establish native frame rate.

Rapids, dwells and other unchanged-stock samples reuse the exact retained surface: the prepared kernel compares the occupied bitset and recorded display profiles and skips extraction when both are unchanged. Native geometry is shared through `Arc` buffers. Closing, editing or changing selection cancels the session; stale IDs cannot publish into a newer session. A seek discards speculative client frames and restores completed stock before applying the requested partial move.

The native renderer no longer runs three consecutive GPU presents inside one AppKit callback when stock changes. Asset-settling updates are spread over separate queued callbacks, coalescing newer camera/tool input between them. The stock entity and material are retained while its mesh changes. Camera/preview/presentation commands are asynchronous; metadata-only stock updates no longer cause JavaScript to rebuild the unchanged toolpath overlay. Routine prefetch does not activate the busy cursor; a small playback status indicator reports an actual underrun.

Completed physical moves are checkpointed before applying the current partial move to a copy. That distinction is essential: rewinding a slider must be able to restore removed material. Cutter compensation is already resolved in the physical timeline, so replay must not cut the nominal in-control contour as a centerline. Verification runs on the prepared complete/scoped timeline and remains available as safety findings; partial frames do not rebuild hidden comparison meshes or label final comparison counts as partial-frame measurements.

## Finer stock display without a denser grid

Reconstruction uses the shared flat, rounded and beveled cutter profiles.
Initial analytic stock and the union of
actual recorded sweeps drive bounded Hermite/crease reconstruction. Small
hole chamfers, exterior bevels and floor fillets receive local detail and
separate normals at sharp joins. Shallow cuts keep their real corner residue.
Exactly extruded strips now merge along any grid axis, reducing redundant
triangles on straight fillets as well as cylindrical walls.

The former XY buckets are replaced by a BVH with at most 2,048 sweep records,
65,536 memoized projections and a charged field-work limit. The existing mesh,
result-cache and playback-buffer budgets are unchanged. An incomplete history
or exhausted work budget falls back to a complete grid surface, never an
incomplete cutter union. Modeled stock and unresolved sub-cell topology retain
their grid limits. No occupied bits or verification measurements are changed.

See [remaining-stock surfaces](CAM_STOCK_SURFACE.md) for the reconstruction
equations, precise limits and reproducible fixtures. CAM's
existing lighting is unchanged; no extra screen-space rendering pass is needed.

## Busy feedback and verification

Generating/editing CAM intent, regenerating toolpaths, planning, and initial stock preparation use a non-blocking activity indicator. A small accent ring appears beside the cursor after 150 ms; very fast cache hits do not flash it. Pointer-follow layout updates are limited to 20 Hz. The matching status label also covers keyboard use. Routine playback prefetch does not create cursor rings or repeated native overlay cutout/layout work. Existing app colors, borders, and radii are reused.

This follows the distinction between indeterminate activity and known progress in Apple's [progress-indicator guidance](https://developer.apple.com/design/human-interface-guidelines/progress-indicators) and [loading guidance](https://developer.apple.com/design/human-interface-guidelines/loading). It is an app-styled indicator, not an imitation of the OS beachball and not a claim of native `NSProgressIndicator` integration.

Regression coverage includes cache hits with zero mesh extractions, tool-change invalidation, extending scoped checkpoints, partial cutting/rewind restoration, physical in-control/arc boundary equivalence, 118°/90° display slope reconstruction, no stretched curved rectangles, prepared-sample/fresh-simulation equivalence, unchanged-stock extraction skipping, bounded look-ahead, no future-stock publication, cancellation during preparation, stable selected-tool pose, setup cache identity, explicit operation/setup playback, move controls, orbit during play, and regeneration activity.

Native visual QA uses the dev-only Bevy image lab with the production stock
material, lights and AA. Synthetic fixtures check geometry and shading, not
end-to-end playback frame rate. A deep hole's apex can be occluded by its wall
at shallow view angles; it is not artificially deepened or drawn through stock.

Reproduce the geometry capture with `LIMO_CAD_CAM_MESH_CAPTURE=/tmp/cam-stock.json cargo test -p limo-cad-cam --release capture_drilled_display_mesh -- --ignored --nocapture`, then set `LIMO_CAD_CAM_LAB_MESH=/tmp/cam-stock.json` for `bevy-ui-lab` (optional `LIMO_CAD_CAM_LAB_CLOSEUP=1` selects the fixture's first hole). The opt-in capture test is excluded from ordinary test runs. Browser interaction tests use the real Rust WASM kernel at a reduced grid budget; they are not native performance evidence. Machine, fixture, holder, and shank collision certification remains out of scope.
