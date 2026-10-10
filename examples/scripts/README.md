# Bundled script examples

The separate human UI survey uses saved fixtures under
[`examples/checkpoints`](../checkpoints), without replaying these plans.
[`surface-ui-rigid-joint-human-ui.limo`](../checkpoints/surface-ui-rigid-joint-human-ui.limo)
retains two stock definitions and a rigid face-to-face joint after retained
editing, Undo/Redo, Cancel and exact UI save/reopen. Its acceptance scope and
remaining joint modes are recorded in the
[surface coverage ledger](../../docs/qualification/surface-coverage.json).

[`surface-ui-line-chain-human-ui.limo`](../checkpoints/surface-ui-line-chain-human-ui.limo)
retains a fully constrained 25 by 10 mm triangular sketch made through the Line
tool, including numeric validation, retained dimension edit, Cancel, Undo/Redo
and exact UI reopen. Its active-tool dimension text has a recorded polish defect;
the committed color fix still needs deployed qualification.

[`surface-ui-three-point-arc-human-ui.limo`](../checkpoints/surface-ui-three-point-arc-human-ui.limo)
retains a three-point arc after endpoint editing, creation/edit Undo/Redo,
Cancel and exact UI save/reopen. Collinear picks are safely rejected, but the
runtime's misleading validation message still needs the committed fix deployed.

[`surface-ui-center-arc-human-ui.limo`](../checkpoints/surface-ui-center-arc-human-ui.limo)
retains a center-point arc after safe zero-radius rejection, draft cancellation,
endpoint dragging, Undo/Redo and exact UI save/reopen. Active toolbar feedback
and stale validation status have separate recorded polish defects.

[`surface-ui-fit-spline-human-ui.limo`](../checkpoints/surface-ui-fit-spline-human-ui.limo)
retains an open four-point spline created with Finish Spline, then translated,
cancelled, undone/redone and exactly reopened through the native UI. The
advertised Enter completion route and completion-button layout have recorded
interaction/polish failures; individual fit-point editing is not qualified.

[`surface-ui-two-point-rectangle-human-ui.limo`](../checkpoints/surface-ui-two-point-rectangle-human-ui.limo)
retains a fully constrained 30 by 20 mm rectangle after size validation, typed
width/height entry, retained dimension edit, Cancel, Undo/Redo and exact UI reopen.
The existing dimension-ink repair also needs a deployed rectangle recheck.

[`surface-ui-center-rectangle-human-ui.limo`](../checkpoints/surface-ui-center-rectangle-human-ui.limo)
retains a fully constrained 20 by 12 mm origin-centered rectangle after zero-height
validation, typed sizes, retained height edit, Cancel, Undo/Redo and exact native
UI reopen. Shared dimension readability and active-variant toolbar repairs still
need deployment and presentation rechecks.

[`surface-ui-center-circle-human-ui.limo`](../checkpoints/surface-ui-center-circle-human-ui.limo)
retains a fully constrained 24 mm origin-centered circle after zero-diameter
validation, typed diameter, retained edit, Cancel, Undo/Redo and exact native
UI reopen. Active construction still exposes the shared dimension-ink defect.

[`surface-ui-two-point-circle-human-ui.limo`](../checkpoints/surface-ui-two-point-circle-human-ui.limo)
retains a 16 mm circle centered at 10,0 after two-endpoint diameter creation,
zero validation, retained diameter edit, Cancel, Undo/Redo and exact native reopen.
Its two positional DOF remain free. Active variant and dimension ink have recorded
presentation failures covered by committed, undeployed repairs.

[`surface-ui-center-to-center-slot-human-ui.limo`](../checkpoints/surface-ui-center-to-center-slot-human-ui.limo)
retains a tangent capsule after center-axis picks, zero-width rejection, typed
10 mm width, retained 12 mm width edit, Cancel, Undo/Redo and exact native reopen.
Its four DOF leave length and placement free. Active dimension ink still needs
the shared deployed repair; the other slot modes remain separate.

[`surface-ui-overall-slot-human-ui.limo`](../checkpoints/surface-ui-overall-slot-human-ui.limo)
retains the outside-endpoint slot after too-wide rejection/recovery, valid 10 mm
width, retained 12 mm width edit, Cancel, Undo/Redo and exact native reopen.
Its four DOF remain free. The misleading rejection message is repaired in source;
deployed message and shared dimension-ink rechecks remain pending.

[`surface-ui-center-point-slot-human-ui.limo`](../checkpoints/surface-ui-center-point-slot-human-ui.limo)
retains the initially symmetric center/end capsule after zero-width rejection,
typed 10 mm width, retained 12 mm width edit, Cancel, Undo/Redo and exact native
reopen. Four DOF remain free; the shared dimension-ink recheck awaits deployment.

