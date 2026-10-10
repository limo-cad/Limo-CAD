# Vertical-axis turbine

The `vertical-axis-turbine` recipe builds the actual editable model, places its
components with native joints, adds the 72:18 gear relationship, and creates its
drawing package. The same source builds the model in fast mode or teaches the
construction in presentation mode, with pause, step and speed controls. There is no second animation model or imported
STL/STEP geometry.

The [drawing guide](2D_DRAWINGS.md) records an unresolved live presentation gap:
the assembly sheet can show crosshairs without linework even when the native
SVG/DXF exports contain the placed geometry. Part sheets and the BOM rendered
in the inspected run; that does not qualify the missing live assembly views.

Choose **Vertical-axis turbine** in **Scripts**, then **Run in new design**.
For independent headless comparisons, follow the
[developer replay guide](DEVELOPMENT.md#replay-a-recipe) with recipe ID
`vertical-axis-turbine` and `--repeat 2`. Through MCP, use
`cad_interface` with `action: "script"`, `recipe: "vertical-axis-turbine"`,
`mode: "fast"`, and `validate: true` on a blank document. For an attached desktop,
use `mode: "present"`; pause, step and playback speed are presentation controls,
not separate execution modes. The equivalent command uses `--session <id>` and
`--present --speed 1`; independent `--repeat` checks run headlessly.
Do not replay into an unrelated working document.

The reviewed JSONC lives in `examples/scripts/vertical-axis-turbine.limo.jsonc`.
`cargo run -p limo-cad-recipes --example author_turbine` regenerates it from the Rust
authoring helper. That helper performs geometry calculations and emits ordinary
MCP commands; the existing native interpreter performs all CAD operations.
Commit the helper and generated source together. Returned topology references,
rather than recorded body IDs, drive the construction and drawings.
Presentation frames each new part profile, then its placed component. Captions,
300–450 ms camera transitions and part colors explain the same model: orange
rotor, golden drive, blue guard and teal supports. Fast replay skips presentation
delays. These colors are visual designations, not vendor-qualified filaments.

## Design and editing

Two identical 100 mm stages form a 180 mm diameter, 200 mm bucket-height Savonius
rotor. Each stage has a 198 mm bottom disc and two 2 mm semicircular bucket walls,
with 18 mm overlap. The 24 mm clamp hub ends 18 mm above the plate underside,
leaving the real 8 mm shaft through the rest of the overlap region. The hub and
both bucket walls join the 3 mm bottom disc, which transfers their load. This
keeps the M3 clamp and its flat seats accessible without a full-height blockage.
The second occurrence is staggered 90 degrees; the final cap
is separate so each stage prints upright without a large unsupported roof.
The assembly uses an uncut 300 mm long, 8 mm diameter steel shaft and stands
302 mm high. Individual printed parts fit the conservative 235.5 x256 x256 mm
X2D envelope. The source's `turbine/design.rs` supplies one dimensional contract
for construction, assembly, coupons and exported design metadata.

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

The current source uses six intervals per involute flank. The Rust author
bounds the continuous profile deviation from the exact involute below 0.01 mm:
0.002766 mm for the 72-tooth gear and 0.006855 mm for the 18-tooth pinion.
It adds the maximum intervening involute arc length to a sampled point-to-edge
distance bound, so checking only matching vertices cannot hide chord error.
This refinement changes the geometry reference from earlier development runs;
the final acceptance compares two runs of this committed source.

The pinion's M2 clamp has 5.4 mm nominal engagement on a provisional 6 mm
projecting shaft, leaving a 0.6 mm gap from the motor face. Its
head/nut recess locally leaves about 2.0 mm minimum tooth-face thickness below the
recess. This is explicit prototype geometry, not a strength rating. The hub
requires a printed slip/torque coupon and a measured motor before physical
release; do not enlarge the modeled motor shaft merely to make the hub fit.
The fixed motor bracket has vertical mounting slots providing +/-6 mm cradle adjustment.
Use that adjustment to align the gear faces after measuring the motor; it does
not guarantee adequate engagement for every shaft or make all 32 mm motors
interchangeable. Preserve the face gap, complete tooth-face overlap and access
to the M2 clamp at the chosen position. The reference body omits unmeasured
terminal and bearing-boss details, which must clear the mount and guard.

The stage, carrier, rotor gear and cradle use 6.4 mm round head recesses and
5.8 mm across-flats captive M3 hex-nut pockets. Their remaining clamp grips are
8, 10, 9.6 and 16 mm respectively. The rotor gear uses a 24 mm hub. The pinion's
12 mm hub has a 4.8 mm round head recess, a 4.3 mm across-flats captive M2 pocket
and a 4 mm grip. The actual M2 nut is nominally 4 mm across flats and 1.6 mm
high; its upward/downward pocket vertices set the local tooth-face minimum.
The cradle's back tab attaches to the separate fixed motor bracket, while its
split clamp ears grip the motor case. These modeled seats retain the nuts
against turning. Confirm head/nut dimensions and usable engagement against
the procurement drawing before printing.
The ears and back tab extend continuously from the print bed to local Z=20 mm.
The revised cradle reserves 4 mm below the provisional motor case for its rear
terminal opening and independent wire channel; the case and shaft positions
remain fixed while the cradle moves down by 2 mm. This allowance still requires
checking the actual terminals and insulated wire pair.

The rotor uses two noncontact-shielded 608ZZ bearings (8 x22 x7 mm), rather than
an unspecified 608 seal variant. Noncontact shields reduce seal drag but do not
provide the water protection of contact seals; this is the indoor baseline.
[NSK's shield comparison](https://www.nsk.com/content/dam/nsk-marketing/projects-completed/literature/product-brochures/deep-grooves-bbs_product-brochure/en_deep-grooves-bbs_product-brochure/preview-pdf_deep-grooves-bbs_product-brochure_en/EN_Deep%20Grooves%20BBs_Product_Brochure_low-res.pdf).
The purchased collars are 8 mm bore x16 mm OD x8 mm wide with M4 x4 set screws,
matching the [Ruland MSC-8-F dimensional catalog](https://media.distrelec.com/Web/Downloads/_t/ds/Ruland_3258224_eng_tds.pdf).
Each inner-race shim is nominally 8 x12 x1 mm metal, a specified narrow ring
such as [MISUMI CIMRS8-12-1.0](https://us.misumi-ec.com/vona2/detail/110310833039/?HissuCode=SH-PACK10-CIMRS8-12-1.0).
A generic M8 washer can have a larger outside diameter and rub a bearing shield.
The reference [SKF 608-2Z drawing](https://docs.rs-online.com/ec36/A700000009501532.pdf)
specifies a 10–12 mm shaft abutment diameter, approximately 12.15 mm inner-ring
shoulder and 19.2 mm outer-ring recess. The native representation separates
inner ring, outer ring and shield clearance envelopes. Verify the actual faces,
shim tolerances and shield recess before accepting a supplier substitution;
these simplified envelopes do not represent internal rolling elements.

The base is 170 x140 x12 mm. Lower and upper bearings span Z=12–19 and
47–54 mm respectively. The upper shim spans 54–55 mm; the lower shim spans
10.8–11.8 mm, above the lower collar at 2.8–10.8 mm. All heights are assembly
coordinates measured from the underside of the base, not part print positions.

Rotor weight passes through the 24 mm long printed support sleeve, rotor gear
and upper shim into the upper bearing's inner ring, then through that bearing
into the carrier. There is no clamped spacer between the two bearings. The
lower collar and shim capture upward shaft movement with 0.2 mm nominal axial
endplay, set after assembly. They must not preload the bearing pair. The sleeve
is 18 mm OD with an 8.3 mm bore and provides a positive seat beneath the lower
stage. See [bearing-stack guidance](../knowledge/concepts/bearing-stacks.md) for
contact, float and installation checks.

## Print and assemble

PETG is the baseline. Start with a 0.4 mm nozzle and 0.2 mm layers as a
provisional process; qualify the actual filament and profile. The recipe's
`print_plates` export contains eleven native one-part plates. Each selects one
real occurrence, upright at [110, 110, 0], with the other occurrences hidden.
The acceptance output writes these as `<part-id>.limo` and `<part-id>.3mf`.
Print the stage plate twice; the other ten plates each supply one part.
Open a supplied plate model and export its visible assembly, or use its supplied
3MF directly. Keep the native placement when slicing. A raw **Part coordinates**
export can include negative X/Y and is not an arranged print plate. The complete
assembled turbine also includes purchased hardware and is not a print layout.
The stage, cap, base, carrier, fixed motor bracket, cradle, support sleeve,
guard, lid and gears are designed to avoid broad unsupported ceilings.
Horizontal holes and nut pockets still require slicer review. Start with the
bundled `turbine-fit-coupons` recipe: editable specimens cover the stage
shaft, lower bearing seat, adjustable motor mounting and actual pinion. Each
has an associative drawing with its bore, nominal hardware size, allowance and
print orientation. The same Rust author emits both sources and reuses the
cradle, clamp and pinion construction. `turbine-fit-coupons.3mf` contains the four
coupons in their authored native plate arrangement. The four individual coupon
3MFs retain those same positive assembly poses and Z=0 contact; they need no
automatic rearrangement.
The shortened shaft/bearing specimens reproduce the local fit and clamp seats;
they do not establish full-rotor stiffness, fatigue life or load capacity.
These supplied plates target the X2D's main nozzle. The 198 mm stage also fits
within the shared area's dimensions, but its supplied X=11–209 mm placement
extends left of that area's X=20.5 mm boundary; a dual-nozzle layout needs its
own deliberate positioning and validation.
Reserve the required brim/skirt space in the actual plate layout; geometry
bounds alone do not establish print adhesion or the usable layout on another printer.

The base's collar-key passage preserves its full 6 mm circular tool envelope.
A separate editable cut adds 45-degree tangent roof faces and a 1 mm flat
ceiling at Z=10.5426 mm, leaving 1.4574 mm minimum roof stock. This replaces the
unsupported last wall of the original circular roof while retaining the tool
path. Inspect the actual final wall and its preceding layer as well as the
short ceiling bridge; support-free intent is not established by counting zero
support paths. See [slicer evidence guidance](../knowledge/concepts/export-print.md).

1. Print short shaft, motor and bearing fit coupons first. The starting
   diametral allowances are 0.3 mm for the 8 mm stage/rotor shaft, 0.3 mm for
   the 22 mm bearing seats, 0.6 mm for the 32 mm motor case and 0.2 mm for the
   2 mm motor shaft. These are independent allowances, not a general tolerance.
2. Insert the lower 608ZZ bearing from below the carrier and the upper bearing
   from above. The lower seat opens directly to the underside; attaching the
   base retains it. The relief between seats cannot pass a 22 mm bearing.
   Apply insertion force to the outer ring being seated, not through the balls.
   Preload the carrier and fixed-bracket mounting nuts before fitting the gear
   or adjustment hardware. Lower the loaded carrier onto the base and fasten
   it and the fixed motor bracket. Insert the shaft from above, then fit the
   lower shim and collar from below. Fit the upper shim and rotor gear.
   Tighten carrier clamps only enough to retain the outer
   races, then check free rotation. Seat the rotor's downward load on the upper
   shim and adjust the lower collar to leave 0.2 mm axial movement. Its set screw
   faces -Y; a 6 mm tool corridor through the front of the 12 mm base reaches it
   at Z=6.8 mm. Check the actual endplay with the complete rotor fitted, rather
   than inferring it from printed nominal dimensions.
   Set the carrier clamps before installing the generator assembly.
3. Load the cradle's clamp nut off the base. Drop its two adjustment nuts
   through the empty open cradle and slide them toward +Y into the rear
   captive seats; the loading throat extends from Y=20 to Y=10 mm. These nuts
   must be installed before the motor blocks the passage. Insert the motor
   from above, slide the loaded cartridge toward +X into the fixed bracket,
   then fit the adjustment screws from +Y. Confirm actual terminals, wire
   routing, case length and shaft projection before printing these parts.
   Set vertical position for the measured specimen, align both tooth faces,
   and check face gap and shaft engagement before tightening. The +/-6 mm
   slots provide fitting adjustment; moving an engaged motor through the full
   range is not an operating or service motion. The wire exit is separate from
   the lower-collar tool corridor.
4. Preload the pinion's captive nut and fit the pinion from above. Check the
   motor and rotor gear clamps using their flat head/nut seats.
   Check clamp slip, axial engagement, backlash and free rotation through a
   complete revolution before applying electrical load. Separate 608ZZ bearings
   support the rotor; the motor is not used as its structural bearing.
   Fit the support sleeve above the rotor gear before enclosing the drive.
5. Capture the eight M3 nuts through the guard's open ends before closing it.
   Its bosses are
   staggered 45 degrees on the 122 mm bolt circle to clear both the generator
   cradle and gear sweep. The guard is 132 mm OD with a 126 mm cavity; it spans
   Z=12–69 mm, with the 3 mm lid above. Fit the lid with M3 x8 ISO 7380-1 button
   screws whose heads stand no more than 1.65 mm above it. The rotor disc begins
   19 mm above the lid, leaving 17.35 mm nominal clearance above those heads.
   Fasten the lid off the base, then lower the assembled guard/lid over the
   shaft and drive; its 30 mm center opening passes over the 18 mm sleeve.
   Fit the guard's base screws from below. Install the lid before the rotor
   stages: its bolt tool path needs 45 mm
   clear above the head. Later lid service requires removing the rotor stages;
   the head clearance alone is not screwdriver clearance.
   These screw dimensions are available as [M3 x8 ISO 7380-1 buttons](https://www.accu.co.uk/socket-button-screws/8102-SSB-M3-8-A2).
   Check the actual assembled stack and rotor runout before operation.
6. Seat and clamp the first stage onto the support sleeve. Fit the repeated
   stage at 90 degrees, then the cap and upper
   collar. Stage undersides are at 91 and 191 mm; the cap spans 291–294 mm and
   the upper collar spans 294–302 mm. The shaft runs from 2 to 302 mm, engaging
   the full upper collar. The gear spans 55–67 mm and the sleeve 67–91 mm.
   Check axial retention, balance, guard clearance and
   fastener access with the electrical load disconnected.
7. Anchor the base to a stable bench or test fixture before any fan or wind
   test. Use its four 5.5 mm mounting holes with suitable M5 hardware; bolt
   length depends on the fixture. The raised rotor creates overturning leverage,
   so standing the base loose on a table is not an operating setup.

The native TUR-BOM sheet lists the printed/purchased definitions and all clamp,
base and guard fasteners with quantities and lengths. Native screw/nut envelopes
are purchased-hardware references: omit them from printing and compare their
actual head/nut dimensions with the modeled seats. The bearing rings/shields,
shaft/collars/shims and generator are also simplified purchased-part envelopes;
use the supplier drawings and measured specimen for procurement and fitting.

PLA is useful for dimensional teaching iterations. PETG is the initial functional
indoor prototype. An ASA outdoor variant must retain the same functional
interfaces while revisiting fit, warping, layer strength and exposure. Changing
a material label does not qualify the same geometry under a different load.
Use the grade-specific references in `flagship-engineering.md`, and record the
filament, profile, orientation and coupon results with each tested configuration.

## Measure the experiment

The [Vernier KW-GEN3](https://www.vernier.com/product/kidwind-wind-turbine-generator-with-wires/)
is the reference generator, listed in the US catalog on 11 September 2026 at
$25 for three before shipping. A purchase price is not a stock guarantee.
The supplier's [generator measurements](https://www.vernier.com/files/kidwind/wind_turbine_generator_specs.pdf)
include 2.13 V open-circuit at 720 RPM and 4.25 V at 1,440 RPM. One loaded point
at 1,440 RPM is 2.85 V at 36 mA, or 102.6 mW. These are measurements of the
generator at driven speed, not this rotor's output. The table's resistance
labels do not consistently equal V/I; use a measured resistor rather than
copying those labels as verified loads.

Vernier's [current specification FAQ](https://www.vernier.com/til/3155) gives
approximately 32 mm diameter, 34 mm overall length including the shaft, and a
2 mm shaft. The linked [full specification sheet](https://www.vernier.com/til/wp-content/uploads/sites/2/2023/06/3155-wind_turbine_and_hi_torque_generator_specs.pdf)
contains motor performance curves on both pages, not a dimensioned mounting
drawing. The supplier's public product, FAQ, specification and kit-manual
sources reviewed on 11 September did not establish case length, projecting
shaft length or terminal geometry. The modeled 28 mm case/6 mm shaft split is
a reference specimen assumption, not a supplier specification. Measure those
dimensions separately before printing the replaceable motor-specific parts.

For a 0.180 m x0.200 m swept area and illustrative air density 1.225 kg/m³,
`P_air = 0.5 * density * area * wind_speed³`: approximately 0.176 W at 2 m/s,
0.595 W at 3 m/s, and 2.756 W at 5 m/s pass through the swept area. Only a
fraction can become shaft power, and another fraction becomes electrical power.
For planning only, an assumed power coefficient C_p of 0.05–0.10 gives
30–60 mW shaft power at 3 m/s and 138–276 mW at 5 m/s. Drivetrain and generator
losses reduce electrical output further; startup may prevent reaching these
operating points. A [two-stage experiment](https://myresearchspace.uws.ac.uk/ws/portalfiles/portal/58300379/2022_12_16_Shamsuddin_et_al_Experimental_final.pdf)
measured C_p = 0.062 at 5 m/s and 0.104 at 9 m/s, with all-angle startup at
5 m/s in its own apparatus. Its rotor, bearing losses and airflow differ from
this design. The values motivate a modest planning range, not a promised
coefficient or starting wind speed for these prints.

For this 90 mm rotor radius, `rotor_rpm = 60 * lambda * wind_speed / (2 * pi * R)`.
Assuming tip-speed ratio lambda = 0.5–0.8 gives 159–255 rotor RPM at 3 m/s and
265–424 RPM at 5 m/s. The 4:1 drive then gives 637–1,019 and 1,061–1,698 generator
RPM. Using approximately 2.95 mV/RPM from the supplier's low-speed open-circuit
data implies about 1.9–3.0 V and 3.1–5.0 V respectively, conditional on reaching
those speeds. Load current lowers voltage and increases the resisting torque.
Neither airflow power nor open-circuit voltage is usable electrical output.

The 4:1 ratio is an experimental starting point. It multiplies generator drag
as well as speed. A higher ratio or a motor advertised as "high torque" is not
automatically an improvement. Match voltage per RPM and running/breakaway torque
to the rotor before changing parts. See the reusable
[rotor and generator guidance](../knowledge/concepts/small-wind-generators.md).

At 5 m/s the same assumptions give dynamic pressure `q = 0.5 * density * v² =
15.3 Pa`, and `q * area = 0.551 N` is a force scale. Actual rotor force also
depends on its drag coefficient, orientation and unsteady flow; this does not
establish the bearing or guard design load.

At an illustrative rotor torque of 0.004 N·m, the installed 45.25 mm center
distance gives a 36.2 mm rotor working pitch radius and a 9.05 mm pinion radius.
The working pressure angle is `acos(45 * cos(20°) / 45.25) = 20.85°`, giving
`F_t = T / r = 0.110 N` and `F_r = F_t * tan(20.85°) = 0.042 N`.
These are ideal involute-pair calculations, not measured tooth loads.
Ideal generator torque is 0.001 N·m at four times the speed;
friction reduces what reaches the shaft. This is comparable in scale to
Vernier's stated motoring rated load of 10 gram-force centimetres
(0.000981 N·m), but it is not a generator-mode continuous torque rating.
The supplier's 5.9 V motoring stall torque of 40 gram-force centimetres
(0.00392 N·m) is also not a permissible continuous generator operating target.

No teaching load establishes this rotor's torque, printed tooth strength or
motor radial-load limit. Tooth roots, layer direction, clamp slip and cyclic
loading require physical tests. The motor supports only its own pinion, but
that pinion still applies a gear side load which must be qualified.

Start with the load disconnected and measure startup from multiple rotor
angles. Then introduce known resistors from lighter to heavier loading and
record simultaneous rotor/generator RPM, terminal voltage/current and actual
resistance. Check V/I and V*I, bearing temperature, hub slip and any contact.
Recheck startup under each intended load. Use a meter before an LED: Vernier
reports that its red-LED demonstration needs about 700 generator RPM, so a
dark LED alone does not diagnose a failed generator.
[Supplier test guidance](https://www.vernier.com/til/3183).
A fan is not a calibrated wind tunnel; record airspeed, measurement location
and fan arrangement instead of claiming a power curve.

## Validation and physical boundary

The [native validation snapshot](manufacturing/vertical-axis-turbine.validation.json)
records the tested source revision, source hashes, counts and native output hashes.
It distinguishes the headless lifecycle from live presentation and physical testing.
Its results apply only to the listed source hashes; later design edits require
fresh replay, assembly, export and slicer checks before inheriting those claims.

The complete Rust MCP mechanical gate passed on the frozen stable-underside
source. It covers both stage instances under thickness and shaft-bore edits,
cold reload, associative dimensions, hardware insertion, bearing/wire clearances
and forward, reverse and multi-turn drive. Native interference samples use
0.25-degree phases through one tooth period and 15-degree samples through a
full rotor turn. These are discrete checks, not proof of continuous contact.

The final source moves only four drawing leaders. Two independent blank replays
of each final recipe and exact non-drawing parity connect the delivered files
to that mechanical baseline. All 18 sheet pairs match the reviewed native
exports, and all 11 main plates plus five coupon files match the actual
reviewed X2D slicer inputs. The snapshot keeps these source scopes separate;
neither native geometry nor toolpath checks establish physical print quality.

The drawings contain associative projected geometry, critical part dimensions,
base size and shaft spacing, overall assembly dimensions and a hardware BOM.
Small gears and bearing/motor supports use enlarged views for legibility.
They are editable manufacturing references,
not a claim that a printer achieves a GD&T class. Physical fit, startup, output,
clamp strength, fatigue, bearing loads, guard access and durability remain to be
qualified. Use as a supervised near-ground science experiment for ages 8–12;
age 5 requires closer hands-on adult guidance. Keep fingers and loose objects
away from the rotor. No child-safety or outdoor-product rating is claimed.
