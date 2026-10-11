# Section inspection and diagram capability

Section Analysis is a modeling inspection tool in the native Bevy desktop.
It shares exact OCCT section primitives with drawings without creating features
or drawing sheets and without changing source geometry. Inspection uses a focused
section query; it does not run the drawings' full hidden-line projection.

## Capability audit and order of work (2026-10-06)

1. **Modeling section inspection (this change).** OCCT section intersections,
   drawing sections, hatch primitives, associative drawing dimensions, SVG/DXF
   export and Bevy screenshot capture already existed. The modeling ribbon's
   Section Analysis command was a disabled placeholder. Expose the shared
   engine through a disposable Bevy panel and MCP read, with sampled material
   intervals, a flat diagram and an orbitable, capped 3D half-solid. Both retain
   OCCT as their geometry authority. No second drawing engine or chart dependency.
2. **Interactive geometric measurements.** Drawing dimensions already exist;
   modeling's Measure placeholder still needs a general selected-point/edge/face
   tool. Reuse the topology references and measurement/unit forms. The section
   probe here is a directional material-span measurement, not automatic minimum
   wall thickness.
3. **Further 3D inspection and comparison presentation.** Axial source-body
   clipping/caps are included here. Add assembly/oblique sections and section
   picking alongside the Bevy renderer. Exact
   assembly interference and source-solid comparison already exist; improve
   discoverability and visual evidence before adding another geometry API.
4. **Engineering report composition.** Named views, screenshot capture and
   drawing exports already supply most ingredients. Add a shared review layout
   with model-revision provenance, multiple views and diagrams. Do not recreate
   those primitives in bespoke Python as an application feature.
5. **Structural analysis ([issue #336](https://github.com/limo-cad/Limo-CAD/issues/336)).** No production structural solver
   was found. Require material provenance, boundary conditions, mesh convergence
   and benchmark validation before presenting force/strain predictions.

## Use

Solid workspace → Check → Section Analysis opens a compact panel on the right
and automatically shows a 3D cutaway through the selected body's center. Choose
one source body and an XY/XZ/YZ plane. Drag the position bar to move the cut
through that body's extent, or enter an exact offset. The cutaway updates live;
there is no separate Inspect step. Above/Below plane changes the retained side.
Changing the body or plane centers the cut in the new axis. An optional probe's
vertical coordinate measures directional material spans.
Fields accept the document's length units or an explicit `mm`, `cm` or `in`
suffix using the existing arithmetic input. The diagram and returned distances
are explicitly millimetres. These are source-definition coordinates, independent
of occurrence transforms, exploded presentation and print-bed placement.

`solid_section_review` takes `body_id`, `plane` (`xy`, `xz`, `yz`), `offset_mm`,
optional `probe_mm`, and optional `deflection_mm` (0.001–0.1; default 0.01).
It returns an explicit outcome (`material_section`, `boundary_contact` or
`no_intersection`), section-only bounds, sampled intersection curves, separate
`probe_spans` with start/end/length in mm, and an SVG document. The SVG can be
saved by a client without creating a drawing sheet. XY uses X/Y, XZ X/Z, YZ Y/Z
as horizontal/vertical axes. The offset refers to Z, Y, X respectively.
The Bevy panel's Copy SVG button places the current diagram on the native
clipboard for saving or pasting into another tool; Escape closes the panel.
Orange surfaces identify the cut faces in the modeling viewport. Normal orbit,
pan, zoom, orientation and Fit controls remain available in both section modes.
Fit frames the inspected source body. **2D diagram** shows a compact flat preview;
**3D cutaway** returns to the retained half-solid.
Close restores the source scene with its original visibility choices. Temporary
cut faces are not selectable as modeling topology. Other bodies and occurrence
poses are excluded from this source-definition review.

Persistent drawing sections remain in the Drawing workspace, with associative
parent views, cutting-line annotations, hatching and SVG/DXF export. The modeling
inspector does not create or change those sheets.

Optional `include_cutaway` (default false) also returns `cutaway`, a tessellation
of an exact OCCT half-solid. `keep_positive` (default false) retains coordinates
above the plane instead of below. The viewport and drawings share the same
`BRepPrimAPI_MakeHalfSpace` / `BRepAlgoAPI_Common` clipping helper. The cutaway
copies source geometry before clipping/tessellation and is limited to one million
vertices, one million triangles and 100,000 edge points.

Intersections use exact OCCT section primitives shared with drawings; hatching
uses the existing bounded paper-graphics code. Cutaway meshing runs only when
requested. Probe distances use sampled section curves, including holes and
disconnected material with even/odd
crossings. Open/ambiguous probe intersections reject rather than invent a
thickness. Invalid requests and kernel failures return errors. A plane that
touches the boundary without splitting a connected source solid returns
`boundary_contact`, with empty material spans and no cutaway. The diagram can
show its boundary contours; 3D mode preserves the source instead of displaying
an empty clipped body. `no_intersection` returns no bounds and an empty SVG.
Curves are limited to 100,000 points and probe
results to 128 intervals. A probe on a boundary follows a half-open convention;
move it inside the material for an interior measurement.

The preview worker uses an independent copy of the selected exact source body,
captured once per document revision/body. Section queries do not hold the live
engine lock or occupy its mutation worker, so dragging and camera navigation
remain available. One query runs at a time; intervening edits collapse to the
latest draft. During a sustained drag, compatible completed positions can update
the viewport while the newest position computes. SVG copy is available only for
the completed current draft. Changed bodies, planes, sides, view modes, document
owners and revisions reject incompatible completions. Closing the panel releases
its controls, preview and worker without waiting for an in-flight read.
Every completion also retains the backend's private source proof and validates
the original scene and native body identity before being displayed.
Reads create no feature, drawing, history entry or geometry revision.

This tool supplies geometric evidence. It does not calculate snap force,
displacement, stress, strain, creep or slicer wall-loop behavior.
