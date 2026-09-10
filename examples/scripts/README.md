# Native command examples

- [Sketch, extrude, ease the edges](fillet-basics.nbcad.jsonc): a first-part lesson
  for Solid → Refine → Fillet. Builds a fully located 60 × 30 mm sketch, extrudes
  12 mm of stock and rounds the four top edges by 2 mm. Exports two captioned
  scene frames for an isolated preview, plus the editable final model.
- [Crown garden bench](garden-bench.nbcad.jsonc): a complete timber assembly from
  dimensioned sketches, native features and physical mating references. Includes
  authored captions, camera framing and final manufacturing contracts.
- [Version 1 editor schema](nbcad-script.schema.json): step and expression guidance
  for JSONC-aware editors. The Rust interpreter validates references, and the
  shared interface owns each modeling operation’s argument schema.

Run the commented file with `cargo xtask run-script FILE --server MCP_EXECUTABLE`.
Use `--repeat 2` for independent headless comparison. After preserving the current
document, use `--session UUID --new --present --speed 2` to create a blank design tab
and watch the same sequence in that existing window. Omit `--new` if the named tab
is already blank. The script refuses to construct over an existing model.

See [the native script format](../../docs/native-scripts.md) and
[the bench’s design intent](../../docs/garden-bench.md). Each `.nbcad.jsonc` source
replays construction; the generated `.nbcad` project retains the editable result.
