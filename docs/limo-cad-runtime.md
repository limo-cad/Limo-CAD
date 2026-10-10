# Limo CAD runtime identity

The Bevy desktop and MCP share the same Rust engine. The desktop workspace is
`desktop/`; all engine crates use `limo-cad-*` package names and `limo_cad_*`
Rust imports. `cargo xtask` owns builds, packages, WASM bindings and MCP setup.

The Windows deployment is `%LOCALAPPDATA%/limo-cad/bevy/Limo-CAD.exe`.
This is the sole installed executable. GUI launches and MCP workers with
`--headless` use it; legacy application names are registry aliases to this path.

For local Windows iteration, run:

```text
cargo xtask deploy-native --computer-control --restart --launch
```

The task reuses a verified current installation or builds the current checkout,
selects the executable reported by Cargo, stages its SDK runtime and verifies
source and payload hashes before promotion.
The default keeps local crates at optimization level 1 and third-party dependencies
at level 3 with incremental compilation. `--release` selects full optimization.
`--restart` explicitly terminates the installed GUI/MCP workers without saving;
without it, a changed build cannot replace a running installation.
`--jobs N` controls build concurrency and `CARGO_TARGET_DIR` retains the shared cache.

The managed installation's `runtime-manifest.json` binds the executable to its
checkout, source digest and OCCT SDK. Matching source, build profile, channel and
native-control mode reuse the installed executable only after checking every
payload hash and the SDK's headers, linked import libraries, runtime DLLs and
copyright notices. This reuse avoids relinking or replacing the running
installation solely because the Git index timestamp changed. Older manifests
without an SDK content digest rebuild once to establish this provenance.
The standard pinned Rust toolchain supports reuse. Inherited compiler, compiler
wrapper, flag or target/profile overrides go through Cargo; builds made with
those overrides remain ineligible for reuse after the overrides are removed.
SDK digest changes invalidate the OCCT bridge build as well as the installation.
Local `test-mcp`, `verify-package-mcp`, `run-script` and `cad-call` commands prepare
the recorded checkout, then select that canonical executable. Only an explicit
`deploy-native` changes the source binding. A failed build, changed source or
locked promotion stops the command instead of falling back to an old artifact. Hosted GitHub package
checks keep their explicit artifact subjects. Unix local runtime promotion is
not implemented by this task; existing Unix packaging remains available.

`cargo xtask install-mcp` registers `limo-cad`, removes retired CAD server entries
and preserves unrelated servers. Its default builds and promotes the unified
application; it never creates a second MCP installation. `--no-build` verifies
the managed installation, including an explicit canonical `--binary` path.
Machines without a managed installation may register a portable executable with
`--in-place`; managed machines reject other executable bindings.
Reload the client configuration after setup.

```text
cargo xtask install-mcp --clients cursor,codex --no-build --binary ABSOLUTE_LIMO_CAD_PATH --in-place --server-arg --headless --desktop ABSOLUTE_LIMO_CAD_PATH
```

New projects use `.limo` ZIP archives and `limo-cad-project` model/manifest
identifiers. Readers accept existing `.nbcad` and `.tfcad` projects; saving an old
project retains its selected path and migrates its model and manifest without
losing ancillary archive entries. New command scripts use `.limo.jsonc`.
Script parsing remains content based, so existing command files remain readable.
Project-file launch arguments use the same guarded File workflow as UI and MCP
opens. Windows registers `.limo` and `.nbcad` projects with the current executable.

Native computer control is an opt-in `native-computer-control` Cargo feature;
normal builds leave it disabled. `deploy-native --computer-control` enables it
for the local UI audit, and `--no-computer-control` disables it. Automatic managed
rebuilds retain the installed mode. Tool discovery reports the compiled feature
and platform availability.

On Windows, `cad_computer_control` provides real OS mouse and keyboard input for
the owned CAD window. It is also available through `cad_interface execute`, group
`document/session`, operation `cad_computer_control`. Observe returns a one-shot
token and physical client-pixel coordinates. Focus, observe again, capture the
rendered window, then send one input and inspect its visible result. Owner,
build, layout, foreground and occlusion checks reject stale targets. An
`input_sent` receipt confirms Windows accepted input; it does not confirm the
product action. Enigo supplies keyboard, Unicode text, mouse buttons and wheel
input. Window ownership, focus and verified cursor placement use the existing
Windows bindings; direct cursor placement retains multi-monitor support that
Enigo's Windows absolute-move implementation does not currently provide.
`action=move` positions only the pointer at the qualified physical client point;
it sends no mouse-button or keyboard primitives. Its receipt includes the verified
position. Observe again before a subsequent wheel or other gesture; movement
does not claim that a tooltip or any other UI response has appeared.
Wheel deltas are multiples of 120, matching Enigo's whole-notch Windows API.
Pointer actions may hold Ctrl or Shift for one gesture. A drag takes either one
endpoint or one bounded path of at most eight waypoints, with up to 800 ms dwell
per waypoint and 1600 ms total dwell. Optional cancellation releases modifiers,
sends Escape while the button is held, then releases it. The complete path is
qualified before input; ownership, cursor and foreground checks continue during
movement and dwell. Error receipts report partial input and cleanup failures.
Verified start and end pointer coordinates describe OS input, not the resulting
model or UI state.
CAD-owned Windows modal dialogs are observed through `windows-capture` 2.0.1,
with PNG images encoded by that crate's public in-memory image encoder. The image
includes the window frame; `client_to_image_offset` maps physical client points to image pixels.
Native editable controls provide the focus check for Unicode text input.

