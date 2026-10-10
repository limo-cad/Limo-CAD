# Repository agent guidance

Before planning or carrying out UI/MCP qualification, read and follow
`.cursor/rules/surface-qualification.mdc` and its coverage ledger at
`docs/qualification/surface-coverage.json`.

The user requires broad UI coverage first, then MCP coverage. Record a pass
once, move to the next item, and reopen only specifically affected cases.
Do not resume the currently paused GUI/build iteration merely because these
instructions or the ledger exist. A later user request to resume testing can
activate the documented sequence.

For the current Bevy transition, keep work in PR124, preserve incoming work
and saved/recovery data, and do not merge into main without Jack's approval.
