# WIP: Bevy 0.20.0 stable and headless widgets

**Draft only. Human manual verification is required before adoption or merge.**
This exploration starts from remote `main` at
`736758f291289ff2bb1942a0405027b44e919997`. The default desktop moves from
`=0.20.0-rc.2` to `=0.20.0` stable. The widget prototype is opt-in and is not
qualified for release. No human UI passes or broad qualification-ledger changes
are claimed by this PR.

## Why headless widgets

Bevy's headless widgets supply behavior without a visual theme. Keep the current
`ViewportUiTheme`, retained nodes, glyphs, control sizes, spacing, localization
and layout. Feathers has useful editor controls, but adopting its default
appearance would require an additional design/parity pass.

| Control | WIP implementation | Remaining adoption work |
| --- | --- | --- |
| Shared buttons and menu items | `bevy::ui_widgets::Button`, `Pressed`, `Activate` | Manual mouse, keyboard, focus and modal parity |
| Shared boolean checkboxes/toggles | `Checkbox`, `Checked`, `ValueChange<bool>` | Manual command-state and accessibility parity |
| Text, dimension and script fields | Existing Bevy `EditableText` plus native adapters | Preserve ordered editor flush, AltGr/Option, IME ownership and multiline behavior before using `TextInput` directly |
| Sliders/ranges | Existing native range adapter | Bevy sliders use `f32`; CAD ranges use `f64`. Verify precision, step rounding, eight-pixel track inset, drag capture and preview coalescing before migration |
| Choice fields/dropdowns | Existing native choice adapter | Preserve current cycling/keyboard behavior and controlled reducer values before using list/menu widgets |
| Workspace tabs and radio groups | Existing controls and selection reducer | Evaluate `TabList`/`Tab` and radio grouping against current focus order and document ownership |
| Dialogs, popovers and scrolling | Existing native layout/modal policy | Verify stacking, clipping, dismissal, focus return and canvas input ownership before changing behavior |
| Orientation dial and CAD annotations | Existing custom visual/geometry controls | Product-specific controls need their current geometry and hit behavior |

## Input and state ownership

The temporary `experimental-ui-widgets` Cargo feature changes shared button and
checkbox behavior in the real native host. It does not start a second document
controller or create a second UI. Build/run a scratch document for manual review:

```sh
cargo run --locked --manifest-path desktop/Cargo.toml --features experimental-ui-widgets
```

The ordered native adapter still admits gestures against the presented layout,
original document, entity generation, binding, disabled state and modal scope.
Admitted presses/releases and activation keys go through the actual Bevy widget
observers. Their resulting `Activate`/`ValueChange` must occur inside that
admission; unadmitted picking/focus events cannot dispatch a native command.
Automatic picking input is disabled in the experiment to prevent a second copy
of the OS gesture from bypassing the ordered adapter. Cancellation retires
Bevy's pressed state on release outside, window deactivation or host cancellation.

Checkbox state is controlled by the existing application reducer. A verified
boolean proposal forwards the original click/key command intent, because some
toggles perform commands such as Fix/Unfix rather than writing a standalone
boolean. `Checked` is projected from the accepted application state; it does
not optimistically change before command acceptance. Direct MCP/accessibility
actions continue through their existing stamped reducer route.

The native host's existing accessibility projection remains authoritative;
widget-required unnamed accessibility nodes are removed to avoid duplicate
controls. This adapter still needs human screen-reader verification.

The feature is temporary. Remove the experiment flag and the replaced adapter
behavior only after manual parity is accepted; unresolved control families
remain explicit work rather than being described as migrated.

## Stable dependency and window-host patch

The desktop pins `bevy`, `bevy_ecs`, `bevy_gizmos` and `bevy_reflect` to
`=0.20.0`; the desktop lockfile resolves the Bevy family to stable versions.
Stable also advances Bevy's WESL dependencies to 0.6.0, requiring native render
verification in addition to compilation.

The published stable `bevy_winit` source files are identical to rc.2. Refresh
the normalized stable manifest and archive provenance, preserving the only
local source patch: explicit service wakes advance schedules without an OS
redraw. Its regression must pass on the upgraded dependency graph. See
[the patch record](../desktop/vendor/bevy_winit/LIMO-PATCH.md).

## Source checks

```sh
cargo check --locked --manifest-path desktop/Cargo.toml --all-targets
cargo check --locked --manifest-path desktop/Cargo.toml --all-targets --features experimental-ui-widgets
cargo test --locked --manifest-path desktop/Cargo.toml --lib --features experimental-ui-widgets native_viewport::interface_shell -- --test-threads=1
cargo test --locked --manifest-path desktop/Cargo.toml -p bevy_winit explicit_service_wakes_advance_app_without_os_redraw_and_return_to_idle -- --test-threads=1
cargo test --locked --manifest-path desktop/Cargo.toml --test bevy_020_widgets
```

The existing native-host CI additionally runs the focused widget regressions
with the experiment enabled on Linux, macOS and Windows. Source/headless checks
do not establish visual or physical UI parity.

Local macOS source validation on 2026-10-14 passed both all-target compile
checks, the CI all-target/all-feature Clippy check, 96 distinct native-interface
regressions across scoped runs, the explicit-wake regression and all three
stable text-widget characterization probes. The windowless native GPU
UI/compositing regression also passed. These results leave the human checklist
below outstanding; they do not qualify supported platform packages or full
custom-shader coverage.

## Required human manual verification

Compare the same scratch fixture on remote main/rc2, stable/default, and
stable/experiment at matched window size and display/interface scale. Record
build identity, expected versus actual behavior and screenshots where useful.
Do not reuse previous qualification passes as evidence for changed controls.

- [ ] Light/dark themes, localization, 100%/200% display and interface scaling:
      labels, glyphs, dimensions, spacing, hover, selected/pressed/disabled
      colors, focus rings, clipping and tooltip placement match.
- [ ] Toolbar, ribbon, dialog and menu buttons activate exactly once on release;
      dragging outside, releasing outside, window deactivation and cancellation
      do not activate. Re-entry while captured matches the baseline.
- [ ] Checkboxes and command toggles reflect accepted state, including rejected
      operations. Rapid repeated clicks and double-click behavior match.
- [ ] Tab/Shift-Tab, Enter/Space, held-key repeat, modified activation keys,
      context menus and menu navigation match. Bevy buttons suppress repeated
      key activation; verify whether that differs from the baseline and resolve
      any difference before adoption.
- [ ] Opening/replacing a modal, switching/reopening a document, reusing a row,
      hiding/disabling a pressed control and model work in progress cannot
      activate a replacement target or leak input into the canvas.
- [ ] Text edits commit in the original event order before button activation;
      dimensions, script editing, clipboard, AltGr/Option and Japanese/Chinese
      IME remain correct through focus and document changes.
- [ ] Existing sliders, range precision/rounding, previews, dropdowns, tabs,
      scrolling, Undo/Redo and save/reopen behave as before.
- [ ] Screen-reader names, roles, checked/disabled state, tab order and activation
      are correct with no duplicate controls or stale focus.
- [ ] CAD rendering, custom WESL shaders, native GPU stock, resizing, occlusion,
      idle service wakes and owned-frame presentation work on supported platforms.

Keep the PR draft/WIP until a human records these results and explicitly accepts
the scoped parity. Broad UI/MCP qualification remains governed by the existing
paused ledger and is not resumed by this exploration.
