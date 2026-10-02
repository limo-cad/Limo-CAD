# OCCT Packaging and Browser/WASM Strategy

Status: implemented for the current macOS, Windows x64 and ARM64, Ubuntu 26.04
x64, and browser baseline.

Use [Install noBS CAD](INSTALL.md) for downloads or the
[developer guide](DEVELOPMENT.md) for SDK setup and `cargo xtask package`.
This document explains the dependency staging and verification performed by
that shared desktop build command.

## 1. Ownership boundary

noBS CAD does not keep separate native and browser CAD models.

```text
Sketch/document state
        │
        ▼
Rust solid planner (definitions, history, IDs, reference validation)
        │ serialized RecomputePlan
        ├───────────────────────────┐
        ▼                           ▼
Native OCCT 7.9.x             OpenCascade.js
`crates/occt`                 `src/engine/occtBrowser.ts`
        │                           │
        └──────── KernelScene ──────┘
                    │
                    ▼
       Rust validation + commit + document DTO
```

`crates/solid` is authoritative for:

- feature definitions and replay order;
- rollback/recompute transactions;
- stable `BodyId`, `FaceId`, and `EdgeId` assignment;
- profile and target validation;
- planar-face references and broken-reference errors;
- mesh, face, edge, and plane DTO validation.

The native and browser adapters are deliberately narrow: construct OCCT shapes,
perform booleans, tessellate, enumerate topology, return `KernelSceneDto`, and
serialize selected live B-reps to AP242 STEP. **STL / 3MF mesh packaging** is
native + MCP only today (`nbcad-export`); the browser adapter throws
`file.meshNativeOnly` until explicit parity work.

On native OCCT, the writer is constructed first, schema index 5 is selected,
and `STEPControl_Writer::Model(Standard_True)` creates a fresh AP242 model
before transfer. Without that final new-model step OCCT silently retains its
default AP214 model.

## 2. Native development

The bridge is implemented with `cxx` in `crates/occt`. It is tested with
OCCT 7.9.3 and accepts any compatible 7.9.x SDK during local development.

Homebrew setup:

```sh
brew install opencascade
cargo test -p nbcad-occt --features native-occt
cargo check --manifest-path src-tauri/Cargo.toml
```

Pinned SDK setup:

```sh
export OCCT_ROOT=/absolute/path/to/opencascade-7.9.3
cargo test -p nbcad-occt --features native-occt
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
3. copies the closure to generated `src-tauri/occt-libs`;
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
src-tauri/target/release/bundle/macos/noBS CAD.app
src-tauri/target/release/bundle/dmg/noBS.CAD_0.2.2_aarch64.dmg
```

Useful manual release audit:

```sh
APP="src-tauri/target/release/bundle/macos/noBS CAD.app"
otool -L "$APP/Contents/MacOS/nbcad"
otool -l "$APP/Contents/MacOS/nbcad"
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

Ubuntu 26.04 LTS is the official Linux baseline. The package uses Ubuntu's
OCCT 7.9 runtime and a full native Bevy/Winit window with Vulkan rendering.
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

The browser host combines two WASM modules:

- `nbcad_wasm`: Rust product engine and recompute planner;
- exact `opencascade.js@2.0.0-beta.b5ff984`: B-rep kernel.

For browser-specific work, use the
[browser development instructions](DEVELOPMENT.md#browser-development).
With `wasm-pack` and the Rust WASM target installed, build and run:

```sh
npm run build:wasm
npm run dev
```

`src/engine/occtBrowser.ts` is loaded dynamically only when a solid operation
first needs it. It consumes the same Rust `RecomputePlanDto` contract used by
the native bridge and returns the same `KernelSceneDto`.

The same adapter owns live browser B-reps and uses `STEPControl_Writer` for
AP242 export through Emscripten's in-memory filesystem. STEP bytes are passed to
the shared frontend file layer; display meshes never participate.

The current production Vite build emits approximately:

- 50 MB raw (about 14 MB gzip) for the full OpenCascade.js WASM module;
- 2.1 MB raw (about 0.7 MB gzip) for the noBS CAD Rust WASM module.

The full prebuilt module is acceptable for current development and browser preview.
Before public browser distribution, generate a custom OpenCascade.js build that
contains only the symbols reached by `occtBrowser.ts`, then lock it by content
digest and run the native/browser conformance suite. Threaded OpenCascade.js is
deferred until hosting provides COOP/COEP cross-origin isolation.

## 7. Version and CI policy

- Local development: Homebrew OCCT 7.9.x is supported.
- macOS CI currently installs the Homebrew OCCT formula, requires it to resolve
  to exactly 7.9.3, and passes its prefix through `OCCT_ROOT`. Move signed
  releases to an immutable 7.9.3 SDK artifact before treating the workflow as a
  reproducible release authority.
- Windows CI: use the committed vcpkg baseline and OCCT 7.9.3 override,
  installed as the dynamic `x64-windows` or `arm64-windows` triplet. Preserve
  vcpkg binary packages in an ABI-keyed CI cache; a cache miss must rebuild
  from the pinned sources.
- Linux CI: build the DEB on `ubuntu-26.04` against Ubuntu's OCCT 7.9
  packages, and verify Vulkan viewport startup under headless X11 and
  Weston/XWayland sessions. Build the AppImage in an Ubuntu 22.04 container
  against OCCT 7.9.3 compiled from pinned, checksummed source
  (`scripts/build-occt-linux.sh`, cached by the script's hash), refuse it if
  it needs glibc newer than 2.35, and verify it under X11 on Ubuntu 22.04 and
  26.04.
- Browser: keep the exact OpenCascade.js package version; upgrades require
  native/browser conformance fixtures and a checked bundle-size report.
- The lockfile is committed with the exact browser-kernel package resolution.
- A release is blocked by absolute non-system dylib paths, signature failure,
  mismatched topology IDs, or divergent native/browser feature results.

## 7. Current limitations

- `To Face` supports a parallel planar target face.
- `Through All` uses a finite ±1,000,000 mm construction extent.
- Taper is a uniform centroid-scaled loft, not yet production draft analysis.
- Multiple disjoint and nested profile loops are supported. Odd-depth loops are
  cut as holes and even-depth loops become material regions, including islands.
  Ambiguous touching or self-intersecting boundaries are rejected upstream.
- Stable topology IDs persist when the adapter returns the same topology key.
  Topology-changing edits and booleans can intentionally invalidate downstream
  face references; the timeline then reports a broken reference.
- Public-web payload optimization and native/browser fixture automation remain
  release hardening work.

## 8. Upstream references

- OCCT build guidance: <https://dev.opencascade.org/doc/overview/html/build_upgrade__building_occt.html>
- OCCT meshing guidance: <https://github.com/Open-Cascade-SAS/OCCT/wiki/mesh>
- Homebrew OCCT formula: <https://formulae.brew.sh/formula/opencascade>
- Microsoft Visual C++ runtime deployment:
  <https://learn.microsoft.com/cpp/windows/redistributing-visual-cpp-files>
- vcpkg binary caching:
  <https://learn.microsoft.com/vcpkg/users/binarycaching>
- OpenCascade.js prebuilt workflow: <https://ocjs.org/docs/app-dev-workflow/pre-built>
- OpenCascade.js custom builds: <https://ocjs.org/docs/app-dev-workflow/custom-builds>
- OpenCascade.js file size notes: <https://ocjs.org/docs/getting-started/file-size>
