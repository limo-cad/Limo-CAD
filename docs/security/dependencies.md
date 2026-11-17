# Dependency review record

The 0.3.0 package review was refreshed on **2026-11-17 (UTC)** against PR #535
head `f501220f1451adb86717c905ac058228a69e5cb8`. GitHub built merge commit
`98eef195f656c95386c9810f9da6be983ef72a48`, whose tree is identical to that head.
[Run 43676043101](https://github.com/limo-cad/Limo-CAD/actions/runs/43676043101)
produced the five reviewed archives. The
[retained review summary](reviews/v0.3.0-2026-11-17.json) binds their hashes,
inventories, advisory results and Windows DLL inspection. Later commits must
rebuild and scan their own packages; this record does not approve publication.

| Dependency | Evidence and status | Follow-up / owner |
| --- | --- | --- |
| zlib 1.3.2#3 (`z.dll`, final 0.3.0 Windows x64 and ARM64 only) | Fresh Grype 0.120.1 scans still match **CVE-2026-85091** at High because the binary version remains 1.3.2. Both reviewed archives contain the checksum-matched vcpkg 1.3.2#3 DLL, and their build logs select the checked overlay. Its six port files match upstream exactly; its source regression passes with the repair and detects the invalid write without it. Independent inspection of all 58 PE images per architecture found only `freetype.dll` and `libpng16.dll` importing zlib, by name and without `gz*` imports. **Reviewed backport match:** the two literal final-release paths below replace the expired rc-only rules. | Owner: @jackControls. Review by **2026-12-17**. Reassess on any zlib source, recipe, consumer or package-layout change. Every new build retains the source regression, DLL consumer guard, inventory verification and advisory scan. |
| AppImage `libudev1` 249.11-0ubuntu3.23, identified as systemd | The only AppImage match is **CVE-2026-40228**, Low. [Ubuntu's advisory](https://ubuntu.com/security/CVE-2026-40228) concerns `systemd-journald` forwarding terminal escapes with `ForwardToWall=yes`. The verified archive inventory contains `libudev.so.1`, not journald. This component match does not establish the affected daemon is shipped. No ignore rule is added; the finding remains visible. | Owner: @jackControls. Review by **2026-12-17** and if the AppImage library set changes. Host operating-system services remain outside the archive inventory. |
| `rustix` 1.1.5 | [RUSTSEC-2026-0486](https://rustsec.org/advisories/RUSTSEC-2026-0486.html) reports unsound extended-attribute buffer handling, fixed in 1.1.6. The five reviewed packages contain 1.1.5; source inspection of their 16 direct consumers found no calls to the affected functions. The release follow-up nevertheless updates **all three affected lockfiles to 1.1.6**; the MCP lock has no rustix entry. Rebuilt-package CI must qualify that change. | Maintainers: retain the patched lock entries and current RustSec checks. No advisory exception is introduced. |
| `ttf-parser` 0.25.1 | RUSTSEC-2026-0192 remains an unmaintained dependency warning in the compiled text stack, not a demonstrated vulnerability. | Maintainers: evaluate a compatible text-stack replacement and qualify font behavior. |
| Other Rust maintenance warnings | The fresh four-workspace audit also reports `unsafe-libyaml` (engine tooling); `zbus_names`, `zvariant`, `zvariant_derive` (desktop lock); and `atomic-polyfill`, `fxhash`, `paste` (simulation lock). These warnings are retained in audit reports; no ignore list is used. The old `rustybuzz` warning is absent from this scan. | Maintainers: track compatible upstream replacements. Locked/source-only components are not automatically shipped runtime components. |

## Windows zlib backport evidence

The overlay selects **zlib 1.3.2#3** from
[vcpkg commit `520c3b78`](https://github.com/microsoft/vcpkg/commit/520c3b781ac2abf391887d286d569f74ae6bf752).
Its upstream-named `CVE-2026-76844.patch` backports
[zlib commit `df84af25`](https://github.com/madler/zlib/commit/df84af25dc1942490e1d1c899a07619152a46148),
which repairs CVE-2026-85091. The name of the patch is not an advisory identity.
The [source regression](../../native/occt-overlay/zlib/README.md) verifies the
source archive SHA512, exercises stalled compressed and direct writes, runs
zlib's upstream example and requires AddressSanitizer failures in both modes
when only the repair is removed. This was repeated successfully during review.

The reviewed final-package DLL SHA256 values are:

| Architecture | z.dll SHA256 | Build evidence |
| --- | --- | --- |
| x64 | `c93da951e897efee9f7f9181492b62be240596ef80ebfd84c724177b02d2722d` | [Windows x64 job](https://github.com/limo-cad/Limo-CAD/actions/runs/43676043101/job/132697100789) |
| ARM64 | `bfae145dd6f25f2f3843d4fe654c44d901fb2f562102e31197f6ae8def5345a9` | [Windows ARM64 job](https://github.com/limo-cad/Limo-CAD/actions/runs/43676043101/job/132697100791) |

The native metadata binds these hashes to `pkg:vcpkg/zlib@1.3.2` with
`port_version=3`. Grype's binary identity omits that revision, which explains
its retained version match. The only approved exception locations are:

- `**/Limo-CAD-0.3.0-windows-x64/z.dll`
- `**/Limo-CAD-0.3.0-windows-arm64/z.dll`

Both require advisory CVE-2026-85091, package `zlib`, version `1.3.2`, type
`binary`. These rules do not wildcard the product version. The four rc.2/rc.3 rules
expired on 2026-11-06 and are removed, not renewed; the historical scan remains
[run 39917475155](https://github.com/limo-cad/Limo-CAD/actions/runs/39917475155).
No claim is made about unrelated zlib installations or future product versions.

## Scan coverage and remaining investigation

The unfiltered scan used Grype **0.120.1**, with no ignore rules, and a validated
v6.1.10 database built **2026-11-16T06:30:54Z**. macOS and the DEB had no matches;
the AppImage had the single Low finding above; each Windows archive had the
single High backport match above. All five archive checksums and their
CycloneDX/SPDX/source-input/native-coverage checks passed at the built revision.
These PR archives have no release attestation bundles, and the macOS archive is
ad-hoc signed. Tag builds still require their own signed evidence, Developer ID
signing and notarization.

Cargo-audit **0.22.2** used RustSec commit
`02e2517f457a359fc6d3440ea4560c8ea188fee8` (2026-11-15), including yanked-crate
checks. All four locks reported zero advisory entries classified as
vulnerabilities; the unsoundness and maintenance warnings are reviewed above.
After updating rustix to 1.1.6, all four audits pass without the unsoundness
warning. Maintenance warnings remain visible. That classification must not be
mistaken for proof that all code is safe.

The current OCCT **8.0.1** inventories had no Grype matches. The older OCCT
7.9.3 investigation below remains unresolved: a changed scanner range is not
proof of parser remediation. This gate repair neither suppresses OCCT findings
nor issues a `not_affected` VEX claim. The application still imports STEP data;
the old parser assessment and regressions need a separate completed review
before claiming that untrusted designs are safe.

| Historical dependency | Evidence and status | Follow-up / owner |
| --- | --- | --- |
| OCCT 7.9.3: high parser findings | Grype 0.120.1 matches [CVE-2026-42476](https://nvd.nist.gov/vuln/detail/CVE-2026-42476) (STL), [CVE-2026-42477](https://nvd.nist.gov/vuln/detail/CVE-2026-42477) (OBJ), and [CVE-2026-42478](https://nvd.nist.gov/vuln/detail/CVE-2026-42478) (VRML) at **High**. NVD's affected ranges include 7.9.3; its primary scores are 7.1, 7.1 and 7.5, with lower secondary scores also recorded. **Under investigation; release-blocking matches**, not confirmed exploitability in every platform package. | Maintainers: inspect which affected modules are shipped/reachable on each target, reproduce against the exact SDK, verify an upstream fix or evidence-based VEX status and add regressions before releasing. The checked matrix patches do not establish parser remediation. |
| OCCT 7.9.3: medium VRML findings | [CVE-2026-42479](https://nvd.nist.gov/vuln/detail/CVE-2026-42479) and [CVE-2026-42480](https://nvd.nist.gov/vuln/detail/CVE-2026-42480) describe VRML parser bounds errors and match the 7.9.3 CPE at **Medium**. **Under investigation**; shipped modules and reachability vary by platform. | Maintainers: assess with the high parser findings, retain the evidence and reassess before tagging. |
| OCCT 7.9.3 with checked `math_DoubleTab` source/header patches | [NVD CVE-2026-42481](https://nvd.nist.gov/vuln/detail/CVE-2026-42481) includes affected versions through 7.9.3 and reports STEP/IGES parser bounds/recursion problems. The application uses OCCT STEP import. **Under investigation**; the existing matrix-storage patches are not verified remediation for this advisory. No `not_affected` VEX claim is issued. | Maintainers: reproduce against the shipped patched SDK, obtain/verify an upstream fix or mitigation, add a parser regression and reassess before tagging. Until assessed, importing untrusted designs remains a known risk. |

## Enforcement

Grype matches on release SBOMs are triage inputs, including any inferred CPE
matches. Keep false-positive or risk decisions with the affected version,
platform, investigation evidence, owner and next review date. The release gate
blocks high/critical native findings and known RustSec vulnerabilities. The only
suppressions are the rules in `.github/grype/exceptions.yaml`. Immediately
before PR/tag scans and the scheduled rescan apply them, `cargo xtask
advisory-exceptions` fails closed unless:

- each rule names a CVE or GHSA identifier and a literal package name, version
  and type, plus a location ending in a literal file under a named directory;
- each rule sets no other field, and the file sets no other grype option;
- each rule equals exactly one entry in `.github/grype/reviews.yaml`;
- that entry's owner and `review-by` date appear in a row above;
- that date, checked in UTC, has not passed and is at most 90 days away.

The Version guard validates the ledger on pull requests before package jobs.
The five-package advisory job runs on qualifying pull requests as well as tags;
only PR inventories may omit tag attestations. All publication signature checks
remain mandatory. The scheduled job also validates the ledger after Rust setup,
even when the latest stable release has no SBOMs. Each scan loop checks again
immediately before each Grype invocation so crossing midnight UTC cannot extend
a review. The scheduled query still selects the latest stable release; it does
not establish prerelease scan coverage.

No advisory is suppressed without a reviewed, unexpired decision.
