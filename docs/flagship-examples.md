# Flagship parametric examples

These are development targets, not released or fabrication-qualified designs.
The three examples are a printable vertical-axis wind turbine with an integrated
motor used as a generator, a timber garden bench, and a functional screw vise.
The vise adds screw-driven linear motion, load calculations and replaceable wear
surfaces to the rotating turbine and static timber assembly.

The bench is accepted at its current development milestone; it is not finished
or fabrication-qualified. The active example work now moves to the turbine and
vise. The user's 10 September decisions, proposed starting designs, material
implications and calculation assumptions are recorded in
[the engineering brief](flagship-engineering.md). Keep those decisions with the
recipes and distinguish them from untested engineering proposals.

All three now have executable native construction sources in the
[recipe library](../examples/scripts/README.md). The [turbine](vertical-axis-turbine.md)
includes the generator drive, reusable rotor stages, assembly and part drawings;
the [D-screw vise](d-screw-vise.md) includes its retained printed screw, coupled
jaw travel, six drawing sheets, print layout and paired thread coupon. Their
recipe layers include Rust authors, JSONC sources and native lifecycle checks.
They are software-validated manufacturing candidates awaiting physical qualification;
none is a released or load-rated product.

The product purpose is one source that does the work, teaches the process and
shows it: fast execution, controlled step-through inspection and paced rendering
use the same operation sequence. Feature lessons are part of that capability,
not sidecar demos maintained in a different language.

## Order of work

1. Resolve component-context editing with Jack in issue #94, then validate a
   translated and rotated occurrence and its repeated sibling through MCP.
2. Complete associative drafting and manufacturing exports (#93), using the
   same product commands as the editor.
3. Complete multi-document MCP control (#12). Keep explicit document ownership,
   concurrency checks and ordered execution.

All three areas remain in scope. Independent example development can proceed
while the editing decision is pending; do not silently choose its product behavior.

## Release evidence for each example

- Replay from an empty document through MCP using native sketches, driving
  dimensions, features, reusable parts and assembly relationships. No manual
  model-file surgery or imported meshes replacing editable history.
- Recompute after intended dimension changes, verify dependent geometry and
  joints, save/reopen in a fresh process, and edit again. Validate errors and
  geometry rather than only operation success or body counts.
- Produce native editable drawing sheets through MCP: assembly and detail views,
  sections where needed, associative manufacturing dimensions, fits/tolerances,
  materials, quantities/BOM, purchased hardware, assembly sequence and relevant
  print/machining notes. Export the drawing package and validate dimensions after
  an upstream edit. A few projected views and a note are not comprehensive drafting.
- Provide fast replay and a paced live demonstration from the same command plan,
  with revision/build provenance and front/side/isometric review images.
- Iterate before release. Separate automated geometric checks from physical
  fabrication and functional evidence; neither substitutes for the other.

## Printable vertical-axis turbine

This is a functional near-ground science and development experiment with an
integrated low-cost generator for supervised children's electrical demonstrations.
It should teach tolerancing, additive manufacturing, rotating mates, parametric
relations and mathematics. Electrical integration initially concerns the generator
motor; additional controllers, storage and other electrical hardware are outside
the first iteration. Break it into
printable, replaceable parts with constrained rotor motion, shaft retention,
bearing seats, accessible assembly fasteners and a removable generator mount.
Printed parts plus explicitly documented purchased bearings, shaft and hardware
are allowed; do not assume that every wear surface must be printed.

The user's printer is a Bambu Lab X2D. Its main-nozzle envelope is
256 x 256 x 260 mm; the shared dual-nozzle area is 235.5 x 256 x 256 mm.
Aim provisionally for individual parts within 200 mm per axis, retaining margin,
to cover more printers; this is not a universal compatibility guarantee.
Exact filament grade/nozzle profile, qualified fits and loaded output remain
unmeasured. Keep envelope and fit allowances
as named design inputs and verify each oriented part fits with printing margins.
Use fit coupons to select running, locating and bearing-seat allowances for the
actual printer/material before committing a complete print. Distinguish radial
from diametral clearance; document orientation, support and post-processing.

Choose motor mounting and drive ratio from actual shaft dimensions, motor data,
expected rotor speed, starting resistance and the desired electrical load. Check
rotation and clearances throughout motion. Bench-test startup, sustained rotation
and loaded electrical output before claiming useful generation. Leave voltage,
power, outdoor durability and load ratings unclaimed until measured.

[Prusa's design guidance](https://help.prusa3d.com/article/modeling-with-3d-printing-in-mind_164135)
explains why mating parts need allowance and material/process effects matter.
[Maxon's generator guidance](https://support.maxongroup.com/hc/en-us/articles/360004496254-maxon-Motors-as-Generators)
explains motor speed/voltage selection and generator loading; it is not a motor
purchase recommendation.

## Bench and functional vise

The existing garden bench provides native editable stock, repeated parts and
construction checks (see garden-bench.md). Its full drawing package and
component-context editing remain incomplete. Keep construction simple and
accessible; retain material/fastener qualification as explicit open inputs.

The vise should work as a tool and explain force, pressure, motion and durability.
FDM is the primary manufacturing process, with minimal assembly and preferably
no generated supports. Favor useful print-in-place and self-supporting geometry
where it retains good load paths, fits and serviceability. A fixed jaw, guided
moving jaw, retained printed lead screw/handle and replaceable wear surfaces form
the starting architecture; a purchased metal screw is not silently substituted
for the requested printable screw.

The default is the user's selected single-piece D-shaped screw, printed on its
flat side. Keep its remaining section and thread engagement explicit in the
calculations, and provide a circular rotating envelope through the nut and guides.
A flat underside alone does not establish that the complete thread prints unsupported.
Validate a short thread and guide-fit coupon before a full-length screw/frame.

CNC and molded versions should preserve functional dimensions and interfaces,
with process-specific variants for cutter access, draft, shrinkage and undercuts.
Do not represent changing a material label as process qualification. Show the
calculation inputs and uncertainty with the model; verify travel, retention,
clamping force, creep and wear before publishing a working load or service life.

## Smaller capability demonstrations

The [native recipe library](../examples/scripts/README.md) carries the short
fillet lesson, plate, spacer, bracket, repeated assembly and complete bench sources.
Future feature lessons extend that same collection. Run a source with
`cargo xtask run-script FILE --server MCP`; the same Rust interpreter supports
headless comparison and live presentation. The older `cargo xtask test-mcp`
workshop, drawing, live, controls and contracts drivers remain focused test entry
points, not a second recommended authoring format.

The mounting-plate, revolved-spacer, angle-bracket and repeated-bracket fixtures
from #89 now use the native JSONC path. Rust MCP stdio tests retain their analytic,
independent replay, native restore and STEP/STL/3MF checks; the plate, spacer and
bracket profiles are fully located. The shared catalog exposes only executable
recipes. Inventory actual capabilities and
add short replayable demonstrations where missing, including failure recovery and
parameter editing. There is no arbitrary all-tools percentage gate or duplicate
CI matrix. See [the script interface review](script-interface-review.md) for the
current draft's remaining integration work.
