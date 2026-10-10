# Drawing commands for replayable engineering examples

Drawing edits use the existing shared document and the same `drawing` workspace
groups as the interactive editor. A recipe stores view intent and topology
references, not precomputed dimension labels or imported line art.

- `drawing/sheet`: create/select/delete sheets, BOM, template create/apply/delete,
  revision append and release status.
- `drawing/views`: add/update/delete views, including section, removed section,
  auxiliary, detail and broken view derivations; `drawing_projection`.
- `drawing/dimensions`: associative linear, radial and angular dimensions.
- `drawing/annotate`: notes and typed add/update/delete for every saved annotation
  family. The engine allocates IDs and validates current topology and view scope.
- `drawing/output`: `drawing_export {sheet_id, format: "svg" | "dxf"}`.

Export returns `{format, encoding: "utf8", content, sheet_id}`. Saving that content
is the caller's responsibility; an export operation does not open a dialog or
choose a filesystem path. A live attached MCP call runs on the owning desktop
engine. Headless calls use the native OCCT kernel in the same process. Both use
the Rust sheet exporter, current exact hidden-line projection and persistent
drawing document. Exported SVG/DXF are review artifacts; the editable `.limo`
project and Rust replay recipe remain the design sources.

Derived views include their source markers on the parent view. Section and
removed-section cutting lines extend across the parent bounds with arrowheads
and labels; detail boundaries, auxiliary arrows and break indicators use the
same paper conventions as the editor. Short source datum pairs define a plane,
not the length of its cutting line. Exact arc centers remain usable when viewed
edge-on and after assembly placement. Both SVG and DXF include these markers;
missing source topology rejects export instead of substituting fallback points.

View positions specify the center of the projected bounds in paper millimetres,
matching the interactive editor. Scale converts model millimetres to paper
millimetres. Dimensions resolve current edge IDs and stable keys. Diagnostic
fallback points never become an accepted substitute for lost topology.

Native edge keys are OCCT ordinals, so drawing references also capture the
body's owning feature and exact structural connectivity signature. A normal
dimensional edit can keep its associations; a changed edge/vertex/wire graph or
owning feature requires explicit reassociation before export. The signature
excludes dimensions and mesh quality. This is conservative invalidation, not
complete OCCT historical naming across arbitrary Boolean edits or graph
symmetries. Existing captured guards are never refreshed by an unrelated drawing
edit. Legacy unguarded references must be explicitly recreated against current
geometry; saved fallback coordinates never establish their identity. Unrelated
sheets can still export when another sheet needs reassociation.

`drawing_add_radial_dimension` accepts the circular reference from a projection,
mapped to `fallback_center`, `fallback_normal` and `fallback_radius`. It allocates
the annotation ID, as do the other dimension commands. A stale or excluded
reference rejects the entire edit without consuming an ID. Replacing a BOM with
attached item balloons is rejected so balloons cannot silently change meaning.

## Validation and remaining work

Regression tests use real native MCP calls to create solid geometry, add all
three dimension kinds, set a BOM, export, save and reload. They check exact repeat
output, current dimensional values and atomic rejection of stale references and
invalid BOM quantities. Host-neutral tests additionally check the UI's centered
view transform, edited geometry, XML/DXF text escaping and hatch voids.

The native exporter covers linear, radial and angular dimensions, straight-edge
Length/Distance/Angle dimensions, point-to-line dimensions, center marks,
two-circle centerlines, revision clouds, notes, title
information and BOM. Straight dimensions resolve current projected endpoints,
including occurrence identity, and use the same pure geometry and narrow-span
label layout as native paint. Missing, excluded or stale references reject the
whole export; diagnostic endpoints cannot repair them.

The engine passes the existing document's mm/cm/in display setting to the shared
formatter. Geometry and DXF `$INSUNITS` remain millimetres. Length tolerances and
secondary units use the same conversion as native paint; angular labels remain
degrees and retain but do not display linear secondary-unit metadata. The
host-neutral `export_sheet` helper defaults to millimetres; callers that own
document settings use `export_sheet_with_units`.

