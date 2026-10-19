# Contribution and review lifecycle

This policy applies to every contributor and every development tool used on the
project. Personal editor settings and tool-specific instruction files stay out
of the repository. Shared agreements belong in CONTRIBUTING.md and these docs;
AGENTS.md is a short entry point to the same policy.

## Group work before review

- Keep one PR per coherent feature or fix. Implementation steps, experiments,
  review repairs and checkpoints belong as commits in that feature branch.
- Consolidate related implementation PRs before requesting review. Squash
  superseded checkpoints into meaningful feature commits; do not ask reviewers
  to assemble a feature from a chain of per-function or per-step PRs.
- Split genuinely independent concerns or reviewer ownership boundaries. Use a
  short stack only for real dependencies between complete feature groups.
- Preserve source, review discussions and validation evidence when consolidating.
  Verify that the replacement branch is pushed and contains the intended changes
  before closing superseded PRs. Keep a mapping to their original reviews.

## Draft and ready states

- Open unfinished work as draft. A requested change returns the PR to draft
  before repairs; preserve the review rather than dismissing it to clear status.
- Mark a grouped PR ready only when its agreed scope is complete, known review
  findings are addressed, relevant checks pass and current required evidence is
  attached. A request to keep work draft remains controlling.
- Validate coherent batches of changes. Freeze the relevant source, combine
  compatible targeted checks, and rerun affected cases after repairs. Do not
  rebuild the full project after every small edit or repeat unaffected passes.
- CI passing alone does not establish product quality or review readiness.
- Keep descriptions brief: the resulting behavior, useful visual evidence,
  concise changes, validation and material limitations. Identify pending work.

## Approval and merge

- Follow CODEOWNERS and repository protection. Authors cannot approve their own
  PRs; obtain independent approval covering the current changes. Main requires
  Jack's approval. Material changes after approval need renewed review.
- Squash merge is the project default. Check the exact reviewed head, required
  checks, resolved discussions and applicable approval immediately before merge.
- When the current task authorizes merging and all gates pass, complete the
  squash merge rather than stopping at approval. An explicit instruction not to
  merge takes precedence. Never bypass protection or imply approval of one's
  own work; explain a concrete blocker when a gate prevents completion.
- Preserve active branches, workspaces, models and recovery data. Cleanup after
  merge requires verified source custody, settlement and no active consumers.

For UI, Help and teaching changes, follow [localized UI](localized-ui.md) and
[visual evidence](visual-evidence.md). Qualification follows the existing
[surface sequence](surface-qualification.md) and its coverage ledger.
