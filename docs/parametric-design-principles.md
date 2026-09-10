# Reliable native CAD examples

A model is successful when it preserves design intent, can be edited after reopening,
and describes parts that can actually be made and assembled. A clean recompute alone
is insufficient. Apply these principles while designing, then choose a few executable
checks that would catch the most consequential mistakes.

## Start with use and construction

Define the user-facing side, load path, stock sizes, grain axes, tolerances, tools,
and assembly order before decorative features. Explain how each member is supported
and retained. Prefer bearing surfaces and accessible side-grain fastening over
unexplained butt contacts. Keep a cut list and fastener schedule linked to the model.

The USDA Forest Products Laboratory's [Wood Handbook, chapter 8](https://research.fs.usda.gov/download/treesearch/62253.pdf)
explains why fastening design depends on grain direction and moisture changes, and
why screw behavior in end grain differs from side grain. This is the basis for our
side-grain checks, not a load rating or universal screw-sizing rule. Match pilot size,
edge distance, fastener material and head geometry to the selected timber and actual
fastener manufacturer's instructions before fabrication. Wood movement needs space;
a nominal gap is a design input, not proof that every species and climate will fit.

The published [Kreg cedar bench construction sequence](https://learn.kregtool.com/plans/diy-cedar-garden-bench/)
provides a useful example of documenting stock, machining, workholding, support
members and ordered assembly. Our crown bench is an original design, not a copy of
that model or its pocket-hole joint system.

## Build editability into the history

Use an origin-based stock sketch with driving dimensions. Locate geometry explicitly:
fully constrain position and orientation as well as size. Disable interactive snapping
when requesting exact coordinates through MCP. Fix a locating datum when necessary;
do not freeze the entire sketch to obtain a misleading zero-DOF result.

Use separate component definitions when machining differs, even if blank dimensions
match. Reuse occurrences only for interchangeable finished parts. Name stock,
features, components and dimensions by function, and keep the native feature history.

Prefer stable origin/datum references where supported. Resolve faces from geometric
intent rather than numeric list positions. Assemble machined parts using fresh faces.
Face-mounted operations need downstream edit tests. FreeCAD's official
[topological naming guidance](https://github.com/FreeCAD/FreeCAD-documentation/blob/main/wiki/Topological_naming_problem.md)
explains how chains of generated-face references can break and why origin-based
supports reduce the dependency. A CAD system's mitigation does not make every face
reference equally robust.

## Check outcomes, not an inventory of API calls

For each example choose checks that distinguish a plausible-looking failure from the
intended design. The bench checks material removal, solved bearing geometry, which
side of the rails meets the sitter, fastener engagement, crossing shafts and driver
access in assembly order. Use actual scene geometry and solved poses where possible.
Keep analytic assumptions explicit: axis-aligned stock envelopes and a driver box
are conservative access checks, not exact collision or strength analysis.

Exercise an intended dimension change, recompute, save, reopen in a fresh process,
and edit again. Check geometry and relationships after the edit, not just command
success. Capture at least front, side and isometric views for human review. A screenshot
cannot establish native editability, and a successful edit cannot establish usability.

Add a regression when a failure teaches a general lesson. Do not impose a count of
all tools or snapshots of every UI label. Keep examples executable through the same
`cad_interface` groups and `cargo xtask test-mcp` used by existing CI.

## Bench audit lessons

A dimensioned rectangle can still translate: assert its degrees of freedom and
locate one intentional vertex at the part origin. Naming the stock sketch and each
machining sketch makes the native history understandable without reading the demo
source. Add a construction plane when a feature actually uses it; the picket's
mid-thickness plane drives a symmetric slot cut.

Check handed machining before reusing a component. Mirroring an arm's placement does
not mirror a shared definition's bores. Use separate finished definitions when the
holes differ. Connect assembly mates at real mating surfaces, and validate symmetry
from solved geometry rather than assuming that two independently typed coordinates
are mirrored.

Use explicit clearance stacks and a fabrication process that can achieve alignment.
For screw joints, independently positioned pilots can exceed clearance even when each
part meets its drawing tolerance; transfer pilots from the clamped clearance-drilled
member. Label wood-movement assumptions separately. WOOD's [wood-movement guidance](https://www.woodmagazine.com/woodworking-how-to/wood-preparation/dealing-with-wood-movement)
discusses allowing movement across grain; the bench's narrow seat and arm boards use
one fastening row across their width. [Ian Kirby's stock preparation guidance](https://www.finewoodworking.com/1978/12/01/preparation-of-stock)
explains the reference face/edge convention behind the per-part datum descriptions.

Exact placed-solid interference is now available through MCP and shares the native
implementation. A zero-overlap result does not establish suitable joints, tolerance
margin, comfort, strength, or editability. Those require separate, concrete checks.
