# D-screw vise manufacturing candidate

The committed source is `examples/scripts/d-screw-vise.nbcad.jsonc`. Run it with
`cargo xtask run-script --server PATH_TO_NBCAD_MCP --recipe d-screw-vise`, or choose the recipe in Scripts
and use **Run in new design**. `d-screw-vise-fit` is the smaller paired thread
coupon. Both use the ordinary native MCP operations and retain sketches,
driving dimensions, datum planes, features and assembly joints. The Rust
`author_vise` example writes these readable command files; it does not construct
geometry outside the MCP interpreter.

This is a light assembly fixture to qualify by printing, not a rated shop vise.
The teaching target is ages 8–12 with adult guidance; a five-year-old can observe
or help with an adult controlling assembly and motion. Keep the small purchased
hardware under adult control. It is not a climbing support or a load-bearing toy.
Its main screw is one continuous printed part with an integral paddle, a true
M20 × 2.5 right-hand helical thread and a flat through its axis. It is not a
stack of rings, a D-shaped female bore or an imported mesh.

## Assembly and load path

Five parts are printed: the slotted frame, guided jaw, replaceable threaded nut,
D screw and keeper. A purchased M3 × 40 socket screw and M3 hex nut positively
retain the keeper; an M3 × 25 socket screw and second M3 nut capture the wear
cartridge. Their native bodies are explicitly simplified clearance
envelopes; the print plate excludes them. Supplier dimensions must be checked
before choosing hardware: the modeled head is Ø5.5 × 3 and the nut is 5.5 across
flats × 2.4 thick, consistent with the [Bossard nut drawing](https://media.distrelec.com/Web/Downloads/_t/ds/1241613_eng_tds.pdf).

1. Seat the large threaded cartridge in the frame. Load its M3 nut into the
   hex pocket from the rear (X8), then insert the M3 × 25 bolt from the front
   (head seated at X32). Its axis is Y12/Z42, clear of the drive thread. Fit this
   fastener before the drive and jaw, while both ends are accessible. The
   cartridge's rectangular outside prevents rotation and its end shoulders
   transfer axial force into the frame.
2. Turn the D screw through the circular female thread. The screw's phase is
   registered to the actual native helix, including its cutter start allowance.
3. Load the small M3 nut from the underside of the jaw and hold it in its
   hexagonal recess. Lower the jaw onto the rails 30 mm toward the fixed jaw
   from its home position, clear of the screw head, then slide it left over
   the head through its open rear chamber. It cannot drop directly over the
   head through the closed chamber floor.
4. Lower the keeper over the neck. Its U throat opens downward; the reverse
   orientation cannot be installed through the top opening.
5. Fit the M3 retainer from above. Its recessed seat is at Z49; the 40 mm shaft
   ends at Z9, above the frame's Z8 top face. The nut seats against the pocket
   roof at Z11.8. Tighten only enough to retain the cap; this fastener does not
   supply the vise's clamping force.

Closing thrust passes from screw head to the jaw's front shoulder and the
workpiece, then through the fixed jaw, base, housing and large nut back to the
screw. The keeper carries opening force and the M3 fastener prevents the keeper
from lifting. The screw's rotating envelope is circular despite its printed D
section. The handle is kept ahead of the base throughout the full 48 mm travel:
its forward edge starts at X−49 and ends at X−1. Checking only one finished
orientation would miss a handle sweeping through the base.

The assembly has a Screw–Revolute–Slider loop. Only the screw angle is driven;
the solver determines jaw travel and retention rotation. The nominal unloaded
geometry uses 0.4 mm guide-side, guide-roof and axial gaps. The jaw bottom and
frame top share the Z8 support datum in intended sliding contact, so gravity
does not have to lower a suspended jaw and consume its neck clearance.
Physical backlash and elastic seating
under load are not simulated by this ideal kinematic joint.

The cartridge cross-bolt positively arrests lifting throughout a complete
drive turn. Retention does not depend on gravity, friction or the orientation
of the interrupted D-thread. A Ø3 shaft in the housing and cartridge's Ø3.4
holes permits at most 0.4 mm relative radial float in rigid geometry. Native
tests check the free gap at 0.38 mm, collision at 0.42 mm, and housing contact
before ±1° rocking about the bolt axis. These are geometric limits, not a
prediction of bolt bending or printed housing deformation under load. Remove
the jaw and drive before undoing this bolt to replace the wear cartridge.

Mount the frame on a sacrificial board using four M5 through-bolts, washers and
nuts through the 18 × 6 mm slots; select bolt length for the board thickness.
The slots are outside the jaw path and remain accessible from above. Snug the
washers without crushing the printed base. This mounting hardware is installation
equipment and is not part of the nine modeled component envelopes.

## Editing the design

The rectangles have real width and height driving dimensions and a single
located corner. Circles have a driving diameter and a located center. They are
not individually fixed collections of lines. Named datum planes locate the
sketches; stock length is an editable extrusion distance. The hexagon's first
edge length drives the other edge lengths through `d1` expressions, with
explicit coincident endpoints closing the contour.

For example, reopen **Moving jaw / 60 mm gripping face** and change its 60 mm
dimension to change jaw width. **Jaw guide left / running clearance** has a
4.8 mm channel dimension around a 4 mm rail; changing it changes the clearance
cut. Finish the sketch and recompute the solids. The native regression test
edits both dimensions and restores them, checking the intended volume changes.

The feature **male_thread** uses `solid_external_thread`; its persisted feature
can be edited with `solid_edit_external_thread`. Thread pitch must be changed
in both mating thread features and in the Screw joint's lead. Cartridge width
and its housing pocket are separate manufacturing dimensions. This example
does not pretend that every source constant is already a single global master
parameter: coordinated fit changes must update the mating features together.

## Thread fit and printing

The male is modeled at the ISO M20 × 2.5 / 6g maximum-material envelope. The
female uses a **custom 20.5 × 2.5 ISO-derived 60° profile**. Increasing its nominal
diameter supplies 0.25 mm radial process relief while retaining pitch and flank
form; it is not a standard M20 6H nut. Native tests evaluate the resulting
pitch-diameter clearance across the male tolerance envelope and sample exact
solid interference during coupled motion. The closest unloaded surface
clearance is not the same quantity as radial pitch-diameter relief.

Start with the coupon, using the same material and intended print orientations
as the full parts. Record nozzle, layer height, extrusion width, wall count,
infill, material conditioning, dimensional results, turning torque and wear.
An easy fit with severe rocking is not a successful fit. Do not increase a
slicer compensation blindly to conceal an incorrect mating helix.

The full five-part plate fits within approximately 204 × 166 × 52 mm, inside
the conservative 235.5 × 256 × 256 mm target. The D screw rests on its flat;
the nut's thread axis is vertical; the jaw and keeper rest on their broad end
faces. The frame has two 45° roof faces above its circular screw envelope,
replacing the large horizontal bore bridge. Its apex is Z42.991, leaving about 9 mm
of stock below the housing top. The small cartridge cross-hole and hex pocket
still need approximately 3.4 mm short bridges qualified with the print profile.
Try this layout without supports and inspect it
before applying load; process success still depends on the extrusion profile.
The standard 3MF retains
separate manifold bodies and material assignments. It is an export of the
print poses, not the assembled model scaled as one object.

PETG is the provisional indoor material. The coupon and initial build use the
existing Bambu PETG HF presets. A material tag or thicker dimensions do not
prove resistance to creep, layer separation, thread wear or nut pullout. The
[Prusa design guide](https://help.prusa3d.com/article/modeling-with-3d-printing-in-mind_164135)
explains why overhangs, orientation and fit must be designed into the geometry;
the [material guide](https://help.prusa3d.com/article/petg_2059) is useful for the
initial process choice. Manufacturer test specimens are not a load rating for
this interrupted printed screw.

PLA is appropriate for a fit and teaching prototype; changing to PETG for the
functional candidate requires repeating the fit and wear checks. ASA is a
separate outdoor or warmer-environment variant whose shrinkage, fits, orientation
and layer adhesion need qualification. Different colors or a material preset
do not substitute for a process-specific geometry check.

## Visible calculation assumptions

For an illustrative 0.25 N·m input torque, 20% overall screw efficiency and
2.5 mm lead, `F = 2π η T / lead` gives approximately **126 N**. A 600 mm² contact
patch gives a nominal average pressure of **0.21 MPa**. These are sensitivity
calculations, not an allowable clamp load: efficiency, actual contact area,
layer strength, stress concentrations and long-term creep are unqualified.
The half-round screw has an eccentric section; using a solid circular-shaft
torsion formula would overstate its capability.

A CNC counterpart should preserve jaw travel, mating axes and assembly order,
while changing inaccessible pockets, cutter radii, screw material and nut
details. A molded family needs draft, more uniform wall sections, shrinkage
allowances and parting/tool access. The same function does not imply identical
geometry across FDM, machining and molding.

## Validation and remaining qualification

The native MCP recipe tests cover independent deterministic replay, editable
dimensions, thread edit/restore, native save/reload, closed and positive exported
meshes, print-bed bounds and part separation, analytic stock/coupon volumes,
joint travel and atomic rejection beyond travel, and exact interference at
sampled driven positions. These checks run against actual OCCT geometry.

Ordinary tests create and remove their own temporary files. To retain CAD,
3MF, drawings and failure diagnostics, set `NBCAD_RECIPE_ARTIFACT_DIR` to an
output directory before running
`cargo test --manifest-path mcp-server/Cargo.toml --test recipes d_screw_vise`.
The harness never deletes that explicitly selected directory.

The committed [native validation snapshot](manufacturing/d-screw-vise.validation.json)
records the reachable source revision, normalized source hashes and actual
artifact hashes. Its headless baseline is separate from attached presentation
qualification and from physical fabrication.

Before promoting this candidate, print the coupon and the complete set, verify
assembly access and hardware fit, measure play and turning effort, apply a
controlled light load, then inspect deformation and wear after repeated use
and a sustained hold. No physical validation, durability rating or safety
factor is asserted by the current CAD result.
