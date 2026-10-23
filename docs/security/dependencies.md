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
| zlib 1.3.2 (`z.dll`, Windows x64 and ARM64 only) | Grype 0.120.1 matched [CVE-2026-85091](https://nvd.nist.gov/vuln/detail/CVE-2026-85091) at **High** on both `v0.3.0-rc.2` Windows SBOMs ([run 39917475155](https://github.com/limo-cad/Limo-CAD/actions/runs/39917475155)); no fixed zlib release exists. The overflow is in `gz_vacate()`, a `local` function in zlib's `gzwrite.c` called only from `gzprintf()`/`gzvprintf()` on a non-blocking gzip file handle. In both packages, `z.dll` is imported only by `freetype.dll` (`inflate`, `inflateEnd`, `inflateInit2_`, `inflateReset`) and `libpng16.dll` (those plus `inflateReset2`, `deflate*`, `crc32`); no binary imports a `gz*` function, names a zlib DLL for runtime loading, or embeds another zlib copy. macOS and Linux packages have no zlib match. **Exception, not affected (vulnerable code not reachable)**: ignored by `.github/grype/exceptions.yaml` only for zlib 1.3.2 at `Limo-CAD-*-windows-*/z.dll`; any other version, path or platform still fails the gate. | Owner: @jackControls. Review by **2026-11-06**, or sooner when zlib or the vcpkg `zlib` port ships a fix: upgrade, delete the ignore rule and this row. Re-run the import check if a new Windows DLL links `z.dll`. |
| `rustybuzz` 0.20.1 | [RUSTSEC-2026-0206](https://rustsec.org/advisories/RUSTSEC-2026-0206.html) reports an unmaintained dependency in the text stack. Retained as a maintenance warning; not a demonstrated exploitable vulnerability. | Maintainers: evaluate the upstream Bevy/text dependency update or replacement and verify shaping/font behavior. |
| `ttf-parser` 0.25.1 | [RUSTSEC-2026-0192](https://rustsec.org/advisories/RUSTSEC-2026-0192.html) reports an unmaintained dependency in the text stack. Retained as a maintenance warning. | Maintainers: follow the upstream replacement/update and verify native text rendering and imported fonts. |

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
suppressions are the rules in `.github/grype/exceptions.yaml`; each names one
vulnerability, package, version and location and must have a row above with its
evidence. No advisory is suppressed to make the pipeline pass.
