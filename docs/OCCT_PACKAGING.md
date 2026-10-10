# OCCT Packaging and Browser/WASM Strategy

Status: native packages are implemented for macOS, Windows x64 and ARM64, and
Ubuntu x64. The Bevy browser host and OCCT WASM port remain unfinished.

Use [Install Limo CAD](INSTALL.md) for downloads or the
[developer guide](DEVELOPMENT.md) for SDK setup and `cargo xtask package`.
This document explains the dependency staging and verification performed by
that shared desktop build command.

## 1. Ownership boundary

Limo CAD does not keep separate native and browser CAD models.

```text
Rust document/history and solid planner (crates/core, sketch, solid)
                         |
Native OCCT 7.9.x adapter (crates/occt)
                         |
Rust validation + commit + document DTO
```

The planned browser host shares the Rust model and Bevy UI. It will offload exact
geometry to a native Rust/OCCT service. An optional fully local browser kernel
requires a separate OCCT WASM port; the engine facade alone is not a browser CAD app.

`crates/solid` is authoritative for:

- feature definitions and replay order;
- rollback/recompute transactions;
- stable `BodyId`, `FaceId`, and `EdgeId` assignment;
- profile and target validation;
- planar-face references and broken-reference errors;
- mesh, face, edge, and plane DTO validation.

The native adapter constructs shapes, performs booleans, tessellates, enumerates
topology and returns validated kernel DTOs. It exports live B-reps to AP242 STEP.
Manufacturing mesh packaging lives in the Rust `limo-cad-export` crate.

On native OCCT, the writer is constructed first, schema index 5 is selected,
and `STEPControl_Writer::Model(Standard_True)` creates a fresh AP242 model
before transfer. Without that final new-model step OCCT silently retains its
default AP214 model.

## 2. Native development

The bridge is implemented with `cxx` in `crates/occt`. It is tested with
OCCT 7.9.3 and accepts any compatible 7.9.x SDK during local development.

macOS SDK setup:

```sh
brew install cmake ninja freetype
cargo xtask build-occt --prefix "$HOME/Library/Caches/limo-cad/occt-7.9.3"
export OCCT_ROOT="$HOME/Library/Caches/limo-cad/occt-7.9.3"
cargo test -p limo-cad-occt --features native-occt
cargo check --manifest-path desktop/Cargo.toml
```

Pinned SDK setup:

Linux SDK builds require the compiler, CMake, Ninja, pkg-config, FreeType and
Fontconfig development packages. Ubuntu uses `libfreetype6-dev` and
`libfontconfig-dev`; the SDK compiler probe checks them before compiling OCCT
and includes their libraries and headers in its cache identity.

```sh
export OCCT_ROOT=/absolute/path/to/opencascade-7.9.3
cargo test -p limo-cad-occt --features native-occt
```

`OCCT_ROOT` must contain `include/opencascade` (or `include`) and `lib` (or
`lib64`). A Windows SDK may use `inc` plus a supported `win64/vc*/lib`
directory. Homebrew locations are probed only when `OCCT_ROOT` is absent.

## 3. Reproducible macOS application bundle

Do not ship a desktop binary linked directly to `/opt/homebrew` or another SDK
prefix. Copying dylibs without changing the executable's load commands is not
sufficient.

After SDK setup, use the shared package entry point on macOS:

```sh
cargo xtask package
```

The Rust packager:

1. validates native SDK and signing prerequisites;
2. discovers the recursive OCCT/TBB dylib closure with `otool -L`;
3. copies the closure to generated `desktop/occt-libs`;
4. changes dylib IDs and non-system dependencies to `@rpath`;
5. stages the project license, third-party notices, OCCT license and exception;
6. records the discovered native library closure in `occt-libs/libraries.json`;
7. links the Rust executable against those staged libraries and adds
   `@executable_path/../Frameworks` to `LC_RPATH`;
8. creates the `.app` and `.dmg`, seals local builds ad hoc when no signing
   identity is supplied, and verifies both the code signature and disk image.

