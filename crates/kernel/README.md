# Limo kernel

Limo CAD's own boundary-representation kernel: exact solids, written in
Rust with no native dependencies. It models the same recompute plans as the
Open CASCADE adapter and serves the same sections, drawings, measurements,
interference checks and STEP files. Every bundled recipe gives the same
solids on both kernels; the time spent in this one is about a twentieth.

It is opt-in. Open CASCADE stays the default kernel, and what this one does
not model yet it refuses by name rather than approximate.

## Try it

```sh
# The kernel on its own: about a hundred and thirty tests, under a minute.
cargo test -p limo-cad-kernel

# Time the sample parts, then model each at 2000 random sets of dimensions.
cargo run --release -p limo-cad-kernel --example bench -- --sweep 2000

# 600 pairs of solids in random poses: unite, cut, intersect, audit, check the books.
cargo run --release -p limo-cad-kernel --example pairs -- 600

# A live demo in the browser: one self-contained HTML page.
cargo xtask kernel-demo
```

In Limo CAD itself the kernel is chosen per process:

```sh
LIMO_CAD_KERNEL=limo limo-cad          # desktop
LIMO_CAD_KERNEL=limo limo-cad-mcp      # headless MCP server
```

Both hosts go through `OcctKernel` in `crates/occt`, which hands every
recompute and every geometry service to this crate when the variable is
set. Sketching, history, the viewport and scripts are unchanged: they only
see what the kernel returns.

## How it is built

The kernel is data-oriented rather than object-oriented.

- **Geometry is tagged plain data.** A surface or curve is a kind, a frame
  and a few numbers ([`geom`](src/geom.rs)). Evaluation is a `match`. There
  are nine surfaces: plane, cylinder, cone, sphere, torus; the two that
  wind along an axis, the helicoid of a thread's flank and the coil of its
  rounded corner; the spline, for anything free-form; and the pipe, which
  is what a ball leaves behind when it rolls along a crease.
- **What does not fit in a few numbers is written once and referred to.**
  A spline has as many control points as it needs, and a pipe has a spine.
  Those live in a store of their own ([`form`](src/form.rs)), never changed
  once written, and the plain data carries one word that says which. A
  surface still copies like a number, and two that refer to the same
  definition are the same surface.
- **Topology is flat arrays.** A [`Body`](src/topo.rs) is a handful of
  vectors indexed by `u32`. Half-edges are implicit: edge `e` owns `2e` and
  `2e + 1`, and the twin of `h` is `h ^ 1`. There are no seam edges: a full
  cylinder is one face with two circular loops, and the flank of a thread
  is one face however many turns it makes.
- **Operations are functions from bodies to bodies.** Nothing is edited in
  place, so the session keeps the result of every history step and replays
  only what changed.
- **Exact where it matters.** Edges are lines, circles, ellipses, helices,
  splines, the closed-form curve in which a cylinder meets another
  quadric, and for everything else the curve in which two surfaces meet,
  written on one of them and solved to rounding at every point. Triangles
  are derived for display and never fed back into modelling.

A Boolean ([`boolean`](src/boolean.rs)) intersects surfaces in closed form
where there is one and traces the curve where there is not
([`meet`](src/meet.rs)), clips the curves to both faces, merges everything
into one arrangement of vertices and edges, splits each face along it,
decides by ray casting which pieces are inside the other body, and sews the
chosen pieces. Coincident faces are handled by imprinting one boundary on
the other. The tracer searches a grid on one of the two surfaces, and looks
between its lines as well: for a curve that turns back within one cell, and
for a closed one too small to cross any, as round the tip of a cone that
only just pokes through.

Fillets and chamfers ([`blend`](src/blend.rs)) are modelled as the solid
they remove or add: a prism along a straight edge, a solid of revolution
around a circular one, a sphere piece where three rounds meet. A picked
edge is followed along its tangent chain, as a rolling ball would; a blend
runs through an inside corner until it meets the next one, and ends on the
surface of whatever face ends its edge. An edge of any other shape that
closes on itself (the rim where a rod is cut off on a slant, the saddle
where one bore runs into another) is rounded by the ball's own surface:
the pipe about the curve its centre follows, which is where the two faces
moved by the radius meet. The tool is a ring of three faces, two of them
strips of the body's own surfaces, and the curves the ball touches along
are its edges, so nothing ever has to find where two surfaces that only
touch would meet.

