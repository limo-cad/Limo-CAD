# Measure

The Solid workspace's **Inspect → Measure** command opens a disposable,
read-only panel. It uses native OCCT geometry for measurements. The ordinary
selection HUD continues to identify its display-mesh estimates separately.

The interaction follows Autodesk's [Fusion Measure reference](https://help.autodesk.com/cloudhelp/ENU/Fusion-Model/files/GUID-D070A3CB-729F-410F-9DA4-D9DF7F4D55B5.htm)
and [Measure objects](https://help.autodesk.com/view/fusion360/ENU/?contextId=SLD-MEASURE)
documentation, accessed October 11, 2026.

Select an object to inspect its properties, then select a second object for
their relationship. Picks remain ordered. Clicking a selected object removes
it; selecting a third distinct object starts a fresh measurement. Existing
preselection seeds the panel, and closing it restores that selection while the
same document and geometry remain current. Clear removes the measurement
selections. Escape closes the panel, including when a panel control has focus.

| Selection | Values |
| --- | --- |
| Edge or arc | Native curve length; radius, diameter and center for analytic circles/arcs |
| Face | Native surface area and boundary loop length; cylinder radius/diameter; circular planar boundary radius/diameter and center |
| Body | Surface area, solid volume when available, and geometric bounding size |
| Vertex or circle-center snap | World X, Y and Z position |
| Two objects | Minimum distance and its closest points |
| Two lines or planar faces, or a line and a plane | Smaller geometric angle, in the range 0–90° |
| Two circular edges | Center distance; maximum distance and farthest points for parallel complete circles |

Selection filters cover faces/edges/vertices, bodies, and vertices alone.
Show Snap Points displays available edge endpoints and circular centers on
hovered geometry. Ctrl on Windows/Linux or Command on macOS restricts geometry
picking to snap points. Shift hides automatic snap points. Snaps use screen
tolerance and reject references hidden behind physical faces.

Precision controls zero through six decimal places. Length, area and volume
use the document's units, with an optional second unit shown in parentheses.
XYZ deltas run from Selection 1 to Selection 2 between the closest points.
Choose global axes or the local axes of either selected object's placement.
Changing formatting never rounds the stored canonical measurement.

Click a result value to copy its displayed text, including units. Hovering a
value highlights its object or the appropriate distance segment. Minimum,
center and maximum distances use their own points. XYZ guides use red, green
and blue for X, Y and Z. Longer result lists scroll inside the panel.

Two repeated occurrences of one source body remain distinct selections. Exact
queries run on detached selected geometry through the ordered native worker.
Document incarnation, model revision, authoritative placement, native geometry
identity and measurement generation are checked before publishing a result.
Model or placement changes retire results and request new picks. Inspection
does not author geometry, create history entries, or replace modeling previews.
Idle guides reuse retained storage.

This implementation covers native solid topology. Active/finished sketch
curves, sketch profiles, midpoint snaps, whole component groups, oblique-circle
maximum distance and nonlinear-surface angles are not added by this change.
Unavailable measurements are omitted rather than inferred from tessellation.
Body size uses native world-axis bounds; selection-local deltas use rigid
placement axes, including named-view placement.

Focused native geometry and headless panel checks are recorded in
`docs/qualification/measure-20261011.json`. These do not establish packaged
GPU appearance, physical clipboard operation or live pointer acceptance. The
broader UI-first/MCP-second qualification sequence remains paused.