[`surface-ui-inscribed-polygon-human-ui.limo`](../checkpoints/surface-ui-inscribed-polygon-human-ui.limo)
retains the six-edge polygon after side-count/radius validation, form Cancel,
free vertex edit, Undo/Redo and exact native reopen. It has twelve DOF and no
regularity constraints. The committed radius-message repair awaits deployment.

[`surface-ui-circumscribed-polygon-human-ui.limo`](../checkpoints/surface-ui-circumscribed-polygon-human-ui.limo)
retains the four-edge polygon after circumscribed radius/rotation creation,
side-count validation, form Cancel, free vertex edit, Undo/Redo and exact native
reopen. Original edges lie 10 mm from center; eight DOF remain unconstrained.
The shared radius-message dependency needs only a deployed validation recheck.

[`surface-ui-midpoint-line-human-ui.limo`](../checkpoints/surface-ui-midpoint-line-human-ui.limo)
retains an origin-centered horizontal line after coincident rejection, draft
Cancel, endpoint resize, Undo/Redo and exact native reopen. Its midpoint and
orientation remain constrained with one length DOF. An extra unchanged drag
history step and active toolbar identity have committed, undeployed repairs.

[`surface-ui-point-human-ui.limo`](../checkpoints/surface-ui-point-human-ui.limo)
retains a free point after pointer creation, exact preview Cancel, retained drag,
creation/edit Undo/Redo and exact native reopen. Two DOF remain free. The shared
unchanged-drag history dependency requires a deployed recheck.

[`surface-ui-sketch-move-copy-human-ui.limo`](../checkpoints/surface-ui-sketch-move-copy-human-ui.limo)
retains two free lines after signed Move/Create copy, selection removal, invalid
input, exact Cancel, Undo/Redo and native reopen. Adjacent-field Tab focus has a
committed repair awaiting deployment; geometry and persistence passed.

[`surface-ui-sketch-scale-human-ui.limo`](../checkpoints/surface-ui-sketch-scale-human-ui.limo)
retains a free line after nonzero-origin enlargement, negative-factor scaling,
zero validation, exact Cancel, Undo/Redo and native reopen. Four DOF remain free;
only the shared adjacent-field Tab dependency awaits deployed qualification.

[`surface-ui-sketch-offset-human-ui.limo`](../checkpoints/surface-ui-sketch-offset-human-ui.limo)
retains concentric radii 10/14/8 after outward/inward Offset, validation, exact
Cancel, retained gap edit, Undo/Redo and native reopen. Geometry passes with two
DOF; collapse feedback, adjacent labels and active-form ink await deployed fixes.

[`surface-ui-sketch-trim-human-ui.limo`](../checkpoints/surface-ui-sketch-trim-human-ui.limo)
retains a manually trimmed line and independent crossing boundary after
preview/Cancel, validation, endpoint edit, Undo/Redo and native reopen.
The separate reopened-document modeling footer awaits live requalification.

[`surface-ui-sketch-extend-human-ui.limo`](../checkpoints/surface-ui-sketch-extend-human-ui.limo)
retains a line extended to its independent vertical boundary, including a
point-on-line constraint, endpoint edit, Undo/Redo and exact native reopen.

[`surface-ui-sketch-break-human-ui.limo`](../checkpoints/surface-ui-sketch-break-human-ui.limo)
retains two horizontal line pieces sharing an edited point after menu Break,
Cancel/validation, Undo/Redo and exact native reopen.

[`surface-ui-sketch-fillet-human-ui.limo`](../checkpoints/surface-ui-sketch-fillet-human-ui.limo)
retains the radius5 tangent corner after validation/Cancel, creation history
and exact native reopen. Retained radius3/7 edits failed safely on the tested
build; their source repair and clearer zero feedback await deployed acceptance.

[`surface-ui-sketch-chamfer-human-ui.limo`](../checkpoints/surface-ui-sketch-chamfer-human-ui.limo)
retains the normal equal5 chamfer after zero/Cancel, exact history and native
reopen. Its5-to7 retained edit caused severe free-carrier drift, preserved in
external receipts; the saved baseline supports rechecking that failure.

[`surface-ui-sketch-mirror-human-ui.limo`](../checkpoints/surface-ui-sketch-mirror-human-ui.limo)
retains a reflected independent line and its edited endpoint after missing-axis
validation/Cancel, exact history and native save/reopen. Source and axis stay
unchanged; curve/oblique/multiple-source variants remain unqualified.

[`surface-ui-sketch-rectangular-pattern-human-ui.limo`](../checkpoints/surface-ui-sketch-rectangular-pattern-human-ui.limo)
retains a3x2 array with20mm spacings and one independently edited copied
endpoint. Validation/Cancel/history/native reopen passed; keyboard traversal
awaits the shared focus fix.