Free-form solids ([`loft`](src/loft.rs)) are skins of rational splines.
Every segment of a section is one curve of degree two, which a line and an
arc both are exactly, and the skin over the same segment of every section
is the spline surface through those curves. The first and last sections
keep their true lines and arcs. A profile kept facing one way along a path
sweeps surfaces whose control points are the sums of the profile's and the
path's, and whose edges are again all lines and arcs.

Volume, area, centre of mass and the inertia tensor ([`props`](src/props.rs))
are integrals along the exact edges of every face: Green's theorem turns
the sum over a face into a line integral round its loops. No face is cut
into triangles, and the answers are good to rounding. A body can also be
held to its own geometry ([`audit`](src/audit.rs)): every edge in both of
its faces and ending at its vertices, the skin the right way out.

## Names that survive edits

A feature keeps the names of the faces and edges it was applied to, and a
name says where a face came from, not where it stands in a list
([`name`](src/name.rs)). Every face carries a tag set by the feature that
made it and handed on by every operation that cuts it up, moves it or
merges it; an edge is named after the two faces it lies between. So:

- a round stays on its edge when a hole is added before it, a sketch line
  is inserted, or the sketch is resized;
- an edge that a later cut divides is still that edge, all of its pieces;
- a reference to something that is gone says so and asks to be reselected.
  It never quietly lands on a neighbour;
- a feature can be deleted anywhere in the history: what follows is
  replayed by the names it already has. The host first tries the deletion
  on a copy of the project, and refuses it, changing nothing, if a later
  feature was applied to something only the deleted one made;
- where a round rim starts is a vertex of the model, and stays one when a
  later feature cuts the rim somewhere else, so a dimension tied to it
  still finds it.

Documents written with index keys (`face:3`, `edge:7`) still resolve.

## What it models

| Feature | State |
|---|---|
| Extrude a profile or a flat face: new body, join, cut, intersect, with taper | yes |
| Revolve, full or partial | yes |
| Sweep along lines and arcs, profile square to the path; round a corner the legs meet sharp or the profile swings round, as asked | yes |
| Sweep with the profile kept facing one way, along any lines and arcs | yes, exact |
| Sweep steered by a guide rail | yes; exact at its stations, close between |
| Loft through sections of lines, arcs and circles, turned against each other or not: ruled, or one smooth skin through any number | yes; a square can become a circle |
| Boolean union, cut, intersect, split body, combine | yes, for any pair of surfaces |
| Holes: simple, counterbore, countersink, drill point | yes |
| Threads, on a face or in a hole, to a depth or all the way, either hand: ISO metric (6g, 6H), Unified inch on its basic profile, and the rounded 30° form | yes, modelled exactly |
| Fillet and chamfer on straight edges between flat faces and cylinders | yes, with mitres, sphere corners and inside corners |
| Fillet and chamfer on circular edges between flat faces, cylinders and cones about one axis | yes, along whole tangent chains |
| Fillet on an edge of any shape that closes on itself, between planes, cylinders, cones, spheres and tori | yes, as the rolling ball's own surface |
| Shell, and moving faces in or out | yes, while the faces keep their neighbours |
| Rib, move, copy, mirror, linear and circular patterns | yes |
| Section analysis, hidden-line drawings | yes |
| Measurement, mass properties with moments of inertia, exact bounds | yes |
| Clearance and interference between bodies | yes |
| STEP export (AP242) and import: planes, cylinders, cones, spheres, tori and splines, rational or not | yes |

Not yet:

- rounds on edges that are neither straight nor circular and do not close
  on themselves, rounds between free-form faces, and rounds whose radius
  varies;
- lofts along a centerline or with guide rails, and lofts between outlines
  with different numbers of segments (a circle against an outline is fine);
- free-form faces with an edge collapsed to a point, or closed both ways
  round; STEP surfaces of revolution, extrusion and offset;
