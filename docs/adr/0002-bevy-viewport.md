# ADR 0002 — Native Bevy viewport boundary

- Status: Accepted viewport contract; shell/composition ownership superseded by [ADR 0003](0003-bevy-interface.md)
- Date: 2026-07-30
- Tracking: [#20](https://github.com/limo-cad/Limo-CAD/issues/20)

The retired shell/composition contract is retained in the
[pre-cutover record](https://github.com/limo-cad/Limo-CAD/blob/c537f0232ab86f28b1be3ee46fa6bf5ff46db5b9/docs/adr/0002-bevy-viewport.md).

## Shared GPU ownership

The native interface and windowless CAD worker share a GPU device and queue.
Window configuration and frame acquisition exclude queue operations in the
interface's render world. CAD extraction/recovery and GPU render updates share
the same submission guard with those window operations. Mesh rebuilding and
pipeline prewarming/compilation run outside the guard, preserving menu rendering
while geometry and shaders prepare. On shutdown, a stopped worker cancels its
pending submission wait so the interface can finish its last frame.
Teardown also services any pending main-thread surface work.

The Extrude tool volume is a translucent fill without a duplicate edge cage;
the source sketch remains visible in its normal retained sketch layer.

## Exact solid-face feature sources

A feature that consumes a planar solid face records the source as an owning
`BodyId`, a deterministic `FaceId`, and the creation-time OCCT topology key
(`face:n`). These identifiers are stable only inside the source body's replay
stage: a later Boolean can reuse the same body-local face slot for a different
surface. The saved topology key therefore resolves the corresponding
`TopoDS_Face` only while the kernel is at that feature's upstream history
state. The face's plane basis is cached with the definition so preview and
extent direction remain deterministic while the history graph is rebuilt.

Construction-plane frames obey the same temporal boundary. A datum sourced
from a solid face or edge resolves and persists its `PlaneBasis` at its own
history landmark. A final downstream scene must never be used to re-resolve
that upstream reference; the persisted creation-time frame remains
authoritative while later topology-changing features are active. Project load
briefly replays affected datum landmarks before returning to the saved history
marker. This both normalizes valid projects and repairs older saves whose
cached datum or dependent-sketch basis was contaminated by downstream face-slot
reuse.

Extrude passes that exact `TopoDS_Face` to both the native and browser OCCT
kernels. A straight extrusion prisms the face directly. A tapered extrusion
uses OCCT's exact outer `TopoDS_Wire` and every inner wire, lofting the outer
boundary and subtracting the inner-wire tools. Holes, analytic curves, and wire
orientation therefore come from B-rep topology rather than reconstructed
display triangles.

Bevy may use the selected face's tessellation for a fast translucent preview,
hover fill, or direction manipulator. That mesh is presentation-only: it is
never sent back to OCCT and never becomes modeling input. Feature replay fails
clearly if the stored body/face reference cannot be resolved; it must not fall
back to manufacturing a polygonal face from tessellation.

## Document tabs and memory retention

Each warm desktop tab retains its Rust document plus OCCT kernel/B-reps and
Bevy model meshes. A normal tab switch activates those retained layers; it
does not replay the feature tree or reconstruct unchanged Bevy meshes.

The serialized parametric model remains the durable recovery and eviction
boundary. The active tab is never evicted. An inactive tab becomes cold after
60 minutes without use, or earlier under a portable physical-memory pressure
estimate. Constrained pressure releases the least-recently-used inactive tab;
critical pressure releases every eligible inactive tab. Active tabs, unfinished
sketches and saves in progress are protected. Releasing a tab drops its OCCT
context, Bevy mesh handles and drawing projection/raster caches while retaining
its model snapshot, file/archive state, document ownership and Undo/Redo history.
Selecting a cold tab performs one transactional recompute, checks body identities
and feature errors against the retained baseline, and verifies the rebuilt
sketch state before restoring its retained editing sessions and making it warm.
Finished-sketch command stacks stay in a separate in-memory record; project
files do not acquire session Undo/Redo data.
A failed reconstruction preserves the snapshot and previously active tab.

The existing native watcher probes memory every 30 seconds through `sysinfo`
on Windows, Linux and macOS, away from the UI thread. It uses
deliberately conservative thresholds (10%/1 GiB constrained and
5%/512 MiB critical, whichever threshold is larger). This is a safety valve,
not a fixed resident-tab cap or an instruction to discard useful filesystem
caches.

## Camera and 6DoF interaction contract

Viewport navigation describes **part motion from the user's stationary point of
view**. Touchpad orbit and 3D-mouse rotation share the center of the visible
OCCT tessellation as their pivot. That presentation pivot is independent from
the camera target reported to a 3Dconnexion driver; a driver-side automatic
pivot update must not move it.

Mouse orbit, Shift+trackpad swipes, the Orbit tool and orientation-dial drags
rotate around the view's right and up axes without a vertical angle limit.
The camera orientation turns with its position, so repeated full rotations
pass smoothly through the top, bottom and inverted views. Pan follows the
current screen axes, and zoom retains the current orientation.

The macOS installed-driver path was calibrated against real cap motions on
2026-07-31. Device input is canonicalized once at the adapter boundary:

| Physical cap motion | Dominant driver value | Camera API value |
|---------------------|-----------------------|------------------|
| Push right | `+Tx` | `+X` |
| Push away | `-Ty` | `+Y` |
| Lift | `-Tz` | `+Z` |
| Tilt forward | `+Rx` | `+Rx` |
| Tilt right | `-Ry` | `+Ry` |
| Twist clockwise | `+Rz` | `-Rz` |

Equivalently, translation maps as `[Tx, -Ty, -Tz]` and rotation as
`[Rx, -Ry, -Rz]`. In particular, forward/backward tilt keeps the driver's
`Rx` sign. Preserve this canonical basis across native-driver, raw-HID, and
browser-driver transports. The native adapter and its tests are in
`desktop/src/six_dof_mouse.rs` and `desktop/src/six_dof_mouse/tests.rs`.

Raw/native 6DoF translation and rotation share one persisted speed multiplier.
The default is `1.5` (150% of the calibrated base rate), adjustable from 25%
through 300% in Settings. The multiplier is applied after axis canonicalization
so it cannot change the direction contract above.

Lateral translation pans the camera rig so the part follows the cap. Depth
translation is a bounded dolly around the fixed look target on both macOS and
Windows. It must not translate the target through the stationary model: that
drifts the later orbit center and can eventually carry the part behind the
camera. The supported focus-distance range is 2 through 5000 model units.

Navigation input is transient. Production builds do not expose an input
recorder, persist raw 6DoF/touchpad packets or camera traces, or periodically
capture the user's viewport. Regression debugging uses synthetic automated
inputs. The feature-gated visual lab captures only its fixed development fixture and
never attaches to a live user document or input stream.

## Native widget styling

The native host uses Bevy `=0.20.0` stable. The opt-in
[headless widget exploration](../bevy-stable-widgets.md) preserves the current
`ViewportUiTheme` and retained component builders while evaluating shared
button and checkbox behavior.