[`surface-ui-sketch-circular-pattern-human-ui.limo`](../checkpoints/surface-ui-sketch-circular-pattern-human-ui.limo)
retains a clockwise partial circle pattern with an independently edited center.
Full-turn and partial placements, validation/Cancel/history and native reopen
passed; keyboard traversal awaits the shared focus fix.

[`surface-ui-sketch-coincident-human-ui.limo`](../checkpoints/surface-ui-sketch-coincident-human-ui.limo)
retains two coincident points after shared drag. Selection/Cancel, inspector
delete/restore, history and native reopen passed. Point-only camera framing
and degenerate Fit await the committed camera repair.

[`surface-ui-sketch-midpoint-human-ui.limo`](../checkpoints/surface-ui-sketch-midpoint-human-ui.limo)
retains a midpoint on a translated horizontal40mm line. Relation, Cancel,
retained inspector/history and native persistence passed. Re-edit framing
hides the right endpoint; camera acceptance remains pending.

[`surface-ui-sketch-collinear-human-ui.limo`](../checkpoints/surface-ui-sketch-collinear-human-ui.limo)
retains correctly aligned free lines. Wrong-type selection/Cancel, readable
inspector, relation history and native persistence passed. Endpoint dragging
failed silently; saved reproduction stays available for the repair queue.

[`surface-ui-sketch-horizontal-vertical-human-ui.limo`](../checkpoints/surface-ui-sketch-horizontal-vertical-human-ui.limo)
retains independently edited Horizontal and Vertical lines. Closest-axis
creation, Cancel/duplicate guidance, inspectors, exact history and native
persistence/re-edit passed. Shared future camera framing acceptance is pending.

