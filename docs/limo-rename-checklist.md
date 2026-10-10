# Limo rename checklist

Checkpoint: **2026-10-05 UTC**. The name was accepted in
[ADR 0007](adr/0007-limo-name.md). The repository is
[`limo-cad/Limo-CAD`](https://github.com/limo-cad/Limo-CAD).
The complete native runtime migration is on `feat/bevy-interface`, tracked by
[PR #124](https://github.com/limo-cad/Limo-CAD/pull/124), and awaits review
before merging into `main`.

## Completed on the Bevy branch

- [x] English, Spanish and German use **Limo CAD**; Simplified Chinese uses
      **砺模 CAD**. The four README entrances use the selected product name.
- [x] Rust packages, imports, executable names, build variables, MCP metadata
      and resource URIs use `limo-cad`, `limo_cad` or `LIMO_CAD_*`.
- [x] The native application lives in `desktop/`; React, Tauri, npm manifests
      and Node build drivers are removed.
- [x] New projects use `.limo`, format `limo-cad-project`, and `.limo.jsonc`
      scripts. Readers retain existing `.nbcad` and `.tfcad` projects.
- [x] New links use `limo-cad://`. Existing `nbcad://` recipe and knowledge
      links remain readable.
- [x] Native preferences move atomically from `org.nbcad.desktop` to
      `org.limocad.desktop`; conflicts preserve both profiles and report an error.
- [x] Fresh leases and inboxes use `limo-cad-sessions`; installer-managed
      Cursor/Codex entries use `limo-cad` and retire duplicate old CAD entries.
- [x] Source package builders emit Limo-named Windows ZIPs, Linux DEB/AppImages
      and macOS bundles. Linux package conflicts retire the previous package.
- [x] Thunder's installed desktop, MCP, Start menu, project/recipe associations
      and old launch aliases route to the qualified Limo Bevy payload. The
      previous preferences retained their file hashes. Live attach, inspection
      and read-only execution pass without changing the open document.
- [x] Current download links use the published release's actual asset names;
      showcase anchors and native source-build commands use the renamed targets.

## Remaining release and cleanup work

- [ ] Qualify and publish packages from the complete Limo identity migration.
      The single public preview still uses source `9b082687` and its original
      filenames; do not invent renamed download URLs. See
      [installation](INSTALL.md) and [transition status](native-transition-status.md).
- [ ] Finish ARM64 and macOS package qualification and publish the qualified
      AppImage. Windows signing and hardware-specific input/printing checks
      retain their own release gates.
- [ ] Coordinate the required-check context rename with the active GitHub
      ruleset. Keep `frontend_regressions / Frontend regression tests` until
      protection and reporting change together; its implementation is Rust.
- [ ] Finish translated public-site entrances and contributor review of localized
      copy and identity assets. The English concept banner remains a proposal.
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
implemented source, the installed Windows build, public packages and unfinished
WASM work.
