# Drawing commands for replayable engineering examples

Drawing edits use the existing shared document and the same `drawing` workspace
groups as the interactive editor. A recipe stores view intent and topology
references, not precomputed dimension labels or imported line art.

- `drawing/sheet`: create/select/delete sheets and `drawing_set_bom`.
- `drawing/views`: `drawing_add_view`, including the existing section, removed
  section, auxiliary, detail and broken view derivations; `drawing_projection`.
- `drawing/dimensions`: associative linear, radial and angular dimensions.
- `drawing/annotate`: notes.
- `drawing/output`: `drawing_export {sheet_id, format: "svg" | "dxf"}`.

Export returns `{format, encoding: "utf8", content, sheet_id}`. Saving that content
is the caller's responsibility; an export operation does not open a dialog or
choose a filesystem path. A live attached MCP call runs on the owning desktop
engine. Headless calls use the native OCCT kernel in the same process. Both use
the Rust sheet exporter, current exact hidden-line projection and persistent
drawing document. Exported SVG/DXF are review artifacts; the editable `.nbcad`
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

The initial native exporter covers the annotations needed by these flagship
recipes: linear, radial and angular dimensions, notes, title information and BOM.
It rejects other annotation kinds and dual-unit presentation explicitly. The
existing interactive export retains its wider annotation coverage. Migrating
that presentation code to Rust remains work; do not represent this layer as
complete native parity with every drawing-editor annotation. Basic dimensions use
boxed text; leaders and angular dimensions have arrowheads. The title block
retains responsibility, material, tolerance and release fields, and positioned
revision tables retain every revision field. Reserve the bottom-right 180 by
44 mm for the title block, inside the 10 mm sheet border.

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
