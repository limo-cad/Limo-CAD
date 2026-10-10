# Release notes

One file per release tag, named exactly after the tag: `v0.2.1.md` is the body of
the `v0.2.1` release. `cargo xtask version --check` (the Version guard on every pull
request) fails while `docs/release-notes/v<VERSION>.md` is missing, and
`desktop-packages.yml` refuses to publish a `v*` tag whose file is missing, so the
notes are written and reviewed with the version bump instead of being improvised
after the build.

## Writing one

Start from the record, not from memory:

```sh
git log --oneline --no-merges <previous-tag>..HEAD
gh pr list --state merged --search 'merged:>=<date-of-previous-tag>'
```

The published releases share this shape:

- a one-line summary and the pre-alpha reminder;
- a **Downloads** table naming the package files and where the checksums live;
- **What changed since the previous release**, grouped by area, each item tied to
  a pull request number or an issue;
- **Project compatibility** whenever the model schema moves — state whether the
  new release opens older projects and whether older builds open new ones;
- **Source**: the tag, the revision, the build channel and the checksums.

Write `{{commit}}` where the tagged revision belongs. The publish job substitutes
the real commit, because the notes are committed before the tag exists. Historical
entries may hold the literal hash instead; that is what was published.

## Two rules

- **Release notes are history.** Do not add them to the version carriers in
  `xtask/src/release_tooling/version.rs`: a later bump must never rewrite an older release's
  notes.
- **Say what is missing.** An unshipped platform, a known gap or a dead link
  belongs in the notes, not only in the pull request.
