# Release SBOMs

Every new Windows x64/ARM64 ZIP, macOS ARM64 DMG, Ubuntu DEB and Linux AppImage
has its own inventory generated **after packaging, signing and notarization**.
The generator extracts or mounts the final archive and inventories:

- Rust components and dependency relationships embedded in the executable by
  cargo-auditable. Lockfiles are used for advisory/source checks; their optional
  or build-only packages are not substituted for the compiled release graph.
- OCCT 7.9.3, its source archive digest, license/exception and the exact local
  storage patch digests. Unix builds use the checked SDK's completed-file receipt;
  Windows uses the vcpkg port's cached DLL/source/patch receipt, including its
  additional packaging patches and package metadata digests.
- Every shipped native executable and shared library, including aliases, with
  its final-file SHA-256 and bundled loader dependencies. OCCT file identities
  must match the receipt-verified SDK. Package-manager attribution is used only
  when the packaged native identity matches the installed source file.

Host OS libraries, drivers and dependencies installed separately by the consumer
are outside this archive inventory. Statically embedded non-Rust native code
without identifiable metadata is not claimed to have a complete component
inventory. Unknown versions, producers and licenses are reported explicitly;
the CycloneDX composition is marked `incomplete`. An SBOM or signature does not
prove a component safe, unaffected by a CVE, or fully compliant with federal
minimum elements.

## Files beside each archive

| Suffix appended to the archive name | Content |
| --- | --- |
| `.sbom.cdx.json` | CycloneDX 1.6: compiled components, OCCT, native files, dependency graph, author, timestamp, tools and post-build context. |
| `.sbom.spdx.json` | SPDX 2.3: corresponding packages, native file hashes and loader relationships; unknowns use `NOASSERTION`. |
| `.sbom.metadata.json` | Source revision/version/target, dirty-source flag, input and report digests, SDK receipt and source/patch evidence, all native paths/imports and unknown-field details. |
| `.sbom.SHA256SUMS.txt` | Checksums of the two inventories and their metadata. |
| `.sbom.sigstore.json` | Tag-only signed CycloneDX statement whose subject is the archive. |
| `.provenance.sigstore.json` | Tag-only signed build provenance for the archive and all four unsigned sidecar files, binding SPDX and metadata digests as well. |

PR and manual diagnostic builds have the four inventory/checksum files. Only a
pushed `v*` tag produces the two signatures and publishes. No release can publish
without all five inventory sets, correct hashes/input identity, successful
advisory gates, verified signatures and the existing package acceptance gates.
CI retains advisory reports for 90 days; SBOMs and portable attestation bundles
are published as release assets. Weekly scans verify the latest release's signed
inventories before checking current advisory data. Older releases without SBOMs
remain a reported coverage gap until an inventoried release is available.

## Generate locally

Use the same versions as `.github/actions/setup-release-sbom`: Syft **1.54.1**
and cargo-auditable **0.7.7**. CI checks the downloaded cargo-auditable archive's
platform-specific SHA-256; Syft's installer is pinned by commit and version.
For a local build, install the pinned tools from their official distributions
or use `cargo install cargo-auditable --version 0.7.7 --locked`.
`cargo auditable --version` reports Cargo's version, not the wrapper's; record
the installed wrapper version separately.

```sh
export LIMO_CAD_AUDITABLE=1
export LIMO_CAD_AUDITABLE_VERSION=0.7.7
export OCCT_ROOT=/absolute/path/to/checked-sdk
cargo xtask build-occt --prefix "$OCCT_ROOT"
cargo xtask package --bundle deb
cargo xtask sbom release --artifact /absolute/path/to/package.deb \
  --sdk "$OCCT_ROOT" --target x86_64-unknown-linux-gnu
cargo xtask sbom verify --directory /absolute/path/to/bundle/deb
```

Run the release generator on the package's native host: Windows uses the PE
import inspector, Linux uses `readelf`/`dpkg-deb`, and macOS uses `otool`/`hdiutil`.
Re-run the current checked builder for a Unix SDK without `.limo-sdk-sbom.json`.
It verifies a compatible completed SDK, adds the evidence and republishes its
receipt without discarding existing build objects. Windows vcpkg SDKs need the
updated overlay's `share/opencascade/limo-occt-build.json`; the overlay changes
its cache key. SDKs without verified evidence cannot generate release SBOMs.
Packaging without the auditable graph also fails SBOM generation.

For a clearly labeled pre-build inventory across all three Rust workspaces:

```sh
cargo xtask sbom source
```

Output defaults to `target/sbom/`. It includes build and optional dependencies
and does not describe the shipped native runtime.

## Verify a downloaded tag release

Download an archive and its six sidecars plus its `.sha256` to one directory.
With the expected tag and source commit from the reviewed release, run:

```sh
sha256sum -c package.zip.sha256
sha256sum -c package.zip.sbom.SHA256SUMS.txt
gh attestation verify package.zip --repo limo-cad/Limo-CAD \
  --bundle package.zip.sbom.sigstore.json \
  --predicate-type https://cyclonedx.org/bom \
  --signer-workflow limo-cad/Limo-CAD/.github/workflows/desktop-packages.yml \
  --source-ref refs/tags/vX.Y.Z --source-digest EXPECTED_COMMIT \
  --deny-self-hosted-runners
for file in package.zip package.zip.sbom.cdx.json package.zip.sbom.spdx.json \
  package.zip.sbom.metadata.json package.zip.sbom.SHA256SUMS.txt; do
  gh attestation verify "$file" --repo limo-cad/Limo-CAD \
    --bundle package.zip.provenance.sigstore.json \
    --signer-workflow limo-cad/Limo-CAD/.github/workflows/desktop-packages.yml \
    --source-ref refs/tags/vX.Y.Z --source-digest EXPECTED_COMMIT \
    --deny-self-hosted-runners
done
```

On macOS use `shasum -a 256 -c` for the checksum commands. With a checkout of the
expected commit, `cargo xtask sbom verify --directory PATH` also verifies input
digests, report identities and native coverage. The publication job adds
`--require-release-evidence` to require the exact five target/archive pairs,
unmodified source and signature bundles, then cryptographically verifies the
bundles with `gh attestation verify`. Presence checks alone do not authenticate
a signature.
