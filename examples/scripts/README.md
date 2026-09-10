# Native recipe library

The CAD program **does the work, teaches the process and shows the result** from
one committed construction source. Maximum speed executes its native commands;
Pause and Step let you inspect the same sequence; paced presentation renders its
chapter notes and authored camera moves. These are modes of one Rust interpreter,
not separate modeling, tutorial and animation scripts.

- [Sketch, extrude, ease the edges](fillet-basics.nbcad.jsonc): a first-part lesson
  for Solid → Refine → Fillet. Builds a fully located 60 × 30 mm sketch, extrudes
  12 mm of stock and rounds the four top edges by 2 mm. Exports two captioned
  scene frames for an isolated preview, plus the editable final model.
- [Four-hole mounting plate](mounting-plate.nbcad.jsonc): fully located 60 × 40 × 5 mm
  stock, four 5 mm through bores and face-basis projection after each cut.
- [Revolved annular spacer](revolved-spacer.nbcad.jsonc): a located radial section,
  20 mm outside diameter, 10 mm bore and 12 mm length, revolved about Y.
- [Dimensioned angle bracket](angle-bracket.nbcad.jsonc): a closed, fully constrained
  L section with 40 × 30 mm envelope, 5 mm walls and 20 mm extrusion width.
- [Repeated bracket assembly](repeated-bracket-assembly.nbcad.jsonc): three native
  part definitions, four occurrences and explicit mating frames. Editing one
  bracket extrusion from 20 to 25 mm updates both of its occurrences. This is a
  component/placement lesson, not a fastened or manufacturing-qualified assembly.
- [Crown garden bench](garden-bench.nbcad.jsonc): a complete timber assembly from
  dimensioned sketches, native features and physical mating references. Includes
  authored captions, camera framing and final manufacturing contracts.
- [Version 1 editor schema](nbcad-script.schema.json): step and expression guidance
  for JSONC-aware editors. The Rust interpreter validates references, and the
  shared interface owns each modeling operation’s argument schema.

Run the commented file with `cargo xtask run-script FILE --server MCP_EXECUTABLE`.
Alternatively, select a bundled source by its stable ID:
`cargo xtask run-script --recipe mounting-plate --server MCP_EXECUTABLE`.
MCP lists the same collection with `cad_interface {"action":"recipes"}` and runs
one with `{"action":"script","recipe":"mounting-plate","mode":"fast"}`.
The app's example list is supplied by that same Rust catalog in `crates/recipes`.
Catalog discovery does not build anything. A caller must choose a file or recipe;
there is no implicit bench run.
Use `--repeat 2` for independent headless comparison. After preserving the current
document, use `--session UUID --new --present --speed 2` to create a blank design tab
and watch the same sequence in that existing window. Omit `--new` if the named tab
is already blank. The script refuses to construct over an existing model.

See [the native script format](../../docs/native-scripts.md). Each `.nbcad.jsonc` source
replays construction; the generated `.nbcad` project retains the editable result.

The three flagship targets are the bench, printable windmill/generator and small
screw vise. Only the bench currently has a complete construction recipe. The
windmill and vise remain [committed design briefs](../../docs/flagship-examples.md)
until their real native recipes and checks are implemented. They are deliberately
absent from the runnable catalog. No flagship yet has a complete approved drawing
package or physical fabrication qualification.

The migrated small fixtures preserve #89's analytic bounds/volume, independent
replay, fresh-process native restore, STEP round trip and STL/3MF checks in
`mcp-server/tests/recipes.rs`. The repeated bracket check restores both the 20 mm
baseline and 25 mm edited project, checks solved occurrence poses and verifies
all three profiles retain zero degrees of freedom. Export is a tested derivative;
native sketches and features construct the source model. The old Node recipe
runner and custom argument substitutions are not part of this library.

To add a feature lesson, commit its `.nbcad.jsonc`, add one catalog entry, and add
the focused geometry/edit check that establishes the feature's intent. Titles,
chapters, step counts and the actual operation list come from the source. Declare
the operation the lesson teaches; do not advertise every incidental setup command
as a hover lesson. Use existing native CI and replay tests, without an all-tools
percentage gate or an additional runner.