The `text` action currently supports printable Basic Multilingual Plane (BMP)
characters in Bevy fields, and printable Unicode including non-BMP characters
in native dialogs. An entire Bevy text request containing a character above
U+FFFF is rejected before sending any input, with its unsupported code point;
the tool never silently deletes or substitutes characters. Observation and tool
availability report `printable_bmp` or `printable_unicode` for these targets.
Manual UI comparison preserved `Rust control ✓ 🦌` in the Windows Open filename
field but lost the final non-BMP character in a Bevy field before this guard.
The pinned [Enigo 0.6.1 Windows text implementation](https://github.com/enigo-rs/enigo/blob/b297a14e807abdf818b7809a70b445ade4fc5897/src/win/win_impl.rs#L475-L505)
sends each UTF-16 surrogate as a separate packet keypress. The pinned
[Winit 0.30.13 Windows keyboard adapter](https://github.com/rust-windowing/winit/blob/v0.30.13/src/platform_impl/windows/keyboard.rs#L179-L226)
finalizes text when the next keyboard message is not a character message;
its [text conversion](https://github.com/rust-windowing/winit/blob/v0.30.13/src/platform_impl/windows/keyboard.rs#L587-L610)
cannot form valid Unicode from the separated surrogates. The guard remains
necessary until the input dependency path preserves non-BMP committed text.

Capture uses a scoped `--headless` child of the same installed CAD executable,
without creating a document or a second installed binary. The parent imposes a
three-second capture deadline and terminates only that child if Windows capture
startup or shutdown stalls. Capture rejects changed owners, process lifetimes or
window geometry, and dimensions that cannot be mapped to verified screen bounds.
No external computer-control helper or named pipe is required for this CAD surface.

Windows native-input fixtures use this same MCP path, launching its worker from
the exact owned GUI executable and observing before each gesture. Clipboard-only
fixture operations use `arboard` 3.6.1; they restore text, not arbitrary clipboard
formats. The obsolete generic PowerShell/C# mouse and keyboard driver and its CAM
and script-dialog input helpers have been removed. Specialized PowerShell/C# IME,
UI Automation and print-dialog probes still inspect Windows-specific behavior;
the IME probe retains its qualified keyboard-layout/preedit input and restoration.
Linux XTest and macOS CoreGraphics fixture drivers remain platform-specific.
Windows fixture builds must enable the product's `native-computer-control` and
xtask's separate `native-control-harness` features. Both default to disabled.
The xtask feature compiles its Windows input backend and clipboard dependency;
disabled input requests fail explicitly. Other xtask commands remain available.
Build or run the operator with the feature explicitly:

```text
cargo build --locked -p xtask --features native-control-harness
cargo run --quiet --locked -p xtask --features native-control-harness -- cad-call --interactive
```

The product deployment flag `deploy-native --computer-control` is independent
of the xtask feature and still preserves the installed mode on later rebuilds.

Recipe and knowledge links use `limo-cad://`; old `nbcad://` links remain accepted
at the same restricted parsing boundaries. MCP build metadata is `limo-cad/build`.
Build, SDK, desktop and diagnostic overrides use the `LIMO_CAD_*` prefix.
Old `NBCAD_*` variables are no longer consumed; rerun MCP setup for fresh settings.

The profile is `org.limocad.desktop`. On first launch the complete previous
`org.nbcad.desktop` folder moves atomically, preserving preferences, private
libraries and recovery data. A `LIMO_CAD_CONFIG_DIR` override bypasses migration.
If both profiles exist, startup reports the conflict without overwriting either.

New desktop leases and inboxes use the private `limo-cad-sessions` registry
(with the effective user ID appended on Unix). During deployment, save and close
old desktop windows before restarting all MCP workers. Existing recovery files
and snapshots are preserved; new windows publish only to the new registry.

Packages use `Limo-CAD-<version>-windows-<arch>.zip`,
`Limo.CAD_<version>_amd64.deb`, `Limo.CAD_<version>_amd64.AppImage` and
`Limo.CAD_<version>_<arch>.dmg`. The macOS bundle is `Limo CAD.app`;
Linux uses `limo-cad.desktop` and the `limo-cad` package. Debian replaces and
conflicts with the previous `nbcad` package to prevent parallel installations.

Recorded diagnostic inputs and previously published previews retain the names
and hashes of the actual artifacts. They also exercise project read compatibility.
