# Build and test Limo CAD

To use CAD or connect an agent, [install the application](INSTALL.md). Build from
source when developing the project. Start with current `main`; check out a
release's tag instead when reproducing that release or pairing a source-built
MCP server with a downloaded desktop.

Install Git and the
[Rust toolchain](https://rustup.rs/), then:

```sh
git clone https://github.com/limo-cad/Limo-CAD.git
cd Limo-CAD
```

## Build the desktop package

`rust-toolchain.toml` pins the compiler and formatting/lint components.
`cargo xtask doctor --scope desktop` checks the compiler and OCCT headers/link
libraries without building or opening a CAD window. An explicit SDK override
must be complete and use the OCCT 7.9 ABI; it never silently falls back.

Set up the native SDK for your machine below, then use the same build command
on Windows, macOS and Linux:

```sh
cargo xtask package
```

It selects the existing platform packager, which builds the native application and embedded MCP server, stages dependencies and license notices,
and produces the application package. SDK and signing environment overrides
pass through unchanged. Desktop builds do not require `wasm-pack` or a browser
WASM build. Run commands from the repository root.

### Windows SDK

Install Visual Studio C++ Build Tools with the Windows SDK and your
target architecture, and the pinned vcpkg dependency set from the
[Windows setup](WINDOWS_PACKAGING.md#local-windows-build).

The command selects x64 or ARM64 to match the running Rust toolchain. It uses
the matching `vcpkg_installed/x64-windows` or `vcpkg_installed/arm64-windows`
prefix by default; set `OCCT_ROOT` only when your SDK is elsewhere.

The ZIP is written under
`desktop/target/<rust-target>/release/bundle/portable/`.
Runtime requirements remain those in the installation guide.

<details>
<summary>Build for the other Windows architecture</summary>

Install the other Rust target, Visual Studio target tools and matching vcpkg SDK,
then select it explicitly:

```powershell
rustup target add aarch64-pc-windows-msvc
cargo xtask package --target aarch64-pc-windows-msvc
```

Use `x86_64-pc-windows-msvc` for x64. If `OCCT_ROOT` is set, it must point
to the SDK for the selected target. The `--target` option is Windows-only.

Add `--computer-control` to opt a Windows package into guarded native OS input.
The package workflow enables this mode and runs xtask with
`--features native-control-harness` to qualify keyboard and clipboard behavior
against the extracted executable. Local packages leave computer control disabled
unless the flag is supplied.

</details>

### macOS SDK (Apple silicon)

Install Xcode Command Line Tools and Homebrew, then:

```sh
brew install cmake ninja freetype
cargo xtask build-occt --prefix "$HOME/Library/Caches/limo-cad/occt-7.9.3"
export OCCT_ROOT="$HOME/Library/Caches/limo-cad/occt-7.9.3"
```

The builder checks the source and qualifies the repaired native storage runtime.
The `.app` and `.dmg` are written under `desktop/target/release/bundle/`.
Local builds are ad-hoc signed; production Developer ID signing and notarization
belong to the release workflow. See [OCCT packaging](OCCT_PACKAGING.md) for SDK
overrides and signing details.

### Ubuntu 26.04 SDK (x86_64)

The committed container supplies the reproducible Linux SDK. With Docker installed:

```sh
docker build -f scripts/docker/ubuntu-26.04.Dockerfile -t limo-cad-ubuntu-26.04 .
docker run --rm -v "$PWD:/workspace" -w /workspace limo-cad-ubuntu-26.04 \
  sh -lc 'cargo xtask package'
```

The `.deb` and AppImage are written under `desktop/target/release/bundle/`.
Release AppImages are built on Ubuntu 22.04 instead so they run on older glibc;
see [Ubuntu packaging](LINUX_PACKAGING.md#reproducible-container-build).
The container builds packages; launch them on a desktop with Vulkan support.
For native SDK setup and X11/Wayland checks, use
[Ubuntu packaging](LINUX_PACKAGING.md).

## Verify changes

Use a scoped check for normal development; it compiles without running a suite
or starting the application. Add `--fmt` to check the selected workspace's
formatting, `--clippy` for linting, and `--timings` for a Cargo build report:

```sh
cargo xtask check --scope engine --fmt --clippy
cargo xtask check --scope desktop --timings
cargo xtask check --scope mcp
```

The engine, desktop and MCP retain separate workspaces to keep native SDK
features out of host-neutral builds. Shared engine/tooling dependency versions
live in the root `workspace.dependencies`; member manifests own feature choices.
`deps --scope desktop` reports duplicate versions. `deps --unused` uses
cargo-machete; `deps --advisories` uses cargo-deny. Install either explicitly with
`cargo xtask bootstrap --tool cargo-machete` or `--tool cargo-deny`.

`check --timings` records Cargo's report during the requested build. It does not
start a second benchmark. Reuse a stable `CARGO_TARGET_DIR` for compatible builds;
Cargo checks compiler, profile and flag fingerprints. Separate simultaneous
agents' output directories to avoid target-directory locks, and reuse those
directories rather than creating a fresh one for every run.

Optional `check --sccache` uses the pinned tool from `.cargo/tools.toml` and
prints cache statistics. Install it with `bootstrap --tool sccache`; its local
build disables cloud backends. This invocation disables incremental compilation,
which sccache cannot cache; ordinary development retains Cargo's incremental
defaults. Link steps are still uncached. No linker, optimization level, LTO,
symbol policy or global cache wrapper is changed.

OCCT source builds reuse verified downloads and compatible CMake/Ninja objects:

```sh
cargo xtask build-occt --prefix /absolute/path/to/a/fresh/sdk --cache-dir /absolute/path/to/build-cache
```

`LIMO_CAD_BUILD_CACHE` supplies the default cache location; otherwise it is
`target/limo-cad-build-cache`. The key covers the source checksum, compiler/target,
FreeType inputs, recipe and checked storage replacement content. Interrupted
builds retain objects, while completion receipts are published only after
SDK/library/notices checks and `cargo xtask verify-occt-storage --prefix PATH`
succeed against the actual runtime. An unmanaged
or differently keyed install prefix is preserved; choose a fresh prefix for a
different compiler/recipe. `--sccache` optionally caches C/C++ compilation too.
Source files are checked against the checksum-verified archive before reuse and
again before publishing an SDK; modified sources require a fresh cache directory.
Install-prefix locks prevent concurrent installers even when they use different
cache directories. SDK receipts fingerprint internal symlink targets and reject
dangling or external links.
Docker BuildKit retains Rust and OCCT cache mounts across application source
edits; source mounts do not enter the image. BuildKit/GitHub cache quotas govern
retention; the Rust command does not delete other SDKs or user build directories.

For shared Rust model and interface changes:

```sh
cargo test --locked --workspace
cargo xtask knowledge check
cargo xtask version --check
```

Compilation, packaging, WASM engine checks and repository tasks use Cargo.

Version carriers are covered by `cargo test --locked -p xtask release_tooling::`; see
[Versioning and releases](RELEASING.md) before changing `VERSION`.

For native geometry and MCP changes, with the matching OCCT SDK available:

On Windows, add that SDK's DLL directory to the current shell before running
native tests (use the matching ARM64 prefix for an ARM64 toolchain):

```powershell
$env:OCCT_ROOT = "$PWD/vcpkg_installed/x64-windows"
$env:PATH = "$env:OCCT_ROOT/bin;$env:PATH"
```

```sh
cargo test --locked -p limo-cad-occt --features native-occt
cargo test --locked --manifest-path mcp-server/Cargo.toml -- --test-threads=1
```

The desktop shell is its own Cargo workspace, so the root `--workspace` command
above does not reach it. With the matching OCCT SDK available, run:

```sh
cargo test --locked --manifest-path desktop/Cargo.toml
```

The default desktop build compiles the Bevy interface, Winit host, native sketch
editor, and controller. The temporary `dev-bevy-host` switch and React desktop
host have been removed. The retired browser frontend was not a desktop
build dependency. The transition remains under validation on draft PR #124.

The native host supports middle-button pan, right-button or Shift+middle-button
orbit, wheel zoom, trackpad pan, Shift+scroll orbit, and pinch zoom. These use the
rendered camera and remain available while the modeling worker is busy. Escape
or loss of window focus ends a camera drag; new navigation interrupts a timed
view transition. An OCC operation that has already started still runs to completion.

Run the complete native modeling lifecycle against an explicitly chosen blank
document in a native desktop build:

```sh
cargo xtask test-mcp native-lifecycle --server /absolute/path/to/limo-cad --session BLANK_DOCUMENT_UUID --out /absolute/path/to/fresh-evidence-directory
```

The fixture creates its own tab, draws a rectangle through native controls, checks
Extrude preview/invalid input/edit/Cancel/Apply and Undo/Redo, then saves, closes,
and reopens the `.limo` file through native File actions. A separate headless
process recomputes the saved archive to check that it does not depend on live
editor caches. Captures and a JSON report stay in the supplied evidence directory;
partial runs are preserved. This requires a graphical desktop and complements
the library tests; it is not a cross-platform visual parity check.

The native File lifecycle regressions exercise the real ordered worker and
`.limo` archives, including failed Save As, cancelled Save-and-close pickers,
same-tab replacement during Save, and partial Save-all failure/retry. Run them
without opening an OS window or file picker:

```sh
cargo test --locked --manifest-path desktop/Cargo.toml --lib session_bridge::native_interface::controller::files::tests -- --test-threads=1
```

These controller tests complement live rendered checks; they do not establish
visual, keyboard, or native-picker parity. Browser checks cover the separate web target.

The MCP suite includes complete recipe acceptance tests and can take a while.
Run its native tests sequentially so heavy OCCT operations do not compete for
memory and request deadlines. The **Desktop packages** workflow also checks the
final packages' launch and MCP behavior; a successful compilation alone does
not establish that a distributable package works.

`cargo xtask verify-package-mcp --server CAD_EXECUTABLE --server-arg --headless`
checks the shipped runtime without a display or developer SDK. Add `--desktop`
in a graphical session to also verify default stdio, automatic binding to one
owned window, a constrained sketch, Save, unsaved edits surviving stdin disconnect,
stdout EOF while the window remains alive, and
guarded exit through an exact-session headless worker. The report retains the
saved fixture's path. A second empty window verifies that closing through its
own stdio flushes the acknowledgement before exit. Windows run sequentially;
this check never attaches to another running CAD process.
The checks use private session and configuration directories on every platform,
preserving the developer's recovery and settings. Desktop checks require an
isolated graphical session; CI supplies disposable GitHub-hosted runners.

## Replay a recipe

With this checkout and Rust installed, the replay CLI can use the packaged CAD
application. This path needs no OCCT SDK. Use `--headless` for the worker process
so automated runs do not open extra windows. Pass it separately from the absolute
executable path:

```sh
cargo xtask run-script --recipe fillet-basics --server /absolute/path/to/limo-cad --server-arg --headless --repeat 2 --out replay-proof
```

On Windows, use the full path to `Limo-CAD.exe`; on macOS use
`/Applications/Limo CAD.app/Contents/MacOS/limo-cad` and quote paths containing spaces.
The [installation guide](INSTALL.md#choose-the-executable-and-try-it) lists the
packaged paths. A standalone `limo-cad-mcp` server needs no `--server-arg`.

For an AppImage without FUSE, pass each argument explicitly:

```sh
cargo xtask run-script --recipe fillet-basics --server /absolute/path/to/Limo.CAD_0.2.2_amd64.AppImage --server-arg --appimage-extract-and-run --server-arg --headless
```

`--repeat 2` compares independent headless runs. To watch in an existing CAD
window instead, omit `--repeat` and add `--session UUID --new --present --speed 2`.
The session must identify the intended live document; `--new` preserves it and
opens a blank design tab. Add `--save /absolute/path/result.limo` to save that
live result. [Native scripts](native-scripts.md) describes the source and controls.

### Headless editable projects (including CI)

`--save` also works **without** `--desktop` or `--session`. It exports the native
engine's complete project model into the normal `.limo` ZIP container, then
reopens those bytes in an independent headless engine. A changed model, failed
geometry recomputation or missing body fails the command before the destination
is written. Existing live-session saves still use the desktop's normal Save.

```sh
cargo build --locked --release --manifest-path mcp-server/Cargo.toml
cargo xtask run-script --recipe garden-bench --server ./mcp-server/target/release/limo-cad-mcp --save ./target/demo-projects/bench.limo
cargo xtask run-script --recipe d-screw-vise --server ./mcp-server/target/release/limo-cad-mcp --save ./target/demo-projects/vise.limo
cargo xtask run-script --recipe vertical-axis-turbine --server ./mcp-server/target/release/limo-cad-mcp --save ./target/demo-projects/turbine.limo
```

Use `limo-cad-mcp.exe` on Windows. No display server, browser, desktop session or
virtual framebuffer is needed. Sketches, feature history, assemblies, drawings,
appearances, visibility and CAM intent remain editable; these are not mesh
exports. Generated archives use fixed epoch timestamps for reproducibility and
the MCP engine's application version (which need not equal the xtask version).

The **MCP server** workflow retains `bench.limo`, `vise.limo`, `turbine.limo`
and `demo-projects.json` (source commit, version, sizes and SHA-256 hashes) in
`Limo-CAD-demo-projects-<platform>-<commit>` artifacts after successful tests.
It reuses the native vise/turbine acceptance exports and saves the bench during
its existing replay check. The workflow runs for matching PR/main changes, version
tags, and manual dispatch. These are **Actions artifacts**, not public release
assets: release publication must upload the three files from the matching tagged
commit. This workflow has no release-write permission and does not publish them.

`--init-timeout-seconds` bounds only the MCP handshake (30 seconds by default,
configurable from 1 to 600); long modeling and presentation runs keep their own
normal behavior. Use `cargo xtask run-script --help` or `cargo xtask cad-call --help`
for the complete command options.

## Standalone MCP server

<details>
<summary>For developers who need a separate server binary</summary>

The application always includes local stdio MCP; `--headless` only suppresses its
window. A separate source build is useful when changing the server without
rebuilding the desktop:

```sh
cargo build --release --locked --manifest-path mcp-server/Cargo.toml
```

This produces `mcp-server/target/release/limo-cad-mcp` (`limo-cad-mcp.exe` on Windows),
unless `CARGO_TARGET_DIR` overrides the output directory. This executable starts
headlessly with stdio available and needs no launch arguments.

It requires the same native OCCT runtime as its SDK. On Windows, the matching
SDK's `bin` directory must be in the server process's `PATH`; `OCCT_ROOT` alone
does not configure the DLL loader. Pair live desktop and server builds from the
same source revision.

For supported client presets, use the existing source installer:

```sh
cargo xtask install-mcp --dry-run
cargo xtask install-mcp --clients cursor
```

The source installer builds and promotes the unified Windows desktop, registers
that executable with `--headless`, backs up client configuration and preserves
unrelated entries. It creates no standalone MCP copy. Use
`cargo xtask deploy-native --restart --launch` for build-and-launch iteration;
the managed local drivers select its newly built executable automatically.
Portable installations on any OS use explicit `--binary PATH --in-place`.
See the [source MCP installer guide](agentic/INSTALL_MCP.md) and
[runtime identity](limo-cad-runtime.md) for supported clients and build selection.

</details>

## Browser development

The browser replacement reuses the desktop Bevy UI. The React application and npm
dependencies have been removed. See [the browser host](../web/README.md) for the
remaining Bevy host and browser/geometry-service work. The planned first host
offloads geometry to native Rust/OCCT; an optional in-browser OCCT port is separate.

Install wasm-pack and Chrome to check the existing Rust engine facade:

```sh
cargo xtask bootstrap --wasm
cargo xtask build-wasm
cargo xtask smoke-wasm
```

This checks engine bindings, not a completed Bevy browser application.

## Where the code lives

Repository maintenance runs through the same Cargo entry point on Windows,
Linux and macOS. These commands do not require Node or npm:

```text
cargo xtask audit-icons
cargo xtask knowledge check
cargo xtask knowledge index --check
cargo xtask knowledge site
cargo xtask knowledge media --verify
cargo xtask ci mcp-shard core
cargo xtask ci stage-demo-projects
```

`knowledge index` regenerates the committed index. `knowledge media` stages
the verified public videos into a fresh `_site/media` directory; `--verify`
checks publication without fetching video bodies. Site/demo staging refuses
existing output, rather than merging artifacts from different attempts.
The demo task reads `GITHUB_SHA` and `VERSION` for its provenance receipt.

- Rust crates own project data, sketches, feature history, references, drawings,
  assemblies, kinematics and recompute planning.
- Native OCCT supplies exact geometry through a narrow C++ bridge.
- Bevy owns the native interface and viewport; Winit supplies window integration.
- The MCP server and Rust script interpreter drive the shared product interface.
- The browser replacement shares the Bevy UI and uses a native Rust/OCCT service;
  an optional in-browser geometry backend requires the OCCT WASM port.

See [architecture](proposed-architecture.md), [assemblies](ASSEMBLIES.md),
[drawings](2D_DRAWINGS.md), [the MCP harness](mcp-harness.md), and
[the knowledge library](../knowledge/index.md) for their contracts.
