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
- **Printable windmill with optional generator:** the next manufacturing target.
  Its [design brief](flagship-examples.md#printable-windmill) is committed, but a
  runnable source has not yet been built. The recipe must retain printer-envelope
  and fit inputs, rotor motion, shaft/bearing retention and an accessible removable
  motor mount. Physical output and load claims require measured evidence.
- **Small screw vise:** the proposed third flagship, with a committed brief and
  no runnable source yet. It should demonstrate guided travel, a retained lead
  screw and replaceable jaws, with purchased hardware and fit choices explicit.

The windmill and vise are not placeholder catalog items. Add them when actual
native sketches, features, joints and checks construct a useful design. Comprehensive
editable assembly/detail drawings, tolerances, BOM and process/assembly notes are
part of each flagship's intended recipe work, not a later mesh-only deliverable.
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
