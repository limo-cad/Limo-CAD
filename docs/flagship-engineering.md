# Turbine and vise: engineering inputs

This records the user's 10 September 2026 direction and the resulting engineering
proposals. Neither example has a construction recipe yet. The bench remains at
its accepted development milestone. These are inputs to native, editable models,
shared Rust/MCP operations, teaching chapters and comprehensive drawing packages.

## Agreed purpose

- Build a functional vertical-axis turbine for a near-ground science experiment.
  Integrate an inexpensive, documented motor as a generator. Start with that
  electrical component; use common mechanical hardware and simple, strong drive
  connections. Teach tolerances, rotation, relations, mathematics and additive
  manufacturing. Include low-energy demonstration use and physical guarding of
  the transmission in the design; child-safety qualification is not established.
- Build a functional vise primarily for FDM. Minimize parts, assembly effort and
  generated supports. Investigate the user's flat-printing semicircular screw.
  Show force, contact pressure, stiffness and measured durability. Retain related
  CNC and molded configurations with common functional interfaces.
- Use the three flagships for complementary workflows and small native recipes
  for features that do not naturally belong in them. No separate animation model,
  JavaScript modeling driver or mesh import should replace parametric history.

## Printer and materials change the design

The [X2D specification](https://blog.bambulab.com/two-extruders-one-purpose-what-is-x2d-direct-drive-extrusion-and-auxiliary-extrusion/)
gives 256 x 256 x 260 mm for the main nozzle, and 235.5 x 256 x 256 mm for the
shared dual-nozzle volume. Use a provisional 200 mm individual-part envelope,
with placement/brim margins checked separately, for wider printer compatibility.
The assembled design may be larger. Support-free single-material construction
must not depend on the X2D's auxiliary nozzle. A 0.4 mm nozzle and 0.2 mm layers
are provisional starting assumptions, not a reading of the user's installed setup.

The slicer controls deposited walls, infill, cooling and supports. Material choice
also changes the engineering of the supplied geometry:

- Stiffness affects how far the jaw and bearing carriers deflect. Increasing rib
  depth or shortening the load path can be more useful than simply adding infill.
- Sustained clamp load can cause thermoplastic creep and force relaxation. Layer
  direction, root sections and retention geometry must suit the loads; transient
  tensile strength alone does not establish a vise rating.
- Fit depends on grade, print orientation and process settings. Bearing seats,
  running gaps, guide clearance and thread allowance need separate named inputs
  and physical coupons. Distinguish radial from diametral clearance.
- Thread roots, loaded holes and flexures require intentional wall geometry.
  Thicker snap arms can require more force and strain for the same deflection;
  useful changes include arm length and root radius as well as thickness.
- Print orientation trades support demand against layer strength and surface
  quality. Inspect the actual toolpaths around threads, bridges and bearing seats.

These decisions follow [Stratasys FDM design guidance](https://www.stratasys.com/contentassets/1a0cc7a8e7d14f29ac972189bfeade4c/dg_fdm_designconsiderationsfdmtooling_0718a.pdf)
and [Prusa's fitting/orientation guidance](https://help.prusa3d.com/article/modeling-with-3d-printing-in-mind_164135).
They are not measurements of our parts.

Provisional material choices are PETG for the first functional indoor structures,
PLA for dimensional/teaching iterations, and an ASA turbine configuration for
persistent outdoor exposure. PLA is not universally less stiff than PETG; PETG's
toughness does not guarantee a stiffer vise. PETG overhangs and bridging need
profile-specific verification; ASA requires attention to warping. Use a named
grade and its own data sheet, then qualify the printed part. See
[Prusa PLA](https://help.prusa3d.com/article/pla_2062),
[Prusa PETG](https://help.prusa3d.com/article/petg_2059) and
[Bambu ASA guidance](https://us.store.bambulab.com/collections/filament-single-discount/products/asa-filament).

Support-free construction and print-in-place construction are different choices.
Use integrated or captured parts when they improve the product; retain access to
wear surfaces, generator mounting and fit adjustment. Broad shoulders should
carry clamp loads; small retention snaps should not accidentally become the main
load path. The first printable screw remains part of the vise's intended scope.

## Turbine starting architecture

Propose a Savonius rotor: two short bucket stages with a selectable relative angle,
initially 90 degrees. Reuse the same stage definition. Print upright with an
integral bottom plate and a separate final cap, avoiding a broad unsupported roof.
This is an initial construction hypothesis pending slicing and startup tests.
Keep stage angle, overlap, height, diameter and clearances explicit. A helical
variant can compare twist and lofted geometry through the same mounting interfaces.

Staggering stages has experimental support for improving startup behavior, but
published larger rotors do not qualify this particular design. See the
[two-stage rotor experiment](https://myresearchspace.uws.ac.uk/ws/portalfiles/portal/58300379/2022_12_16_Shamsuddin_et_al_Experimental_final.pdf).
Verify startup from multiple angles under the intended electrical load.

The preferred motor candidate is the
[Vernier KidWind KW-GEN3](https://www.vernier.com/product/kidwind-wind-turbine-generator-with-wires/),
listed on 10 September at $25 for three, approximately $8.33 each before shipping.
The supplier gives an approximately 32 mm diameter body, 34 mm total length
including the shaft, and a 2 mm shaft. Confirm projecting shaft length and mounting
dimensions before finalizing the pinion/carrier. The supplier's
[generator-mode measurements](https://www.vernier.com/files/kidwind/wind_turbine_generator_specs.pdf)
report 2.13 V open circuit at 720 RPM and 4.25 V at 1,440 RPM. One reported loaded
point at 1,440 RPM is 2.85 V and 36 mA, or 102.6 mW. The table's resistance labels
are inconsistent with V/I; do not copy them as verified electrical loads. Measure
our specimen with a known load. These data do not predict our rotor's output.

Use separate spaced rotor supports and positive axial retention. Do not assume
the generator bearings can carry rotor weight, bending or arbitrary gear side load.
Propose one interchangeable spur-gear stage, initially around a 4:1 speed increase.
That ratio remains an experimental input: speed multiplication also increases the
required rotor torque. Use a split-clamp motor-pinion hub with a through bolt and
captured nut, subject to a slip/fit coupon. The candidate motor shaft is round;
do not invent a D-flat. Confirm that a printable hub fits the available shaft
length and inspect motor side load before freezing this connection.

This follows [maxon's generator selection principles](https://support.maxongroup.com/hc/en-us/articles/360004496254-maxon-Motors-as-Generators)
and [shaft-clamp principles](https://www.ruland.com/technical-resources/technical-articles/technical-article-what-are-shaft-collars).
Industrial metal-clamp ratings are not ratings for our printed hub. No hardware
has been purchased; motor selection is a documented proposal.

## Vise starting architecture

Aim for a frame/fixed jaw, guided moving jaw, printed screw/handwheel and, if useful,
a captured replaceable nut cartridge. The cartridge adds a part but allows better
print orientation, alternate fits and replacement after wear. Keep the load path
short and thrust retention accessible. Choose broad locating and load-bearing
faces before adding decorative contours.

The semicircular screw interpretation is still awaiting the user's clarification:

- Two halves printed flat and joined retain full circumferential engagement but
  need positive registration and a seam that transfers torque and axial force.
  Split one authored helix into its complementary halves. Mirroring one half
  changes helix handedness and does not produce the missing half.
- A single half-round rotating screw has interrupted engagement and asymmetric
  stiffness/reaction forces. It needs a circular rotating envelope, not a matching
  D-shaped nut bore. It can remain engaged through a turn, but ordinary full-screw
  contact-area assumptions no longer apply.

Both require short thread/guide coupons in the intended print orientation. A flat
shaft base alone does not prove that its thread flanks or mating nut need no support.
Consider a coarse, rounded-root profile with adequate engagement. Custom rounded
or trapezoidal profiles need native support; the current ISO/Unified feature must
not be mislabeled to disguise a different thread form.

For CNC/molded versions retain the same functional datums, jaw opening, travel
and interfaces, with appropriate manufacturing variants. CNC needs cutter access
and internal tool radii. Molding needs wall control, draft, parting/ejection and a
deliberate solution for thread/guide undercuts. A print-in-place assembly is not
automatically manufacturable as one molded part. See
[Protolabs machining guidance](https://www.protolabs.com/resources/design-for-machining-toolkit/)
and [molded-thread guidance](https://www.protolabs.com/resources/design-tips/molded-threads-and-how-to-design-them/).

## Calculations shown with the geometry

Calculations must use named, unit-labeled inputs and state their assumptions.
They should execute through the shared Rust/product path and appear in teaching
notes and final reports. This section specifies required outputs; they are not
currently implemented as a load-analysis capability in the CAD solver.

For a vertical rotor, projected swept area is `A = diameter * height`, in m².
Available wind power is `P_air = 0.5 * air_density * A * speed^3`. Shaft/electrical
output requires separately characterized rotor efficiency and drivetrain/generator
losses. At an illustrative diameter of 0.18 m, height 0.20 m and air density
1.225 kg/m³, available air power is 0.595 W at 3 m/s and 2.756 W at 5 m/s.
These are neither generated power nor a target-specific power coefficient. Use
measured speed, torque, voltage and current when comparing experiments. See the
[DOE wind-power relation](https://www.energy.gov/sites/prod/files/2015/05/f22/Enabling%20Wind%20Power%20Nationwide_18MAY2015_FINAL.pdf).

For the vise, retain `lead = pitch * starts`, `travel = lead * turns` and
`input_torque = tangential_hand_force * handle_radius`. A first energy-balance
estimate is `axial_force = 2 * pi * input_torque * efficiency / lead`, with lead
in metres and torque in N·m. Efficiency is a measured/assumed operating input,
not a universal plastic constant. The model must state whether it includes thrust
and guide friction. See [Thomson lead/torque guidance](https://www.thomsonlinear.com/en/support/tips/when-considering-lead-screws-what-specifications-are-most-important-to-look-at).

Illustration only: 0.5 N·m, 4 mm lead and assumed overall efficiency 0.25 give
196 N axial force. Over an actual 600 mm² jaw contact patch, mean pressure is
0.327 MPa. A 200 mm² patch under the same force sees 0.982 MPa. This is not a
clamping-force rating, contact-stress solution or durability claim.

Before rating the design, also check thread bearing/stripping, root torsion,
frame/jaw bending and deflection, retention and self-locking with measured friction.
For screening, `moment = force * offset` and `bending_stress = moment * c / I`;
real thread load sharing, corner stresses and layer anisotropy need attention.
Show durability through clamp/release cycles and hold tests: force relaxation,
backlash, wear and retention versus time/cycles under recorded material, print,
temperature and loading conditions. An instantaneous strength calculation does
not predict service life.

## Capability work belongs under the examples

The current source review identifies these shared implementation needs:

- A driven-coordinate mechanism solve for the vise: hold the commanded screw
  coordinate while solving passive jaw/retention joints. Ordinary joint motion
  currently assigns one coordinate and may leave a closed loop inconsistent.
- External-thread operations through MCP, suitable custom printed thread support
  if selected, and motion-study commands through the same product interface.
- Gear coupling if the turbine transmission is shown moving: prescribe one
  driver and derive the other shaft's angle from tooth counts. Independent angle
  animation is not a mechanically coupled assembly.
- Named material/process/fit and mathematical relations that survive native edit,
  recompute, save/reopen and a second edit. Script bindings alone are not editable
  master parameters; extend the existing Rust expression/parameter path as needed.
- Print-oriented part selection/layout/envelope checks, plus assembly-aware drawing
  projections, specialized dimensions, BOM and drawing output. Existing assembly
  export placement does not establish a printable layout or a complete drawing.

Build these in reviewable layers below recipes that need them. The same recipe
must build, teach, show and validate the editable result. Keep physical fit/load
evidence distinct from deterministic geometry and do not change the accepted
bench source as part of selecting the two new designs.