Recipes are bundled construction scripts in the same Scripts feature as your
own `.limo.jsonc` files. Open **Scripts → Browse examples** in CAD, select an
example to inspect its source, then choose **Run in new design** in the source
editor. Edit and **Validate source** before running if you want to change it.
Start with **Sketch, extrude, ease the edges**: it builds a small part,
changes its extrusion from 12 to 18 mm while retaining the fillet, and restores
the 12 mm reference. Follow [the first-part guide](../../docs/INSTALL.md#make-your-first-part)
to edit, save and reopen the result yourself.

Maximum rate builds the same model without presentation waits. Pause and Step
let you inspect the sequence; paced presentation adds chapter notes and camera
moves. All modes use the same Rust interpreter and construction source.

- [Sketch, extrude, ease the edges](fillet-basics.limo.jsonc): a first-part lesson
  for Solid → Refine → Fillet. Builds a fully located 60 × 30 mm sketch, extrudes
  12 mm of stock and rounds the four top edges by 2 mm. Demonstrates an 18 mm
  extrusion edit and restoration, with captioned preview frames and final checks.
- [Four-hole mounting plate](mounting-plate.limo.jsonc): fully located 60 × 40 × 5 mm
  stock, four 5 mm through bores and face-basis projection after each cut.
- [Component edit and recovery](component-edit-recovery.limo.jsonc): edit a
  translated, rotated repeat in place, undo an incorrect driving value and
  update both shared occurrences to 30 × 10 × 3 mm. Includes paced chapters,
  camera focus, captioned previews, analytic checks and editable save/reopen.
- [Revolved annular spacer](revolved-spacer.limo.jsonc): a located radial section,
  20 mm outside diameter, 10 mm bore and 12 mm length, revolved about Y.
- [Dimensioned angle bracket](angle-bracket.limo.jsonc): a closed, fully constrained
  L section with 40 × 30 mm envelope, 5 mm walls and 20 mm extrusion width.
- [Repeated bracket assembly](repeated-bracket-assembly.limo.jsonc): three native
  part definitions, four occurrences and explicit mating frames. Editing one
  bracket extrusion from 20 to 25 mm updates both of its occurrences. This is a
  component/placement lesson, not a fastened or manufacturing-qualified assembly.
- [Crown garden bench](garden-bench.limo.jsonc): a complete timber assembly from
  dimensioned sketches, native features and physical mating references. Includes
  authored captions, camera framing and final manufacturing contracts.
- [Vertical-axis turbine](vertical-axis-turbine.limo.jsonc): two reused Savonius
  stages, constrained 72:18 generator gearing, native part and assembly drawings,
  printable definition exports and explicit physical qualification inputs.
- [D-screw vise](d-screw-vise.limo.jsonc): six printed parts, 100 mm gripping
  faces and 90 mm captured-jaw travel. Its custom rounded Ø24 × 4 mm screw has a
  shallow print flat and detachable thrust fitting. Includes seven drawing
  sheets, a print layout for each part and simplified M5/M6 hardware envelopes;
  30 bodies including optional mounting hardware.
- [D-screw fit coupon](d-screw-vise-fit.limo.jsonc): four specimens for the actual
  print process: the custom rounded screw, matching female thread and a
  male/female captured-guide pair. Qualify their fit before making a full vise.
- [Turbine fit coupons](turbine-fit-coupons.limo.jsonc): four native specimens
  reuse the shaft clamp, bearing seat, motor cradle and pinion geometry, with
  driving fit dimensions, print orientations and associative drawings.
- [Version 1 editor schema](limo-cad-script.schema.json): step and expression guidance
  for JSONC-aware editors. The Rust interpreter validates references, and the
  shared interface owns each modeling operation’s argument schema.

For hands-on UI and typed MCP work, open the editable
[garden bench checkpoint](../checkpoints/garden-bench.limo) with **File → Open**.
It was rebuilt through published UI controls and typed MCP operations on matching
Bevy GUI and MCP builds at `abf4667d`. This checkpoint has 53 named features,
25 fully constrained sketches, 18 body definitions and 32 placed occurrences.
It reaches the continuous arm supports; it is not the complete bench recipe.

The separate [human-operated checkpoint](../checkpoints/garden-bench-human-ui.limo)
records the Windows UI walkthrough through `0db4c009` on 7 October 2026.
It contains eighteen named features, four separately defined and placed posts,
two shared long-apron occurrences, two shared upper side rails, two left-front apron pilots,
six right-front rail/arm pilots, six shared apron clearance positions,
four shared upper-rail clearance positions, a separate lower-rail stock and five
presentation views. The three-model walkthrough is still in progress.
Every modeling change used real mouse/keyboard input through
the optional Rust `native-computer-control` feature, with no recipe replay or
direct MCP modeling commands. Read-only MCP inspection checked the resulting
geometry and references.

1. Create an XY sketch and anchor a 65 × 65 mm rectangle at the origin, with
   driving width/height dimensions. Finish the sketch and extrude 630 mm.
2. Rename the sketch **Front leg and arm post / stock from A-B-C** and the
   extrusion **Front leg and arm post / stock from A-B-C / Stock 630 mm**.
   Undo/Redo the sketch rename; its extrusion reference and feature IDs survive.
3. Apply **Deep green** (`#41544D`) and **Painted timber (visual designation)**.
   These are visual metadata, not structural material qualification.
4. Make the reusable **Front leg and arm post** component, name its occurrence,
   place it at `[65, 30, 0]` with identity rotation and ground it.
5. Save through the native picker, close and reopen the document. Hide the
   finished sketch and save **01 / Front post stock** as a presentation view.
   Undo removes the view, Redo restores it, and Recall restores its camera.
   A temporary occurrence offset previews separately from mechanical placement;
   Escape removes the offset and restores the prior presentation.
6. Create the separate **Right front leg and arm post / stock from A-B-C** XY
   sketch. Constrain its rectangle to 65 × 65 mm at the origin, then finish and
   create **Right front leg and arm post / Stock 630 mm**, with zero taper.
7. Make and name the reusable **Right front leg and arm post** component and
   its occurrence. Place it at `[1070, 30, 0]` with identity rotation. Apply the
   same painted-timber visual metadata and deep-green color to both posts.
8. On the translated left post's front face, create **Front post / Apron pilot
   Ø3.5 x 39 / Z350** at source-local U = 32.5 mm and V = 350 mm. Use a simple
   3.5 mm hole, 39 mm blind depth and flat bottom. Undo/Redo removes and restores
   the cut without changing component placement or grounding.
9. Save **02 / Front posts and first apron pilot** with both bodies visible,
   then save the document. Recall the first view to isolate the original post;
   the second view shows both placed stocks.
10. Repeat the fully constrained 65 × 65 mm XY stock for the separate left/right
    rear posts, extruding 790 mm. Name both sketches, extrusions, definitions
    and occurrences. Place them at `[65, 380, 0]` and `[1070, 380, 0]`, with
    identity rotations and the same deep-green visual metadata.
11. Duplicate and rotate the right-front occurrence temporarily. Create a
    source-local pilot through the placed occurrence and verify that both
    shared instances update. Remove the temporary instance; Undo restores its
    exact ID, visibility and pose, and Redo removes it without deleting the
    definition or its geometry. Restore the retained post to identity rotation.
12. Edit the left-front apron feature to add U = 32.5 mm, V = 390 mm while
    retaining V = 350 mm. A blank added position blocks Apply; removing that
    blank preserves both completed rows. Apply and Undo/Redo preserve the ten
    feature IDs and four component placements.
13. Correct the right-front pilot support to its outward +X face, with source
    origin `[65, 0, 0]`, U along +Y and V along +Z. Set U = 32.5 mm and V =
    365, 400, 170, 205, 590 and 615 mm in one named rail/arm pilot feature.
    Keep the simple 3.5 mm diameter, 39 mm blind depth and flat bottom.
14. Save **03 / Four post stocks and handed pilots** with all four bodies visible,
    then save the project through the native picker. Named views retain camera
    and visibility, so recalling an earlier chapter shows its bodies' current
    geometry; it does not roll back the feature history.
15. Create a fully constrained 1070 × 28 mm XY rectangle and extrude 155 mm
    as **Long apron — face lap / stock from A-B-C / Stock 155 mm**. Apply the
    same painted-timber metadata, make one reusable definition and place its
    front/rear occurrences at `[65, 2, 260]` and `[65, 445, 260]`, with identity
    rotations. Their tops are at Z = 415 mm. Save **04 / Front and rear face-lap
    aprons** with all five bodies visible, then save the document.
16. Pick the front apron's outward −Y face. Create **Long apron / Post and
    bearer clearances Ø5.5 x 28 / Six positions** with U/V pairs `(32.5, 90)`,
    `(32.5, 130)`, `(1037.5, 90)`, `(1037.5, 130)`, `(517.5, 35)` and
    `(552.5, 35)`. Set simple style, 5.5 mm diameter, 28 mm distance, flat
    bottom and no flip. Both shared apron occurrences update. Undo removes
    this feature without changing their IDs or poses; Redo restores the six
    positions and name. Save the document.
17. Create an origin-anchored XY rectangle, 28 × 415 mm, with driving dimensions
    and zero remaining degrees of freedom. Name it **Upper side rail / stock
    from A-B-C** and create **Upper side rail / Stock 90 mm**, a separate body
    extruded 90 mm along +Z with zero taper. Apply the same painted-timber
    appearance. Make one reusable **Upper side rail** definition; name its two
    instances **Left upper side rail** and **Right upper side rail**, placed at
    `[37, 30, 325]` and `[1135, 30, 325]` with identity rotations. Hide the source
    sketch and save. The assembly now has six definitions and eight occurrences.
18. Pick an upper rail's outward −X face and create **Upper side rail / Post
    clearances Ø5.5 x 28 / Four positions**. Its displayed basis has origin at
    local Y = 415 mm, U along −Y and V along +Z. Enter `(32.5, 40)`, `(32.5, 75)`,
    `(382.5, 40)` and `(382.5, 75)`, with 5.5 mm diameter, 28 mm distance, simple
    style, flat bottom and no flip. Both placed rails receive the four bores.
    Review from Left and Right, with a close-up. Undo/Redo preserves their IDs
    and poses and restores the feature name and positions. Save **05 / Upper
    side rails and post clearances** with all six bodies visible, then save.
19. On matched clean GUI/MCP build `0db4c009`, open this checkpoint in a separate
    tab while preserving other open documents. Create **Lower side rail / stock
    from A-B-C** on XY, with an origin-coincident 28 × 415 mm rectangle and two
    driving dimensions. Read-only inspection confirms zero degrees of freedom.
    Create a separate body with **Lower side rail / Stock 90 mm**, using Create
    Body, 90 mm distance, zero taper and no flip. Rename both features and save
    through the physical Ctrl+S shortcut. The saved project has eighteen features
    and seven bodies; the lower stock is still at its source origin and has no
    bores, appearance or component definition. Its reusable definition must remain
    separate from the upper rail.
20. On clean matched `7c235990`, isolate the lower stock, select its outward −X
    face and create **Lower side rail / Post clearances Ø5.5 x 28 / Four positions**
    through the Hole dialog. Enter `(32.5, 40)`, `(32.5, 75)`, `(382.5, 40)` and
    `(382.5, 75)`, committing numeric fields with Tab. Set simple style, 5.5 mm
    diameter, 28 mm distance, flat bottom and no flip. Physical Undo removes the
    bores; Redo restores their feature identity, positions and name. Save with
    Ctrl+S. The new [lower-clearance checkpoint](../checkpoints/garden-bench-lower-clearances-human-ui.limo)
    has nineteen features, seven bodies, six definitions and eight occurrences;
    its live model matches the archive. The original stock checkpoint remains
    preserved. Lower-rail appearance, its separate definition and placements,
    remaining pilots, joints, drawing sheets and print layouts remain unfinished.
21. Reopen the lower-clearance checkpoint through the native Open dialog. On
    clean matched `d2ba9017`, select Body7 and apply the existing upper rail's
    appearance: `#41544D`, **Deep green**, **Painted timber (visual designation)**,
    family **Painted timber**, brand **Generic**. In Assembly, use **Make component**
    and name the separate reusable definition **Lower side rail** and its instance
    **Left lower side rail**. Save with Ctrl+S. The preserved
    [lower-component checkpoint](../checkpoints/garden-bench-lower-component-human-ui.limo)
    has seven definitions and nine occurrences, and its archive matches the live
    model. The new instance is still at the source origin; its placement and
    shared right instance remain unfinished.
22. On clean matched `19ca11c7`, set the left lower instance's placement to
    `(37, 30, 130)` mm with zero rotation, apply and save. Duplicate that instance
    in Assembly, select the duplicate, rename it **Right lower side rail**, and
    apply `(1135, 30, 130)` mm with zero rotation. Save with Ctrl+S and use Fit.
    The [lower-placement checkpoint](../checkpoints/garden-bench-lower-placements-human-ui.limo)
    has nineteen features, seven definitions and ten occurrences. Both lower
    instances share definition 14 and all four bores; the upper rails retain
    their separate definition 12. The saved model matches the live document.
    Top pilots, remaining post pilots, geometry, joints and drawing/layout work
    remain unfinished.

23. On clean matched `08efc3e7`, select the lower rail's top face through the
    viewport and add the two stretcher pilots through Hole. The face basis is
    centred at `(14, 207.5, 90)` mm with U along X and V along Y: enter U/V
    `(0, -12.5)` and `(0, 27.5)` for stock points `(14, 195, 90)` and
    `(14, 235, 90)`. Use Ø3.5 mm, Distance 32 mm, Simple, Flat bottom, no flip.
    Name the feature **Lower side rail / Stretcher top pilots Ø3.5 x 32 / Two positions**
    and save. The [lower-pilot checkpoint](../checkpoints/garden-bench-lower-pilots-human-ui.limo)
    has twenty features; both lower occurrences retain their placements and
    share the new bores. Read-only archive inspection confirms every parameter
    and an exact live/saved match. Upper-rail top pilots and remaining post
    pilots, geometry, joints and drawing/layout work remain unfinished.
24. On clean matched `35722a50`, isolate the upper stock and select its top face
    through the viewport. Add five Ø3.5 mm, 32 mm deep, simple, flat-bottom pilots
    with no flip at U/V `(0, -195)`, `(0, -105)`, `(0, -15)`, `(0, 75)`, `(0, 165)`.
    Its centred face basis is the same as the lower stock; these are stock points
    X 14 mm, Y 12.5/102.5/192.5/282.5/372.5 mm, Z 90 mm. Name the feature
    **Upper side rail / Seat slat top pilots Ø3.5 x 32 / Five positions**.
    Restore all seven bodies through their Browser eyes, use Isometric/Fit and
    save. The [rail-pilot checkpoint](../checkpoints/garden-bench-rail-pilots-human-ui.limo)
    has twenty-one features, seven definitions and ten occurrences. The upper
    and lower definitions remain separate; their respective shared pairs show
    five seat-slat and two stretcher pilots. Saved parameters and exact live/archive
    comparison passed. Remaining post pilots, geometry, joints and drawings
    remain unfinished.
25. On clean matched `0226ec9f`, isolate Body1 and select its outward −X face in
    Left view. Its basis is origin `(0, 65, 0)`, U `(0, -1, 0)`, V `(0, 0, 1)`.
    Add six simple Ø3.5 mm, 39 mm deep, flat-bottom pilots with no flip at U 32.5,
    V 365/400/170/205/590/615 mm. Name the feature **Left front post / Rail and
    arm pilots Ø3.5 x 39 / Six positions**. Restore all seven Browser eyes,
    use Isometric/Fit and save. The
    [left-front pilot checkpoint](../checkpoints/garden-bench-left-front-pilots-human-ui.limo)
    has twenty-two features with the previous hole patterns and assembly intact;
    exact live/archive comparison passed. Right-front apron and both rear-post
    pilot patterns, remaining geometry, joints and drawings remain unfinished.
26. On clean matched `cb464867`, isolate Body2 and select its outward −Y face in
    Front view. Its basis is origin `(0, 0, 0)`, U `(1, 0, 0)`, V `(0, 0, 1)`.
    Add two simple Ø3.5 mm, 39 mm deep, flat-bottom pilots with no flip at U 32.5,
    V 350/390 mm. Name the feature **Right front post / Apron pilots Ø3.5 x 39 /
    Z350 and Z390**. Restore all seven Browser eyes, use Isometric/Fit and save.
    The [front pilot checkpoint](../checkpoints/garden-bench-front-pilots-human-ui.limo)
    has twenty-three features, seven definitions and ten occurrences, with both
    front-post patterns complete. Saved parameters and exact live/archive
    comparison passed. Both rear-post patterns, remaining geometry, joints and
    drawings remain unfinished.

27. On clean matched `fe195308`, isolate Body3 and select its outward −Y face in
    Front view. The basis is origin `(0, 0, 0)`, U `(1, 0, 0)`, V `(0, 0, 1)`.
    Add two simple Ø3.5 mm, 39 mm deep, flat-bottom pilots with no flip at U 32.5,
    V 350/390 mm. Name the feature **Left rear post / Apron pilots Ø3.5 x 39 /
    Z350 and Z390**. Restore all seven Browser eyes, use Isometric/Fit and save.
    The [left-rear apron checkpoint](../checkpoints/garden-bench-left-rear-apron-human-ui.limo)
    has twenty-four features, seven definitions and ten occurrences. Saved hole
    parameters and exact live/archive comparison passed. Rear rail/arm pilots,
    right-rear apron pilots, remaining geometry, joints and drawings remain open.

28. On clean matched `0528a7db`, isolate Body4 and select its outward −Y face in
    Front view. Its basis is origin `(0, 0, 0)`, U `(1, 0, 0)`, V `(0, 0, 1)`.
    Add two simple Ø3.5 mm, 39 mm deep, flat-bottom pilots with no flip at U 32.5,
    V 350/390 mm. Name the feature **Right rear post / Apron pilots Ø3.5 x 39 /
    Z350 and Z390**. Restore all seven Browser eyes, use Isometric/Fit and save.
    The [rear apron checkpoint](../checkpoints/garden-bench-rear-aprons-human-ui.limo)
    has twenty-five features, seven definitions and ten occurrences. Saved hole
    parameters and exact live/archive comparison passed. Both rear rail/arm pilot
    patterns, remaining geometry, joints and drawings remain unfinished.

29. On clean matched `e7e0a775`, isolate Body3 and select its outward −X face in
    Left view. Its basis is origin `(0, 65, 0)`, U `(0, −1, 0)`, V `(0, 0, 1)`.
    Add six simple Ø3.5 mm, 39 mm deep, flat-bottom pilots with no flip at U 32.5,
    V 365/400/170/205/590/615 mm. Name the feature **Left rear post / Rail and arm
    pilots Ø3.5 x 39 / Six positions**. Restore all seven Browser eyes and save.
    The [left-rear pilot checkpoint](../checkpoints/garden-bench-left-rear-pilots-human-ui.limo)
    has twenty-six features, seven definitions and ten occurrences. Saved hole
    parameters and exact live/archive comparison passed. Right-rear rail/arm
    pilots, remaining geometry, joints and drawings remain unfinished.

30. On clean matched `7a51d2e0`, isolate Body4 and select its outward +X face in
    Right view. Its basis is origin `(65, 0, 0)`, U `(0, 1, 0)`, V `(0, 0, 1)`.
    Add six simple Ø3.5 mm, 39 mm deep, flat-bottom pilots with no flip at U 32.5,
    V 365/400/170/205/590/615 mm. Name the feature **Right rear post / Rail and arm
    pilots Ø3.5 x 39 / Six positions**. Restore all seven Browser eyes and save.
    The [post-pilot checkpoint](../checkpoints/garden-bench-post-pilots-human-ui.limo)
    has twenty-seven features, seven definitions, ten occurrences and five named
    views. Saved hole parameters and exact live/archive comparison passed. All
    four post pilot patterns are complete; remaining geometry, joints, drawings
    and print layouts remain unfinished.

31. On clean matched `fc275793`, hide the seven stock bodies, create a sketch on
    the XY origin plane and draw a Rectangle from the origin. Type width 28 mm
    and height 415 mm, then finish the sketch. Its saved origin coincidence,
    horizontal/vertical edges and driving dimensions retain the exact profile.
    Select the closed profile in Extrude and create an independent body at
    distance 90 mm, zero taper and no flip. Name the sketch **Center seat bearer /
    stock from A-B-C** and the extrusion **Center seat bearer / stock from A-B-C /
    Stock 90 mm**. Hide its finished sketch, restore all eight Browser eyes and
    save. The [center-stock checkpoint](../checkpoints/garden-bench-center-stock-human-ui.limo)
    has twenty-nine features, eight definitions, eleven occurrences and five
    named views; exact live/archive comparison passed. Its appearance, placement
    and support blocks remain unfinished.

32. On clean matched `d515b39a`, select Body8 and set its appearance to
    **Oiled timber (visual designation)**, **Honey timber**, RGB 187/126/68,
    with an empty material family. In Assembly, Make component absorbs the
    promoted source. Name the reusable definition and its instance **Center
    seat bearer**, then apply instance translation 586/30/325 mm and zero
    rotation. The actual viewport shows the timber-colored bearer crossing
    the frame. Save the [center-bearer checkpoint](../checkpoints/garden-bench-center-bearer-human-ui.limo);
    exact live/archive comparison passed, with twenty-nine features, eight
    definitions, eleven occurrences, five named views, zero scene errors and
    zero display warnings. Both appearance Apply and Make component had failed
    on the prior runtime with **Native interface transition is pending**. The
    new dispatch fence permits only a footer repaint, retaining every owner,
    revision, control, layout and modal guard; both physical retries passed.
    Support blocks and the remaining geometry, joints, drawings and print
    layouts remain unfinished.

33. On the same matched `d515b39a` runtime, isolate the nine-body working area
    and create the front support block through an XY-origin Rectangle, width
    65 mm and height 28 mm. Its saved origin coincidence, four horizontal/vertical
    constraints and two driving dimensions retain the exact profile. Extrude
    an independent body 65 mm, with zero taper and no flip. Name the sketch
    **Front center bearer support block / stock from A-B-C** and its extrusion
    **Front center bearer support block / stock from A-B-C / Stock 65 mm**.
    Hide the finished sketch, restore all nine bodies and save the
    [front support stock checkpoint](../checkpoints/garden-bench-front-support-stock-human-ui.limo).
    Exact live/archive comparison passed; it has thirty-one features, nine
    definitions and twelve occurrences. The front block's appearance and
    placement, rear block and remaining construction are unfinished.

Use the saved **Named Views** for a demonstration:

1. Recall **01 / Front frame assembly**, **03 / Seat support frame** and
   **06 / Seat deck before fastening** to explain the frame and post clearances.
2. Compare **07 / Back rails before pickets** with **08 / Shared back rails lifted
   for joinery review**. Both rails use one part definition.
3. Recall **09 / Crowned back with nine shared pickets**, then **11 / Center picket
   lifted for slot review**. The 18 mm slot is centered on a real mid-thickness
   datum; the exploded presentation leaves mechanical placement unchanged.
4. Return to **10 / Continuous arm supports before armrests** to continue with
   the handed armrests, fastening, joints and drawing package in the design plan.

Build the next steps through UI controls or literal typed MCP operations.
[`cad_route` with `action: "batch"`](../../docs/mcp-harness.md#choose-the-document-owner)
groups up to 16 ordered calls; inspect each receipt and return to the agent loop
for new IDs. Follow `active_session_id` after
Undo, Redo or document replacement, and inspect `build_pair.status` before
qualifying a live GUI/MCP pair.

For command-line replay, use the [developer guide](../../docs/DEVELOPMENT.md#replay-a-recipe).
It covers packaged CAD (`--server-arg --headless`), standalone servers and AppImage arguments.
MCP lists the same collection with `cad_interface {"action":"recipes"}` and runs
one with `{"action":"script","recipe":"mounting-plate","mode":"fast"}`.
The app's example list is supplied by that same Rust catalog in `crates/recipes`.
Catalog discovery does not build anything. A caller must choose a file or recipe;
there is no implicit bench run.
Use `--repeat 2` for independent headless comparison. After preserving the current
document, use `--session UUID --new --present --speed 2` to create a blank design tab
and watch the same sequence in that existing window. Omit `--new` if the named tab
is already blank. The script refuses to construct over an existing model.

See [the native script format](../../docs/native-scripts.md). Each `.limo.jsonc` source
replays construction; the generated `.limo` project retains the editable result.

The bench, turbine and vise are all runnable manufacturing candidates. The two
new sources include their editable drawing packages and teaching notes. Their
native tests check independent replay, model reload, intended dimension edits,
solved motion, interference and printable mesh integrity. See
[flagship status](../../docs/flagship-examples.md) and the individual design docs
for evidence and remaining physical qualification. The bench's full drawing
package remains open, and no flagship has a physical load or durability rating.

The migrated small fixtures preserve #89's analytic bounds/volume, independent
replay, fresh-process native restore, STEP round trip and STL/3MF checks in
`mcp-server/tests/recipes.rs`. The repeated bracket check restores both the 20 mm
baseline and 25 mm edited project, checks solved occurrence poses and verifies
all three profiles retain zero degrees of freedom. Export is a tested derivative;
native sketches and features construct the source model. The old Node recipe
runner and custom argument substitutions are not part of this library.

To add a feature lesson, commit its `.limo.jsonc`, add one catalog entry, and add
the focused geometry/edit check that establishes the feature's intent. Titles,
chapters, step counts and the actual operation list come from the source. Declare
the operation the lesson teaches; do not advertise every incidental setup command
as a hover lesson. Use existing native CI and replay tests, without an all-tools
percentage gate or an additional runner.

- [Compose box from collection](compose-box.limo.jsonc): tiny root script that
  `includes` [collections/box-stock.collection.jsonc](collections/box-stock.collection.jsonc).
  Demonstrates part-file isolation; run with an absolute `path` and `mode: "fast"`.

