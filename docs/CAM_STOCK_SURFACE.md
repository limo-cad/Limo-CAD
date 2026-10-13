# Cutter-aware remaining-stock surfaces

This refines the **display** of actual
remaining stock; it does not alter paths, removed-cell counts, verification
tolerance or the intended CAD model.

## Visible behavior

Hole chamfers and outer-edge bevels have separate normals on the top, bevel
and wall, instead of blending an entire grid triangle across those joins.
Rounded end mills leave a smooth floor fillet using their actual corner radius.
Drill bottoms retain the configured point angle. Shallow cuts retain the smaller
effective cutting radius and the stock it leaves behind; nothing substitutes a
full-diameter cylinder or the finished model for that residue.

The initial stock and recorded cutting sweeps supply the surface. Geometry is
reconstructed in Rust during stock-mesh creation, then reused by the existing
result, native-mesh and prepared-playback caches. Camera motion and drawing a
cached stage do not reconstruct the surface. Cache eviction, a new cut/frame,
or changed inputs can still require computation.

## Geometry

`cutter.rs` shares the profile dimensions used by material removal and display.
With negative values inside the cutter, the field combines its bottom, corner,
outer cylinder and flute-top constraints. For a conical tip of half-angle `a`:

`d_corner(r,z) = r cos(a) - z sin(a)`

For an outer radius `R`, corner radius `c`, and height `z` above the tip, use
`q = (r-(R-c), c-z)` and the tangent-extended quarter-circle field:

`d_corner = length(max(q,0)) + min(max(q.x,q.y),0) - c`

The tangent extension is important: a complete torus would invent a lip in
the flat land. A beveled corner uses the cone field translated to its flat
land radius. These are signed boundary fields, not a claim of exact Euclidean
distance everywhere after Boolean combination.

For an executed swept cutter field `C_i` and analytic initial stock `S`:

`F(p) = max(S(p), -min_i C_i(p))`

The outer stock field clips cutter boundaries at the actual starting envelope.
Box, cylinder and hex stock are supported, including retained same-frame rest
stock. Modeled stock keeps grid-based reconstruction with local cutter
projection; it is not replaced by its bounding box.

Vertical and constant-height line/XY-arc sweeps have direct profile queries.
Sloping straight moves use a bounded one-dimensional field minimization.
Other arc planes and helical moves are represented by bounded line chords,
targeting 2% of the smallest cell dimension. Overlapping vertical pecks with
the same profile merge into one swept interval.

The mesh keeps the numerical grid's resolved topology. Shared vertices use
Hermite edge intersections and regularized tangent-plane least squares,
constrained to their dual cell. This locates a top/bevel intersection without
rounding it into the neighboring faces. Triangles crossing a sharp boundary
are split and shaded separately. Shared-edge, error-driven subdivision targets
2.5% of the smallest cell dimension, with at most two rounds; the limits below
can prevent reaching that target. No global voxel-resolution increase is used.

Planar faces bypass the expensive reconstruction. Exactly extruded strips can
merge along X, Y or Z when the cross-section and normals agree **over every
column of the strip**. Checking only its first column can stretch the far end
into a hole bevel and produce a shading streak on a faced plane.

Reconstruction uses tight executed-arc bounds, including axis extrema and
wraparound; a short corner roll no longer gets a whole-circle search box.
Same-profile sweeps with identical tip-Z intervals form separate search groups.
For their shared meridian field `f`, radial monotonicity gives
`min_i f(r_i,z) = f(min_i r_i,z)`. A center-path BVH therefore finds the nearest
segment/arc once. An inverse radial bound at the current best field value prunes
the search without replacing a segmented path by an ideal circle. Different
profiles, depths and sloping moves remain separate. The finite flute and the
actual corner/point shape participate in that bound.

Field samples and vertex projections use extraction-local direct-mapped caches.
Collisions replace entries, not results; later slices continue benefiting after
a tall part has visited the first 65k vertices. No cache is shared across
changed stock or kept with each playback frame.

## Work, memory and fallback

- At most 2,048 retained sweep primitives; their allocated capacity is included
  in checkpoint byte accounting. Non-horizontal arcs allow at most 256 chords.
- At most 65,536 vertex-projection and 16,384 field-sample cache slots per
  extraction, about 7.12 MiB together on arm64. They are released with the mesh
  builder rather than multiplied across retained playback frames.
- At most four million charged field evaluations per extraction; a sloping
  sweep query is charged more for its bounded minimization.
- The existing 65,536-triangle presentation cap and all retained cache/frame
  budgets remain unchanged. Temporary meshing allocations are separate from
  those retained-cache budgets.
- An incomplete sweep history disables analytic reconstruction. Exhausting the
  field-work budget discards the entire attempted mesh and rebuilds a complete
  grid surface. The returned simulation warnings explicitly identify either
  history or work-budget fallback. Crease splitting also falls back atomically if it cannot fit
  the triangle budget. No partially refined shared boundary is published.
- The existing complete, conservative display-grid coarsening remains the
  last fallback for triangle overflow. Cutting/verification grids do not change.

Only moves that remove occupied cells are recorded. A feature smaller than the
grid may never establish topology or a recorded cut; finer-looking surfaces
do not certify such features. Display reconstruction is disclosed in simulation
warnings. This is not a B-rep export or a machine/fixture/holder clearance proof.

## Regression checks

Synthetic tests cover profile-field signs against the removal envelope,
overlapping-sweep unions, sloped moves against dense pose samples, bounded arc
chords, top/bevel creases, flat-face normals, shallow rounded/beveled cuts,
hole/perimeter bevel and floor-fillet equations, subdivision area/winding, and
whole-mesh fallback. A multi-operation faced/drilled/rounded-mill/chamfered
fixture checks reconstruction at the normal work limit and verifies that
planar strips do not cross into chamfers. Additional tests cover grouped versus
ungrouped unions, inverse profile bounds, arc bounds, and cache replacement.

Capture synthetic Rust output for visual inspection with:

```sh
LIMO_CAD_CAM_DETAIL_CAPTURE=/tmp/cam-stock-detail.json cargo test -p limo-cad-cam --release capture_chamfers_and_fillets -- --ignored --nocapture
node scripts/capture-cam-stock-detail.mjs /tmp/cam-stock-detail.json /tmp/cam-stock-detail.png
```

For a read-only capture of a locally supplied saved job, set
`LIMO_CAD_CAM_DETAIL_PROJECT` to its `.limo` path and run the ignored
`capture_project_stock_detail` test with the same capture variable. It removes
machine configuration only from its in-memory copy and writes just the stock
mesh; it never saves over the project or captures private post settings.

Keep supplied jobs, captures and local timing reports outside the repository.
The isolated preview checks geometry and normals under fixed lighting; it
does not establish production viewport frame rate or machining accuracy.
