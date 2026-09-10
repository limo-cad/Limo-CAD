# Editable crown garden bench

This original native design demonstrates editable timber parts and an assembly:
1200 mm seat width, 450 mm seat height, five seat slats, crowned back pickets and
supported armrests. It is not an imported FreeCAD model. The smaller `bench` suite
remains the rectangular-stock regression fixture.

```sh
cargo xtask test-mcp garden-bench --server /absolute/path/to/nbcad-mcp --out /absolute/path/to/results
```

Build visibly in a new native window and save an editable project:

```sh
cargo xtask test-mcp garden-bench --server /absolute/path/to/nbcad-mcp --desktop /absolute/path/to/nbcad --save /absolute/path/to/buildable-garden-bench.nbcad --out /absolute/path/to/results
```

Use matching engine versions with the slot center-constraint and capsule-tangency
fixes in this PR. This revision changes the recipe, not the desktop binary. It uses
the shared product groups through `cad_interface`; no UI selectors, model-file
surgery, STEP or STL are used to construct the model. Existing documents are preserved.

## Construction intent

Front legs continue up to support the arms. Rear legs continue into the back posts.
Square-ended aprons and side rails overlap post faces and have accessible fastening
from outside the frame. Upper and lower side rails are different definitions because
their drilling differs. Left and right posts are separate machined parts.

The seat bears on both side rails and a center bearer. Support blocks carry that
bearer at the aprons. Three fixing positions per slat reduce the unsupported span.
Seat gaps are 5 mm; square post notches provide 1 mm nominal clearance per side.
The first two slats clear the taller front posts and the last clears the rear posts.
These clearances assume accurately machined, conditioned stock and must be reviewed
for the actual timber and exposure.

Back pickets sit on the seating side of the two back rails, which are fastened to
the rear faces of the posts. Picket screws enter from behind the rails. Armrests
bear on the front post tops and four square bearing cleats; they attach from below
through the cleats into the arm boards. No shaped upright or unsupported arm-end
butt joint is required.

## Machining and assembly order

1. Cut stock to the component envelopes and drill the documented patterns. Machine
   the square post notches. The back-picket crown and slot are optional decoration
   for fabrication, but retained here to exercise editable slot/crown features.
2. Clamp the frame square. Fit the lower side rails and lower stretcher first;
   upper rails would obstruct driver access. Then fit the lapped aprons, upper
   side rails, center support blocks and center bearer.
3. Fit the seat slats with spacers. Seat clearance holes have 10 mm, 90-degree
   countersinks for flush heads. Install the seat before the armrests.
4. Fit the back rails to the posts and fasten the pickets from behind.
5. Fit the arm cleats and secure the arms from underneath. Ease exposed edges,
   including square notches and posts, without removing joint bearing areas.

`report.json` includes the component/sketch/feature map, quantities, finished
component envelopes and a fastener schedule with head coordinates, direction,
length, connected occurrences and installation stage. Native hole features model
5.5 mm clearance bores and 3.5 mm pilot bores with a 2 mm pilot-depth allowance.
Nominal screws are 5 mm; lengths vary with joint thickness. The recipe checks
side-grain entry and retained tip clearance. Fastener solids/threads are not modeled.
Pilot sizes and head envelopes are example assumptions: select actual exterior
fasteners and timber, then reconcile the manufacturer's drilling and edge-distance
requirements. This example has no certified load rating.

## Editable history and evidence

Each stock part begins with a dimensioned native sketch and extrusion. The picket
adds crown/edge fillets, a fully constrained slot and a native through cut. Machined
components are assembled with face-referenced rigid mate frames measured from the
grounded front leg. These are explicit placement dimensions, not a global bench
resizing parameter or a simulation of screw-joint flexibility.

The recipe checks real material removal for notches, slots and drilling, assembly
solutions and poses, seat/arm bearing and front-of-rail picket placement from the
solved mesh envelopes. It rejects crossing screw shafts and checks a conservative
20 mm square by 120 mm driver envelope at each installation stage. These checks
are scoped to axis-aligned stock; they are not a general exact interference test.

It changes slot width 18 to 20 mm and picket height 315 to 340 mm, checks dependent
features and repeated occurrences, restores history in a fresh MCP process and
edits the height again. The saved native file is reopened and its slot edited
20 to 22 to 20 mm. The delivered pickets remain 340 mm tall. Recompute errors,
no-op cuts and broken relationships fail the run. The same example runs in the
existing MCP CI jobs, without a new all-tools coverage gate.

See [design principles](parametric-design-principles.md) for research sources and
the modeling practices behind these checks.

## Mesh export

Native desktop and MCP 3MF/STL export use visible solved occurrences, so this
bench exports 36 positioned meshes rather than 19 overlapping part definitions.
Body selection exports the visible instances of those source bodies. Source
meshes are welded before placement; manifold validation remains enabled for 3MF.
Material lookup retains the original body identity. Unsolved assemblies reject
export instead of silently emitting an incorrect layout.

The bench remains full size in millimetres. Scaling or arranging it for a printer
is separate from assembly export; no automatic bed scaling is applied. Keep the
native .nbcad project for editing; 3MF is a secondary manufacturing output.