The exporter also supports chamfer and hole notes, between-edge centerlines,
symmetry and bolt-circle markings, chain/baseline/continued and ordinate dimensions,
arc lengths, jogged radii, datums/GD&T, surface/edge/weld symbols and balloons.
Each record must satisfy its geometry and layout requirements; stale references
and unsupported constructions still reject the complete export. Basic dimensions use
boxed text; leaders and angular dimensions have arrowheads. The title block
retains responsibility, material, tolerance and release fields, and positioned
revision tables retain every revision field. Reserve the bottom-right 180 by
44 mm for the title block, inside the 10 mm sheet border.

Rust DXF output is **graphical** LINE/TEXT/SOLID artwork, not editable associative
DXF DIMENSION entities. The native File menu exports the active drawing as SVG
or DXF using this shared engine command. The picker selects a destination;
the existing ordered worker verifies document ownership and revision again
before projecting and atomically writing it. Cancellation does nothing, and
unsupported annotations or stale topology fail before touching the destination.
Export leaves document history and the project's `.limo` save path unchanged.
Attached automation uses `cad_interface` with `action: "file"`, command
`export_drawing_svg` or `export_drawing_dxf`, and an absolute `path`; replacing
an existing file requires explicit `overwrite: true`.

Native regressions cover the output bytes against the real engine, save-path
preservation, cancellation, stale receipts and failure before replacement.
The disposable Linux drawing fixture also captures the File menu and compares
both written files to the live engine output; those new live results and actual
OS save-dialog input are still pending. Profile DXF uses the retained manufacturing
profile through the ordered File worker. Native printing and PDF preserve physical
sheet size; focused checks cover the resulting payload and PDF write, while physical
printer and OS-dialog qualification remain separate evidence.

Shared view updates preserve alignment, follow dependent views when the parent moves,
and optionally rescale the linked group. Deleting a view with dependent views or
annotations requires explicit `cascade: true`. Annotation updates reject stale
topology atomically. Issued revision rows remain immutable; content edits return a
released sheet to Draft without erasing issued history. Geometry edits revoke model
sheet release, and assembly placement edits revoke assembly sheet release. Saving,
reopening and recomputing unchanged geometry preserve release metadata.

### Revision-cloud evidence

Revision clouds reuse the native painter's pure scallop geometry over the saved
paper vertices, retaining arbitrary polygon order, repeated vertices, revision
text and the fixed red 0.45 mm solid stroke. The revision label remains 3.2 mm
with shared native/export multiline spacing. Its full final-row rectangle clears
the scallop stroke by at least one paper millimetre; preceding rows grow upward.
This avoids the former multiline label crossing the top scallops without moving
the saved polygon. Cloud geometry is independent
of view scale and document display units. DXF uses its nearest valid discrete
lineweight; SVG and saved intent keep the exact width.

Generation is preflighted before tessellation and shares the sheet graphics
budget. Invalid coordinates or excess work reject the whole export. Cloud
stroke centerlines are trimmed to the sheet rectangle without rewriting saved
vertices. A cloud caption whose native label bounds extend outside the paper
rejects export, since DXF TEXT cannot reproduce partial viewport glyph clipping.
This is a cloud-specific boundary, not general sheet layout or clipping parity.
Rejected export cannot replace an existing destination through the native File
worker's existing atomic write path.

Run `cargo run -p limo-cad-occt --example drawing_cloud_export --
C:\absolute\fresh\cloud-export-evidence` for four synthetic SVG/DXF pairs:
triangle, quadrilateral, loaded seven-vertex polygon and a cloud crossing the
right sheet edge. Exact source records and a manifest are retained. Unit and
native integration tests accompany the change, but running them and independently
rendering/reviewing these artifacts are separate validation steps. The fixture
does not prove native authoring, File UI, OS input or printing.

### Center-marking evidence

