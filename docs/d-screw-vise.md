# Captured-slide printed vise

The active design has **100 mm gripping faces, 90 mm opening, a 230 × 160 × 14 mm
base and a 24 × 4 mm printed lead screw**. It replaces the earlier 60 mm design,
whose screw could not be assembled as prescribed and whose moving jaw could hit
mounting hardware. Earlier replay records do not qualify the replacement.

The source is [d-screw-vise.limo.jsonc](../examples/scripts/d-screw-vise.limo.jsonc).
Choose it in **Scripts → Run in new design**. For headless replay and comparison,
use the [developer replay guide](DEVELOPMENT.md#replay-a-recipe) with recipe ID
`d-screw-vise`.
The Rust [author](../crates/recipes/examples/author_vise.rs) emits readable JSONC;
all geometry is constructed by the native MCP interpreter. Sketches, dimensions,
datums, features, parts, joints and drawing sheets remain editable.

This is a development candidate validated by native geometry, assembly, edit,
independent replay and export tests, plus a rebuilt-desktop Open and exact model
comparison. **300 N is a design load case, not a tested capacity.** Physical fit,
load, fatigue and creep qualification remain open.

## Mechanical design

Six parts are printed: the frame, captured moving jaw, threaded rear bridge,
screw with integral grip, detachable thrust fitting and keeper. The bridge itself
is the replaceable wear nut. Its keyed feet enter recessed deck sockets; the
socket shoulders carry the axial reaction and two M6 bolts retain the bridge.

The moving jaw has a 72 mm-long carriage and two 45° dovetail channels. Each rail
is 8 mm high, 12 mm wide at the root and 28 mm at the head. The channel has
0.4 mm side clearance at the deck and 0.4 mm roof clearance, with matching slopes.
These are process allowances to qualify with a coupon. The carriage rests on the
deck and stays captured against lifting; an ideal slider joint is not its only
restraint. Broad jaw gussets and an 8 mm fixed-jaw root blend spread load. The
carriage has matching front relief so it can approach full closure.

The screw axis is 50 mm above the mounting surface. The integral grip is 20 mm
thick and approximately 44 mm wide, with 4 mm upper and 2.5 mm lower transition
radii and 0.6 mm end-rim chamfers. The central bed flat remains intact. The shaft's
print flat lies 7 mm below its axis: 5 mm is removed from the major-radius side,
rather than half the shaft. The nominal 20 mm root retains about 91% of its full
circular area before local holes. This area estimate is not a torsional rating;
the interrupted thread, keyed neck, layer direction and nut pocket need separate
strength assessment.

The entire oversized thrust fitting detaches. The remaining 18 mm keyed stub
passes through the bridge before the fitting is installed. Closing force passes
from the stub end into the blind fitting floor, through the full round head into
the jaw, workpiece, fixed jaw, frame and keyed bridge. An axial M5 fastener and
captive nut retain the fitting during opening. The keeper retains the rotating
head in the jaw. Its two 45° rear ears bear against matching jaw shoulders after
0.4 mm of axial seating. A transverse M5 pin prevents the keeper from lifting;
the keeper's pin opening has 1 mm of added axial slot travel so the bearing ears
can seat before the pin takes the opening load. These are nominal clearances,
not a tested load-sharing guarantee.

## Assembly and mounting

Purchased hardware is modeled as simplified clearance envelopes, not detailed
fastener threads. Verify supplier dimensions against the pocket and head
clearances before fabrication. The mechanism uses two M6 × 35 bolts and M6 nuts,
one M5 × 25 bolt and M5 nut, and one M5 × 90 bolt and M5 nut. Four optional
M6 × 45 mounting bolts, eight 18 mm-OD washers and four M6 nuts are included in
assembly collision checks. Their illustrated length assumes an 18 mm board.

1. Load the bridge's two M6 nuts through the underside of the frame. Leave the
   bridge off while feeding the moving jaw onto the open rear ends of the rails.
2. Move the jaw forward 85 mm to its service position, leaving a 5 mm gap to the
   fixed jaw. Seat the keyed bridge and fit its two M6 bolts from above.
3. Turn the bare screw through the bridge with the complete thrust fitting off.
   Load the small M5 nut into the now-exposed keyed stub at the service position.
4. Slide the thrust fitting onto the keyed stub from the jaw side. Its internal
   ledge supports the nut in alignment as it covers the loading throat. Tighten
   the axial M5 screw while the forward service position leaves driver access.
5. Slide the jaw back over the secured head. Drop the keeper into its top slot,
   load the keeper nut and insert the transverse M5 pin. Avoid clamping the
   rotating fitting rigidly with its retaining hardware.
6. Mount using the outboard slots or clamp the broad side lands to a tabletop
   edge. Mounting fixtures must stay outside the carriage's complete sweep.

The assembly uses a screw–revolute–slider loop. Driving the screw by one turn
advances the jaw 4 mm. Its 90 mm stroke requires 22.5 turns; permitted joint travel
ends at zero opening. Physical clearance, elastic seating and backlash are not
simulated by the ideal motion relation. Tightening the fitting seats its 0.4 mm
assembly gap; the nominal keeper/head/chamber stack and keeper seating still
allow about 1.4 mm of axial take-up on reversal before elastic effects. Measure
thread play on the coupon and total reversal travel on the assembled prototype
rather than describing the physical drive as zero-backlash.
The larger jaw can still rack under
off-centre clamping; qualify that use instead of inferring rigidity from a mate.

## Thread and support-free print intent

The mating thread is a **custom rounded 30° trapezoidal form**, nominal Ø24 mm,
4 mm single-start lead, 2 mm radial depth and 0.3 mm profile corner radii. The
female feature adds 0.25 mm radial and 0.20 mm total axial relief. It is not an
ISO Tr or ACME tolerance-class claim. Both mating features store the same profile
parameters; the native engine applies female relief without changing the lead.

Native geometry uses circular root/crest blends in a true helical surface.
The same native feature can be created and edited in the normal thread dialogs
or through MCP. The browser fallback explicitly rejects this custom form rather
than replacing it with a different thread shape.

Start with [d-screw-vise-fit](../examples/scripts/d-screw-vise-fit.limo.jsonc): a
40 mm male screw, 28 mm female engagement, and male/female specimens of the actual
captured guide. Use the intended nozzle, layer height, wall settings and material.
Measure fit, rocking and turning effort before scaling up to the full parts.

Each printed part has its own saved native print layout and selected-body 3MF.
All target a conservative 235.5 × 256 × 256 mm usable envelope; the whole kit is
not required to occupy one plate. Layouts hide the other assembly bodies but
retain their editable geometry. Export selection is explicit: hidden bodies are
not implicitly removed from CAD or from other exports.

The screw rests on its shallow flat. The bridge prints with its thread axis
vertical. The jaw rests on its gripping end with its long guide channels aligned
with the print direction. Its keeper pocket opens through the central rear wall
instead of closing with a broad unsupported ceiling; the remaining side seats
and matching keeper ears have 45° slopes. Other parts have broad bearing faces on the
bed, and horizontal fastener bores use print roofs where needed. **Zero supports
is the design objective, not a qualification obtained by disabling supports in
the slicer.** Check undersides, pocket ceilings, first contact and short bridges
in the actual sliced layers, then print the coupon.

The current recorded X2D/PETG HF slices contain no support paths or slicer
warnings on any of the six plates. Layer review confirms that the revised jaw
keeps its rear relief open instead of bridging the former keeper-pocket ceiling.
The screw pocket roof, thread finish and keeper ear tips still need physical
print inspection. Exact inputs, settings and coverage are in the
[validation record](manufacturing/d-screw-vise.validation.json).

PETG is the provisional indoor process. PLA is useful for dimensional prototypes;
ASA or other materials need their own shrinkage, adhesion, fit and sustained-load
qualification. Material tags and thicker dimensions alone do not establish
creep resistance. [Prusa's design guidance](https://help.prusa3d.com/article/modeling-with-3d-printing-in-mind_164135)
explains the orientation, bridging and fit considerations behind these choices.

## Editable intent and drawings

The author has one named set of design inputs. In a saved project, individual
sketches are independently editable; they are not all linked to a global master
parameter. Coordinated changes must update the relevant mating features.

**Moving jaw / 100 mm gripping face** controls gripping width. **Jaw dovetail
left / profile clearance** controls the channel mouth. The external thread and
female hole retain their custom parameters as editable features. A change of
pitch also requires updating the screw joint's lead and regenerating the fit
coupon. Diameter, guide and bridge changes require a complete fit review.

Seven native drawing sheets cover the assembly and each printed part. The
assembly includes a section view and purchased-hardware BOM. Dimensions come
from native projected geometry. Nominal dimensions and stated process gaps do
not claim qualified GD&T, interchangeability or a finished production drawing.

## Calculations and qualification

For 1 N·m input torque, an assumed 20% overall efficiency and 4 mm lead,
`F = 2π η T / lead` gives about **314 N**. Across a 600 mm² contact patch, average
pressure is about **0.52 MPa**. Varying assumed efficiency from 10% to 30% changes
the same estimate from 157 N to 471 N. Applying 1 N·m at a 22 mm effective grip
radius takes about 45 N tangential hand force. Neither these estimates nor a
smooth unloaded CAD motion establish comfortable operation or a permitted load.

The native acceptance tests exercise real MCP replay, dimension and thread
edits, guide capture without ideal joints, assembly paths, motion with mounting
hardware, independent rebuilds, save/reopen, native drawing exports and closed,
positive print meshes. The full candidate test passed in 1,902.17 seconds. A
subsequent label-placement correction passed focused native/MCP checks and
visual review of all seven sheets; all 14 corrected SVG/DXF exports repeated
exactly without changing the saved model. Results and source/artifact hashes are in the
[validation record](manufacturing/d-screw-vise.validation.json). Retain artifacts
by setting `LIMO_CAD_RECIPE_ARTIFACT_DIR` before running
`cargo test --manifest-path mcp-server/Cargo.toml --test recipes d_screw_vise`.

Physical qualification must measure turning effort, backlash, off-centre jaw
movement, retention, deformation during sustained clamping, and wear after
repeated cycles. Record the actual print process and hardware. CNC and molded
variants preserve the function and load path but require different tool access,
corner radii, draft, wall sections and process allowances.

The reusable engineering guidance is in
[additive workholding](../knowledge/concepts/additive-workholding.md) and
[gear pairs](../knowledge/concepts/gears.md), available to agents through the
standard MCP knowledge resources.
