# ADR 0003 — 3MF (and STL) print export

- Status: Implemented for native export with per-body appearance
- Original proposal: 2026-07-27
- Status reviewed: 2026-09-12
- Tracking: [#13](https://github.com/limo-cad/Limo-CAD/issues/13)
- Related: [product directions](../goals.md), MCP print focus in ADR 0006,
  tutor quests [#16](https://github.com/limo-cad/Limo-CAD/issues/16)

## Original context (July 2026)

At the time of this proposal, interchange centered on STEP (AP242 export in the
UI). Makers also needed slicer-friendly mesh packages. **3MF** was selected as
the preferred print format, with STL as a compatibility fallback.

Additive manufacturing is a **main project goal**. A print package that drops
appearance data is incomplete for multi-material and painted/assigned-color
workflows.

The original assessment recorded no 3MF writer or MCP manufacturing export tools,
and theme-only appearance settings. That describes the starting point, not the
current implementation.

## Decision and current implementation

1. Native OCCT tessellation feeds one Rust writer in `limo-cad-export`: **3MF**
   for print packages and binary **STL** for geometry-only interchange.
2. Both the desktop File menu and MCP (`solid_export_3mf`, `solid_export_stl`)
   use this export implementation. Native mesh export is not implemented by the
   browser development kernel.
3. **Materials and colors are part of 3MF export.** Saved per-body appearances
   include named material and filament/catalog metadata. When appearance is
   included, 3MF writes base materials and display colors. Per-face painting is
   outside this initial appearance model.
4. Packages use millimetres. Optional Bambu Studio, OrcaSlicer, PrusaSlicer and
   Cura metadata targets provide compatible filament/color hints; these are not
   complete pre-sliced projects or guarantees for every slicer version.
5. Assembly scope exports visible solved occurrences with their placement and
   repetition; definition scope exports retained bodies in part coordinates.
6. Keep **STEP** for exact CAD interchange. **3MF/STL** contain manufacturing
   meshes; **`.limo`** retains the editable sketches, features and assembly.
7. **STL** does not preserve materials or colors. Material assignments and export
   success do not establish physical print fit or strength.

## Consequences

- Body appearances are persisted project data, separate from application theme.
  The shared catalog and its frontend mirror must stay aligned; see
  [the materials model](../manufacturing/materials.md).
- The Rust writer validates mesh integrity before producing a 3MF package.
  Geometry repair or print qualification is not silently inferred from export.
- The desktop and MCP share export behavior and product grouping. Tool disclosure
  is discovery guidance, not an alternate export implementation.

## Validation status

The original acceptance called for colored cube and multi-body examples, STL
fallback, preserved units, MCP coverage, documentation, and inspection in a real
slicer. Current repository tests cover 3MF millimetre units, base materials,
slicer metadata, mesh integrity and STL output. Native recipe tests exercise
STEP/STL/3MF handoff; committed
[smoke fixtures](../../crates/export/fixtures/smoke) support slicer inspection.

This implementation status does not mark every slicer acceptance check complete.
Use the [manufacturing validation guide](../manufacturing/VALIDATION.md) for
target-specific inspection, and retain the source/build, slicer version and print
profile with each result. The [flagship examples](../flagship-examples.md) track
their own digital and physical qualification separately.
