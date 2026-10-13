# Limo rename checklist

Checkpoint: **2026-10-05 UTC**. The name was accepted in
[ADR 0007](adr/0007-limo-name.md). The repository is
[`limo-cad/Limo-CAD`](https://github.com/limo-cad/Limo-CAD).
The native runtime migration merged in
[PR #124](https://github.com/limo-cad/Limo-CAD/pull/124).

## Remaining release and cleanup work

- [ ] Qualify and publish packages from the complete Limo identity migration.
      The single public preview still uses source `9b082687` and its original
      filenames; do not invent renamed download URLs. See
      [installation](INSTALL.md) and [transition status](native-transition-status.md).
- [ ] Coordinate the required-check context rename with the active GitHub
      ruleset. Keep `frontend_regressions / Frontend regression tests` until
      protection and reporting change together; its implementation is Rust.
- [ ] Finish contributor review of localized public-site copy and identity assets.
      The English concept banner remains a proposal. The authored entrance pages
      do not establish native-speaker, release or external-listing approval.
- [ ] Coordinate external listings and separately owned plugin identities.
      This repository does not rename `dsh-nobs-cad-step` in another project.
- [ ] Finish the separately deferred physical deletion of retired local payloads.
      Preserve live documents, recovery snapshots, unique Git work and active
      worktrees. Compatibility launch aliases must continue selecting Limo.

Old project formats, imported CAM headers, frozen qualification fixtures and
release filenames remain only where required to read existing data or describe
an actual published artifact. New interfaces and authored instructions use Limo.

[Runtime identities and migration](limo-cad-runtime.md) records the supported
paths. [Native transition status](native-transition-status.md) distinguishes
source evidence, public packages and unfinished
WASM work.
