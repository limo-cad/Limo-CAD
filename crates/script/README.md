# Native script foundation

`limo-cad-script` parses versioned `.limo.jsonc` command sequences and executes them
through a caller-supplied host function. It does not open a desktop window, own an
MCP session, or implement CAD operations. The host supplies the existing grouped
interface and remains responsible for document ownership and execution receipts.

The source supports comments, trailing commas, result references, ordered modeling
commands, presentation notes and camera requests, and final checks. Fast execution
skips presentation requests while retaining the same modeling commands and checks.
Preflight rejects invalid references and nested session/transport changes; a failed
operation stops execution before later steps run.

The [editor schema](../../examples/scripts/limo-cad-script.schema.json) describes the
same version 1 format. The Rust parser is the runtime validation authority.

`manufacturing.rs` retains the existing **garden-bench-specific** geometric
verification gate for compatibility with the validated bench recipe. It checks
that recipe's declared parts, contacts and assembly geometry. It is not a general
manufacturing rules system, a structural rating, or a fabrication qualification.

This foundation contains no bundled design recipes or desktop integration. Those
are subsequent stack changes that use this interpreter.

Run the host-neutral tests with `cargo test -p limo-cad-script`. They exercise JSONC
parsing, references, preflight, failure stopping, mode equivalence and the existing
bench verification helpers without requiring OpenCASCADE or a desktop session.

## Collections

A root script may declare `includes`. Each path is relative to the file that
declares it (`.collection.jsonc` or `.limo.jsonc`). Hosts expand them with
`parse_with_includes` / `flatten_includes` before execution. The crate never
opens files itself. An included `.limo.jsonc` contributes steps and checks;
its version, starting state, verification, and exports stay with the root.
Fast mode still skips presentation steps contributed by collections.

## Bounded evaluation

Trusted offline authoring keeps using `run`, `run_with_progress`, or
`ProjectScript::run`. Untrusted preparation can opt into the same interpreter
with `run_bounded` or `ProjectScript::run_bounded` and `EvaluationLimits`.
Project hosts implement the cheap `ProjectReplayHost::checkpoint` callback;
the v1 bounded entry point takes a separate callback. An error stops evaluation,
including `let`, `assert`, selectors/predicates, reference copies, and exports.
A new invocation gets a fresh budget, including after cancellation or failure.

The byte and node limits are cumulative, without refunds. They cover source and
host values, copied strings, object/binding keys, containers, retained bindings,
temporary values and exports. Each Value and map entry reserves 128 bytes in
addition to UTF-8 payload bytes, conservatively allowing container growth. Work
and nesting limits bound the accounting traversal itself. Checks happen before
each interpreter allocation; there is no whole-value serialization to count
bytes and no unchecked reference clone. Resolved arguments and exports move
into their envelopes. Error diagnostics in bounded mode omit value trees.
Textual host results reserve a conservative decoding allowance before parsing;
structured JSON results avoid that extra cost. Accounting arithmetic fails closed.

Package preview uses 8 MiB of cumulative data, 65,536 nodes, depth 64 and
1,000,000 work units. These are separate from its 2 MiB source, 32 MiB expanded
archive, 16 MiB frame sink and shared 32 MiB render cache limits; the existing
512-operation host budget remains unchanged. Checks run during preparation,
outside hover and playback. No new worker, timer or service is needed.

These limits do not measure process RSS or interrupt synchronous kernel calls.
The host still owns source parsing, baseline/kernel allocations and bounded
response construction; the interpreter checks returned values before retaining
or copying them. The legacy garden-bench manufacturing gate is available through
trusted authoring only, because its separate geometric scans are not governed by
the evaluation work budget. Other existing authored guides and public recipes
continue using their existing trusted path.
