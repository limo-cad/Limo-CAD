# Build and test noBS CAD

To use CAD or connect an agent, [install the application](INSTALL.md). Build from
source when developing the project. Start with current `main`; check out a
release's tag instead when reproducing that release or pairing a source-built
MCP server with a downloaded desktop.

Install Git and the
[Rust toolchain](https://rustup.rs/), then:

```sh
git clone https://github.com/jackControls/noBS-CAD.git
cd noBS-CAD
```

## Build the desktop package

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

Install PowerShell 7, Visual Studio C++ Build Tools with the Windows SDK and your
target architecture, and the pinned vcpkg dependency set from the
[Windows setup](WINDOWS_PACKAGING.md#local-windows-build).

The command selects x64 or ARM64 to match the running Rust toolchain. It uses
the matching `vcpkg_installed/x64-windows` or `vcpkg_installed/arm64-windows`
prefix by default; set `OCCT_ROOT` only when your SDK is elsewhere.

The ZIP is written under
`src-tauri/target/<rust-target>/release/bundle/portable/`.
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

</details>

### macOS SDK (Apple silicon)

Install Xcode Command Line Tools and Homebrew, then:

```sh
brew install opencascade
```

Use a compatible OCCT 7.9.x SDK; `OCCT_ROOT` can select an explicit prefix.
The `.app` and `.dmg` are written under `src-tauri/target/release/bundle/`.
Local builds are ad-hoc signed; production Developer ID signing and notarization
belong to the release workflow. See [OCCT packaging](OCCT_PACKAGING.md) for SDK
overrides and signing details.

### Ubuntu 26.04 SDK (x86_64)

The committed container supplies the reproducible Linux SDK. With Docker installed:

```sh
docker build -f scripts/docker/ubuntu-26.04.Dockerfile -t nbcad-ubuntu-26.04 .
docker run --rm -v "$PWD:/workspace" -w /workspace nbcad-ubuntu-26.04 \
  sh -lc 'cargo xtask package'
```

The `.deb` and AppImage are written under `src-tauri/target/release/bundle/`.
Release AppImages are built on Ubuntu 22.04 instead so they run on older glibc;
see [Ubuntu packaging](LINUX_PACKAGING.md#reproducible-container-build).
The container builds packages; launch them on a desktop with Vulkan support.
For native SDK setup and X11/Wayland checks, use
[Ubuntu packaging](LINUX_PACKAGING.md).

## Verify changes

For shared model and browser frontend changes (run `npm ci` for the web target):

```sh
cargo test --locked --workspace
npm run test:frontend
npm run build
npm run check:knowledge
cargo xtask version --check
```

Node.js and npm are required only for the browser app and repository JavaScript checks.
Native compilation, packaging and version/release guards use Rust directly.

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
cargo test --locked -p nbcad-occt --features native-occt
cargo test --locked --manifest-path mcp-server/Cargo.toml -- --test-threads=1
```

The desktop shell is its own Cargo workspace, so the root `--workspace` command
above does not reach it. With the matching OCCT SDK available, run:

```sh
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

The default desktop build compiles the Bevy interface, Winit host, native sketch
editor, and controller. The temporary `dev-bevy-host` switch and React desktop
host have been removed. The separate browser/WASM frontend is not a desktop
build dependency. The transition remains under validation on draft PR #124.

The native host supports middle-button pan, right-button or Shift+middle-button
orbit, wheel zoom, trackpad pan, Shift+scroll orbit, and pinch zoom. These use the
rendered camera and remain available while the modeling worker is busy. Escape
or loss of window focus ends a camera drag; new navigation interrupts a timed
view transition. An OCC operation that has already started still runs to completion.

Run the complete native modeling lifecycle against an explicitly chosen blank
document in a native desktop build:

```sh
cargo xtask test-mcp native-lifecycle --server /absolute/path/to/nbcad --session BLANK_DOCUMENT_UUID --out /absolute/path/to/fresh-evidence-directory
```

The fixture creates its own tab, draws a rectangle through native controls, checks
Extrude preview/invalid input/edit/Cancel/Apply and Undo/Redo, then saves, closes,
and reopens the `.nbcad` file through native File actions. A separate headless
process recomputes the saved archive to check that it does not depend on live
editor caches. Captures and a JSON report stay in the supplied evidence directory;
partial runs are preserved. This requires a graphical desktop and complements
the library tests; it is not a cross-platform visual parity check.

The native File lifecycle regressions carry over selected ownership and exit cases from
`projectSave.browser.test.ts`, `saveOnExit.browser.test.ts`, and
`applicationExit.browser.test.ts`. They exercise the real ordered worker and
`.nbcad` archives, including failed Save As, cancelled Save-and-close pickers,
same-tab replacement during Save, and partial Save-all failure/retry. Run them
without opening an OS window or file picker:

```sh
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib session_bridge::native_interface::controller::files::tests -- --test-threads=1
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
cargo xtask run-script --recipe fillet-basics --server /absolute/path/to/nbcad --server-arg --headless --repeat 2 --out replay-proof
```

On Windows, use the full path to `noBS-CAD.exe`; on macOS use
`/Applications/noBS CAD.app/Contents/MacOS/nbcad` and quote paths containing spaces.
The [installation guide](INSTALL.md#choose-the-executable-and-try-it) lists the
packaged paths. A standalone `nbcad-mcp` server needs no `--server-arg`.

For an AppImage without FUSE, pass each argument explicitly:

```sh
cargo xtask run-script --recipe fillet-basics --server /absolute/path/to/noBS.CAD_0.2.2_amd64.AppImage --server-arg --appimage-extract-and-run --server-arg --headless
```

`--repeat 2` compares independent headless runs. To watch in an existing CAD
window instead, omit `--repeat` and add `--session UUID --new --present --speed 2`.
The session must identify the intended live document; `--new` preserves it and
opens a blank design tab. Add `--save /absolute/path/result.nbcad` to save that
live result. [Native scripts](native-scripts.md) describes the source and controls.

### Headless editable projects (including CI)

`--save` also works **without** `--desktop` or `--session`. It exports the native
engine's complete project model into the normal `.nbcad` ZIP container, then
reopens those bytes in an independent headless engine. A changed model, failed
geometry recomputation or missing body fails the command before the destination
is written. Existing live-session saves still use the desktop's normal Save.

```sh
cargo build --locked --release --manifest-path mcp-server/Cargo.toml
cargo xtask run-script --recipe garden-bench --server ./mcp-server/target/release/nbcad-mcp --save ./target/demo-projects/bench.nbcad
cargo xtask run-script --recipe d-screw-vise --server ./mcp-server/target/release/nbcad-mcp --save ./target/demo-projects/vise.nbcad
cargo xtask run-script --recipe vertical-axis-turbine --server ./mcp-server/target/release/nbcad-mcp --save ./target/demo-projects/turbine.nbcad
```

Use `nbcad-mcp.exe` on Windows. No display server, browser, desktop session or
virtual framebuffer is needed. Sketches, feature history, assemblies, drawings,
appearances, visibility and CAM intent remain editable; these are not mesh
exports. Generated archives use fixed epoch timestamps for reproducibility and
the MCP engine's application version (which need not equal the xtask version).

The **MCP server** workflow retains `bench.nbcad`, `vise.nbcad`, `turbine.nbcad`
and `demo-projects.json` (source commit, version, sizes and SHA-256 hashes) in
`noBS-CAD-demo-projects-<platform>-<commit>` artifacts after successful tests.
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

This produces `mcp-server/target/release/nbcad-mcp` (`nbcad-mcp.exe` on Windows),
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

It builds/copies the standalone server, backs up the selected client's config,
and preserves unrelated entries. It does not install or configure the packaged
desktop application. See the [source MCP installer guide](agentic/INSTALL_MCP.md)
for supported clients, runtime setup and manual configuration.

</details>

## Browser development

<details>
<summary>For browser/WASM changes and browser regression tests</summary>

The browser is a development and testing host with its own kernel adapter.
It is not required to build or use the native application. Install
[`wasm-pack`](https://drager.github.io/wasm-pack/installer/), then:

```sh
rustup target add wasm32-unknown-unknown
npm run build:wasm
npm run dev
```

Open the Vite address. To build and check the browser bundle:

```sh
npm run build
npm run smoke:wasm
npx playwright install chromium
npm run e2e:browser
```

The `e2e:*` commands in `package.json` select individual feature suites when a
change needs a narrower check.

</details>

## Where the code lives

- Rust crates own project data, sketches, feature history, references, drawings,
  assemblies, kinematics and recompute planning.
- Native OCCT supplies exact geometry through a narrow C++ bridge.
- Bevy owns the native interface and viewport; Winit supplies window integration.
- The MCP server and Rust script interpreter drive the shared product interface.
- The browser host uses the Rust model through WebAssembly and OpenCascade.js
  for solid operations.

See [architecture](proposed-architecture.md), [assemblies](ASSEMBLIES.md),
[drawings](2D_DRAWINGS.md), [the MCP harness](mcp-harness.md), and
[the knowledge library](../knowledge/index.md) for their contracts.
