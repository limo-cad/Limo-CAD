# Development process (humans + agents)

## Loop

```text
(optional issue) → branch → implement → verify → PR → follow through → merge
```

## Issues

- Search before filing; close duplicates with a pointer to the survivor.
- Substantial bugs/features benefit from an issue first.
- Small docs/fixes may go straight to a PR.

## Branches and worktrees

Ordinary feature branches are fine. Worktrees help when juggling multiple PRs
or agent sessions; they are optional.

```sh
git worktree add ../limo-cad-issue-42 -b issue/42-fillet-regression origin/main
```

## PR follow-through

| Concern | Action |
|---------|--------|
| Review comments | Fix valid requests; explain disagreements |
| Merge conflicts | Resolve preserving intent; ask if intents clash |
| CI | Fix failures in PR scope; never disable required checks to pass |
| Behind main | Merge/rebase latest `main`, re-verify |
| Validation | Proportional to the change (see CONTRIBUTING) |

## MCP / automation changes

Prefer documenting behavior in public MCP docs (`docs/mcp-harness.md`,
`mcp-server/README.md`) and adding tests where practical. Be honest when work
is headless-only vs UI-visible.

## Cross-platform

Prefer Rust for new engine, export, and MCP surfaces so Windows / macOS /
Linux stay aligned. Document packaging gaps in the issue or PR.
