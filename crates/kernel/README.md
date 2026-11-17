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
# The kernel on its own: about a hundred tests, a few seconds.
cargo test -p limo-cad-kernel

# Time the sample parts, then model each at 2000 random sets of dimensions.
cargo run --release -p limo-cad-kernel --example bench -- --sweep 2000

# 600 pairs of solids in random poses: unite, cut, intersect, check the books.
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
  are seven surfaces: plane, cylinder, cone, sphere, torus, and the two
  that wind along an axis, the helicoid of a thread's flank and the coil
  of its rounded corner.
- **Topology is flat arrays.** A [`Body`](src/topo.rs) is a handful of
  vectors indexed by `u32`. Half-edges are implicit: edge `e` owns `2e` and
  `2e + 1`, and the twin of `h` is `h ^ 1`. There are no seam edges: a full
  cylinder is one face with two circular loops, and the flank of a thread
  is one face however many turns it makes.
- **Operations are functions from bodies to bodies.** Nothing is edited in
  place, so the session keeps the result of every history step and replays
  only what changed.
- **Exact where it matters.** Edges are lines, circles, ellipses, helices,
  the closed-form curve in which a cylinder meets another quadric, and for
  everything else the curve in which two surfaces meet, written on one of
  them and solved to rounding at every point. Triangles are derived for
  display and never fed back into modelling.

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
surface of whatever face ends its edge.

Volume, area and centre of mass ([`props`](src/props.rs)) are integrals
along the exact edges of every face: Green's theorem turns the sum over a
face into a line integral round its loops. No face is cut into triangles,
and the answers are good to rounding.

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
- deleting a finish replays the one after it by the names it already has;
- where a round rim starts is a vertex of the model, and stays one when a
  later feature cuts the rim somewhere else, so a dimension tied to it
  still finds it.

Documents written with index keys (`face:3`, `edge:7`) still resolve.

## What it models

| Feature | State |
|---|---|
| Extrude a profile or a flat face: new body, join, cut, intersect, with taper | yes |
| Revolve, full or partial | yes |
| Sweep along lines and arcs that run on smoothly, profile square to the path | yes |
| Loft through straight-sided or circular sections, ruled | yes |
| Boolean union, cut, intersect, split body, combine | yes, for any pair of surfaces |
| Holes: simple, counterbore, countersink, drill point | yes |
| Threads, on a face or in a hole, to a depth or all the way, either hand: ISO metric (6g, 6H), Unified inch on its basic profile, and the rounded 30° form | yes, modelled exactly |
| Fillet and chamfer on straight edges between flat faces and cylinders | yes, with mitres, sphere corners and inside corners |
| Fillet and chamfer on circular edges between flat faces, cylinders and cones about one axis | yes, along whole tangent chains |
| Shell, and moving faces in or out | yes, while the faces keep their neighbours |
| Rib, move, copy, mirror, linear and circular patterns | yes |
| Section analysis, hidden-line drawings | yes |
| Measurement, mass properties, exact bounds | yes |
| Clearance and interference between bodies | yes |
| STEP export (AP242), and import of files made of planes, cylinders, cones, spheres and tori | yes |

Not yet: lofts that blend smoothly through more than two sections; sweeps
with guide rails, sharp corners or a fixed orientation; tapers that would
need a slanted cone; offsets that change which faces are neighbours; blends
on edges that are neither straight nor circular, and with varying radius;
threads with flanks square to the axis; reading free-form surfaces from
STEP. A thread shows in a drawing by the helices of its crest and root, not
by the outline of its flanks, and a plane through the axis of a thread
(a hole tapped exactly on the corner of a plate) is beyond the tracer. A
face that takes in a whole ring but for one hole that reaches all the way
round it both ways is modelled right and drawn with gaps.

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
time spent in the kernel falls from 36.1 s to 1.9 s, 19 times less; per
recipe the factor runs from 7 to 49. (Apple silicon, release builds, kernel
time only.)

