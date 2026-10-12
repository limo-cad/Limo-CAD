# Secure open-source development practices

This is a project practice map, not a compliance certificate. It applies to the
native Bevy desktop on the PR124 development line and its Rust, OCCT and native
release dependencies. Review it when the build, distribution or trust boundaries
change. Federal procurement obligations depend on the agency and contract.

## Current federal guidance

The Securing Open Source Software Act proposal did not become law. The practical
references for this work are:

- [CISA Open Source Software Security Principles and Practices, July 2026](https://www.cisa.gov/sites/default/files/2026-08/open-source-software-security-principles-and-practices.pdf):
  codebase, community, conduct and configuration (C4).
- [NIST SSDF 1.1, SP 800-218](https://csrc.nist.gov/pubs/sp/800/218/final):
  prepare, protect, produce well-secured software and respond to vulnerabilities.
  [SSDF 1.2](https://csrc.nist.gov/pubs/sp/800/218/r1/ipd) remains a draft at this
  review date; this map uses the final 1.1 practice identifiers.
- [CISA 2026 SBOM minimum elements](https://www.cisa.gov/resources-tools/resources/2026-minimum-elements-software-bill-materials-sbom):
  machine-readable dependency identities and relationships, producers, licenses,
  hashes, generation context, tools, timestamp and author authentication. The
  2026 guidance supersedes the 2021 minimum elements and does not create a new
  legal requirement by itself.
- [OMB M-26-05, January 2026](https://www.whitehouse.gov/wp-content/uploads/2026/01/M-26-05-Adopting-a-Risk-based-Approach-to-Software-and-Hardware-Security.pdf):
  agencies use mission- and risk-based software assurance. It rescinds M-22-18
  and M-23-16; an agency can still request SBOMs or other assurance through its
  procurement process. Publishing an SBOM alone does not satisfy a contract.

## Evidence in this repository

| Practice | Project implementation | Remaining limits |
| --- | --- | --- |
| CISA codebase; SSDF PO.1, PO.2, PO.4 | [Contribution and review rules](../../CONTRIBUTING.md), CODEOWNERS, version and workflow contract tests; security owners and escalation in [SECURITY.md](../../SECURITY.md) | Maintainers must enforce review and repository protection. This change does not audit all organizational policies or configure branch protection. |
| CISA community and conduct; SSDF PO.2, RV.1 | Named maintainers, [community conduct](../../CODE_OF_CONDUCT.md), private vulnerability reporting, documented disclosure process | Response times depend on volunteer availability; upstream dependency maintenance needs ongoing review. |
| CISA configuration; SSDF PO.3, PO.5, PS.1 | Locked Rust workspaces, pinned OCCT/vcpkg inputs, verified Unix SDK and vcpkg OCCT DLL receipts, checksum-verified new tools; new security actions pinned by commit; read-only workflow defaults; signing permissions limited to package jobs | Existing workflow actions are not all pinned by commit. Builds use hosted runners and system/Homebrew dependencies, with no claim of hermetic or reproducible builds. |
| SSDF PS.2, PS.3 | Five artifact-specific CycloneDX/SPDX inventories, archive/file hashes, source identity, OCCT patch provenance, signed SBOM and build attestations, verification before publication | Published releases predating this workflow have no retroactive inventories. Unknown fields and consumer runtime boundaries remain explicit. |
| SSDF PW.4 | RustSec across all three lockfiles, Dependabot for Cargo/actions, native Grype checks on final release SBOMs and weekly signed-release rescans | Automated matching cannot establish exploitability. Native input updates and findings below the blocking threshold need manual review. |
| SSDF PW.5, PW.7, PW.8 | Existing Rust/C++ CodeQL security queries, lint, geometry, package/MCP and platform acceptance checks; tests for SBOM omissions, mismatched identities and broken graphs | This PR adds supply-chain checks; it does not constitute a complete code audit, parser fuzzing campaign or penetration test. |
| SSDF RV.1, RV.2, RV.3 | [Security response process](../../SECURITY.md), retained reports at all severities, [known dependency review](dependencies.md), fixes with regression evidence and release notes | OCCT high/medium parser matches and unmaintained text dependencies remain open investigation/maintenance work. High OCCT matches block tag publication. |

## Trust boundaries to review

| Input or asset | Threat | Review expectation |
| --- | --- | --- |
| STEP/IGES and other imported designs | Malformed files can exercise native parsers and consume memory/CPU | Treat imported files as untrusted; investigate the recorded OCCT advisory. Import is not claimed to be sandboxed. |
| Recipes and MCP clients | A client or script can request document mutations and exports | Retain existing operation controls and user-visible behavior; review file paths, ownership, resource bounds and the optional native computer-control feature when those surfaces change. |
| Dependency sources, SDK caches and package builders | Substituted dependencies or compromised build inputs | Verify locked/source inputs and completed SDK receipts; retain final-file identities and signed release evidence. A compromised authorized builder remains a threat. |
| Published downloads | Substituted archives or inventories | Verify checksums and attestations bound to the expected repository, workflow, tag and source revision. A checksum alone does not authenticate the publisher. |

The app handles local CAD data. Report samples and public CI evidence must not
include confidential designs, credentials or personal machine data.
