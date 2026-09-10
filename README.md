# noBS CAD

**noBS means no cloud, no BS.** noBS CAD is fully local,
fully free, and fully open source. It is designed first for mechanical parts,
around the familiar sketch-and-extrude workflow.

> noBS CAD is currently pre-alpha. Download the current macOS, Windows, and
> Ubuntu desktop snapshots from [GitHub Releases](https://github.com/jackControls/noBS-CAD/releases).
> These builds are for testing and feedback, not production-critical work.

![testPiece modeled in noBS CAD](docs/assets/testPiece.png)

*A simple test piece modeled in noBS CAD. Download the editable
[`testPiece.nbcad`](examples/testPiece.nbcad) project or its
[`testPiece.step`](examples/testPiece.step) geometry backup.*

![Fillet, chamfer, and a modeled threaded hole in noBS CAD](docs/assets/fillet-chamfer-threaded-hole.png)

*Fillet, chamfer, and a modeled M12 threaded hole shown together in the
editable feature history.*

## Why this project exists

We are grateful for projects such as [FreeCAD](https://www.freecad.org/) and
for the community work that proved open-source CAD can be serious and useful.
At the same time, we would love an option with a gentler learning curve and
the kind of clear, modern experience people have come to expect from
commercial and cloud CAD platforms.

That is the direction we are exploring with noBS CAD:

- project and modeling data stay on your computer;
- there is no account, subscription, or cloud backend;
- the complete source is public under an open-source license;
- the software is free to use;
- mechanical-part workflows are the priority.

## What works today

noBS CAD now spans part design, technical drawings, and an early assembly
workflow. It is happiest with small and medium mechanical models; the best way
to understand the real boundary is still to try building something useful and
report where it breaks down.

The current application includes:

- a native Bevy/wgpu desktop viewport beneath the React interface, packaged
  with Tauri for Apple-silicon macOS, x64 and ARM64 Windows, and x64 Ubuntu
  26.04 LTS;
- parametric sketches with dimensions, geometric constraints, snapping,
  construction geometry, patterns, trim/extend, and fillet/chamfer tools;
- extrude, revolve, sweep, loft, rib, hole, fillet, chamfer, shell, combine,
  split-body, mirror, pattern, and construction-plane features;
- modeled hole threads for common ISO metric and Unified standards;
- live feature previews, direct viewport manipulators, body/component
  Move/Copy, stable topology references, and an editable design history;
- ISO and ANSI/ASME drawing sheets with aligned projected and derived views,
  semantic dimensions, center geometry, manufacturing annotations, title
  blocks, DXF output, and the platform print/PDF path;
- reusable components and occurrences, multi-body components, nested
  subassemblies, grounding, and rigid/revolute/slider/cylindrical/planar/ball/
  universal/pin-slot/screw joints;
- multi-joint kinematic previews, direct mechanism dragging, named positions,
  motion studies, path export, and native static/sampled interference checks;
- multi-document tabs, undo/redo, project saving and reopening, and unsaved
  change protection;
- local `.nbcad` project files (ZIP archives containing editable part,
  drawing, assembly, and metadata records); and
- STEP import plus AP242 STEP export, including placed visible assembly
  occurrences as flattened exact geometry.

Drawing and assembly are newer than the core part-modeling path. Assembly is
currently deterministic rigid-body kinematics, not a physics or dynamics
engine, and large/closed-loop mechanisms still need broad real-world testing.
Exact OCCT projection, interference, and export behavior belongs to the native
desktop build; the browser build is a development and automated-test surface.

Not every tool or combination is reliable yet. We would especially like
people to try real mechanical parts and whatever else is useful to you. Tell
us where the workflow becomes confusing, where the geometry fails, and which
missing capability would help most.

The `.nbcad` format may still change during pre-alpha, so we recommend
exporting a STEP copy of any design you care about as a backup. STEP preserves
the final solid geometry for use in other CAD software, but not the editable
noBS CAD feature history.

> **🐞 Edge-case hunters wanted.** Pre-alpha CAD gets reliable by breaking on
> purpose. Model something real — or work the
> [challenge list](docs/EDGE_CASE_HUNT.md) of nasty geometry (thin walls,
> tangent faces, grazing fillets, self-intersecting sweeps). Every report
> becomes a regression test. Join the
> [Edge-case hunt](https://github.com/jackControls/noBS-CAD/issues/45).

## Local automation (MCP)

The [MCP server](mcp-server/README.md) uses the same grouped product interface,
Rust planning model and native OCCT adapter as the desktop app. It can own a
headless document for local testing or attach to an explicitly selected live
design. Modeling commands reach that document's owner in order; the desktop
remains the single writer while MCP waits for execution receipts.

Default **dynamic** disclosure advertises a focus-scoped subset (spine + active
and soft packs with TTL) — guidance for agents, not a hard jail. Out-of-focus
tools stay callable. Use `full_static` or `cad_list_all_tools` when a client
ignores `notifications/tools/list_changed`.

Export via MCP: **STEP** (CAD interchange), **STL** (geometry-only mesh), and
**3MF** (preferred for slicers; per-body color/material plus compatible slicer
Metadata hints — not a full pre-sliced project). Session helpers discover and
attach to running windows/documents. Snapshot refresh and explicit submission
remain diagnostic tools; ordinary attached modeling handles that transport.

Details: [docs/mcp-harness.md](docs/mcp-harness.md).

## Build, learn and demonstrate

An authored `.nbcad.jsonc` recipe is a readable sequence of the same modeling
operations used interactively. **Build** runs it at maximum rate. **Teach** adds
chapter notes and stepping through the editable feature history. **Show** renders
the construction with captions and camera transitions. The source and final
checks stay the same across these modes.

Open **Scripts** in the app to choose a recipe or load a source file. The
[recipe library](examples/scripts/README.md) includes the garden bench and short
sketch, extrusion, hole, revolve, fillet and repeated-part assembly examples.
The same Rust runner supports independent headless checks:

```sh
cargo xtask run-script --recipe mounting-plate --server MCP_EXECUTABLE --repeat 2
```

Use `--session SESSION_ID --new --present --speed 2` with a matching desktop/MCP
build to watch construction in a new design tab while retaining the current one.
The generated `.nbcad` remains an editable parametric project.

The bench, printable windmill with optional generator, and small screw vise are
the [three reference targets](docs/recipe-development.md). The bench is runnable;
the windmill and vise currently have design briefs. Full flagship drafting and
the learning interface are still in development. See the
[native review stack](docs/development-stack.md) for the implementation layers.

## 3D mouse compatibility

noBS CAD is compatible with 3Dconnexion SpaceMouse devices. In the browser
development build, the optional hosted 3Dconnexion driver bridge is loaded
only after the user clicks the 3D-mouse control; it is not downloaded during
ordinary startup.

noBS CAD is an independent project and is not affiliated with, endorsed by, or
certified by 3Dconnexion. 3Dconnexion and SpaceMouse are trademarks or
registered trademarks of 3Dconnexion.

3D input device development tools and related technology are provided under
license from 3Dconnexion. © 3Dconnexion 1992 - 2020. All rights reserved.

## We want your feedback

The most helpful contribution right now is simply trying to make a real part
and showing us what gets in the way. Not sure what to try? The
[edge-case hunt guide](docs/EDGE_CASE_HUNT.md) lists the geometry most likely
to break.

For a bug, it helps to include:

- your operating system and the build you tested;
- the exact steps from a new project;
- what you expected and what happened instead;
- a screenshot or short recording for visual problems;
- a small `.nbcad` file when it is safe to share.

Feature requests are welcome too. We want to know what people actually need,
not just guess from a checklist of CAD commands. Please
[open an issue](https://github.com/jackControls/noBS-CAD/issues) with what you
find.

Contributors: see [CONTRIBUTING.md](CONTRIBUTING.md) for a short, welcoming
contribution guide.

## Where we are going

High-level directions (mechanical CAD first) are in
[docs/goals.md](docs/goals.md): reliable foundation, careful **CAM**, additive
**3MF** (with useful color/material metadata) alongside STEP, strong **local
MCP** automation, and **staged** simulation later. Proposed implementation
ideas stay in [docs/proposed-architecture.md](docs/proposed-architecture.md)
until prototyped.

Near-term engineering priorities:

1. Make sketching, solid modeling, drawings, assemblies, history, undo, and
   project-file workflows more dependable.
2. Harden closed-loop and large-assembly solving, connector repair, collision
   workflows, and component editing.
3. Keep improving selection, manipulators, navigation, responsive desktop UI,
   and accessibility across macOS, Windows, and Ubuntu.
4. Improve interactive preview, picking, recompute, and large-model rendering
   performance.
5. Turn reported failures into focused cross-platform regression tests.

In the longer run, we prefer a true native desktop experience. The browser
build is valuable for development and automated testing, but it is not the
intended final product experience.

We would also like to explore a functional, modern CAM workflow for 3-axis
machines. We know that is ambitious and difficult, so we plan to approach it
carefully: start with research and testable pieces, listen to machinists, and
earn trust one operation at a time. We would love to hear from CAM experts!

## Build locally

### Ubuntu 26.04 LTS

Ubuntu 26.04 LTS is the official Linux desktop baseline. The app uses the
native Bevy/wgpu Vulkan viewport inside the Tauri WebKitGTK window. It runs on
X11 directly and on Ubuntu's standard Wayland desktop through XWayland. The
release job produces an Ubuntu `.deb` and a portable AppImage, then
launch-tests both display routes.

The committed container is the simplest reproducible build environment:

```sh
docker build -f scripts/docker/ubuntu-26.04.Dockerfile -t nbcad-ubuntu-26.04 .
docker run --rm -v "$PWD:/workspace" -w /workspace nbcad-ubuntu-26.04 \
  sh -lc 'npm ci && npm run bundle:linux'
```

On a native Ubuntu 26.04 development system with the documented GTK, Vulkan,
and OCCT packages already installed:

```sh
npm ci
npm run bundle:linux
```

See [Ubuntu 26.04 packaging](docs/LINUX_PACKAGING.md) for dependencies,
artifacts, X11/XWayland verification, and 3D-input permissions.

### Windows x64 and ARM64 portable builds

The Windows release path targets Windows 10 version 1803 or newer and Windows
11. It produces a portable ZIP rather than an installer, uses the WebView2
runtime supplied by Windows, and requires Microsoft's centrally installed
matching Visual C++ v14 Redistributable. The desktop build uses the same native
Bevy viewport as macOS, backed by wgpu's DX12/Vulkan support; React and CSS
remain the real menu, tab, dialog, and accessibility interface.

The build itself requires Windows, Visual Studio C++ Build Tools, the Windows
SDK, Rust, Node.js, and the pinned OCCT 7.9.3 vcpkg dependency.
After installing the pinned vcpkg manifest:

```powershell
$target = "x86_64-pc-windows-msvc" # Use aarch64-pc-windows-msvc for ARM64.
$triplet = "x64-windows" # Use arm64-windows for ARM64.
$env:OCCT_ROOT = "$PWD\vcpkg_installed\$triplet"
npm ci
npm run bundle:windows:portable -- -Target $target
```

See [Windows portable packaging](docs/WINDOWS_PACKAGING.md) for the complete
setup, output layout, runtime requirements, and GitHub Actions workflow.

### Native macOS development bundle

The macOS packaging path uses Tauri with OCCT 7.9.x.

```sh
brew install opencascade
npm ci
npm run bundle:macos
```

The resulting ad-hoc-signed development application and disk image are written
to:

```text
src-tauri/target/release/bundle/macos/noBS CAD.app
src-tauri/target/release/bundle/dmg/noBS CAD_0.1.0_aarch64.dmg
```

Development packages intentionally retain Rust symbols for crash diagnosis.
The desktop packaging workflow treats a `v*` Git tag as the production
boundary and sets `CARGO_PROFILE_RELEASE_STRIP=symbols` for macOS, Windows,
and Linux. To reproduce a stripped production package locally, set that variable
before running the platform bundle command:

```sh
CARGO_PROFILE_RELEASE_STRIP=symbols npm run bundle:macos
```

```powershell
$env:CARGO_PROFILE_RELEASE_STRIP = "symbols"
npm run bundle:windows:portable
```

See [OCCT packaging and browser/WASM strategy](docs/OCCT_PACKAGING.md) for
native SDK overrides, the Apple-silicon GitHub Actions build, and packaging
details.

### Browser development build

The browser build is a development and testing environment. It requires
Node.js, npm, a current Rust toolchain, the `wasm32-unknown-unknown` target,
and `wasm-pack`.

```sh
rustup target add wasm32-unknown-unknown
npm ci
npm run build:wasm
npm run dev
```

Open the local address printed by Vite. Create a production browser bundle
with:

```sh
npm run build
```

## Project structure

- React, TypeScript, and Vite provide the DOM interface; Bevy renders the
  native desktop viewport.
- Host-neutral Rust crates own project data, sketches, feature definitions,
  history, stable references, drawing intent, assembly structure, kinematics,
  and recompute planning.
- Native builds use Open CASCADE Technology through a narrow C++ bridge.
- The browser development build uses the same Rust model through WebAssembly
  and OpenCascade.js for solid operations.
- `.nbcad` files are inspectable ZIP archives containing a manifest and model
  data.

Public technical references:

- [Download pre-release desktop builds](https://github.com/jackControls/noBS-CAD/releases)
- [Goals / directions](docs/goals.md)
- [2D technical drawings](docs/2D_DRAWINGS.md)
- [Assemblies, components, and joints](docs/ASSEMBLIES.md)
- [Proposed architecture](docs/proposed-architecture.md)
- [MCP harness notes](docs/mcp-harness.md)
- [Open Knowledge Format bundle](knowledge/index.md)
- [OCCT packaging and browser/WASM strategy](docs/OCCT_PACKAGING.md)
- [Windows portable packaging](docs/WINDOWS_PACKAGING.md)
- [MCP server](mcp-server/README.md)
- [Icon provenance](docs/ICON_PROVENANCE.md)
- [Generated WASM bundle](src/engine-wasm/README.md)

## Verify changes

Start with:

```sh
npm run check:knowledge
cargo test --workspace
npm run build:wasm
npm run build
npm run smoke:wasm
```

Browser regression suites run through Playwright. For example:

```sh
npm run e2e:m2
npm run e2e:hole
npm run e2e:timeline
```

Run the complete browser release regression set with:

```sh
npm run e2e:release
```

Native OCCT and MCP checks require a compatible local OCCT installation:

```sh
cargo test -p nbcad-occt --features native-occt
cargo test --manifest-path mcp-server/Cargo.toml
```

## Contributing

Bug reproductions, usability feedback, tests, documentation, and focused fixes
are all valuable. Please keep pull requests reasonably focused, explain the
user-visible problem or improvement, and add a regression test when the
behavior can be automated.

For a large feature, starting with a discussion will help us agree on the user
experience and model behavior before a lot of implementation work begins.

## Related projects

| Project | License | What we borrow |
|---------|---------|----------------|
| Open CAD Studio and other peer CAD tools | GPL-3 (typical) | **Ideas only.** Do not copy source, assets, or unique implementations. |
| Open CASCADE Technology | LGPL-2.1 + exception | Bundled per [Third-party notices](THIRD_PARTY_NOTICES.md) |
| Bevy | MIT / Apache-2.0 | Bundled per [Third-party notices](THIRD_PARTY_NOTICES.md) |

Peer CAD projects: borrow **ideas**, not code, unless counsel says otherwise.
See [CONTRIBUTING](CONTRIBUTING.md#license--borrow).

## License

noBS CAD is free and open-source software licensed under the
[GNU Lesser General Public License, version 2.1 or any later version](LICENSE)
(`LGPL-2.1-or-later`).

We moved from Library GPL v2 (`LGPL-2.0-or-later`) so GitHub can detect the
license (it has no `lgpl-2.0` in its catalog). Third-party components retain
their own licenses and notices; see [Third-party notices](THIRD_PARTY_NOTICES.md).