- two free-form skins that cross at a very shallow angle all along (the
  Boolean refuses rather than guess);
- tapers that would need a slanted cone, offsets that change which faces
  are neighbours, shells of free-form faces;
- in a drawing, the outline of a thread's flanks and of free-form faces
  (their edges are drawn);
- the text snapshot of a body with a rolling-ball round can be written but
  not read back.

## Measured

`examples/lab.rs` runs every bundled recipe through the real headless
server once per kernel and compares the solids: body count, volume, bounds,
number and area of planar faces.

```sh
cargo build --release --manifest-path mcp-server/Cargo.toml
cargo build --release -p limo-cad-kernel --example lab
target/release/examples/lab --server mcp-server/target/release/limo-cad-mcp --all
```

All 39 bundled recipes give the same solids on both kernels. Over them the
time spent in the kernel falls from 35.1 s to 2.0 s, 18 times less; per
recipe the factor runs from 7 to 49. (Apple silicon, release builds, kernel
time only.)

| Recipe | Open CASCADE | Limo | Factor |
|---|---:|---:|---:|
| `d-screw-vise` (30 bodies, two 41-turn threads) | 18,034 ms | 886 ms | 20× |
| `vertical-axis-turbine` (27 bodies) | 8,064 ms | 619 ms | 13× |
| `d-screw-vise-fit` | 4,229 ms | 145 ms | 29× |
| `garden-bench` (20 bodies) | 2,519 ms | 209 ms | 12× |
| `show-me-external-thread` | 1,060 ms | 39 ms | 27× |
| `extrude-basics` | 102.7 ms | 10.0 ms | 10× |
| `show-me-shell` | 12.3 ms | 0.35 ms | 35× |
| `show-me-rectangular-pattern` | 14.1 ms | 0.29 ms | 49× |

Two recipes are reported as *match, nearly*: one flat face of one body has
an area of 126.124778 mm² here and 126.130980 mm² on the reference kernel.
That face has a closed form, and it is the first number. The reference
walks along its blends in steps and is a few parts in a hundred thousand
off where one ends on a flat face; the lab remarks on such a face instead
of failing it.

The same threaded screw is 688 faces on the reference kernel and 398 here;
the M8 pin of `show-me-external-thread` is 42 faces there and 9 here.

Beyond the recipes:

- The host's own test suites pass on this kernel as on the reference: the
  84 engine tests (measurement, interference, sections, STEP in and out,
  staged and direct history edits, feature deletion). The deletion tests
  expect different things of the two where they should: the reference
  deletes a finish only at the end of the history, and here the first of
  three finishes goes and the other two keep their edges.
- STEP is checked through the reference kernel. A body is written, read by
  Open CASCADE, written by Open CASCADE and read back, and comes back with
  the same volume and area to seven digits. Free-form solids are weighed
  by Open CASCADE's own integrals straight from the file: a twisted prism,
  a square that becomes a circle, a smooth vase, the vase drilled through,
  its mirror image. Volume and area agree to nine digits or better.
- A thread goes out as splines that follow its screw surfaces to a few
  millionths of a millimetre, and this kernel reads its own file back: a
  solid of spline faces that weighs what the exact thread does to seven
  digits, over seven and a half turns as over one. (The reference reads
  the same file two parts in ten thousand low on the long thread; this is
  the check that the file is right and its sum is not.)
- A round rolled along an ellipse is the ball's own surface to twelve
  digits: its spine the radius from both faces, the curves it touches
  along in them. Open CASCADE, asked to round the same edge of the same
  solid, fits a spline near it and comes within a part in ten thousand.
- Volume, area and centre of mass agree with closed forms to ten digits,
  moments of inertia with the textbook ones for a block, a rod, a ball and
  a ring, and across Booleans of solids in random poses the books balance
  to eight: what two solids share and what they make together weigh as
  much as the two of them. Lofts are held to closed forms as well: a prism
  twisted between two squares to nine digits, a smooth skin through three
  circles or three rectangles to seven.
