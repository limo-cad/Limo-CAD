# Contributing to Limo CAD

Thanks for helping. This guide is meant to be practical and welcoming — not
heavy bureaucracy.

Maintainers today: `@jackControls` and `@jeffglousher`. Either can review the
other’s work; a PR author should not approve their own PR.

## 1. Issues

- Search existing issues before opening a new one.
- **Recommended** for substantial bugs and features so discussion stays
  visible.
- **Optional** for small fixes, typos, and documentation-only PRs — open
  straight to a PR if that is clearer.

Useful labels include: `bug`, `enhancement`, `mcp`, `geometry`, `packaging`,
`documentation`.

### Contributing without writing code

Model a small real part you already understand, then report one concrete
difference between the expected and actual behavior. Include the smallest
editable project that reproduces it, numbered steps, a screenshot, and the
OS/architecture and build revision from Settings → About. Say whether you used
the UI or MCP and whether several CAD windows were open. A STEP backup is useful
for import/export failures, but keep the editable project when history matters.
See [the edge-case guide](docs/EDGE_CASE_HUNT.md) for examples.

Linux installation feedback, clearer instructions, translation corrections,
and a first-use review of a short lesson are also useful contributions. Describe
the exact screen or instruction that caused difficulty; coding experience is
not required. Avoid sharing confidential designs or personal machine data.

If Git is new to you, a GitHub issue with those details is enough to start.
Maintainers can help turn a documentation correction into a small pull request.
Financial support arrangements should be confirmed with a maintainer; this
repository does not currently configure a funding link.

## 2. Branches

From a clean `main`:

```sh
git fetch origin
git checkout -b fix/short-slug origin/main
```

[Git worktrees](https://git-scm.com/docs/git-worktree) are helpful for
**parallel** or agent work. They are **not** required for every contribution.

## 3. While you work

- Keep diffs focused. Prefer small PRs.
- Add or extend tests when you change geometry/MCP behavior.
- Do not commit secrets, `.env*` files, machine-local OCCT paths, or personal
  editor/agent state (those paths are gitignored).
- Do not weaken CI to force a green check.

## 4. Open a PR

- Title: imperative and scoped (`mcp: clarify stdio setup`, not `updates`).
- Link an issue when one exists (`Fixes #N` / `Refs #N`).
- Include a short **test plan** proportional to the change (see template).
- Keep the PR mergeable with `main`.

### Validation (proportional)

Pick what fits:

- Ran `cargo test` (or named crates) — note which
- Ran an MCP or e2e scenario — describe briefly
- Docs-only / no runtime impact — say so explicitly

For desktop viewport rendering, browser output is not visual validation. Bevy
owns the pixels inside the packaged desktop viewport, and it can fail or clip
while browser state and browser tests remain correct. Reproduce the scenario
in a packaged native app and inspect the actual Bevy surface before describing
a desktop visual issue as fixed. Record the tested appearance mode and the
visible result in the PR test plan.

### Version numbers

The product version lives in `VERSION`. Change it there, run
`cargo xtask version --sync`, and run `cargo xtask version --check` before you push — do not
edit the derived manifests by hand. See [docs/RELEASING.md](docs/RELEASING.md).

## 5. Follow through until merge-ready

Stay with the PR until it is ready to merge:

1. Address review comments (or explain disagreements).
2. Resolve conflicts intentionally.
3. Fix CI failures caused by the PR.

Maintainers merge when the above holds and required reviews pass.

## 6. Branch protection (maintainers)

See [docs/branch-protection.md](docs/branch-protection.md).

## License / borrow

Project is **LGPL-2.1-or-later** ([LICENSE](LICENSE)). Peer projects
(e.g. Open CAD Studio, **GPL-3**) — borrow **ideas**, not code, unless
counsel says otherwise. Related-projects table: [README](README.md#related-projects).
