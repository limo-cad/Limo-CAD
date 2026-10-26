# Dependency review record

Reviewed against the PR124 base on 2026-10-11. Revisit these entries before the
next release and when an advisory or upstream version changes. A reviewed entry
does not approve a release or establish that a vulnerability is unreachable.

The OCCT entries below retain the 7.9.3 assessment. They have not been
reassessed for the separately upgraded 8.0.1 SDK; the version change does not
establish parser remediation.

| Dependency | Evidence and status | Follow-up / owner |
| --- | --- | --- |
| OCCT 7.9.3: high parser findings | Grype 0.120.1 matches [CVE-2026-42476](https://nvd.nist.gov/vuln/detail/CVE-2026-42476) (STL), [CVE-2026-42477](https://nvd.nist.gov/vuln/detail/CVE-2026-42477) (OBJ), and [CVE-2026-42478](https://nvd.nist.gov/vuln/detail/CVE-2026-42478) (VRML) at **High**. NVD's affected ranges include 7.9.3; its primary scores are 7.1, 7.1 and 7.5, with lower secondary scores also recorded. **Under investigation; release-blocking matches**, not confirmed exploitability in every platform package. | Maintainers: inspect which affected modules are shipped/reachable on each target, reproduce against the exact SDK, verify an upstream fix or evidence-based VEX status and add regressions before releasing. The checked matrix patches do not establish parser remediation. |
| OCCT 7.9.3: medium VRML findings | [CVE-2026-42479](https://nvd.nist.gov/vuln/detail/CVE-2026-42479) and [CVE-2026-42480](https://nvd.nist.gov/vuln/detail/CVE-2026-42480) describe VRML parser bounds errors and match the 7.9.3 CPE at **Medium**. **Under investigation**; shipped modules and reachability vary by platform. | Maintainers: assess with the high parser findings, retain the evidence and reassess before tagging. |
| OCCT 7.9.3 with checked `math_DoubleTab` source/header patches | [NVD CVE-2026-42481](https://nvd.nist.gov/vuln/detail/CVE-2026-42481) includes affected versions through 7.9.3 and reports STEP/IGES parser bounds/recursion problems. The application uses OCCT STEP import. **Under investigation**; the existing matrix-storage patches are not verified remediation for this advisory. No `not_affected` VEX claim is issued. | Maintainers: reproduce against the shipped patched SDK, obtain/verify an upstream fix or mitigation, add a parser regression and reassess before tagging. Until assessed, importing untrusted designs remains a known risk. |
| zlib 1.3.2 (`z.dll`, rc.2/rc.3 Windows x64 and ARM64 only) | Grype 0.120.1 matched [CVE-2026-85091](https://nvd.nist.gov/vuln/detail/CVE-2026-85091) at **High** on both `v0.3.0-rc.2` Windows SBOMs in the **2026-10-23** scan ([run 39917475155](https://github.com/limo-cad/Limo-CAD/actions/runs/39917475155)); the scan reported no fixed version. NVD published the advisory on **2026-09-03**. The retained rc.2 binary inspection reported only `freetype.dll` and `libpng16.dll` consumers of `z.dll`, using `inflate`, `deflate` and `crc32` functions, with no `gz*` imports. The rc.2 review assessed the vulnerable gzip-writing path as unreachable from those imports. **Every Windows package build repeats this check:** `cargo xtask package windows` fails unless `freetype.dll` and `libpng16.dll` are the only `z.dll` importers, both import by name and no `gz*` function, and no other packaged image embeds `gzprintf`/`gzvprintf` or a second zlib banner. The rc.3 packages built by PR #526 CI passed it on x64 and ARM64 with the same imports as rc.2. The temporary reviewed decision covers zlib 1.3.2, type `binary`, only at the four literal paths below. Unlisted future product versions receive no exception. | Owner: @jackControls. Review by **2026-11-06**; both scans fail after that UTC date until the decision is renewed or removed. Each Windows package build rechecks the shipped binaries; add a consumer to `ZLIB_CONSUMERS` in `xtask/src/windows_runtime.rs` only after reviewing it here. Investigate and verify upstream or vcpkg fixes, then reassess the scanner identity and this scoped decision as described below. |
| `rustybuzz` 0.20.1 | [RUSTSEC-2026-0206](https://rustsec.org/advisories/RUSTSEC-2026-0206.html) reports an unmaintained dependency in the text stack. Retained as a maintenance warning; not a demonstrated exploitable vulnerability. | Maintainers: evaluate the upstream Bevy/text dependency update or replacement and verify shaping/font behavior. |
| `ttf-parser` 0.25.1 | [RUSTSEC-2026-0192](https://rustsec.org/advisories/RUSTSEC-2026-0192.html) reports an unmaintained dependency in the text stack. Retained as a maintenance warning. | Maintainers: follow the upstream replacement/update and verify native text rendering and imported fonts. |

The zlib exception and review ledger each list exactly these locations, with
one leading `**/` to absorb the scanner's extraction prefix:

- `**/Limo-CAD-0.3.0-rc.2-windows-x64/z.dll`
- `**/Limo-CAD-0.3.0-rc.2-windows-arm64/z.dll`
- `**/Limo-CAD-0.3.0-rc.3-windows-x64/z.dll`
- `**/Limo-CAD-0.3.0-rc.3-windows-arm64/z.dll`

The [upstream zlib commit `df84af25dc1942490e1d1c899a07619152a46148`](https://github.com/madler/zlib/commit/df84af25dc1942490e1d1c899a07619152a46148)
and [zlib PR #1320](https://github.com/madler/zlib/pull/1320) are unverified
investigation inputs. Verify a proposed fix against the shipped source and add
a regression before adopting it; these references alone do not justify a
backport or a remediation claim. The native SBOM currently reports `1.3.2`
without the vcpkg port revision. A verified patched `1.3.2#N` port can therefore
retain the same scanner identity and may need a renewed, scoped decision until
that identity changes. Remove the exception and review only after reassessment
shows they are no longer needed, and retain the investigation record. No new
VEX claim is made; the package-build check is the per-build reachability evidence.

Local cargo-audit 0.22.2 checks of the engine, desktop and MCP lockfiles reported
zero known RustSec vulnerabilities and the two warnings above, using RustSec DB
commit `7eebec69c352c7191b1f13eb95dd510eeca5d1de` (2026-10-09). The local check
did not query yanked-crate status. CI fetches current advisories and checks yanked
status; neither result is proof that undiscovered vulnerabilities are absent.

The scanner compatibility test against the generated inventory's OCCT 7.9.3 CPE
returned three high and three medium matches (CVE-2026-42476 through 42481), and
`--fail-on high` exited with status 2 as intended. That test used a synthetic
archive fixture to exercise the export/scanner path; it did not run real OCCT
parsers or verify exploitability. No fix versions were supplied by that scan.
As configured, those high CPE matches block future tag publication until
remediation or a supported investigation changes their status; they are not
ignored to preserve the previous release behavior.

Grype matches on release SBOMs are triage inputs, including any inferred CPE
matches. Keep false-positive or risk decisions with the affected version,
platform, investigation evidence, owner and next review date. The release gate
blocks high/critical native findings and known RustSec vulnerabilities. The only
suppressions are the rules in `.github/grype/exceptions.yaml`. Immediately
before the tag scan and the scheduled rescan apply them, `cargo xtask
advisory-exceptions` fails closed unless:

- each rule names a CVE or GHSA identifier and a literal package name, version
  and type, plus a location ending in a literal file under a named directory;
- each rule sets no other field, and the file sets no other grype option;
- each rule equals exactly one entry in `.github/grype/reviews.yaml`;
- that entry's owner and `review-by` date appear in a row above;
- that date, checked in UTC, has not passed and is at most 90 days away.

The scheduled job also validates the ledger unconditionally after Rust setup,
even when the latest stable release has no SBOMs. Each scan loop checks again
immediately before each Grype invocation so crossing midnight UTC cannot extend
a review. The scheduled query still selects the latest stable release; it does
not establish prerelease scan coverage.

No advisory is suppressed without a reviewed, unexpired decision.
