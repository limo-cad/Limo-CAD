# Vertical-axis turbine

The `vertical-axis-turbine` recipe builds the actual editable model, places its
components with native joints, adds the 72:18 gear relationship, and creates its
drawing package. The same source builds the model in fast mode or teaches the
construction in paced/step mode. There is no second animation model or imported
STL/STEP geometry.

Run `cargo xtask run-script --recipe vertical-axis-turbine --repeat 2 --out <directory>`.
The app's Scripts catalog exposes the same source. Through MCP, use
`cad_interface` with `action: "script"`, `recipe: "vertical-axis-turbine"`,
`mode: "fast"` (or `paced`/`step`), and `validate: true` on a blank document.
Do not replay into an unrelated working document.

The reviewed JSONC lives in `examples/scripts/vertical-axis-turbine.nbcad.jsonc`.
`cargo run -p nbcad-recipes --example author_turbine` regenerates it from the Rust
authoring helper. That helper performs geometry calculations and emits ordinary
MCP commands; the existing native interpreter performs all CAD operations.
Commit the helper and generated source together. Returned topology references,
rather than recorded body IDs, drive the construction and drawings.

## Design and editing

Two identical 100 mm stages form a 180 mm diameter, 200 mm bucket-height Savonius
rotor. Each stage has a 198 mm bottom disc and two 2 mm semicircular bucket walls,
with 18 mm overlap. The second occurrence is staggered 90 degrees; the final cap
is separate so each stage prints upright without a large unsupported roof.
The assembled height exceeds 200 mm, while every printed definition fits within
200 mm on each axis. The purchased shaft is 275 mm long.

Each feature starts on a named datum. Circles have a located center and an
editable diameter. Polygon edges have driving length/angle dimensions and
coincident endpoints; a single fixed point locates the profile. All completed
sketches must have zero remaining degrees of freedom. Extrusion thicknesses,
bores and clamp clearances remain native history features. The exported
`stage_plate_feature` is deliberately exercised by the edit/restore regression.
Editing that definition updates both stage occurrences.

The module-1 spur pair has 72 and 18 teeth, a 20-degree pressure angle, a 3 mm
nominal face, 0.10 mm tooth thinning per gear and 45.25 mm shaft spacing. One
dimensioned involute tooth is patterned and fused to its root disc. Both gears
remain parametric solids. The persistent relationship drives the generator
coordinate as `10 - 4 * rotor_angle` degrees, including multiple full turns and
reverse driving. The 10-degree phase places the pinion gap opposite the first
rotor-gear tooth at the home position.

The pinion's M2 clamp accommodates a provisional 6 mm projecting shaft. Its
head/nut recess locally leaves a minimum 2.1 mm tooth-face thickness below the
recess. This is explicit prototype geometry, not a strength rating. The hub
requires a printed slip/torque coupon and a measured motor before physical
release; do not enlarge the modeled motor shaft merely to make the hub fit.

## Print and assemble

PETG is the baseline. Start with a 0.4 mm nozzle and 0.2 mm layers only as a
provisional process; qualify the actual filament and profile. Print each source
body in its supplied local orientation with Z=0 on the bed. The stage, cap,
base, carrier, cradle, guard, lid and gears have no broad unsupported ceilings.
Horizontal holes and nut pockets still require slicer review and bridge coupons.
The 198 mm plates leave ample room on the stated X2D bed, but their 200 mm
geometry envelope does not itself reserve a brim on a smaller printer.

1. Print short shaft, motor and bearing fit coupons first. The starting
   diametral allowances are 0.3 mm for the 8 mm stage/rotor shaft, 0.3 mm for
   the 22 mm bearing seats, 0.6 mm for the 32 mm motor case and 0.2 mm for the
   2 mm motor shaft. These are independent allowances, not a general tolerance.
2. Insert the lower 608 bearing from below the carrier and the upper bearing
   from above. The lower seat opens directly to the underside; attaching the
   base retains it. The relief between seats cannot pass a 22 mm bearing.
   Place the 28 mm inner-race spacer between the bearings. The upper washer,
   rotor gear and lower shaft collar provide the axial stack. Tighten carrier
   clamps only enough to retain the outer races, then check free rotation.
