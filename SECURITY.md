# Security policy

Report a suspected vulnerability privately through
[GitHub's vulnerability reporting form](https://github.com/limo-cad/Limo-CAD/security/advisories/new).
Include the affected release and platform, reproduction steps, a minimal sample
without confidential CAD data, and the likely impact. Keep exploit details
private while maintainers investigate. Ordinary bugs belong in Issues.

Maintainers `@jackControls` and `@jeffglousher` coordinate investigation, upstream
reports, fixes and disclosure. Support focuses on the latest published release
and current development branch. Older releases should be upgraded; there is no
separate long-term support or guaranteed response-time service.

## Handling reports and dependency findings

1. A maintainer records the affected versions, reachable code, impact and owner.
   A dependency match starts an investigation; it does not by itself establish
   exploitability or justify a claim that a release is unaffected.
2. Confirm the issue using a minimal regression case and coordinate with the
   dependency's maintainers when appropriate. Avoid publishing confidential
   samples or live exploitation details in public CI logs.
3. Ship a reviewed fix or dependency update, regenerate every affected package's
   SBOM, and document the affected/fixed versions and mitigations in an advisory
   and release notes. Check for related failure modes before closing the report.
4. Track lower-severity findings and maintenance warnings too. Document evidence
   and a review date for any risk decision; never silently suppress an advisory.
   Publish a VEX claim only when its status is supported by investigation.

Known RustSec vulnerabilities fail dependency checks. Native release scans block
high and critical findings, retaining JSON results at every severity. Scanner
errors, missing inventories and failed signature verification block publication.
Medium and lower findings still need maintainer review before tagging; the
automatic severity threshold is not an acceptance decision. There is no built-in
advisory ignore list. See the
[dependency review record](docs/security/dependencies.md).

## Release evidence

Each new release archive carries CycloneDX and SPDX SBOMs, input/native build
metadata, checksums and signed GitHub/Sigstore attestations. The inventory covers
the Rust dependencies compiled into the executable, OCCT and every shipped
native executable and dynamic library. Consumer OS libraries and graphics
drivers are outside the archive's scope. Unknown producer, version and license
information remains explicit.

See [SBOM generation and verification](docs/security/sboms.md) and
[CISA/NIST practice mapping](docs/security/cisa-nist.md). These controls support
secure development and procurement evidence. They are not a federal certification
or a claim of complete compliance with the 2026 SBOM minimum elements.
