# Versioning and releases

## One version, one file

`VERSION` at the repository root holds the version of the whole product. It is
the only file a version bump edits by hand.

```sh
printf '0.3.0\n' > VERSION
cargo xtask version --sync
cargo xtask version --check
```

`xtask/src/release_tooling/version.rs` propagates the value to every carrier that has to
agree with it:

| Carrier | Why it exists |
| --- | --- |
| `Cargo.toml` (`[workspace.package]`) | the one Rust version; all eleven engine crates and `xtask` inherit it with `version.workspace = true` |
| `desktop/Cargo.toml`, `mcp-server/Cargo.toml` | separate workspaces, so they declare the version themselves |
| `Cargo.lock`, `desktop/Cargo.lock`, `mcp-server/Cargo.lock` | lockfiles record the version of every local package |
| `vcpkg.json` | native dependency manifest identity |
| `docs/DEVELOPMENT.md`, `docs/INSTALL.md`, `docs/OCCT_PACKAGING.md`, `docs/WINDOWS_PACKAGING.md` | packaged-file examples that quote a version |

The `Version guard` workflow runs the check and
`cargo test --locked -p xtask release_tooling::` on every pull request and every push
to `main`, so a carrier that drifts from `VERSION` fails fast.

Two places derive the version instead of storing it:

- Desktop artifact names come from `VERSION` inside
  `desktop-packages.yml`, so the workflow needs no literal version.
- The binary records `CARGO_PKG_VERSION`, the commit SHA and the build channel
  from `crates/core/build.rs`. A `v*` tag becomes the channel; anything else
  builds as `preview`. **File → Settings → About Limo CAD** shows
  `version+revision`.

Adding a new carrier means adding it to `inventory()` in
`xtask/src/release_tooling/version.rs` and, when it is a file, to the table above. The notes
in [`release-notes/`](release-notes/README.md) are deliberately **not** carriers:
they record what one release contained, so a later bump must never rewrite them.

## Choosing the number

Semantic Versioning, `MAJOR.MINOR.PATCH`:

- Below `1.0.0` the document model, file formats and MCP surface are not frozen,
  so a release that adds capability takes a **minor** bump (`0.2.0`, `0.3.0`).
- A release that only fixes defects takes a **patch** bump (`0.2.1`).
- Reserve `1.0.0` for a release the project is willing to keep compatible.
- Pre-releases take a suffix that sorts before the release they lead to:
  `v0.3.0-rc.1`, `v0.3.0-beta.2`. Publish those as GitHub pre-releases, not as
  the latest release.

Do not reuse a version for a different commit, and do not tag a commit whose
carriers disagree with `VERSION`.

## Cutting a release

1. **Bump, sync and write the notes** on a branch from `main` — `VERSION`, the
   synced carriers and `docs/release-notes/v0.3.0.md` in one PR. The body of that
   file becomes the release description, so it is reviewed like any other change.
   `cargo xtask version --check` (the Version guard on every pull request) fails while
   that file is missing, so the tag build never has to discover it. Confirm
   locally:

   ```sh
   cargo xtask version --check
   cargo metadata --offline --locked --format-version 1 > /dev/null
   ```

2. **Merge** after review. `main` requires an approving review, and the author
   cannot approve their own PR.

3. **Tag the merge commit** and push the tag:

   ```sh
   git checkout main && git pull --ff-only
   git tag -a v0.3.0 -m "Limo CAD 0.3.0"
   git push origin v0.3.0
   ```

   The tag build refuses a tag that does not name the `VERSION` on its commit,
   or whose commit is not already on `main`: `version_preflight` checks both
   before any package job starts, and `publish_release` checks them again
   before it writes the release. A tag cannot ship code that never passed
   review, so tag the merge commit, after the bump PR has landed.

4. **The tag publishes itself.** A `v*` tag makes `desktop-packages.yml` build the
   Windows x64 and ARM64 portable ZIPs, the signed and notarized macOS DMG and the
   Ubuntu DEB and AppImage with `LIMO_CAD_BUILD_CHANNEL` set to the tag name. When all
   four succeed, its `publish_release` job then:

   - checks every package against its `.sha256` and fails if any of the five is
     missing, so a release cannot go out with a gap;
   - creates the GitHub release **as a draft** from `docs/release-notes/<tag>.md`,
     substituting `{{commit}}` with the tagged revision;
   - uploads the packages, their checksums and a generated `SHA256SUMS.txt`, and
     only when all eleven assets are attached publishes the draft (a `-rc.1` tag
     is published as a pre-release and does not take the Latest badge), then
     writes the asset list to the run summary.

   Nothing is downloaded to a workstation. Only a **pushed** tag publishes: a
   manual run of the workflow on a tag rebuilds for diagnosis and leaves the
   release alone. Re-running the job replaces what it uploaded rather than
   duplicating it, as long as the run's build artifacts still exist (they are
   kept for seven days; after that, tag a new version). The job requests
   `contents: write` for itself only; the repository default stays read-only.

   If a package build fails, the publish job is skipped and the tag ships no
   release: fix `main`, then tag again with a new version rather than reusing the
   number.

5. **Repoint the download links.** `README.md` and `knowledge/home.html` name a
   specific release tag, so update them in a follow-up PR after the release is
   published.

## What CI does not decide

The version guard only proves that the carriers agree. Whether a release is
warranted, which number it deserves and whether the notes are honest stay with
the maintainers.
