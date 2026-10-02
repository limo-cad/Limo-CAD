# ADR 0002 — Native Bevy viewport boundary

- Status: Accepted viewport contract; shell/composition ownership superseded by [ADR 0003](0003-bevy-interface.md)
- Date: 2026-07-30
- Tracking: [#20](https://github.com/jackControls/noBS-CAD/issues/20)

## Context

The desktop viewport needs native GPU rendering, accurate picking, low-latency
camera input, Retina / HiDPI correctness, and a path toward CAM simulation.
Putting those responsibilities in the webview couples the most performance-
sensitive part of the application to browser rendering and native-view
composition.

Bevy must **not** replace OCCT. Solids remain B-rep; Bevy displays tessellated
meshes, handles camera/picking/gizmos/3D-mouse, and hosts selection visualization.

React remains valuable for the document shell, accessibility, browser
automation, and form-heavy workflows. A production boundary therefore needs
to preserve DOM semantics without drawing the same viewport control twice.

## Decision

The desktop application uses four explicit layers:

1. **OCCT and the Rust document model are authoritative.** OCCT creates and
   edits B-rep geometry. Tessellation, topology IDs, sketch geometry, and
   presentation state are passed in-process to the viewport.
2. **Bevy owns the complete viewport surface.** This includes meshes, edges,
   sketches, datum/origin geometry, picking, camera navigation, transient
   previews, selection highlighting, the orientation dial, viewport prompt,
   status/readout cards, and the bottom navigation bar.
3. **React owns the application shell and command forms.** Ribbon, browser,
   project tabs, timeline, menus, and form-heavy feature dialogs remain DOM
   elements. Transparent DOM proxies retain keyboard, pointer, accessibility,
   and browser-test semantics for Bevy-owned viewport controls.
4. **Tauri owns native composition.** The Bevy surface is an opaque native
   child beneath the webview. The platform host clips the webview around the
   viewport while preserving real DOM islands such as menus and dialogs above
   it. CSS transparency is not used as the compositor contract.

## DOM flyout and clipping contract

Any React surface that can cross a shell boundary—menus, combobox lists,
popovers, tooltips, context menus, and dialogs—must be rendered through a
React portal under `document.body`. It must not remain a descendant of a
ribbon, sidebar, tab strip, or other ancestor whose `overflow` can clip it.
Increasing `z-index` cannot escape an ancestor's clipping boundary.

Portaled surfaces use fixed viewport coordinates derived from the trigger's
`getBoundingClientRect()`, are clamped to the current window, and are
repositioned after resize, fullscreen, display-scale, and relevant scroll
changes. DOM surfaces that cover the native Bevy viewport retain the
`data-native-viewport-overlay` contract so Tauri preserves them as webview
islands above the native child. Portaling must not bypass native-composition
registration.

The File-menu regression that established this rule had an absolutely
positioned menu beginning exactly at the bottom of an `overflow-hidden`
ribbon: 92 px in the standard shell and 122 px after the Drawing workspace row
was added. The menu state changed and the DOM existed, but its entire painted
and hit-testable area was outside the ancestor's clip. This class of failure
must be tested by opening the surface and hit-testing/clicking an interior point
that lies beyond the trigger's shell boundary. DOM presence, state changes,
overlay-mask rectangles, and screenshots of fullscreen mode alone are not
sufficient regressions.

Bevy UI is built from the stable core `bevy_ui` flex/grid primitives. The
experimental, unstyled `bevy_ui_widgets` crate is not a production dependency.
`ViewportUiTheme` and the component builders in
`src-tauri/src/native_viewport/ui.rs` are the canonical style implementation
for native viewport UI.

The bridge sends explicit palette, HUD, interaction, camera, and presentation
state. It does not send screenshots or serialized OCCT geometry through
JavaScript.

When a workspace such as Drawing has no native 3D viewport, the host hides the
native child and removes the macOS WKWebView cutout mask completely. Retaining
an empty or stale even-odd mask is forbidden: it clips the webview to the old
window bounds after live resize or native full-screen transitions. The
WKWebView and its parent also retain flexible width/height autoresizing so this
contract holds while the Bevy child is unmounted.

Solid command forms publish a small, debounced semantic preview rather than
DOM/SVG viewport pixels. For Extrude this contract contains the authoritative
sketch `PlaneBasis`, selected profile loops, signed extent offsets, Boolean
role, and direction arrow. The profile is triangulated with its holes and Bevy
renders the translucent region, tool volume, and 3D manipulator. Both the
preview and the OCCT request therefore use the same basis; vertical and
internal datum-plane profiles cannot acquire an independent display rotation.
React retains only the accessible dimension input and drag hit target. Browser
regressions inspect this semantic payload without maintaining a second 3D
renderer.

Transient Bevy geometry is retained. Candidate profiles use inexpensive gizmo
outlines; only the hovered/selected region receives a translucent x-ray fill.
Profile and tool meshes are uploaded when their semantic content changes, not
when the camera, annotations, or screen-size arrow transform changes. Camera
motion updates retained manipulator transforms in place. This keeps navigation
allocation-free even while a solid command is open.

The embedded renderer is event-driven. Model, viewport-size, session-binding,
and transient-mesh changes receive a second Bevy update to settle the extracted
render world. Camera and presentation-only changes render exactly once on both
macOS and Windows; duplicating those updates adds pointer latency without
creating a new GPU pipeline.

The Extrude tool volume is a translucent fill without a duplicate edge cage;
the source sketch remains visible in its normal retained sketch layer. Its DOM
drag proxy is hit-test-only and never becomes a webview mask island. Projected
DOM value input is removed from the native cutout while the camera is moving
and positioned once after navigation settles, so orbit cannot trigger
per-frame compositor-mask updates or cover the Bevy arrowhead.

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
and feature errors against the retained baseline, and only then makes it warm.
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
browser-driver transports. The executable contracts are
`scripts/e2e-six-dof-mouse.mjs` and
`scripts/e2e-bevy-interaction-kernel.mjs`.

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
inputs. The feature-gated visual lab below captures only its fixed development
fixture and never attaches to a live user document or input stream.

## Visual regression channel

The `dev-ui-lab` Cargo feature builds a headless, GPU-backed Bevy render target
using the same production UI builders as the embedded viewport. The capture is
served by a development-only Vite route beside a React reference surface:

```text
cargo run --locked --manifest-path src-tauri/Cargo.toml --features dev-ui-lab --bin bevy-ui-lab -- public/__bevy_ui__/native.png
npm run dev
http://127.0.0.1:5173/?bevy-ui-lab=compare
```

The lab may include a representative feature dialog to validate that the Bevy
style system can reproduce the shared visual language. It is a visual contract,
not a second command-form implementation.

## Consequences

- Clear boundary: OCCT = truth, Bevy = presentation/interaction.
- Viewport-local pixels no longer depend on webview/native-view overlap.
- React keeps the DOM surfaces that benefit most from accessibility and agent
  inspection.
- Native controls and transparent DOM proxies require stable control IDs.
- The Rust desktop binary is larger; Bevy features remain explicitly selected.
- The visual lab is feature-gated and excluded from production binaries.
- License audit into `THIRD_PARTY_NOTICES.md` before merge.
- 3Dconnexion / 6DoF input can move closer to Rust HID paths already in Tauri.

## Non-goals

- Rewriting the ribbon, browser, timeline, menus, or command forms in Bevy
- Mesh-only modeling
- Maintaining a second browser renderer for the desktop viewport
- Using the visual regression image as the production renderer
