# Bevy 0.20 UI and code reduction experiment

Historical experiment notes. The current desktop uses Bevy `0.20.0-rc.2` by
default and has retired the old shell. See [native transition status](native-transition-status.md)
for current implementation and package evidence.

- Branch: `feat/bevy-020-code-savings`
- Baseline: `ad01489d2bade99d41941d7def5495abced66fb4` (the Bevy 0.19.1 upgrade in PR #153)
- Updated: 2026-09-22

## Result

PR #153 now runs on **0.20.0-rc.1**. The native workbench follows the React
frontend's ribbon, tabs, orientation dial, navigation toolbar and history strip.
React serves only as the visual reference; these controls are rendered in
Rust/Bevy. The experimental Light editor has been removed from the native
workbench. The default release build still retains the old shell until the
remaining native workflows and release cutover are complete.

The initial compatibility port removed **31 net lines** across its changed
production Rust files. This subsequent UI experiment adds presentation and
integration code; it is not a further code reduction. BSN and Feathers are useful
building blocks, but upgrading alone does not improve the design or remove the
CAD input adapter. This experiment replaces the earlier 0.19.1 proposal in PR #153.

The dependency versions are pinned to the exact release candidate. This is
still a [prerelease](https://github.com/bevyengine/bevy/releases/tag/v0.20.0-rc.1).
Bevy itself declares Rust 1.96.0, but the resolved WESL 0.4.4 dependencies require
1.97.1. This branch pins Rust 1.97.1 locally in `rust-toolchain.toml`; it does not
change the machine's default toolchain. The renderer dependency moves to wgpu 30.

## Native UI experiment

- **Shared ribbon catalog:** Profile, Build, Refine, Repeat, Body, Reference,
  Check, Assembly and Select use the React catalog and English labels. Secondary
  buttons move into their group menus as space shrinks. Existing actions retain
  their controller bindings; unavailable commands are visibly disabled.
- **Presentation:** the React dark gray/iris palette, centered button captions,
  shared SVG icons, compact tab cards with an active top edge and dirty marker,
  and the 48-pixel history footer replace the first experiment's layout. Feature
  forms retain clearer headings and an explicit primary Apply action.
- **Viewport controls:** the orientation dial projects the live camera's XYZ
  axes, provides seven view presets and supports pointer orbiting. The floating
  toolbar is centered within the canvas and supports latched Orbit, Pan, Zoom
  and Zoom Window tools, plus Fit and selection. These actions change the camera
  without changing the CAD model.
- **Menus and tabs:** File gains icons and shortcut hints; native group menus
  include the catalog's full command list. Tabs have individual close controls,
  arrow-key switching and double-click rename. Closing an inactive dirty tab
  uses the existing document ownership and unsaved-changes confirmation flow.
- **History:** a title/count card, transport controls, rounded feature chips,
  rollback marker and selected/error/suppressed states follow React's structure.
  Existing edit, delete and rollback actions remain attached to the same history.
- **Viewport lighting:** a camera-relative directional key and softer fill make
  adjacent model faces distinguishable. The rig now uses a fixed default level.
  The temporary Feathers Light editor and its command/input plumbing were
  removed after comparison with the target workbench.
- **Spacing and File chrome:** command captions use the reference's 8-pixel
  type and centered label slot, ribbon headings share their arrows' flex row,
  and group borders contribute to responsive width calculations. The File
  button uses the bordered NB badge, and its menu preserves the reference's
  section order, full export inventory, row height and shortcut alignment.
  Long tab titles stay clear of their close buttons, and File/history popups
  have an explicit border and shadow. Rename/Save dialogs show a primary action.
  Unavailable native commands remain disabled.
- **Idle accessibility activation:** the candidate's AccessKit handler queues
  actions without waking the indefinitely sleeping Winit loop. A weakly owned
  queue watcher wakes it only when an assistive action exists; ordinary idle
  documents still do not render continuously. The watcher checks every 40 ms
  and retires with its window. Its regression covers idle, queued input and
  owner retirement; actual macOS accessibility clicks now open File immediately.

The native UI retains the ordered CAD text adapter, units/formulas and guarded
controller actions. The 0.20 candidate widget probes remain useful integration
experiments. The former Light editor demonstrated BSN/Feathers, but is not a
product feature and is no longer installed in the native application. Standard
Bevy widget focus preservation remains covered independently.

Visual coverage is not feature parity. Drawing/CAM workspaces, Scripts,
Open script, Import STEP, Export and Settings remain disabled in the native
chrome. Display/Grid settings are also disabled in the React reference. History
drag reordering and dragging the rollback marker are not implemented here.

## Initial port reductions (commit 23fe206)

| Production area | Baseline lines | Experiment lines | Net change |
| --- | ---: | ---: | ---: |
| `interface_shell/fields.rs` | 818 | 777 | -41 |
| `interface_shell.rs` plus new `interface_shell/geometry.rs` | 1,743 | 1,749 | +6 |
| `winit_host/submission.rs` | 51 | 55 | +4 |
| Total | 2,612 | 2,581 | **-31** |

Counts include comments and blank lines in these files. Tests, documentation,
manifests and lockfiles are excluded. Some field savings are ordinary cleanup
of duplicated checks, rather than capabilities unique to 0.20.

- Fields use `TextEdit::is_destructive()` for mutation classification and a
  single read-only gate. IME preedit remains provisional; committing it records
  the draft's undo boundary.
- `EditableText.viewport.offset` replaces the removed `TextScroll` component
  in pointer selection and IME positioning. Native fields explicitly require
  the UI layout components that `TextInputPlugin` would otherwise supply.
- Controls and painted panels share `HitArea`, which uses the renderer's
  public `clip_polygon` function and `CalculatedClip::contains_point`.
  Inspection publishes the clipped polygon's enclosing rectangle; pointer
  input checks the actual shape. Nested transforms, fully clipped nodes and
  unbounded overflow axes use the same clipping rules as Bevy's renderer.
- Render receipts query the primary `ExtractedWindow` component after the
  removal of the `ExtractedWindows` resource. They still require both swapchain
  texture and view; semantic layout alone cannot certify submission.

## Why the full text adapter remains

The new [`TextInput`](https://github.com/bevyengine/bevy/blob/v0.20.0-rc.1/crates/bevy_ui_widgets/src/text_input.rs)
does not replace our document ownership, commit validation, rejected-draft
handling, draft undo/redo, MCP equivalence or event ordering. The three executable
probes in `desktop/tests/bevy_020_widgets.rs` confirm these integration limits:

1. Ordinary typing queues an edit without immediately changing the buffer.
   An owned submit still needs an explicit flush before reading the value.
2. With AltGr represented by Control/Alt/AltGraph, or Option represented by Alt,
   the candidate does not insert the supplied character text. Our adapter needs
   to preserve those input paths. This is a synthetic event probe, not a claim
   of validation with every operating system's keyboard event sequence.
3. Batched IME delivery uses the focus present when the Bevy system runs.
   Changing focus after queuing a commit delivers it to the later field.
   The controller must preserve the original document/control owner and process
   typing, focus changes and model actions in their original order.

The direct `bevy_ui_widgets` dependency supports the probes. The CAD event
router continues to use `EditableText`; CAD fields do not carry `TextInput`,
so a standard widget handler does not also edit them. Passing the probes
records the candidate's current behavior; it does not establish input parity.

## Validation

On macOS arm64 with OCCT 7.9.3:

- Native library: 297 passed, 2 existing ignored tests.
- Native startup: 2 passed.
- Candidate widget probes: 3 passed.
- Default desktop library: 177 passed, 2 existing ignored tests.
- Default desktop startup: 2 passed.
- Native application build passed. The initial port also passed `cargo check`
  for all targets with native-host and UI-lab features.
- Visual-parity pass: compared React and the actual native macOS window in dark
  appearance at 1360 × 860 logical pixels. Also resized the native window to its
  supported 1200 × 760 minimum and a larger 1710 × 959 window. Inspected all nine
  ribbon groups, File and Build menus, orientation dial, centered navigation,
  tab strip and empty/populated history lanes. Direct pointer checks covered
  tab creation, double-click rename, arrow-key switching, the inactive dirty-tab
  close guard, history context menu, camera preset/orbit/Pan and Escape.
- Final build: the 14-step native lifecycle passed: New, rectangle sketch, invalid distance,
  preview, Apply, edit preview, Cancel, edit Apply, Undo, Redo, Save, close, open,
  and independent cold archive recomputation. The final solid was 35 mm high.
- Initial 0.20 baseline: real pointer-selected text entry committed Light = 1.35. Real mouse scrubbing
  changed it to 2.00; before/after camera values and the exported model hash were
  identical. MCP restored the default value of 1.00.

The six added production regressions cover read-only selection, provisional
IME history, scrolled pointer/IME coordinates, fully clipped controls, nested
clips at changed DPI, and rotated clipping for controls and panel occlusion.
Existing ownership, file lifecycle and modeling tests remain in the native run.
The earlier UI trial added numeric validation and standard-widget focus
regressions. The numeric control and its test were removed with the Light editor;
the independent accessibility focus regression remains.
The visual-parity phase adds menu/disabled-command and responsive-ribbon
coverage, model-preserving navigation checks, and an inactive-tab close guard.
The first lifecycle rerun exposed a renamed Isometric control; the dial now
reuses the original control and accessible name, and the final live rerun passes.
Live preset/orbit/Pan checks changed camera values while retaining an identical
exported model hash. Escape returns the toolbar to Select. The default count is
still 297 native tests: removing the temporary Light validation test and adding
the accessibility wake regression leaves that total unchanged.

`cargo xtask audit-icons` and `cargo xtask version --check` pass. The icon audit excludes
only the retained `LICENSE.lucide` notice from SVG validation, while continuing
to inspect every vector source. All eight CI checks passed for the preceding
visual-parity commit `ad80f5b`; new pushes must receive their own CI results.

Reproduce from this worktree, with `OCCT_ROOT` set for the local installation:

```sh
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0
cargo check --locked --manifest-path desktop/Cargo.toml --all-targets --jobs 1 --target-dir target/bevy-020
cargo run --locked --manifest-path desktop/Cargo.toml --bin limo-cad --target-dir target/bevy-020
```

Live evidence was collected from an isolated native macOS app bundle using the
built `limo-cad` executable and a private session directory, not browser rendering.
The lifecycle fixture is `xtask test-mcp native-lifecycle`; its JSON report and
screenshots are local artifacts, not tracked product assets.

An earlier capture after reopening omitted several group-caption glyphs and
part of the experimental numeric text. This was not reproduced in the final
native lifecycle captures or inspected window. The earlier cause is not
established; keep document-transition capture consistency in upgrade coverage.

This is not full accessibility, system clipboard, modifier-key or OS IME
certification. Automated macOS Cmd+A delivery inserted a literal `a` instead of
selecting the field; investigate modifier-event delivery before release cutover.
Plain typing, pointer selection and Escape are independently covered. These runs
make no performance claim. The new UI
has not been exercised on Windows or Linux. The minimum logical window size is
covered; changing monitor DPI is still pending.

## Next experiments, in order

1. Expand native validation to changed DPI and Windows/Linux, and retain
   document-transition capture checks. Finish actual modifier-key, clipboard,
   assistive-technology and OS IME certification before making this the release
   UI. Native lifecycle, populated history, tabs and minimum-size checks now pass.
2. Consider Feathers for a real product field only after keyboard shortcuts, clipboard,
   assistive technology and OS IME work with the existing event ordering. Keep
   CAD unit/formula parsing and owned draft commits intact; measure integration
   cost before converting dimension fields.
3. Only replace the text-input router after an adapter passes ordered typing /
   Tab / typing, IME / submit, stale-document, read-only, clipboard, undo and MCP
   tests without duplicate event delivery. Measure the final net deletion.
4. Revisit the upgrade decision after those results and a stable 0.20 release.
