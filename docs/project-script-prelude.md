# Project-backed script execution boundary

`ProjectScript` parses JSONC version 2 with required `starting_state: "project"`.
It allows an empty steps array for a baseline-only replay. Includes must already
be resolved; the existing 16 MiB source bound applies. Version 1 remains available
through `Script::parse` and its existing runners.

`ProjectScript::run` validates options and every catalog operation, calls the
host's `load_project_baseline` once, then uses the shared step interpreter. A
failed preflight or baseline call submits no later steps. A failed step stops
subsequent steps. Ordinary `run` and `run_with_progress` reject the borrowed v2
script, so inspection cannot accidentally bypass the required prelude.

The host must retain the associated replay package, validate its archive and
known domain semantics, check cancellation and the current owner/revision fence,
commit transactionally, retain ancillary data, and transfer playback ownership
only on success. It must continue checking cancellation and external owner changes
on later calls. This crate sequences callbacks; it does not prove those host
invariants or provide rollback after a committed baseline.

This API is not yet wired into native or MCP script entry points. Those continue
to reject v2 until their package/prelude integration is implemented and qualified.
