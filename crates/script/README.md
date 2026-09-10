# Native script foundation

`nbcad-script` parses versioned `.nbcad.jsonc` command sequences and executes them
through a caller-supplied host function. It does not open a desktop window, own an
MCP session, or implement CAD operations. The host supplies the existing grouped
interface and remains responsible for document ownership and execution receipts.

The source supports comments, trailing commas, result references, ordered modeling
commands, presentation notes and camera requests, and final checks. Fast execution
skips presentation requests while retaining the same modeling commands and checks.
Preflight rejects invalid references and nested session/transport changes; a failed
operation stops execution before later steps run.

The [editor schema](../../examples/scripts/nbcad-script.schema.json) describes the
same version 1 format. The Rust parser is the runtime validation authority.

`manufacturing.rs` retains the existing **garden-bench-specific** geometric
verification gate for compatibility with the validated bench recipe. It checks
that recipe's declared parts, contacts and assembly geometry. It is not a general
manufacturing rules system, a structural rating, or a fabrication qualification.

This foundation contains no bundled design recipes or desktop integration. Those
are subsequent stack changes that use this interpreter.

Run the host-neutral tests with `cargo test -p nbcad-script`. They exercise JSONC
parsing, references, preflight, failure stopping, mode equivalence and the existing
bench verification helpers without requiring OpenCASCADE or a desktop session.
