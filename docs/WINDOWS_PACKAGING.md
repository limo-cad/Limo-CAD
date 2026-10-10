# Windows portable packaging

Status: experimental x64 and ARM64 portable release paths.

To use the application, follow [Install Limo CAD](INSTALL.md#windows).
For development, `cargo xtask package` selects the Windows portable builder;
[the developer guide](DEVELOPMENT.md) is the shared build entry point.

## Supported baseline

The Windows packages have the following compatibility targets. See the
[installation guide](INSTALL.md#windows) for the tested package baseline;
the Windows 10 target is not a verified minimum for this preview.

- Windows 10 version 1803 or newer, or Windows 11;
- x64 (`x86_64-pc-windows-msvc`) and ARM64 (`aarch64-pc-windows-msvc`);
- a portable ZIP rather than an installer;
- a graphics adapter and driver supporting Direct3D 12 or Vulkan;
- the matching centrally installed Microsoft Visual C++ v14 Redistributable.

The Visual C++ runtime is not copied app-locally; use the centrally installed
Redistributable for servicing updates. This branch's Bevy desktop has no
WebView2 runtime dependency. Packaged native validation remains required before
release; published older packages retain their documented requirements.

Permanent Microsoft Redistributable downloads:

<https://aka.ms/vc14/vc_redist.x64.exe>

<https://aka.ms/vc14/vc_redist.arm64.exe>

## Native viewport architecture

Bevy owns the full application window and renders OCCT tessellation through
wgpu's DX12/Vulkan backends. Winit owns OS input, IME, and window/DPI events;
the native controls expose AccessKit accessibility. There is no embedded browser,
child viewport composition, DOM input relay, or React desktop shell.

## Reproducible dependency set

The root `vcpkg.json` pins the vcpkg registry and overrides Open CASCADE
Technology to 7.9.3, the same OCCT line used by the macOS build. The manifest
selects the reviewed storage fix from `native/occt-overlay` at port revision 2.
Native SDK qualification compiles a bounded allocation/copy probe against the
actual `TKMath` runtime; an unchanged header or runtime fails before packaging.
The Rust task discovers the host's MSVC compiler and Windows SDK and supplies
them to both CMake configuration and compilation. A fresh shell does not need a
prior developer-shell setup, and an ambient MinGW compiler cannot qualify an
MSVC SDK. Cross-built packages still need runtime qualification on their target.
The manifest
installs a dynamic Windows prefix for the selected target under:

```text
vcpkg_installed/x64-windows
vcpkg_installed/arm64-windows
```

The portable packager copies every DLL from that prefix's `bin` directory.
Because the prefix is created from an isolated manifest, this is the complete
runtime set for OCCT and its selected dependencies rather than a collection of
DLL names maintained by hand.

## Local Windows build

Install the Visual Studio C++ Build Tools (including the architecture
you are building), a current Windows SDK and Rust. Clone vcpkg
into `.vcpkg` and select the commit pinned by `vcpkg.json`:

```powershell
git clone https://github.com/microsoft/vcpkg.git .vcpkg
$sdkBaseline = (Get-Content vcpkg.json -Raw | ConvertFrom-Json).'builtin-baseline'
git -C .vcpkg checkout $sdkBaseline
```

If `.vcpkg` already exists, use that checkout and select the pinned commit instead
of cloning it again.

Use the matching Visual Studio Developer PowerShell, with its CMake and Ninja
tools available, then select the Rust target and vcpkg triplet:

```powershell
# x64; substitute aarch64-pc-windows-msvc and arm64-windows for ARM64.
$target = "x86_64-pc-windows-msvc"
$triplet = "x64-windows"

rustup target add $target

.\.vcpkg\bootstrap-vcpkg.bat -disableMetrics
.\.vcpkg\vcpkg.exe install `
  --triplet $triplet `
  --x-manifest-root="$PWD" `
  --x-install-root="$PWD\vcpkg_installed"

$env:OCCT_ROOT = "$PWD\vcpkg_installed\$triplet"
cargo xtask package --target $target
```

The command compiles the release native executable without creating an
installer, gathers the native runtime DLLs and license notices, and writes:

```text
desktop/target/<rust-target>/release/bundle/portable/
├── Limo-CAD-0.2.2-windows-<architecture>/
├── Limo-CAD-0.2.2-windows-<architecture>.zip
└── Limo-CAD-0.2.2-windows-<architecture>.zip.sha256
```

The directory contains `Limo-CAD.exe`, the OCCT dependency DLLs, a runtime
requirements README, and license notices. It does not contain the
Microsoft Visual C++ runtime.

Once the native SDK is configured, `cargo xtask package` alone selects the
running Rust toolchain's architecture. An explicit `--target` is useful for
building the other Windows architecture; `OCCT_ROOT` must match that target.
The header is checked when cross-building; qualification of the actual runtime
requires running the probe on that target before publishing the package.

Native packaging is implemented in `xtask/src/package/`. Deleted legacy scripts
and npm aliases have no compatibility wrappers.

## GitHub Actions

`.github/workflows/desktop-packages.yml` runs an x64 job on `windows-2025`
and an ARM64 job on the GitHub-hosted `windows-11-vs2026-arm` runner for pull
requests to `main`, version tags, and manual dispatches. Both jobs:

1. checks out the pinned vcpkg registry;
2. restores an ABI-keyed vcpkg binary cache when one is available;
3. installs OCCT 7.9.3 for the matching `x64-windows` or `arm64-windows`
   vcpkg triplet, compiling only on a cache miss;
4. creates the portable ZIP;
5. launches the packaged executable long enough to catch missing DLL or
   native graphics startup failures;
6. uploads the ZIP and SHA-256 file for seven days.

The binary-cache key includes the pinned dependency manifest, overlay sources,
vcpkg configuration and installed MSVC toolset version. A bounded native probe
must pass before an installed-tree cache is saved or reused for packaging.
The first run for a new combination compiles OCCT and
stores vcpkg's binary packages; subsequent runs restore those packages instead
of rebuilding OCCT from source. GitHub scopes pull-request caches separately,
so a manual run on `main` seeds the default-branch cache that future branches
can reuse. Cache eviction or a dependency, triplet, or toolset change causes a
safe rebuild rather than reusing an incompatible binary.

The workflow intentionally uses a standard GitHub-hosted runner, which is free
for this public repository. Short artifact retention keeps temporary storage
bounded.

## Current limitations

- The portable executable is not yet Authenticode-signed, so Microsoft
  SmartScreen can warn when it is downloaded.
- 32-bit builds are not produced.
- The Visual C++ Redistributable is a documented prerequisite rather than an
  installer-managed dependency.
- The portable ZIP has no shortcuts, file associations, updater, or
  uninstaller.

## Upstream references

- Microsoft Visual C++ runtime deployment:
  <https://learn.microsoft.com/cpp/windows/redistributing-visual-cpp-files>
- vcpkg binary caching:
  <https://learn.microsoft.com/vcpkg/users/binarycaching>