3. Fasten the carrier and generator cradle to the base. The motor cradle has
   separate flat clamp ears; a screw is not expected to bear on a thin round
   wall. Confirm actual terminals, wire routing, case length and shaft
   projection before printing the motor-specific parts.
4. Fit the motor pinion and rotor gear, using their flat head/nut seats.
   Check clamp slip, axial engagement, backlash and free rotation through a
   complete revolution before applying electrical load. Separate 608 bearings
   support the rotor; the motor is not used as its structural bearing.
5. Capture the eight M3 nuts in the guard before fastening it. Its bosses are
   staggered 45 degrees on the 109 mm bolt circle to clear both the generator
   cradle and gear sweep. Fit the lid with M3 x8 low-profile screws whose heads
   stand no more than 1.65 mm above the lid; the rotor disc begins 2 mm above it.
6. Clamp the first stage, then the repeated stage at 90 degrees. Install the
   cap and upper collar. Check axial retention, balance, guard clearance and
   fastener access with the electrical load disconnected.

The native TUR-BOM sheet lists the printed/purchased definitions and all clamp,
base and guard fasteners with quantities and lengths. Screws and nuts are a
procurement list, not hidden printable bodies. Inspect their actual head and nut
dimensions against the modeled seats. Simplified native purchased-part bodies
represent bearings, shaft/collars/spacer and the generator envelope; they are
not substitutes for supplier drawings.

PLA is useful for dimensional teaching iterations. PETG is the initial functional
indoor prototype. An ASA outdoor variant must retain the same functional
interfaces while revisiting fit, warping, layer strength and exposure. Changing
a material label does not qualify the same geometry under a different load.
Use the grade-specific references in `flagship-engineering.md`, and record the
filament, profile, orientation and coupon results with each tested configuration.

## Measure the experiment

The supplier's [KW-GEN3 data](https://www.vernier.com/files/kidwind/wind_turbine_generator_specs.pdf)
include 2.13 V open-circuit at 720 RPM and 4.25 V at 1,440 RPM. One reported loaded
point is 2.85 V at 36 mA (102.6 mW). These are generator measurements, not a
prediction of this rotor's output. The supplier's approximate 32 mm diameter and
34 mm total motor envelope do not establish the modeled 28 mm case/6 mm shaft
split; measure the specimen. The source deliberately flags that dependency.

For a 0.180 m x0.200 m swept area and illustrative air density 1.225 kg/m³,
`P_air = 0.5 * density * area * wind_speed³`: approximately 0.176 W at 2 m/s,
0.595 W at 3 m/s, and 2.756 W at 5 m/s pass through the swept area. Only a
fraction can become shaft power, and another fraction becomes electrical power.
The 4:1 speed increase also increases the torque demanded from the rotor.
Do not present either airflow power or open-circuit voltage as usable output.

Record startup angle, wind speed, rotor/generator RPM, known load resistance,
terminal voltage/current, bearing temperature, hub slip and any contact. Compare
no-load and loaded startup at multiple rotor angles. A fan is not a calibrated
wind tunnel; record measurement conditions instead of claiming a power curve.

## Validation and physical boundary

The Rust MCP acceptance test replays from blank twice, compares complete native
models/scenes/drawings, checks all sketch constraints and solved placement,
exports each printable definition as a selected manifold 3MF, edits the stage
plate and verifies save/reload, and drives the native gear relationship over
multiple turns. Final source checks stop on geometry errors, unsolved joints,
remaining sketch freedom or volumetric assembly interference.

The drawings contain associative projected geometry, critical part dimensions,
an assembly view and a hardware BOM. They are editable manufacturing references,
not a claim that a printer achieves a GD&T class. Physical fit, startup, output,
clamp strength, fatigue, bearing loads, guard access and durability remain to be
qualified. Use as a supervised near-ground science experiment for ages 8–12;
age 5 requires closer hands-on adult guidance. Keep fingers and loose objects
away from the rotor. No child-safety or outdoor-product rating is claimed.