Center marks and two-circle centerlines resolve current closed circles by
body, occurrence, stable edge key and topology signature. The native painter
and exporter share paper-space extent geometry; extension remains paper
millimetres at every view scale. Missing, open, stale, nonfinite or coincident
references reject the export. Saved diagnostic coordinates are never a fallback.
The shared sheet center style supplies dash and width; the small center rings
retain their continuous stroke and white interior.

DXF supports a discrete pen-width enumeration. It uses the nearest valid width
for custom styles (for example, 0.36 mm becomes 0.35 mm); SVG and the saved
document retain the exact width. See Autodesk's
[lineweight values](https://help.autodesk.com/cloudhelp/2023/ENU/AutoCAD-Core/files/GUID-21DF5F82-4F3A-4F93-8FD6-89A942799468.htm).

For eight synthetic projection cases covering both markings, two view scales
and default/custom styles, run `cargo run -p limo-cad-occt --example
drawing_center_export -- C:\absolute\fresh\center-export-evidence`. The example
writes SVG/DXF pairs and exact source records without opening a window. These
artifacts require independent rendering and visual review; they do not prove
live OCCT projection, native placement or physical grip dragging.

### Straight-dimension evidence

Host-neutral regressions cover both formats, current dimensional edits,
occurrence exclusion, signature/key/endpoint failures, finite geometry,
mm/cm/in, full tolerance/fit/basic/reference/secondary-unit intent, vertical
text rotation, ANSI shaft interruption and exact input preservation. A native
engine regression projects a real extruded rectangle and checks loaded document
units plus unchanged full project/history data. Running these tests is separate
from reviewing exported pixels.

For reproducible artifacts without opening a window, run from the repository:

```powershell
cargo run -p limo-cad-occt --example drawing_straight_export -- C:\absolute\fresh\straight-export-evidence
```

The example preserves prior evidence and writes 24 SVG/DXF pairs plus their
source drawing, scene, projection and unit records: Length, Distance, Angle and
Point-line, each in mm/cm/in and default/full presentation. This is an explicit
**synthetic projection** fixture, not proof of OCC projection or physical input.
Render the SVG and inspect DXF through an independent parser/renderer before
claiming visual parity. The manifest deliberately leaves pixel review required.

DXF declares AC1021 (R2007) and writes Unicode text as UTF-8. Declared line-type
and layer tables include unique handles and ownership. Basic and angular
labels mask crossing strokes in primitive order while preserving measured
arrow positions. Display units also appear in the title block; coordinates
remain paper millimetres.

DXF also declares an owned STANDARD text style and ACAD application record.
Every text entity references that style, which retains the first family from
the saved sheet font list through DXF extended font data. It does not guess a
platform font filename or embed a font. The receiving application must have
that family and support extended font data; a CSS fallback list is not a DXF
font fallback chain. See the [DXF font-style format](https://ezdxf.readthedocs.io/en/stable/dxfinternals/tables/style_table.html)
and [font portability limits](https://ezdxf.readthedocs.io/en/stable/tables/style_table_entry.html).
The default Arial family does not provide every Unicode glyph. For a CJK pixel
check, the cloud example accepts an optional second argument such as
`"Microsoft YaHei, Arial, sans-serif"`; it saves that exact document style in
the evidence rather than substituting glyphs in the reviewer.

Assembly views explicitly select `scope: "assembly"` and optional occurrence
IDs. Exact hidden-line removal runs on the combined placed B-reps, including
repeated instances. Associative references include occurrence identity; a
reference excluded from the view rejects atomically. Schema 5 prevents older
readers from silently dropping that placement identity. Schema 1 through 4 files migrate
with their earlier definition-view semantics.

Physical inspection is still needed for print fits, load/creep qualification and
the purchased generator's measured mounting dimensions. A successful drawing
export or exact geometric replay does not qualify a printed part's allowable
load or establish a GD&T tolerance capability for a printer.

Project schema 6 preserves these structural guards. Schema 1–5 files still load, but missing guards stay unverified: loading or resaving cannot establish which historical edge an ordinal meant. Explicitly reassociate those annotations before exporting the affected sheet. Older schema-5 readers reject new files instead of silently deleting the guards.