- `examples/pairs.rs` throws rings, cones, balls, rods and blocks at each
  other in random poses and cuts, intersects and unites every pair. Of the
  first 8,000 pairs 2,181 miss each other and 25 are cuts that leave
  nothing. Of the other 5,794, all three results are closed solids for
  5,790: every edge within a hundred-millionth of a millimetre of both of
  its faces, a clean display mesh, volumes that add up. None is wrong.
  Four are refused rather than guessed at, each a near miss: two pairs of
  cones whose sides all but touch, a ring grazing a cone, a rod running
  along the face of a block a few thousandths of a millimetre inside it.
  Every pair that went wrong on the way to these numbers is now a case in
  `tests/regressions.rs`.

Sample parts on the same machine, modelled from nothing
(`examples/bench.rs`):

| Part | Model | Display mesh | Faces | Edges | Triangles |
|---|---:|---:|---:|---:|---:|
| `mounting-block` | 4.0 ms | 0.7 ms | 35 | 62 | 3,552 |
| `bracket` | 6.4 ms | 0.3 ms | 38 | 83 | 2,952 |
| `rounded-box` | 4.4 ms | 0.3 ms | 26 | 48 | 1,532 |
| `cross-drilled-shaft` | 0.9 ms | 1.0 ms | 7 | 9 | 1,260 |

Every sample part is also modelled at thousands of random dimension sets;
each result must be a closed solid, and the tests check that its display
mesh is a clean skin.

The kernel is about 24,000 lines. The Open CASCADE libraries the desktop
links are about 70 MB.

## Finding out what went wrong

| Variable | Effect |
|---|---|
| `LIMO_CAD_KERNEL_DUMP=<dir>` | Save both operands of every failed Boolean; replay one with `examples/replay.rs` |
| `LIMO_CAD_KERNEL_BODIES=<dir>` | Leave every body of the scene there as text after each recompute |
| `LIMO_CAD_KERNEL_TRACE=1` | Explain splitting, classification and meshing decisions on standard error: which sections were found, where a chain of them has a loose end, which edges a result is left open along |
| `LIMO_CAD_KERNEL_PICTURES=<dir>` | Draw the triangulation of every curved face as an SVG |
| `LIMO_CAD_KERNEL_PROFILE=1` | Print the stage timings of every Boolean |
| `LIMO_CAD_KERNEL_TIMING=<file>` | Append the kernel time of every recompute (set on the host) |

A captured failure becomes a regression test by copying the two `.body`
files into `tests/fixtures`. `lab --verbose` lists, for a body that
differs, the flat faces one kernel has and the other does not.
`pairs 1 N` runs pair `N` of the random pairs again on its own; its number
is all a regression test needs.

## Seeing it in the viewport

An ignored test draws the sample parts with the desktop's own renderer:

```sh
LIMO_CAD_PREVIEW_PROOF_DIR="$PWD/target/limo-kernel-proof" \
  cargo test --release --manifest-path desktop/Cargo.toml --lib limo_kernel_solids -- --ignored
```

## Layout

| Module | Holds |
|---|---|
| `math`, `geom` | Vectors, frames, surfaces and curves |
| `topo`, `io`, `audit` | The body, its checks, its text snapshot, and the audit against its own geometry |
| `make`, `loft` | Extrude, taper, revolve, the helical sweep and primitives; free-form skins, carried profiles and guided sweeps: all built without Booleans |
| `form` | The store of splines and pipes: evaluation, nearest points, knots |
| `ssi`, `meet`, `roots` | Surface–surface intersections in closed form and traced; curve–surface and curve–curve |
| `trim` | Faces in parameter space: containment and interior points |
| `boolean`, `simplify` | Booleans, then merging of faces and edges that became one |
| `name` | Where every face came from, and the names made of it |
| `blend`, `offset`, `thread` | Fillets and chamfers; shells and moved faces; thread grooves |
| `props`, `near` | Volume, area, centre, inertia and bounds; clearance and interference |
| `project` | Hidden-line drawings and plane sections |
| `step` | STEP out and in |
| `tess`, `delaunay` | Watertight display meshes with analytic normals |
| `session` | The recompute plan adapter (feature `dto`) |
| `demo` | Sample parts with their dimension ranges |
