# Recipes are a product capability

One native construction source should let noBS CAD do the work, teach the process
and show how a feature works. **Build** runs it at maximum rate. **Teach** exposes
its chapters, native feature history and Pause/Step controls for inspection.
**Show** renders the same commands with calm captions and purposeful camera views.
There is one Rust interpreter and the ordinary grouped product operations, not a
separate tutorial language, JavaScript modeling runner or canned animation model.

The [recipe library](../examples/scripts/README.md) sits above the replay and
playback capabilities in the review stack. Sources, semantic checks, expected edit
behavior and editable drawing additions should be reviewed together. Keep each
later feature lesson or flagship iteration in a small PR based on that layer's
current tip, with GitHub updating its stack base as parents merge. Shared capability
changes belong below recipes that depend on them.

The [development stack](development-stack.md) records the review/merge workflow
and how to keep working from the current capability tip.

## Committed targets and current evidence

- **Garden bench:** an executable, deterministic native recipe with constrained
  sketches, reusable parts, connected assembly mates and geometric manufacturing
  checks. It remains a design candidate; full editable drafting, selected-occurrence
  editing, hardware/material selection and physical fabrication evidence remain.
  Accepted at the current milestone; active example work now moves to the other two.
- **Printable vertical-axis turbine with integrated generator:** an executable
  [Savonius design](vertical-axis-turbine.md) with two reused rotor stages, a native
  72:18 gear relation, separate bearings, clamped hubs and guarded transmission.
  Native tests compare blank replays, check selected printable definitions, and
  exercise edits, reload and motion. Its drawing package includes assembly,
  critical part dimensions and the hardware BOM. Actual motor fit, startup and
  loaded output remain physical tests.
- **Functional screw vise:** an executable [D-screw design](d-screw-vise.md) with
  five printed parts, purchased keeper retention and a coupled screw/slider loop.
  It includes six native drawing sheets, a print layout and paired thread coupon.
  The tests exercise 48 mm travel, insertion paths, dimensional edits and drawing
  reassociation behavior. Printed fit, force, creep and wear remain unqualified.

The two new recipes use constrained sketches, named datums, native features,
reusable definitions and persistent joints. Materials, fit allowances, assembly
order and calculations are retained with their sources. Blank replay comparisons
are exact on the tested build. A dimension edit/restore may retriangulate the
same surface: the vise test independently checks oriented surface boundaries,
volume, bounds and topology rather than confusing buffer order with geometry.
Existing issues #93 and #94 retain the unresolved drawing and occurrence-editing
decisions; do not copy those issues into a new tracking hierarchy.

## Small feature lessons

The current fillet, through-hole plate, revolved spacer, dimensioned angle bracket
and repeated-bracket assembly are native JSONC examples. They demonstrate source
construction, returned geometry references, parameter changes and native history.
The migrated #89 tests independently check analytic bounds/volume, fresh replay,
native restore, STEP round trip and STL/3MF output. Both assembly occurrences must
retain their solved positions after editing and restoring the shared bracket.

Future lessons add one readable `.nbcad.jsonc` and one entry to the Rust-owned
catalog in `crates/recipes`. Derive titles, chapters, counts and the executed
operation list from source; declare only the intended teaching operation for
feature discovery. Add a focused regression for geometric or editing behavior
that could fail, rather than a quota of successful tool calls. Use the existing
MCP CI and `cargo xtask run-script` path.

The app layout, native preview rendering and presentation ownership are still
under review in [the script interface draft](script-interface-review.md). Recipe
correctness does not by itself approve that interface or certify manufacturability.
