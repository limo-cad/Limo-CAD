# 2D technical drawings

The drawing workspace turns the current parametric model into persistent,
printable vector sheets. It follows the same production boundary used by the
rest of noBS CAD: Rust owns document meaning, OCCT owns exact geometry, React
owns document UI, and Bevy remains the native interactive 3D viewport.

## Ownership boundary

| Layer | Responsibility |
| --- | --- |
| `nbcad-sketch` | Persists standards-aware sheets, title blocks, aligned view relationships, semantic annotations, body filters, scale, and display options inside `model.json`. |
| `nbcad-occt` | Produces visible/hidden vector curves from exact B-reps with OCCT HLR and exposes stable endpoints and fitted circular topology for annotations. |
| Tauri host | Serializes drawing commands and exact projection requests with the live kernel. |
| React/SVG | Lays out sheets, edits properties, and moves views. SVG remains an internal browser/debug surface. |
| DXF writer | Emits editable true-size paper geometry, layers, semantic dimensions, leaders, notes, and title-block content for CAD interchange, plus separate 1:1 model profiles. |
| Browser fallback | Projects tessellated topology for fast UI development when the native kernel is unavailable. It is not an exact manufacturing result. |
| Bevy | Owns the native 3D viewport only. Its child view is explicitly hidden while the drawing workspace is active. |

Generated projection curves are deliberately not saved. A drawing view stores
projection intent (`direction`, `up`, body IDs, scale, placement, and line
options), then regenerates its curves from the active feature history. This
keeps saved files small and prevents stale drawing geometry after a model edit.

## Workspace and sheet workflow

Solid Modeling and Drawing are peer workspaces in the application-level
switcher. Sketch remains a contextual mode under Solid Modeling and must be
finished before switching to Drawing. Drawing commands therefore no longer sit
inside the solid-modeling tool taxonomy.

The Drawing ribbon keeps the frequent commands directly visible and groups the
complete tool set into task flyouts: standard/derived views, linear/feature
dimensions, center geometry/manufacturing notes, tolerancing/document symbols,
and output. This is a responsive command taxonomy, not feature removal; compact
desktop windows retain every command without turning the ribbon into a
horizontal scroller.

Entering Drawing does not mutate the project. If there is no sheet, the user is
shown sheet setup first. Creating a sheet adds its border and title block but no
projected views. From there the user may:

- run **Auto Layout** on an empty sheet to add front, top, right, and isometric
  views using the selected first- or third-angle convention; or
- arm an individual view command to get a live, cursor-following projection
  preview, adjust its group scale without leaving placement, and click its
  position on paper. The sheet's first placed view establishes the projection
  group root and scale. Top/bottom views share its paper X position,
  left/right views share its paper Y position, and every related view inherits
  the root scale. Selecting any child still resolves subsequent placement back
  to that root, so view relationships cannot drift into alignment chains.

## Implemented production baseline

### Sheets, navigation, and history

- ISO A0–A4 and ANSI A–E paper in portrait or landscape, with independently
  selectable first- or third-angle projection.
- ISO 2768 f/m/c/v, common ANSI decimal-place tolerances, no general tolerance,
  or a custom company note.
- Complete title-block identity and approval fields, project-local company
  templates, and a shared style editor for fonts, text/arrow sizes, line
  weights, dash patterns, and section hatch.
- A sheet/view browser, movable views and tables, draggable annotations, and
  modeless cursor-following placement previews.
- Drawing-specific undo/redo. A compound command such as Auto Layout or a
  completed drag is one history operation rather than several internal edits.
- Cursor-anchored pinch/wheel zoom from 25% through 500%. Unmodified macOS
  two-finger movement pans in both axes; middle-button drag pans with a mouse
  on macOS and Windows.

### Projected and derived views

- Front, rear, left, right, top, bottom, isometric, and custom orthographic
  views with body filters, group scale, placement, hidden-line display, and
  tangent-edge display.
- Related orthographic views inherit the root view's scale and remain aligned
  according to the active projection convention.
- Associative full and depth-limited section views, removed sections, detail
  views, auxiliary views, and broken views. Cutting lines, detail boundaries,
  auxiliary references, and break parameters remain editable semantic data.
- Native desktop projections use exact OCCT B-reps and hidden-line removal.
  Section views clip the retained material before HLR, calculate the exact cut
  at the original plane, and return closed hatch regions.

### Dimensions and center geometry

