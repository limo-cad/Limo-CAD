# Crown garden bench: referenced joinery candidate

This original timber design is an editable native example, not a FreeCAD import.
It is a candidate for the flagship set, not a released or load-rated furniture plan.
The smaller `bench` suite remains the rectangular-stock regression fixture.

The construction source is
[`examples/scripts/garden-bench.nbcad.jsonc`](../examples/scripts/garden-bench.nbcad.jsonc).
It is a commented, versioned sequence of the shared interface commands, interpreted
by Rust. It contains native modeling operations, named result references, camera
instructions and authored chapter notes. No JavaScript or imported geometry builds
this example.

```sh
cargo xtask run-script examples/scripts/garden-bench.nbcad.jsonc --server /absolute/path/to/nbcad-mcp --repeat 2 --out /absolute/path/to/results
```

This runs at maximum rate in independent headless processes and compares the final
model, sketches, solved assembly and geometry. Operation errors stop the script at
once; expensive geometry and manufacturing verification runs at the end. References
come from completed operation results, so repeated inspection calls are unnecessary.

To replay in an existing window, preserve the current document and use its session.
`--new` creates the blank design tab in that same window:

```sh
cargo xtask run-script examples/scripts/garden-bench.nbcad.jsonc --server /absolute/path/to/nbcad-mcp --session UUID --new --present --speed 2 --save /absolute/path/to/referenced-garden-bench.nbcad
```

Use matching desktop and MCP builds. The replay refuses a nonempty document. The
presentation adds captions, planned camera views and playback controls to the same
construction sequence. Focused views show the initial stock, notches, crown, datum
slot and arm noses; wider views explain how the members fit together. See
[`native-scripts.md`](native-scripts.md) for the format and runner contract.

## Design and assembly intent

The seat is 1200 mm wide and 450 mm high. Five slats rest on the side rails and a
center bearer. Continuous front posts support the arms; rear posts support the back.
The frame uses square-cut, face-lapped members with outside-accessible fastening.
The center bearer rests on two small, purposeful support blocks at the aprons.

Two continuous arm rails replace the four short arm blocks. They connect the front
and rear posts, support the arm boards along their length, and repeat the side-frame
construction. Armrests are mirrored about the seat center, with an 18 mm plan radius
at their noses and 3 mm eased edges. Their rear ends stop 3 mm before the rear posts.
Left and right armrests are separate machined definitions because their fixing holes
are handed; the continuous rails are interchangeable. No duplicate unused hole pattern
is added just to reuse a component definition.

Crowned, slotted back pickets sit on the seating side of the back rails. The honey
colored seat, arms and pickets against the deep green frame retain the garden-bench
character. Finish colors describe appearance only, not an assigned timber species.
The straight back and level seat have not been validated with a physical comfort mockup.

Assembly mates follow an actual connected member/fastener graph. Each child is mated
to a member it touches, using the finished mating faces and the joint's physical
location. One front leg is grounded. The recipe no longer hangs every member from
arbitrary offsets on that leg. Rigid mates represent the assembled design, not screw
flexibility, clamping deformation, or a structural analysis.

## Machining references and editable history

Every stock profile is dimensioned and fixed at one locating vertex, leaving its size
editable. Every notch is similarly located. Zero degrees of freedom is asserted; the
previous rectangles had two translational degrees of freedom despite having dimensions.
Sketches are named for their owning part and machining operation.

The machining datum convention is A: broad stock face, B: adjacent long-edge face,
C: square end. These meet at local XYZ zero. The run report’s
`exports.verification_inputs` records the corresponding normal and grain axes, stock
sizes, quantities, body IDs and extrusion feature IDs. Named sketches and component
definitions remain in `exports.final_model`. The CAD stock-profile plane need not be the primary
workholding face: a leg can naturally be extruded from its end profile.

The picket slot is sketched on a named mid-thickness construction plane between its
broad faces, then cut symmetrically beyond both faces. Its width and length remain
driving dimensions. Its height is located from the part's bottom datum, so extending
the crown changes the top margin without moving the decorative field. The stock,
plane, slot and cut remain native history; the datum is used by the feature rather
than added as decoration.