Desktop packaging directly builds the native Cargo executable. It does not
install frontend dependencies, embed web assets, or build browser WASM. The
generated OCCT staging directory is ignored.
The results are:

```text
desktop/target/release/bundle/macos/Limo CAD.app
desktop/target/release/bundle/dmg/Limo.CAD_0.2.2_aarch64.dmg
```

Useful manual release audit:

```sh
APP="desktop/target/release/bundle/macos/Limo CAD.app"
otool -L "$APP/Contents/MacOS/limo-cad"
otool -l "$APP/Contents/MacOS/limo-cad"
codesign --verify --deep --strict "$APP"
```

All non-Apple OCCT/TBB loads must be `@rpath/...`; the app must contain the
matching files under `Contents/Frameworks`. The license files must be present
under `Contents/Resources/licenses`.

STEP import/export adds `TKDESTEP`, `TKXSBase`, and `TKDE` as direct native
entry libraries. The staging script discovers their larger recursive closure
exactly like the modeling libraries; do not hand-maintain a partial STEP dylib
list.

For signed/notarized releases, the `v*` tag path in
`.github/workflows/desktop-packages.yml` imports a **Developer ID Application**
identity, enables hardened runtime, submits the app to Apple's notary service,
staples the app and disk-image tickets, and verifies both Gatekeeper assessment
and the stapled ticket. Pull-request and manually dispatched diagnostic builds
remain ad-hoc signed; the local ad-hoc seal is a verification aid, not
distribution signing.

Production tag builds require these GitHub Actions repository secrets:

| Secret | Value |
|--------|-------|
| `APPLE_CERTIFICATE` | Single-line base64 encoding of the exported Developer ID Application `.p12`, including its private key |
| `APPLE_CERTIFICATE_PASSWORD` | Password chosen when exporting that `.p12` |
| `APPLE_API_ISSUER` | App Store Connect API issuer UUID |
| `APPLE_API_KEY` | App Store Connect API key ID |
| `APPLE_API_PRIVATE_KEY` | Complete contents of the downloaded `AuthKey_*.p8` private key |

Never commit these values. The workflow creates an ephemeral keychain, accepts
only an identity whose certificate name starts with `Developer ID Application`,
and removes the certificate archive, notary key, and keychain in an `always()`
cleanup step. An `Apple Development` identity is suitable for development but
is deliberately rejected at the production boundary.

Before creating a tag, run **Desktop packages** manually with
`macos_signing: production`. That exercises the same stripped,
Developer-ID-signed, notarized, and stapled path without publishing a release.
The default manual option, `diagnostic`, remains ad-hoc signed.

## 4. Reproducible Windows portable build

Windows targets are x64 and ARM64 on Windows 10 version 1803 or newer and
Windows 11. They require Microsoft's
centrally installed matching Visual C++ v14 Redistributable.

The root `vcpkg.json` pins both the vcpkg registry and OCCT 7.9.3. The Windows
packager compiles the native Cargo executable, copies the complete
DLL set from the isolated vcpkg prefix beside the executable, adds licenses,
and creates a ZIP plus SHA-256 file:

```powershell
cargo xtask package
```