| Recipe | Open CASCADE | Limo | Factor |
|---|---:|---:|---:|
| `d-screw-vise` (30 bodies, two 41-turn threads) | 18,100 ms | 826 ms | 22× |
| `vertical-axis-turbine` (27 bodies) | 8,837 ms | 623 ms | 14× |
| `d-screw-vise-fit` | 4,241 ms | 129 ms | 33× |
| `garden-bench` (20 bodies) | 2,602 ms | 204 ms | 13× |
| `show-me-external-thread` | 1,131 ms | 37 ms | 31× |
| `extrude-basics` | 113.5 ms | 10.2 ms | 11× |
| `show-me-shell` | 13.1 ms | 0.38 ms | 34× |
| `show-me-rectangular-pattern` | 14.7 ms | 0.30 ms | 49× |

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
  62 engine tests (measurement, interference, sections, STEP in and out),
  the staged and direct history edits, and the 12 feature-deletion tests.
  One of those expects different things of the two: the reference will not
  delete next to bodies that share its geometry, and here bodies share
  nothing, so the delete goes through.
- STEP is checked through the reference kernel: a body is written, read
  by Open CASCADE, written by Open CASCADE and read back, and comes back
  with the same volume and area to seven digits.
- Volume, area and centre of mass agree with closed forms to ten digits,
  and across Booleans of solids in random poses the books balance to
  eight: what two solids share and what they make together weigh as much
  as the two of them.
- `examples/pairs.rs` throws rings, cones, balls, rods and blocks at each
  other in random poses and cuts, intersects and unites every pair. Of the
  first 4,000 pairs 1,119 miss each other and 15 are cuts that leave
  nothing. Of the other 2,866, all three results are closed solids with
  clean display meshes and volumes that add up for 2,862. Two are refused
  rather than guessed at, both pairs of cones: one has a base that passes
  close by the other's tip, the other come too close to touching for the
  curve between them to be followed. Two are right as solids and flawed as
  drawn: the ring face under "not yet" above, and a ball whose mesh, made
  to 0.004 mm, has one triangle 0.011 mm off its sphere where 0.010 is
  allowed. Every pair that went wrong on the way to these numbers is now a
  case in `tests/regressions.rs`.

Sample parts on the same machine, modelled from nothing
(`examples/bench.rs`):

| Part | Model | Display mesh | Faces | Edges | Triangles |
|---|---:|---:|---:|---:|---:|
| `mounting-block` | 3.9 ms | 0.7 ms | 35 | 62 | 3,552 |
| `bracket` | 6.3 ms | 0.3 ms | 38 | 83 | 2,952 |
| `rounded-box` | 4.1 ms | 0.3 ms | 26 | 48 | 1,532 |
| `cross-drilled-shaft` | 0.9 ms | 0.9 ms | 7 | 9 | 1,260 |

Every sample part is also modelled at thousands of random dimension sets;
each result must be a closed solid, and the tests check that its display
mesh is a clean skin.

The kernel is about 19,000 lines. The Open CASCADE libraries the desktop
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
| `topo`, `io` | The body, its checks and its text snapshot |
| `make` | Extrude, taper, revolve, loft, the helical sweep and primitives, built without Booleans |
| `ssi`, `meet`, `roots` | Surface–surface intersections in closed form and traced; curve–surface and curve–curve |
| `trim` | Faces in parameter space: containment and interior points |
| `boolean`, `simplify` | Booleans, then merging of faces and edges that became one |
| `name` | Where every face came from, and the names made of it |
| `blend`, `offset`, `thread` | Fillets and chamfers; shells and moved faces; thread grooves |
| `props`, `near` | Volume, area, centre and bounds; clearance and interference |
| `project` | Hidden-line drawings and plane sections |
| `step` | STEP out and in |
| `tess`, `delaunay` | Watertight display meshes with analytic normals |
| `session` | The recompute plan adapter (feature `dto`) |
| `demo` | Sample parts with their dimension ranges |