Part sketches live in component-definition coordinates. Repeated assembly occurrences
are transforms of those definitions. Sketches appearing back at the origin when edited
in the assembled view expose the component-context editing gap in issue #94; they do
not mean that their stock profiles should be scattered into assembly coordinates.
This candidate must not be presented as a finished in-place editing reference until
that workflow is resolved. The bench also is not one globally resizable master model:
recipe dimensions, feature parameters and assembly coordinates have distinct roles.

## Fit and fabrication sequence

Post notches provide 2 mm nominal clearance per side, with square internal corners
finished by sawing/chiseling or an equivalent process. Under the example's ±0.5 mm
finished-size and ±0.5 mm relative-location budgets, the lateral notch stack retains
1 mm minimum clearance. Arm-to-rear-post clearance is 3 mm nominal and 1.5 mm minimum
under the stated conservative stack. Seat gaps remain 5 mm. These are declared
machining/assembly assumptions, not a moisture-movement allowance or formal GD&T
certification. Choose timber, conditioning, finish and exposure before approving them.

1. Prepare stock from its marked face, edge and end; cut to size and machine notches,
   crowns, slots and edge radii. Drill clearance holes and countersinks. Keep bearing
   areas flat. Dry-fit and clamp each connection before transferring pilot centers.
2. Fit the lower frame and stretcher first, then the aprons, upper side rails, center
   support blocks and bearer. Check frame squareness before tightening.
3. Fit seat slats with spacers. Fix along one centerline across each narrow slat so
   the board can move across its width into the gaps.
4. Fit the back rails and pickets; the picket fasteners enter from behind the rails.
5. Fit the continuous arm rails from outside, then the handed arms from above with
   flush countersunk heads. The screw row lies over each support rail; the arm can
   expand across its width away from that row. Preserve the rear clearance.

The nominal fasteners are 5 mm screws, with 5.5 mm clearance bores and 3.5 mm pilots.
Seat and arm heads use a 10 mm, 90-degree countersink envelope. Pilot bores are
transferred from the clamped mating parts, with a depth stop; do not independently
locate both sides of a screw connection to ±0.5 mm and assume they will align.
Select actual exterior fasteners and reconcile their head geometry, pilot size,
engagement and edge-distance requirements with the chosen timber before fabrication.
The model includes nominal bores, not screw solids or threads.

## Evidence and release limits

The final gate checks retained bore axes and diameters, fully constrained stock,
notch and slot profiles, mirrored arm geometry, rear clearance, bearing contacts,
side-grain screw entry, tip clearance, separated screw shafts, and a conservative
20 mm by 120 mm driver-access envelope in
assembly order. Exact retained OCCT solids at solved occurrence poses must have zero
volumetric interference. A separate regression distinguishes overlap from touching and
positive clearance; broad-phase envelopes alone are not used to declare an overlap.

The authored sequence finishes by widening the slot from 18 to 20 mm and extending
the picket from 315 to 340 mm. This demonstrates a deliberate design refinement and
updates all nine instances. Temporary test edits are not interleaved with the visible
construction. The final gate checks the retained feature history, solved placements,
all fully located profiles and exact solid interference. Independent replay compares
persisted model and geometry data, excluding tool-disclosure hints and transient
sketch editing caches; it does not infer determinism merely from a passing
interference check. This comparison tests the given script and binary/kernel build.
It does not establish identical tessellation across future kernel versions or platforms.

The run report includes the final native model and semantic verification inputs.
Those inputs retain stock sizes, datum conventions, expected occurrence poses, bores
and the 87 nominal fastener connections. They support the final manufacturing checks
without inserting inspection round trips throughout the construction. These checks
establish selected geometric and editing properties; they do not certify comfort,
loads, timber movement, or every possible parameter change.

Manufacturing release still requires timber/hardware selection, a comfort and assembly
mockup, the component-context editing workflow, and a comprehensive reviewed drawing
package. See `parametric-design-principles.md` and `flagship-examples.md`.

Native 3MF/STL export contains the visible solved occurrences at their assembled
positions, in millimetres. It is a secondary output. Keep `.nbcad` for parameter edits;
printer scaling and bed arrangement are separate operations.