- The primary **Dimension** command is context-aware. Two points or analytic
  centers produce a linear dimension, a complete circular edge produces a
  diameter, and an open circular arc produces a radius. Center and circumference
  have distinct hit regions so a hole can still participate in center-to-center
  dimensions. Selecting a center or endpoint and then an exact straight edge
  (in either order) produces an associative perpendicular point-to-edge
  dimension. Selecting one exact straight edge starts a live edge-length
  preview whose placement is constrained normal to that edge. Selecting a
  second parallel edge changes the same preview into perpendicular separation;
  selecting a second nonparallel edge changes it into the included angle. These
  relationships retain exact OCCT edge references rather than projected pixel
  shortcuts. Intent-heavy families remain explicit choices in the Dimensions
  flyout rather than being guessed.
- Aligned, horizontal, vertical, diameter, radius, three-point angular,
  chain, baseline, continued, ordinate/coordinate, arc-length, and jogged-radius
  dimensions.
- Dimension presentation supports precision, prefix/suffix, symmetric or
  deviation tolerances, limits, basic/reference state, dual units, and fit/class
  text while keeping one shared measured value.
- Linear-dimension typography follows the active sheet standard instead of one
  generic SVG treatment. ISO values sit above an uninterrupted dimension line;
  ASME values may use the conventional centered interruption when they fit.
  When a span is too narrow for the value and two uniform arrowheads, both
  standards reverse the arrowheads outside the extension lines. The value can
  remain between them when it still fits; otherwise it moves beyond one
  terminator. Text masks must never consume an arrowhead.
- Associative center marks, centerlines through two circles, symmetry axes
  between parallel edges, automatic view symmetry axes, and bolt-circle center
  patterns. Their stored extension remains editable numerically, and selecting
  any of these annotations exposes endpoint grips for direct length adjustment;
  dragging a grip changes only paper presentation, never the referenced model
  topology.
- Circular and endpoint picking uses analytic OCCT topology. Coincident
  projected rims collapse to one visible target without discarding their exact
  edge identity.

### Manufacturing annotations

- Feature-driven hole and thread notes. When the projected circle belongs to a
  modeled Hole feature, diameter, depth/THRU, counterbore, countersink, thread,
  thread depth, quantity, and pattern intent come from feature history instead
  of being guessed from pixels.
- Automatic ISO/ANSI chamfer notes from eligible true-shape straight chamfer
  edges.
- Datum feature symbols and targets; typed feature-control frames with datum
  order and material-condition modifiers; surface-texture, edge-requirement,
  and weld symbols.
- Item balloons linked to a sheet BOM, draggable BOM and revision tables, and
  revision clouds.
- Missing topology is shown as unresolved. **Reassociate** offers compatible
  live topology and requires explicit user confirmation; diagnostic fallback
  coordinates are never silently accepted as manufacturing references.

### Output and document control

- AutoCAD 2013 ASCII sheet DXF in paper-space millimetres. It includes distinct
  border, visible, hidden, center, cutting-plane, phantom, break, hatch,
  dimension, leader, note, label, and warning layers. The style registry emits
  `NBS_HIDDEN`, `NBS_CENTER`, `NBS_CUTTING`, `NBS_PHANTOM`, and `NBS_BREAK`
  linetypes consistently with the on-screen and print renderers.
- Face-on circles remain `CIRCLE`; associative measurements remain semantic
  `DIMENSION` entities with anonymous graphics blocks and noBS CAD metadata;
  callouts remain `LEADER` plus `MTEXT`; sections include `HATCH` entities.
- A separate, explicitly selected manufacturing-profile DXF exports one sketch
  material region at true 1:1 model scale. Its even-depth outside wire goes to
  `PROFILE_OUTER`, its immediate hole wires go to `PROFILE_HOLES`, and no paper,
  title-block, view-placement, or sheet-scale geometry is included.
- The platform print dialog is the printable PDF path. SVG is retained as the
  common internal screen/print representation and browser verification surface,
  not advertised as the industrial interchange format.
- Revisions carry author/checker/approver, date, change-order, and lifecycle
  state. A released revision is immutable. Editing issued sheet content returns
  the working sheet to Draft while preserving the last issued revision record.

## ISO/ANSI-ASME standards baseline

