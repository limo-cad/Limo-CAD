# Repository agent guidance

Before planning or carrying out UI/MCP qualification, read and follow
`.cursor/rules/surface-qualification.mdc` and its coverage ledger at
`docs/qualification/surface-coverage.json`.

The user requires broad UI coverage first, then MCP coverage. Record a pass
once, move to the next item, and reopen only specifically affected cases.
Do not resume the currently paused GUI/build iteration merely because these
instructions or the ledger exist. A later user request to resume testing can
activate the documented sequence.

Preserve incoming work and saved/recovery data. Apply the approval and merge
requirements in `.cursor/rules/pr-lifecycle.mdc`, including independent review
for our own PRs and the standing direction to merge eligible approved PRs.

Follow `.cursor/rules/pr-lifecycle.mdc` for PR and merge work. For UI, Help or
teaching changes, also follow `.cursor/rules/localized-ui.mdc` and
`.cursor/rules/visual-change-evidence.mdc`.
