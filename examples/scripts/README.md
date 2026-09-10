# Native command sources

The [version 1 editor schema](nbcad-script.schema.json) describes the commented
source format. The Rust interpreter validates references; the shared interface
owns each modeling operation's argument schema.

Bundled designs and their validation belong to the recipe-library layer above
the interpreter and playback adapters. This layer supports loading a source file
without requiring any particular example to be built into the application.

Run the commented file with `cargo xtask run-script FILE --server MCP_EXECUTABLE`.
Use `--repeat 2` for independent headless comparison. After preserving the current
document, use `--session UUID --new --present --speed 2` to create a blank design tab
and watch the same sequence in that existing window. Omit `--new` if the named tab
is already blank. The script refuses to construct over an existing model.

See [the native script format](../../docs/native-scripts.md). Each `.nbcad.jsonc` source
replays construction; the generated `.nbcad` project retains the editable result.
