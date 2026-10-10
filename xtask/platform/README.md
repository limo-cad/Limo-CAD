# Native platform input checks

On Windows:

`cargo run --quiet --locked -p xtask --features native-control-harness -- test-mcp native-platform --desktop-input --server ABSOLUTE_NATIVE_BINARY --out EMPTY_ABSOLUTE_DIRECTORY`

On Linux and macOS:

`cargo xtask test-mcp native-platform --desktop-input --server ABSOLUTE_NATIVE_BINARY --out EMPTY_ABSOLUTE_DIRECTORY`

Build the native binary; on Windows include `--features native-computer-control`.
The xtask input backend and clipboard dependency require `native-control-harness`;
default xtask builds return an explicit disabled-feature error for Windows input.
Use a disposable desktop: the
fixture focuses its own newly spawned window and uses the system clipboard. It
restores prior text clipboard contents in memory, but not other clipboard formats.
No existing document or window is selected. The fresh process owns its private
session directory and is stopped when the fixture exits.

MCP opens Rename and focuses the existing text field. Shortcut keys then go
through the guarded Rust/Enigo `cad_computer_control` path on Windows, macOS
CoreGraphics, or Linux X11 XTEST (`xdotool`),
into the real Winit event loop. Assertions cover select-all without a literal
letter, selection collapse, OS copy/paste, and Unicode round trips. Captures use
the product's Bevy window capture endpoint. `report.json` describes the event
source and results; captures still require visual review for caret/selection and
layout correctness.

The opt-in `Native sketch visual regressions` workflow runs this fixture on all
three OSes, plus the separate GPU sketch-boundary test. Linux needs `xclip`,
`xdotool`, and a working X11 display; CI runs Xvfb with Mesa at scale factors 1 and
2. macOS requires Accessibility permission to post CoreGraphics events; a TCC
denial is a failing check with a specific error, not a skipped input test.
Windows needs an interactive desktop that permits focusing the owned window.
Its headless MCP worker is the exact GUI executable, with a fresh owner-fenced
observation before each gesture. Specialized Windows UIA, print and IME probes
remain separate instrumentation; ordinary mouse, keyboard, wheel and native
file-dialog input do not use a PowerShell driver. Clipboard fixture operations
use the Rust `arboard` crate and restore text only.

The additional Linux IME run starts its own Xvfb, D-Bus session, and IBus daemon
with a private profile (`run-linux-ime.sh`). XTEST types `nihao` through the real
libpinyin engine; the fixture captures provisional preedit in Bevy, accepts
`你好`, then tests Escape cancellation and restored Home navigation. The QA-only
popup helper verifies that the candidate window belongs to that daemon's process
tree, measures its position against the owned native field at 100% and 200%
scale, and captures only that popup window. It never captures the root desktop
or changes the product's window capture endpoint. Popup bounds catch misplaced
origins; exact caret alignment and glyphs still require reviewing the PNGs.

The ordinary platform run does not exercise IME; Unicode paste is not IME. The
separate IBus run covers that engine on X11 only. Neither run proves other input
methods, physical keyboard layouts, Wayland input, moving between monitors with
different DPI, or visual correctness without reviewing the captured pixels.

The opt-in Linux native-host job also runs owned paper, chamfer, CAM row/WCS,
and CAM geometry/linking fixtures on fresh private Xvfb displays at both scales.
Their launchers require `--desktop-input`, verify the private display before
starting a host, and retain the new window's exact process/session identity.
They never attach to an existing desktop document. The product window captures
and exact history/archive checks still require review; passing XTEST gestures
does not establish mixed-monitor or physical-device behavior.
