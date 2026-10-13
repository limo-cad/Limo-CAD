# Ubuntu 26.04 Linux packaging

Ubuntu 26.04 LTS x86_64 is the Linux package target. Bevy owns the full
native interface and CAD viewport; OCCT owns exact geometry. This draft branch
has removed the React/Tauri desktop host. Final native package qualification is
still required before publishing a release.

For development, `cargo xtask package` selects the Linux builder; see the
[developer guide](DEVELOPMENT.md). Published packages retain their own release
notes and requirements.

## Supported desktop paths

Winit creates the application window directly on X11 or Wayland. Rendering uses
wgpu/Vulkan. Desktop portals supply native file and print dialogs; no WebKit runtime is needed.
Disposable CI uses Mesa lavapipe for correctness, not performance acceptance.

The AppImage is the exception to Ubuntu 26.04 as a build system. An AppImage
bundles the CAD runtime while using the host graphics/window loaders and C
library. It runs only where glibc is at least as new as the build system's. It is
therefore built on Ubuntu 22.04 (glibc 2.35) against OCCT 7.9.3 compiled from pinned source by
`cargo xtask build-occt --prefix PATH`, because Ubuntu 22.04 does not package OCCT 7.9.
Release CI refuses an AppImage that needs a newer glibc, and launches it on both
Ubuntu 22.04 and 26.04. The Debian package also bundles the checked OCCT 7.9.3
runtime in `/usr/lib/limo-cad`. Its executable and OCCT libraries use relative
loader paths, so the package works without the build SDK and cannot silently
load an older system OCCT instead. Graphics/window loaders remain host libraries.

Like glibc, the Wayland client libraries come from the host rather than the
AppImage. The host's Mesa Vulkan and EGL drivers load into the application and
link those libraries; Mesa 26 needs symbols that Ubuntu 22.04's Wayland 1.20
lacks, so bundled client copies stopped every GPU driver from loading on Ubuntu
26.04. The bundler excludes `libwayland-client`, `libwayland-cursor`, and
`libwayland-egl` through linuxdeploy's `--exclude-library` arguments
in the pinned native linuxdeploy builder and fails
if the AppImage contains them. It still bundles `libwayland-server`, which the
application links directly and which is not guaranteed on an X11-only or
minimal desktop. Cross-version verification explicitly installs the host EGL,
Vulkan, and Wayland client loaders because GitHub's Ubuntu runner is a minimal
server image rather than the Ubuntu desktop represented by that runtime
contract.

Winit opens `libX11.so.6`, `libX11-xcb.so.1`, `libXcursor.so.1` and `libXi.so.6`
dynamically. X11 hosts therefore need `libx11-6`, `libx11-xcb1`, `libxcursor1`
and `libxi6`; the DEB declares them, and both AppImage verification desktops
install them explicitly. Removing the unused GTK development SDK exposed a
missing cursor library on the minimal Ubuntu 22.04 runner. These native window
dependencies replace reliance on GTK's indirect packages.

Unicode DXF labels are shaped from installed monochrome fonts and exported as
standard solid hatches, with a hidden original TEXT entity for editing. The
Debian package recommends Noto core and CJK fonts. AppImage users need installed
fonts covering their drawing text; exports report unsupported glyphs instead of
fabricating them. SVG retains editable text and its saved font-family rules.
Native printing resolves whole grapheme font runs with the same monochrome
font policy as DXF, then prints those outlines without silently dropping mixed
script labels.

## Reproducible container build

```sh
docker build \
  -f scripts/docker/ubuntu-26.04.Dockerfile \
  -t limo-cad-ubuntu-26.04 \
  .

docker run --rm \
  -v "$PWD:/workspace" \
  -w /workspace \
  limo-cad-ubuntu-26.04 \
  sh -lc 'cargo xtask package'
```

That container can build both packages. The published AppImage comes from the
Ubuntu 22.04 SDK instead, which compiles OCCT once while the image builds:

```sh
docker build \
  -f scripts/docker/appimage-ubuntu-22.04.Dockerfile \
  -t limo-cad-appimage-ubuntu-22.04 \
  .

docker run --rm \
  -v "$PWD:/workspace" \
  -w /workspace \
  limo-cad-appimage-ubuntu-22.04 \
  sh -lc 'cargo xtask package --bundle appimage'
```

`cargo xtask package --bundle deb` builds only the Debian package.

Both containers build the same checked OCCT 7.9.3 source recipe through Rust xtask.
BuildKit caches retain compiler-compatible SDK objects and Rust dependencies.
The resulting packages carry the repaired native runtime without depending on
distribution OCCT packages.

## Native Ubuntu build dependencies

The authoritative dependency list is in
`scripts/docker/ubuntu-26.04.Dockerfile` and the shared
`.github/actions/setup-linux-desktop/action.yml` used by package and native-host
checks. It includes:

- Desktop portals and their GTK backend for native file and print dialogs;
- Vulkan, Wayland, X11/XKB (including `libxkbcommon-x11-dev`) and udev development files;
- FreeType development files and the checked OCCT 7.9.3 source build;
- Rust (the pinned toolchain); and
- Native packaging utilities including `patchelf`, `file`, FUSE 2 and
  `squashfs-tools` (the AppImage permission audit reads the image with
  `unsquashfs`).

After installing those dependencies:

```sh
cargo xtask package
```

Artifacts are written under:

```text
desktop/target/release/bundle/deb/*.deb
desktop/target/release/bundle/appimage/*.AppImage
```

Each artifact has a neighboring `.sha256` file. The bundler fails if the
project, third-party, OCCT copyright, or LGPL notices are
missing from either package.

Winit loads `libxkbcommon-x11.so.0` dynamically. The DEB therefore explicitly
depends on `libxkbcommon-x11-0`. The bundle script stages that SONAME and its
non-glibc dependency closure in the AppImage's `usr/lib`, together with the
Ubuntu package copyright notices and referenced common-license texts. After
extraction it checks ELF dependencies and resolves them against the bundled
libraries; falling back to an unstaged host dependency fails the audit.

Native packaging is implemented in `xtask/src/package/`. Deleted legacy scripts
and npm aliases have no compatibility wrappers.

## Native package verification

The ordinary package build is the Bevy application; there is no migration flag.
After building, use fresh evidence directories:

```sh
bash scripts/verify-linux-native-package.sh path/to/Limo-CAD.deb /tmp/native-deb-evidence
bash scripts/verify-linux-native-package.sh path/to/Limo-CAD.AppImage /tmp/native-appimage-evidence
scripts/verify-linux-viewport.sh path/to/Limo-CAD.AppImage x11 /tmp/limo-cad-x11
scripts/verify-linux-viewport.sh path/to/Limo-CAD.deb wayland /tmp/limo-cad-wayland
```

The input checks own a private Xvfb/D-Bus desktop and exercise the existing native
keyboard, clipboard, and Bevy window-capture fixture. The Wayland check uses a
private headless Weston compositor and the desktop lifecycle/MCP fixture; it
does not claim physical Wayland keyboard or IME coverage. Package metadata and
recipe URI registration are also checked. These scripts never reuse a user's
open design or display.

## 3D mouse permissions

Linux HID devices can require a distribution udev rule before an unprivileged
application may open their `hidraw` node. Install the vendor's Linux driver or
an administrator-provided least-privilege udev rule for the specific device,
then reconnect it. Do not run Limo CAD as root. Ordinary mouse, touchpad and
keyboard navigation do not require extra permissions.

## Scope

The CI release is x86_64. The source and Ubuntu SDK also compile on AArch64,
but an AArch64 release artifact is not part of the official matrix yet. Package
installation, signing/repository distribution, and automatic udev-rule setup
remain separate release-engineering work.
