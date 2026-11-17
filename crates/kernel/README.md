# Limo kernel

Limo CAD's own boundary-representation kernel: exact solids on analytic
surfaces, written in Rust with no native dependencies. It models the same
recompute plans as the Open CASCADE adapter, runs them an order of magnitude
faster, and compiles to a 555 KB WebAssembly module.

It is opt-in and incomplete. Open CASCADE stays the default kernel; this one
covers the prismatic and turned parts that make up most of the bundled
recipes, and says so plainly when a feature is outside what it can do.

## Try it

```sh
# The kernel on its own: 38 tests, under a second.
cargo test -p limo-cad-kernel

# Time the sample parts, then model each at 2000 random sets of dimensions.
cargo run --release -p limo-cad-kernel --example bench -- --sweep 2000

# A live demo in the browser: one self-contained HTML page.
cargo xtask kernel-demo
```

In Limo CAD itself the kernel is chosen per process:

```sh
LIMO_CAD_KERNEL=limo limo-cad          # desktop
LIMO_CAD_KERNEL=limo limo-cad-mcp      # headless MCP server
```

Both hosts go through `OcctKernel` in `crates/occt`, which hands every
recompute to this crate when the variable is set. Sketching, history, stable
face and edge references, the viewport and scripts are unchanged: they only
see the scene the kernel returns.

## How it is built

The kernel is data-oriented rather than object-oriented.

- **Geometry is tagged plain data.** A surface or curve is a kind, a frame
  and two numbers ([`geom`](src/geom.rs)). Evaluation is a `match`; every
  surface has a closed-form signed distance.
- **Topology is flat arrays.** A [`Body`](src/topo.rs) is a handful of
  vectors indexed by `u32`. Half-edges are implicit: edge `e` owns `2e` and
  `2e + 1`, and the twin of `h` is `h ^ 1`. There are no seam edges; a full
  cylinder is one face with two circular loops.
- **Operations are functions from bodies to bodies.** Nothing is edited in
  place, so the session keeps the result of every history step and replays
  only what changed.
- **Exact where it matters.** Faces lie on planes, cylinders, cones, spheres
  and tori. Edges are lines, circles, ellipses and the closed-form curve in
  which a cylinder meets another cylinder or a sphere. Triangles are derived
  for display and never fed back into modelling.

A Boolean ([`boolean`](src/boolean.rs)) intersects surfaces in closed form,
clips the curves to both faces, merges everything into one arrangement of
vertices and edges, splits each face along it, decides by ray casting which
pieces are inside the other body, and sews the chosen pieces. Coincident
faces are handled by imprinting one boundary on the other.

Fillets and chamfers ([`blend`](src/blend.rs)) are modelled as the solid they
remove or add: a prism along a straight edge, a solid of revolution around a
circular one, a sphere piece where three rounds meet. A picked edge is
followed along its tangent chain, as a rolling ball would.

## What it models

| Feature | State |
|---|---|
| Extrude a profile or a flat face: new body, join, cut, intersect | yes, without taper |
| Revolve, full or partial | yes |
| Boolean union, cut, intersect, split body, combine | yes |
| Holes: simple, counterbore, countersink, drill point | yes; modelled threads no |
| Fillet and chamfer on straight edges between flat faces | yes, with mitres and sphere corners |
| Fillet and chamfer on circular edges between a flat face and a cylinder | yes, along whole tangent chains |
| Rib, move, copy, mirror, linear and circular patterns | yes |
| Blends on other edges, shell, sweep, loft, taper, modelled threads, STEP import | not yet |
| Section analysis, drawings, STEP export, interference | still Open CASCADE only |

Surface pairs whose intersection has no closed form here (a plane cutting a
torus at an angle, a cone cut in a parabola or hyperbola, a torus against a
surface on another axis) are refused with a message that names the pair.
A fillet around a rounded corner must be smaller than that corner, and an
angled chamfer stays on the picked edges instead of following their chain.

## Measured

`examples/lab.rs` runs every bundled recipe through the real headless server
once per kernel and compares the solids: body count, volume, bounds, number
and area of planar faces.

```sh
cargo build --release --manifest-path mcp-server/Cargo.toml
cargo build --release -p limo-cad-kernel --example lab
target/release/examples/lab --server mcp-server/target/release/limo-cad-mcp --all
```

Of the 39 bundled recipes, 29 give the same solids on both kernels, none
differ, and 10 stop at something this kernel does not have yet: modelled
threads, loft, sweep, shell, a section or drawing service, or one of the
surface pairs above. Over the matching recipes the time spent in the kernel
falls from 284 ms to 16.5 ms, 17 times less; per recipe the factor runs from 8
to 80. (Apple silicon, release builds, kernel time only.)

| Recipe | Open CASCADE | Limo | Factor |
|---|---:|---:|---:|
| `extrude-basics` | 110.77 ms | 8.66 ms | 13× |
| `fillet-basics` | 14.49 ms | 1.80 ms | 8× |
| `mounting-plate` | 13.75 ms | 1.09 ms | 13× |
| `show-me-hole` | 10.88 ms | 0.65 ms | 17× |
| `show-me-revolve` | 2.53 ms | 0.22 ms | 12× |
| `repeated-bracket-assembly` | 28.99 ms | 1.30 ms | 22× |
| `show-me-rectangular-pattern` | 14.70 ms | 0.18 ms | 80× |

Sample parts on the same machine, modelled from nothing (`examples/bench.rs`):

| Part | Model | Display mesh | Faces | Edges | Triangles |
|---|---:|---:|---:|---:|---:|
| `mounting-block` | 4.26 ms | 0.59 ms | 35 | 62 | 3,056 |
| `bracket` | 5.73 ms | 0.26 ms | 38 | 83 | 2,480 |
| `rounded-box` | 3.43 ms | 0.10 ms | 26 | 48 | 1,452 |
| `cross-drilled-shaft` | 0.80 ms | 0.50 ms | 7 | 9 | 944 |

The bracket's edge break is one fillet over 22 edges, ten of them torus
segments. Its volume agrees with the closed form to 0.002 mm³, and its 38
faces are as many as Open CASCADE builds for the same recipe. Every sample
part is also modelled at thousands of random dimension sets; each result
must be a closed solid, and the tests check that its display mesh is a clean
skin.

The kernel is about 9,600 lines. As WebAssembly it is 555 KB (189 KB
compressed), with no imports; the Open CASCADE libraries the desktop links
are about 70 MB.

## Finding out what went wrong

| Variable | Effect |
|---|---|
| `LIMO_CAD_KERNEL_DUMP=<dir>` | Save both operands of every failed Boolean; replay one with `examples/replay.rs` |
| `LIMO_CAD_KERNEL_TRACE=1` | Explain splitting, classification and meshing decisions on standard error |
| `LIMO_CAD_KERNEL_PROFILE=1` | Print the stage timings of every Boolean |
| `LIMO_CAD_KERNEL_TIMING=<file>` | Append the kernel time of every recompute (set on the host) |

A captured failure becomes a regression test by copying the two `.body` files
into `tests/fixtures`.

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
| `make` | Extrude, revolve and primitives, built without Booleans |
| `ssi`, `roots` | Surface–surface, curve–surface and curve–curve intersections |
| `trim` | Faces in parameter space: containment and interior points |
| `boolean`, `simplify` | Booleans, then merging of faces and edges that became one |
| `blend` | Fillets and chamfers |
| `tess` | Watertight display meshes with analytic normals |
| `session` | The recompute plan adapter (feature `dto`) |
| `demo` | Sample parts with their dimension ranges |