This selects the running Rust toolchain's architecture and the Rust portable
ZIP builder. Install the matching SDK first;
the [Windows setup](WINDOWS_PACKAGING.md#local-windows-build) also documents
explicit `--target` selection and `OCCT_ROOT` overrides.

The desktop packaging GitHub Actions workflow uses one lightweight path
classifier and then conditionally runs the affected package jobs. It builds the
x64 Windows package on `windows-2025`, the ARM64 Windows package on
`windows-11-vs2026-arm`, and the Apple-silicon DMG on `macos-15`,
launch-smoke-tests each packaged executable, and uploads the package plus its
SHA-256 file. Documentation-only pull requests skip the expensive runners.
See [Windows portable packaging](WINDOWS_PACKAGING.md) for exact Windows setup
and runtime requirements.

## 5. Reproducible Ubuntu 26.04 packages

Ubuntu 26.04 LTS is the official Linux baseline. The package bundles the checked
OCCT 7.9.3 runtime and a full native Bevy/Winit window with Vulkan rendering.
GTK supplies file dialogs and the desktop portal supplies printing. Package
verification exercises private X11 and Wayland displays; it no longer checks
an embedded browser's child surface.

After SDK setup, use the same entry point on Ubuntu:

```sh
cargo xtask package
```

It creates and audits a `.deb`, an AppImage, their SHA-256 files, and the
required project/OCCT license notices. See [Ubuntu 26.04 packaging](LINUX_PACKAGING.md)
for the exact SDK, runtime requirements, and verification commands.

Native packaging is implemented in `xtask/src/package/`. Deleted legacy scripts
and npm aliases have no compatibility wrappers.

## 6. Browser/WASM development

The retired React app and OpenCascade.js package are no longer build inputs.
The browser replacement must reuse the desktop Bevy UI and connect to the native
Rust/OCCT geometry service. An optional fully local kernel requires an OCCT WASM
port. Neither browser host is supplied by the current engine bundle command.

Build the existing Rust engine facade, which can be checked independently:

```sh
cargo xtask bootstrap --wasm
cargo xtask build-wasm
```

See [the browser host backlog](../web/README.md) for the remaining integration
work and [browser development](DEVELOPMENT.md#browser-development) for the
optional Chrome engine smoke test.

## 7. Version and CI policy

- Local development: a compatible OCCT 7.9.x SDK is accepted for engine checks;
  distributed packages require the checked 7.9.3 SDK.
- macOS and Linux CI build verified OCCT 7.9.3 sources through Rust xtask.
  The compiler, FreeType, source and checked storage replacements determine
  the cache identity. A bounded probe qualifies the actual `TKMath` runtime
  before the SDK receipt is published or reused.
- Windows CI: use the committed vcpkg baseline and OCCT 7.9.3 override,
  installed as the dynamic `x64-windows` or `arm64-windows` triplet. Preserve
  vcpkg binary packages in an ABI-keyed CI cache; a cache miss must rebuild
  from the pinned sources.
- Linux CI: build the DEB on `ubuntu-26.04` with its private checked OCCT 7.9.3
  runtime, and verify Vulkan viewport startup under headless X11 and
  Weston/XWayland sessions. Build the AppImage in an Ubuntu 22.04 container
  against OCCT 7.9.3 compiled from pinned, checksummed source
  (`cargo xtask build-occt --prefix PATH`, cached by verified compiler/SDK/recipe fingerprints), refuse it if
  it needs glibc newer than 2.35, and verify it under X11 on Ubuntu 22.04 and
  26.04.
- Browser publication requires the shared Bevy UI, native geometry service,
  browser host integration and native/browser conformance evidence.
- Native release guards reject absolute non-system dylib paths, signature
  failures and invalid topology results.

## 8. Current limitations

- `To Face` supports a parallel planar target face.
- `Through All` uses a finite ±1,000,000 mm construction extent.
- Taper is a uniform centroid-scaled loft, not yet production draft analysis.
- Multiple disjoint and nested profile loops are supported. Odd-depth loops are
  cut as holes and even-depth loops become material regions, including islands.
  Ambiguous touching or self-intersecting boundaries are rejected upstream.
- Stable topology IDs persist when the adapter returns the same topology key.
  Topology-changing edits and booleans can intentionally invalidate downstream
  face references; the timeline then reports a broken reference.
- A runnable Bevy browser app remains separate migration work.

## 9. Upstream references

- OCCT build guidance: <https://dev.opencascade.org/doc/overview/html/build_upgrade__building_occt.html>
- OCCT meshing guidance: <https://github.com/Open-Cascade-SAS/OCCT/wiki/mesh>
- Homebrew OCCT formula: <https://formulae.brew.sh/formula/opencascade>
- Microsoft Visual C++ runtime deployment:
  <https://learn.microsoft.com/cpp/windows/redistributing-visual-cpp-files>
- vcpkg binary caching:
  <https://learn.microsoft.com/vcpkg/users/binarycaching>
