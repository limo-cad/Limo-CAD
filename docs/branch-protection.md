# Main branch protection and validation

The React frontend was retired on the Bevy integration branch on 2026-10-02.
The required check identity `frontend_regressions / Frontend regression tests`
is preserved by `required-interface.yml` calling `rust-interface-contracts.yml`.
It now runs Rust shared-interface tests and compiles the WASM engine boundary;
it does not run React or npm. The Bevy merge therefore needs no ruleset change
to satisfy this existing context. `Rust WASM engine facade` and
`Repository Rust contracts` are additional checks. The repository is now
`limo-cad/Limo-CAD`; required check identities survive the rename.

## Verified repository policy

Read back on 2026-10-03, `limo-cad/Limo-CAD` uses the active **[Protect main](https://github.com/limo-cad/Limo-CAD/rules/19790895)**
ruleset (id `19790895`). Main requires one approving review, dismissal of stale
approvals, and resolution of review threads. Force pushes and branch deletion are
blocked. The PR author cannot supply their own approval.

Two status checks are **required** for normal PR merges:
`VERSION matches every carrier` and
`frontend_regressions / Frontend regression tests`. Both must come from the GitHub
Actions integration (app id `15368`). Requiring an up-to-date branch is disabled;
the new checks do not introduce a mandatory rebase/rebuild after every main push.

The current API readback did not expose an owner bypass; do not assume one is
available. The September 20 audit, when the repository was named
`jackControls/noBS-CAD`, recorded an owner-only PR bypass for `jackControls`
(user id `31257982`). That dated observation remains historical evidence,
not current merge authorization; it did not authorize direct pushes, force
pushes or branch deletion.

Updating the ruleset requires repository administration; collaborator push and
triage access is insufficient. Inspect all applicable results on the exact PR
head before merging; the two required checks do not replace native acceptance or
package verification when those are relevant.

Tracking: [#14](https://github.com/limo-cad/Limo-CAD/issues/14) (parent
[#9](https://github.com/limo-cad/Limo-CAD/issues/9)).

### Release tags

There is no tag ruleset yet. The desktop packaging workflow refuses a `v*` tag
whose commit is not on `main` or does not name its `VERSION`, but a workflow
cannot stop a **force-updated** `v*` tag from rebuilding and rewriting a release
that already shipped (`publish_release` edits the release and replaces its
assets). Blocking updates and deletion of `v*` tags is a repository ruleset
(Rules → New tag ruleset, target `v*`, restrict updates and deletions), which
needs repository administration. Add it before relying on published assets
staying immutable.

## Required-check rollout

Use the Check Run `name` strings as shown in the PR Checks UI and Checks API, not
only the workflow file titles. Reusable-workflow jobs include the caller job id as
a prefix.

### Lean set for `main`

The two always-reporting checks below are now required. The remaining two are
the intended next stage for #14, after their workflows always report a result.
Prefer this lean set over requiring the full desktop package matrix.

| Check Run name | Workflow | Status and purpose |
| --- | --- | --- |
| `VERSION matches every carrier` | Version guard | **Required.** Always runs on every PR and main push; cheapest always-reporting gate. |
| `frontend_regressions / Frontend regression tests` | Required native interface → Rust interface contracts | **Required.** Always runs on every PR via the reusable Rust workflow. Copy this prefixed Check Run name from the PR; the bare job title is not the required context. |
| `MCP tests (Ubuntu)` | MCP server | **Not yet required.** Lean OCCT/MCP gate on Ubuntu; needs always-reporting behavior first. |
| `Ubuntu host-neutral crates` | Linux engine tests | **Not yet required.** Workspace `cargo test --locked` without packaging cost; needs always-reporting behavior first. |

Do **not** require Windows portable package jobs (`Windows x64 portable ZIP`,
`Windows arm64 portable ZIP`) or the other Desktop packages matrix builds for #14.
Keep them informative; the issue title calls for staying lean on Windows portable.
Likewise leave Windows `mcp-tests`, pages-knowledge `build`/`deploy`, and
`Classify desktop build inputs` optional.

### Path-filter and skip pitfalls

`MCP tests (Ubuntu)` and `Ubuntu host-neutral crates` only start when their
workflows' path filters match. A docs-only or otherwise filtered PR never creates
those check runs. If they are marked required in Protect main before they always
report (success, failure, or an explicit always-on stub), that PR waits forever.

Until those workflows gain an always-reporting gate, keep only the two always-on
names required. Add the MCP and host-neutral names after the always-reporting
gates land; do not require docs-only PRs to touch an unrelated watched path.

Desktop package jobs that classify to skip are the same class of trap — do not
require them.

## Validation responsibilities

- **Frontend regression tests** now runs shared Rust interface tests and compiles
  the WASM engine boundary. It does not run React/npm or qualify a completed
  Bevy browser UI. Native desktop compilation and package checks remain separate.
- **Desktop release publication** handles pushed `v*` tags: `version_preflight`
  first refuses a tag that does not name the `VERSION` on its commit or whose
  commit is not on `main`, so a tag cannot publish unreviewed code; once the four
  package builds succeed, `publish_release` repeats that check, verifies each
  package against its `.sha256`,
  creates a draft release from `docs/release-notes/<tag>.md`, uploads the
  packages, their checksums and a generated `SHA256SUMS.txt`, and publishes the
  draft only when all eleven assets are attached. That job alone requests
  `contents: write`; the repository default stays read-only.
- **Ubuntu host-neutral crates** runs `cargo test --locked --workspace`, including
  export, MCP mutation mapping, and installer tests. Source and lockfile changes
  trigger it; compiled dependencies are cached.
- **MCP server** tests native OpenCASCADE on Windows and Ubuntu. It also checks
  Rust formatting and keeps Windows installer coverage. Both PR and main-push
  filters include `crates/cam/**`, so CAM-only changes receive native acceptance.
  Export tests are owned by the host-neutral workspace job instead of repeated
  in the Windows MCP job.
  New pushes cancel obsolete runs; lockfiles are enforced. Windows reuses the
  desktop SDK cache keyed by runner, MSVC version, vcpkg pin, and manifest.
  Core, turbine and vise acceptance run on separate runners per platform; the
  existing top-level MCP checks require all three before publishing demo artifacts.
- **Desktop packages** classifies build inputs before starting Windows x64/ARM64,
  macOS ARM64, and Ubuntu packages, and waits for frontend/version preflights to
  pass. Retain native launch checks: frontend or MCP
  tests do not establish that a packaged viewport starts correctly.
- **pages-knowledge** validates and publishes the active knowledge site. It is
  scoped to knowledge changes and is not a CAD runtime gate.
- **Version guard** checks `VERSION` against every carrier and unit-tests those
  carriers on every pull request and main push through Rust Cargo tooling.

The default-branch ARM SDK warmer, acceptance sharding, artifact gates and their
performance tradeoffs are documented in [CI performance](ci-performance.md).

Start local development with scoped compilation/linting and relevant guards:

```sh
cargo xtask check --scope engine --fmt --clippy
cargo xtask knowledge check
cargo xtask knowledge index --check
cargo xtask audit-icons
cargo xtask version --check
```

Use `--scope desktop` or `--scope mcp` with the platform OCCT SDK for native work.
Run a targeted test or an owned native fixture when the change requires it;
the broad suite is not the default local loop. WASM build/smoke checks exercise
the current engine facade, not a complete browser app. See [Development](DEVELOPMENT.md).

## Adding further required checks

After the remaining jobs always report and have passed on a PR, an administrator
(Jack) can add their exact Check Run names to **[Protect main](https://github.com/limo-cad/Limo-CAD/rules/19790895)**,
preserving its existing checks, review rules, and deletion rules.
Reusable-workflow check names may include the caller job prefix; copy the name
from the actual PR.

Do not mark path-filtered engine or MCP workflows required until they have an
always-reporting gate. Keep platform packaging informative until its cost and
reliability justify making it required. Do not disable failing checks just to
permit a merge.

The live ruleset and `gh pr checks --required` were verified after enabling the
two checks. No merge was attempted to test enforcement. The broader #14 rollout
still needs always-reporting engine/MCP gates and verification of the complete
required set, including docs-only PRs.

## Repository ownership

Collaborators can use branches directly on `limo-cad/Limo-CAD` with that
repository as `origin`; a personal fork is optional. Before deleting a fork,
preserve unique commits and finish or migrate any upstream PR using its branches.
Disable redundant fork Actions separately so retiring a fork does not remove
upstream test coverage.
