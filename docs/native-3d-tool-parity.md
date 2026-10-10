# Native 3D tool parity audit

The native dialogs were compared with the user's noBS reference checkout
(`cam-pr180-followups`, revision `391a025c2ec7047bc7c369e972cd7a4799f604b2`)
and the supplied Extrude screenshots. The reference was its existing React
implementation: `ExtrudeDialog`, `RevolveDialog`, `SweepDialog`, `LoftDialog`,
`RibDialog`, `HoleDialog`, `SolidEdgeDialogs`, `BodyFeatureDialog`,
`ConstructionPlaneDialog`, `ViewportSelectionField`, `DimensionInput`, and the
viewport's profile triangulation and Extrude manipulator.

## Changes

All 19 native modeling/construction forms share the reference's purple dialog
outline, tinted header, selection cards, separate Clear button, selection status
and helper text, viewport picking prompt, and number steppers. Reference summaries
come from accepted form references; the accessible control names and shared solid
request payloads retain their existing identities.

Extrude uses the reference's two-by-two Create Body / Add / Subtract / Common
buttons, operation explanation, shaded translucent tool volume, yellow source
outline/fill, blue direction arrow, and floating distance editor. Earcut triangulates
concave profiles and holes instead of filling them with a triangle fan. The preview
and arrow edits change only the draft until Apply. The floating and panel editors
use the same measurement, including units and expressions. Invalid values retain
the selected source without showing an obsolete tool volume.

Programmatic measurement focus selects the suggested value for replacement.
Ordinary pointer focus still places the caret at the clicked position. Spinner
buttons remain clickable but do not interrupt Tab traversal. Enter from either
number editor confirms the feature after validation; an invalid value keeps the
form open.

Profile, planar-face, path, and axis candidates highlight before their first
selection. Hole projects retained sketch snap points onto the candidate support
face; its first click accepts both the face and the snapped position. A cursor or
window-focus change removes the hover candidate without discarding the accepted
feature preview. Ordinary keyboard entry and paste update linked number editors
while the field remains focused.

Double-clicking an existing sketch dimension opens a compact value editor beside
its label, with the number selected. Enter applies the value and Escape cancels
the edit; the adjacent actions menu retains Delete, Driving/Reference, and
Reposition. Escape cancels active drawing tools and edit forms without finishing
the sketch. Finish Sketch returns to an overall isometric view, including after
dependent geometry is recomputed.

Single face and edge selections no longer highlight their entire owning body or
automatically open Body appearance. The lower-right measurement card reports
edge length and radius, two-edge angle, and face measurements. Mesh-derived
measurements are explicitly marked approximate. Escape closes appearance,
Named Views, and Print Settings edits; Enter in their text fields applies or
saves a valid draft.

## Panel coverage

Every row below was rendered and captured from the native app, supplied with
references until Apply was available, and canceled with the project model unchanged.

| Group | Tools | Additional layout comparison |
| --- | --- | --- |
| Build | Extrude, Revolve, Sweep, Loft, Rib | Extrude operation grid; paired two-sided distances, custom axis coordinates, Sweep options, and Rib dimensions; optional guide/centerline fields |
| Refine | Hole, External Thread, Fillet, Chamfer, Shell | Selection cards, automatic number focus, paired hole/thread parameters, number steppers |
| Repeat | Mirror, Rectangular Pattern, Circular Pattern | Body/plane/edge selection cards, compact vectors and spacing/count rows |
| Body | Move/Copy, Combine, Split Body | Body/reference selection cards, operation captions, framed Create copy checkbox and explanation |
| Reference | Offset Plane, Midplane, Plane at Angle | Reference cards, numeric focus/steppers, existing offset manipulator |

## Validation

- Desktop Rust suite: 943 passed, zero failed, 10 optional GPU/display tests ignored;
  strict Clippy passed for all targets and features.
- Native app: light and dark GPU captures, accepted reference selections, and
  Escape cancellation with the model unchanged for all 19 panels.
- Alternate modes: five Extrude extent layouts, operation buttons, source Clear and
  reselection, custom Revolve axis, Sweep guide, Loft centerline/guide, Rib dimensions,
  Hole counterbore/countersink, and the scrolled Move/Copy checkbox.
- Native Extrude: selected suggested text, shared floating/panel values, both spinner
  locations, invalid Enter, Enter confirmation from the floating editor, and exact
  geometry restoration through Undo/Redo. Physical keyboard typing and paste keep
  both values synchronized; its preview survives window focus and cursor changes.
- Interaction checks: inline dimension replacement/Cancel, initial profile hover,
  initial Hole snap/first-click placement, independent edge highlight and length,
  ordinary face selection, invalid feature cancellation, and physical Cmd+Z/Redo.
- Additional editors: Named Views Enter/Cancel, Body appearance Enter/Cancel, and
  Print Settings Enter/invalid-draft Cancel and Undo.
- Combined workflow: snapped rectangle and dimensioned circle, extrusion with a hole,
  fillet, Undo/Redo, history edit, save/close/reopen, sketch dimension edit, and workspace
  round trip.

The automated previews cover untapered extrusion volumes. A nonzero taper retains
the source selection and reports that the kernel calculates the final result on
Apply; it does not display a fabricated untapered volume. Other tools retain their
existing reference highlights and guides rather than introducing new kernel preview
behavior. This audit covers the modeling and construction forms, not separate CAM,
drawing, or assembly workflows.