The drawing system treats ISO and ANSI/ASME as semantic drafting profiles,
not as two label themes. The baseline references are
[ISO 128-2:2022](https://www.iso.org/standard/83355.html) for line
conventions, [ISO 128-3:2022](https://www.iso.org/standard/83356.html) for
views/sections/cuts, [ISO 129-1:2018](https://www.iso.org/standard/64007.html)
for dimension presentation, and the official
[ASME Y14 family index](https://www.asme.org/codes-standards/y14-standards)
(notably Y14.2, Y14.3, Y14.5, Y14.6, Y14.36, and Y14.100). Company practice
may narrow these defaults but must never silently change their meaning.

The implemented standards model has six coordinated semantic groups:

1. **Line and center geometry.** One line-style registry owns visible, hidden,
   center, cutting-plane, phantom, break, dimension, extension, leader, and
   hatch appearance across screen, print, and DXF. A line-pair center axis is
   accepted only when the exact projected edges are distinct, parallel,
   separated, and have a meaningful overlapping span; nonparallel pairs are
   not guessed into an angle bisector.
2. **Dimension families.** Every dimension stores topology, measurement mode,
   presentation, and paper placement separately. ISO and ASME formatting is
   selected at render/export time while the measured semantic value remains
   shared.
3. **Sections, cuts, details, breaks, and auxiliary views.** These remain
   associative children of their source view rather than detached SVG copies.
4. **Hole and thread intelligence.** Representation and callout dialect follow
   [ISO 6410-1](https://www.iso.org/standard/12750.html),
   [ISO 6411](https://www.iso.org/standard/12753.html), ASME Y14.6, and the
   selected dimensioning profile.
5. **Datums and GD&T.** Typed specifications retain ordered datum systems,
   material-condition modifiers, tolerance-zone parameters, and exact topology
   attachments. ISO mode follows
   [ISO 1101:2017](https://www.iso.org/standard/66777.html) and
   [ISO 5459:2024](https://www.iso.org/standard/87855.html); ANSI mode follows
   ASME Y14.5. They are semantic fields, not interchangeable glyph strings.
6. **Manufacturing symbols and document control.** Surface texture follows
   [ISO 21920-1:2021](https://www.iso.org/standard/72196.html) or ASME Y14.36,
   and defined/undefined edge requirements use
   [ISO 13715:2017](https://www.iso.org/standard/61328.html). Weld symbols, item
   balloons/BOMs, revision tables, and company templates remain semantic data
   with leaders/attachments rather than precomposed text.

Across all six groups, Rust persists and validates intent, OCCT resolves exact
topology and analytic geometry, React supplies placement/editing, and output
writers format the selected standard. Raster/tessellation proximity is useful
for hover feedback only; it is never the saved manufacturing reference.

## Projection contract

`DrawingProjectionRequest` describes an orthographic camera in model space.
`direction` points from the model toward the viewer and `up` specifies page up.
The native kernel orthogonalizes these vectors, runs `HLRBRep_Algo` against the
selected live B-reps, and returns page-space polylines split into visible and
hidden sets. It also projects stable OCCT topology edge endpoints through the
same basis. Stable model edges are fitted as circles in model space and only
exposed as circular annotation targets when their normals are face-on to the
drawing view. The caller controls curve deflection; output remains in model
millimetres and is scaled during sheet layout.

A section request additionally carries a cutting-plane point/normal and an
optional positive depth. A full section retains the half-space behind the
cutting plane; a depth section retains only the finite slab between the front
plane and the requested back depth. HLR runs on that clipped solid while the
section curves themselves are calculated against the original exact shape at
the front plane. This prevents geometry in front of the cut, or beyond a depth
limit, from leaking into the section view.

The browser fallback shares this request and response format. It projects
topological edge polylines, derives mesh silhouettes, clips topology and mesh
triangles to the same half-space/slab, and performs depth tests against the
remaining tessellation. It is suitable for visual UI work and agent/browser
verification but must not be used to approve a production drawing.

Every edge endpoint is retained in the projection response even when adjacent
edges share one geometric vertex. The UI collapses only the coincident generic
point markers; edge-specific annotations continue to resolve the exact stable
OCCT edge ID and backend key. This distinction is required for chamfer notes,
where selecting the cut edge must not silently bind the callout to either
carrier edge.

## Chamfer callouts

noBS CAD uses the explicit distance-and-angle form as its interoperable
default. ISO sheets render `3 × 45°`; ANSI/ASME sheets render `3 X 45°`. The
screen, printable SVG/PDF path, and DXF `LEADER`/`MTEXT` output share the same
formatter. A shorthand such as `C3` is deliberately not the cross-standard
default because it depends on drawing/company conventions.

The command accepts only straight chamfer edges shown in true shape. An
eligible edge must join nonparallel, nonperpendicular straight carrier edges at
both endpoints. Coincident front/back projected candidates are collapsed to
one deterministic target, preferring the OCCT HLR-visible edge and then the
front-most edge. The persisted annotation keeps both endpoints of that exact
edge, the measured setback, angle, and paper-space leader position.

References: [ISO 13715:2017](https://www.iso.org/standard/61328.html),
[ISO 129-1:2018](https://www.iso.org/standard/64007.html), and
[ASME Y14.5-2018](https://www.asme.org/codes-standards/find-codes-standards/y14-5-dimensiones-y-tolerancias).

## Persistence and compatibility

Drawing data is an additive, defaulted field in the existing project schema.
Older `.nbcad` files open with an empty drawing document. Saving a project
round-trips drawing intent through the Rust model; generated lines never enter
the archive. Drawing IDs are project-global and validated along with active
sheet references, standards settings, view bases, parent/alignment
relationships, positions, body filters, annotations, and scale values. Point
anchors retain body ID, edge ID, backend topology key, endpoint role, and a
diagnostic fallback model point. Circular references retain the same exact
topology identity plus fitted center, normal, radius, and closed/arc intent.
Exact ID resolution is preferred, with the backend key available across
compatible recomputes.
Straight-edge references used by symmetry centerlines retain body ID, OCCT
edge ID, backend topology key, and diagnostic model endpoints. The rendered
midline is always regenerated from the resolved projected edges, so view
scale, alignment, and model recompute cannot turn it into detached paper
geometry.

Derived-view references use the same rule. Their cutting lines, detail centers,
and auxiliary directions are re-resolved against the current scene before each
projection. Stored fallback coordinates are diagnostic only. If neither the
stable ID nor backend key resolves, the view/annotation stays visibly broken
until the user chooses a compatible replacement through **Reassociate** and
confirms the change.

New drawing fields use serde defaults, so older archives upgrade in Rust. The
frontend also normalizes documents at the engine boundary. This makes a stale
development WASM build or an older additive project schema safe to inspect
without distributing partial-document checks throughout the UI.

Browser eye-toggle state is project data as well. Bodies and construction
planes persist by stable model ID, while sketches persist by their unique saved
name; transient Browser row IDs are deliberately never written to the archive.
The same state is restored after Save/Open and after a project tab is unloaded
and reconstructed.

## Verification and release boundary

The drawing acceptance path covers Rust schema/default/validation round trips,
native OCCT full/depth section clipping, frontend type/build checks, and a
browser end-to-end workflow that creates ISO and ANSI sheets, places aligned
views, creates/drags associative annotations, exercises undo/redo, and parses
both sheet and 1:1 profile DXF structure.

DXF and printed PDF are the supported delivery paths in this implementation;
DWG is not emitted. Company and regulatory drawing release still requires the
organization to review its chosen template, fonts, tolerances, weld/GD&T
practice, and approval process. The software keeps the data semantic and
standards-aware, but does not replace that engineering sign-off.

Future annotation types must follow the same invariant: persist semantic
topology references and measurements, not sampled screen coordinates. React
may edit and render those annotations, Rust validates them, and OCCT remains
the authority for exact geometry.


## Part definitions and placed assemblies

Every drawing view persists a `scope`: `definition` (the legacy default) or
`assembly`. Definition views use retained part coordinates. Assembly views
project the current solved visible occurrences together, including repeated
parts and nested placements. This lets one exact B-rep hide another in OCCT's
hidden-line pass. `body_ids` filters source parts; `occurrence_ids` filters
instances, and selecting a subassembly includes its descendants. Empty filters
select everything in the chosen scope. An empty visible assembly selection is
an error and never falls back to definition geometry.

The Drawing inspector exposes the same scope and occurrence selection used by
MCP `drawing_projection` and stored views. Scope belongs to the view, so a sheet
can contain an assembly overview beside definition views for manufacturing.
No placement is baked into the retained part geometry or feature history.

Projected endpoints and circles include `occurrence_id`. Associative point,
line, and circular references preserve this id, preventing a dimension from
attaching to a different copy of the same part. Older definition references
remain definition references. A scope change can require reassociating them.
Derived assembly cutting planes and auxiliary directions resolve the placed
source topology. Native desktop and MCP use `nbcad_occt::project_drawing`;
the development browser's tessellated fallback consumes the same Rust-solved
instance poses. Exact hidden-line generation remains in the native OCCT bridge.
