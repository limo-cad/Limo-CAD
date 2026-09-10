# Flagship parametric examples

These are development targets, not released or fabrication-qualified designs.
The three examples are a printable windmill with an optional motor used as a
 generator, a timber garden bench, and a small screw vise. The vise is the
proposed third example: it adds linear motion, screw/shaft fits and replaceable
jaws to the rotating windmill and static timber assembly.

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

## Printable windmill

This is a functional small windmill, not a decorative mesh. Break it into
printable, replaceable parts with constrained rotor motion, shaft retention,
bearing seats, accessible assembly fasteners and a removable generator mount.
Printed parts plus explicitly documented purchased bearings, shaft and hardware
are allowed; do not assume that every wear surface must be printed.

Printer model, usable X/Y/Z envelope, filament, nozzle, intended airflow/location,
rotor size, motor/generator, shaft/bearings and useful electrical load are open
inputs. There is no single standard printer bed. Keep envelope and fit allowances
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

## Bench and vise

The existing garden bench provides native editable stock, repeated parts and
construction checks (see garden-bench.md). Its full drawing package and
component-context editing remain incomplete. Keep construction simple and
accessible; retain material/fastener qualification as explicit open inputs.

The proposed small screw vise should demonstrate a fixed jaw, guided moving jaw,
retained lead screw, handle and replaceable jaw pads. Choose the physical size,
materials and purchased thread hardware before manufacturing release. Verify
travel limits, retention, assembly access and fit; do not claim a clamping load
without physical validation.

## Smaller capability demonstrations

The [native script format](../examples/scripts/README.md) is the authored
construction format. The recipe-library layer above playback carries the short
fillet lesson and complete bench sources, plus future small feature lessons. Run a source with
`cargo xtask run-script FILE --server MCP`; the same Rust interpreter supports
headless comparison and live presentation. The older `cargo xtask test-mcp`
workshop, drawing, live, controls and contracts drivers remain focused test entry
points, not a second recommended authoring format.

Draft PR #89 preserves mounting-plate, revolved-spacer and angle-bracket fixtures
plus analytic and assembly round-trip checks. Its STEP protocol and shared-client
fixes already landed through #91; the remaining fixtures need integration with the
native script path before the draft is ready. Keep those useful checks rather than
introducing another JavaScript modeling runner. Inventory actual capabilities and
add short replayable demonstrations where missing, including failure recovery and
parameter editing. There is no arbitrary all-tools percentage gate or duplicate
CI matrix. See [the script interface review](script-interface-review.md) for the
current draft's remaining integration work.
