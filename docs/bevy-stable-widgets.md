# WIP: Bevy 0.20.0 stable and headless widgets

**Draft only. Human manual verification is required before adoption or merge.**
This exploration originally started from remote `main` at
`736758f291289ff2bb1942a0405027b44e919997`. Current main already includes Bevy
`=0.20.0` stable, so the remaining PR change is the opt-in headless widget
prototype. It is not qualified for release. No human UI passes or broad
qualification-ledger changes are claimed by this PR.

The visual acceptance criterion is a comparable appearance and an intact
workflow. Minor differences are acceptable when they do not obscure state,
change command meaning, clip required content, or prevent an existing action.
Pixel identity is useful evidence, but is not an adoption requirement.

Latest focused follow-up: [Snap focus is fixed and verified; large Settings
drags remain blocked](#settings-dragging-and-snap-focus-follow-up).

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
| Sliders/ranges | Standard `Slider`, `SliderThumb`, `SliderStep`, `SliderPrecision` and controlled `SliderValue` | Step-index conversion retains CAD `f64` values; oversized/invalid ranges retain the native adapter. Current source/live checks recorded below. |
| Shared feature dropdowns | Standard `MenuButton`, `MenuPopup` and `MenuItem` over the existing inline choice layout | Existing form reducer owns open/selected state. Other cycling choice fields remain native; no separate popup framework was added. |
| Radio groups and tabs | Standard `RadioGroup`/`RadioButton` and `TabList`/`Tab` | Settings, feature and sketch-origin radios; Assembly tabs. Document tabs keep native navigation after the standard-tab trial failed focus continuity. |
| Dialogs and scrolling | Standard dialog title dragging plus a small resize adapter; existing scrolling/modal policy | See the movable/resizable dialog scope and retained limitations below. |
| Orientation dial and CAD annotations | Existing custom visual/geometry controls | Product-specific controls need their current geometry and hit behavior |

## Input and state ownership

The temporary `experimental-ui-widgets` Cargo feature changes the migrated
control behavior listed above in the real native host. It does not start a second document
controller or create a second UI. Build/run a scratch document for manual review:

```sh
cargo run --locked --manifest-path desktop/Cargo.toml --features experimental-ui-widgets
```

The reworked experiment leaves Bevy's standard picking pipeline enabled. Its UI
backend chooses the control, `Button`/`Checkbox` recognize primary pointer and
keyboard activation, `Hovered`/`Pressed` drive the existing theme, and
`InputFocus` plus Bevy's `TabNavigation` API own focus. A Bevy `GhostNode` holds
the Tab group while leaving the retained UI roots in their original layout. Decorative checkbox squares,
checks and labels are not separate pick targets. The previous `WidgetInput`,
`adapt_input`, `adapt_actions` and synthetic pointer/focused-key events are gone.
The native host bypasses its click recognizer and activation-key path for these
controls; it does not activate the same control a second time.

Tab requests use the native host's existing per-event modifier snapshot with
Bevy's `NavAction::Next` / `Previous`. The automatic Tab plugin reads final
batch key state and failed reverse traversal in the macOS check. This keeps
Bevy's traversal algorithm while preserving Shift on the original event.

The application still chooses eligible controls and Tab order, blocks controls
outside the active modal, and distinguishes primary UI input from CAD secondary
clicks and camera gestures. Published document/binding stamps accompany Bevy's
`Activate`/`ValueChange` notifications into the existing command reducer. A
document change cancels pending pointer state; rebinding or disabling an
unrelated control no longer cancels a dialog-owned gesture. Native text edits are
flushed before queued widget commands are reduced. These are CAD ownership and
transaction responsibilities, not a second widget event implementation.

Checkbox state remains controlled by accepted application commands. Some
checkboxes perform commands such as Fix/Unfix, so a value proposal preserves the
command intent and `Checked` is projected from the accepted state. Text editing, cycling choice fields outside shared feature forms,
document strips and ordinary menus retain their existing adapters. The flag keeps the old default available as the
visual/workflow baseline during this draft experiment.

Migrated controls retain their real Bevy accessibility nodes. The native
projection supplies their localized labels, bounds and command bindings on
those same entities, instead of creating duplicate proxy controls or deleting
Bevy's nodes. Unmigrated controls still use their existing accessibility
projection. Screen-reader behavior remains a separate acceptance item.

The 2026-10-16 results below describe the earlier guarded adapter. They are
historical evidence, not passes for this standard-input rework. The separate
rework checks below use clean builds with PR433 and PR434 locally integrated;
the broad qualification ledger remains paused.

## Standard-input rework, 2026-10-17

PR420 source `8b0051f4` replaces the guarded adapter with standard Bevy picking,
button/checkbox activation, hover/press state, and focus navigation. The local
validation branch combines main `49dec66d`, PR433 `5cde5c0d`, and PR434 `fcbde1b7`.
The dependencies are not included in PR420's published branch.

Fresh native macOS checks on clean integration `62e3572f` verified File/Settings,
light and dark themes, Chinese labels at 125%, scrolling, pointer/Space settings
toggles, two-profile Merge extrusion, 12 mm field editing and single Apply,
earlier-sketch editing, Undo/Redo, save/reopen, and canceling an edited feature.
The saved schema-15 model is exactly equal to the default baseline's model.
The Snap square now toggles successfully; the existing focus retirement after
changing Snap remains, so a subsequent Space does not toggle it until focus is
reacquired. This was an unresolved baseline limitation at that revision; the
[latest follow-up](#settings-dragging-and-snap-focus-follow-up) fixes and verifies it.

Thirteen capture pairs are retained with raw differences and revision identities.
The controls retain comparable labels, layout and sizing. Some pairs have
intentionally different selected settings, preview distances, camera positions,
focus, or browser expansion; those pairs are layout comparisons, not pixel
identity claims. The reopened Extrude dialog differs by 62 pixels within its
640-by-1252-pixel region. Checkbox glyph/focus differences do not block the tested
workflows.

Three regressions were found and repaired: a non-UI focus parent excluded roots
from layout (use Bevy's `GhostNode`); widget input partitioning could consume an
existing native drag release (preserve its current owner); and Shift+Tab moved
forward on macOS (pass the native event's modifiers to Bevy's `TabNavigation`).
Final integration `5ebe37bc` passed strict library/test Clippy, 98 interface tests,
13 native-host tests, and the release package build. Live native input then
verified reverse Tab in Settings, focus across native fields and Bevy controls,
Space activation, release outside Finish Sketch followed by a normal click,
and canvas/slider drags released over buttons. The slider stayed active through
release without closing Settings; subsequent Reset and Close clicks worked.
After editing/canceling and finishing the sketch, the saved model still matches
the default baseline byte for byte. The scoped result is **12 passed behavior
cases and one shared Snap focus issue**, with no pending affected rechecks.

![Default and standard Bevy reopened Extrude dialog](qualification/bevy-widgets-standard-input-20261017/extrude-edit-reopened.png)

![Default and standard Bevy dark Chinese settings](qualification/bevy-widgets-standard-input-20261017/settings-dark-zh-125.png)

MCP was used only for matched-build inspection and image capture. All modeling
input used the user-authorized native app automation. No scripts, replay,
raw inbox writes, or MCP mutations were used. The broad qualification ledger
remains paused; this is a focused comparison, not complete UI or screen-reader
qualification. The feature remains opt-in and the PR remains draft.

See [rework evidence](qualification/bevy-widgets-standard-input-20261017.json).

## Movable and resizable dialogs

With `experimental-ui-widgets`, Settings and the shared solid-feature panels
(Extrude, Revolve, Sweep, Loft, Fillet, Chamfer, Hole, patterns and the other
forms using that panel) can be moved by their title bar and resized from the
bottom-right corner. Position and size last for the open dialog; reopening
starts from its usual placement. The dialog stays inside the client window,
including after changing interface scale or window size.

Bevy's `Dialog`, `DialogDragHandle` and `DialogPlugin` recognize title dragging
and retain pointer ownership. One shared adapter adds a resize handle using
Bevy `PointerDrag`, enforces readable minimum sizes, and keeps CAD's existing
modal layer order. Existing panel painters use the resulting size, preserving
scrolling and Apply/Cancel. Canvas dimension fields remain anchored to geometry.
There is no separate pointer-event recognizer, docking system or saved window
manager. Other bespoke panels and native operating-system file choosers retain
their existing behavior.

The focused source checks pass: 101 interface tests, seven Settings tests,
strict release checks, and a clean V8 macOS build (`3a688963`) with PR433 and
PR434 locally integrated. Real GUI startup exposed duplicate `DialogPlugin`
registration in V7; `bafaf561` reuses the plugin installed by Bevy and adds a
regression test.

Eleven focused live cases passed: Settings movement/resizing, scrolling,
forward/reverse Tab, 125% scaling and default geometry on reopening; Extrude
draft/annotation preservation, scrolling and Cancel; Revolve boundary/minimum
width and validation; apply/undo/save; and operation after disk cleanup. Applying
14 mm from a moved Extrude dialog worked. Undo and Save restored the original
12 mm model byte-for-byte.

One functional item remains open: some large automated Settings drags dismiss
the dialog or focus a different control instead of dragging. Ordinary move and
resize gestures passed, but input delivery versus a product defect is not yet
isolated. Exact minimum-height and host-window-resize live cases remain unrun.
This remains a draft, opt-in prototype; the large-drag issue is not accepted as
a minor visual difference. Native host CI was still running when recorded.
See [dialog evidence](qualification/bevy-dialogs-20261017.json).

![Settings moved and resized using Bevy dialog handling](qualification/bevy-dialogs-20261017/settings-dragged.png)

![Extrude draft retained in a moved and resized dialog](qualification/bevy-dialogs-20261017/extrude-resized.png)

## Rebase onto merged dependencies

PR420 was rebased onto main `5303f9a1` after PR433 and PR434 merged. Its patch
is byte-identical to the pre-rebase patch; formatting and locked manifest
checks pass. The final dependency versions add profile and history fixes
beyond the V8 test snapshot. V8 captures remain historical evidence; the
Extrude apply/undo/save flow needs a targeted recheck on the new combined build.
The existing large-drag issue and unrun boundary cases remain open. This
checkpoint performs no new live UI or MCP qualification. See the
[rebase record](qualification/bevy-rebase-20261017.json).

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

## Focused integration check, 2026-10-16

The resumed comparison uses clean integration revision `da44953a`, combining
main `49dec66d`, the updated PR433 head `5cde5c0d`, PR434 `fcbde1b7`, and PR420
code checkpoint `21e0ef5d`. The dependencies remain locally merged only; they
are not included in PR420's published diff. The earlier initial-window run at
`1e393585` used PR433 `467fe8b9` and is preserved as historical evidence.

Jack authorized native macOS app automation for this focused comparison.
Two separately signed, self-contained Apple silicon apps were built from the
same clean integration revision, with the widget feature disabled and enabled.
Both load bundled OCCT 8.0.1. Native pointer and keyboard input exercised isolated
scratch documents; MCP only inspected and captured their state. The matched
client size was 1360 × 860 logical pixels, at 200% display scale.

The scoped run records **11 passed behavior cases, one shared baseline issue,
and 13 paired visual states**. No new widget regression was observed in these
cases. This is not full UI qualification or human adoption acceptance.

| Exercised behavior | Observed result |
| --- | --- |
| File menu and Settings | Matching labels, icons, disabled entries and single activation |
| Light/English/100%; dark/Chinese/125% | Comparable sizing, selection, focus and translated labels; lower settings remain reachable by scrolling |
| Settings boolean toggle | Pointer switches it off; Space switches it on, verified in the accepted control state |
| Two sketch regions and Merge profiles | Center and ring select independently; merging previews one combined volume |
| Distance edit and Apply | 12 mm is committed before one extrusion is added; one body and two total history features |
| Earlier sketch edit | Later extrusion disappears from the canvas while Sketch1 is edited, then returns on Finish |
| Release outside Finish Sketch | Widget build stays in sketch mode; the canceled gesture does not finish it |
| Undo/Redo and save/reopen | Body and history restore; both saved schema-15 model payloads are exactly equal |
| Feature edit and Cancel | Two profiles, Merge and 12 mm persist; canceling a flipped preview restores the original body |
| Snap palette row/square | Shared issue described below; not counted as a functional pass |

The primary modeling captures match after accounting for different fixture
names and a two-pixel-high history-track line. Settings and File menu captures have
different empty-grid positions; their controls have matching bounds and state.
The reopened feature dialogs are pixel-identical. Additional differences are
confined to the header, browser expansion and history track after the differing
inspection sequences.
Raw pixel differences and capture hashes are retained, rather than described
as a universal pixel-identity pass.

![Default and widget extrusion comparison](qualification/bevy-widgets-20261016/extrude-applied.png)

![Default and widget dark Chinese settings comparison](qualification/bevy-widgets-20261016/settings-dark-zh-125.png)

![Default and widget earlier sketch editing comparison](qualification/bevy-widgets-20261016/history-sketch-edit.png)

**Shared baseline observation:** in both apps, clicking the displayed Snap
square did nothing, while clicking its row toggled it. After the row toggled,
inspection reported no focused control and Space did not toggle it. Snap was
restored in both fixtures. This historical case remains recorded as an issue,
not a retroactive pass. The [latest follow-up](#settings-dragging-and-snap-focus-follow-up)
fixes the remaining focus loss and verifies square clicking and Space.

The updated PR433 integration passed two profile-selection migration tests and
five native OCCT region tests. The earlier integration passed all 97 native
interface tests, including seven widget cases, and the native GPU compositing
regression. Their revisions are recorded separately. Those source checks are
not substitutes for the native interaction evidence.

Live Revolve, the complete control catalog, held-key/re-entry/deactivation edge
cases, full focus traversal, IME, screen readers, other display densities and
Windows/Linux visual checks remain untested in this scope. Native file-chooser
automation needed deliberate key presses on one retry; the verified path opened
successfully afterward. Neither that automation behavior nor earlier window
enumeration failures are attributed to a locked Mac or a product defect.

The [focused evidence record](qualification/bevy-widgets-20261016.json) includes
expected/actual results, build hashes, fixture paths, captures, unresolved
observations and explicit exclusions. The broad UI/MCP ledger remains paused
and unchanged. The feature stays opt-in and this PR stays draft.

## Required human manual verification

Compare the same scratch fixture on stable/default and stable/experiment at
matched window size and display/interface scale. Record
build identity, expected versus actual behavior and screenshots where useful.
Do not reuse previous qualification passes as evidence for changed controls.

- [ ] Light/dark themes, localization, 100%/200% display and interface scaling:
      labels, glyphs, dimensions, spacing, hover, selected/pressed/disabled
      colors, focus rings, clipping and tooltip placement remain comparable. Minor
      differences are acceptable if the workflow remains intact.
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


## Standard sliders, choices, radio groups and tabs

This follow-up remains behind `experimental-ui-widgets`. Sliders use Bevy's
pointer capture, drag and keyboard implementation. The existing theme paints
the track and thumb; the thumb is two pixels larger to match Bevy's measured
travel with the existing eight-pixel inset. The widget range is expressed in
step indices, then converted back to the CAD field's `f64` origin and step.
Nonfinite or more-than-1,048,576-step ranges keep the existing adapter.

Settings theme/language/scale, shared feature radios and sketch-origin radios
use `RadioButton`/`RadioGroup`. Assembly tabs use `Tab`/`TabList` with automatic
selection. Document tabs keep the native strip, including navigation to hidden
tabs and double-click renaming. The standard document-tab trial exposed a focus
lifetime issue; replacing that path would need more custom handling than this
migration permits.

The shared feature forms retain their inline dropdown appearance and use
`MenuButton`, `MenuPopup` and `MenuItem` for input, navigation, dismissal and
focus return. Application code connects menu events to the existing form
reducer and prevents a completed selection from toggling the list open again.
Choice fields elsewhere that currently cycle values retain their existing
behavior. No Bevy fork or alternate pointer recognizer was added.

The focused native-interface, Settings, document and host checks passed: 215
distinct checks, including the standard-widget input schedules and native
document-tab input ownership. Strict release Clippy and formatting checks passed.

Clean native macOS build `54c6a644` passed the Settings slider's track click,
drag/release outside and keyboard endpoints; radio selection by pointer/arrows;
production Tab/Shift+Tab traversal; English/Chinese, light/dark and 125% Settings;
feature-menu pointer/keyboard selection and Escape; feature radios; and Assembly
tab selection and disabled-state checks. Extrude Apply, Undo, Redo, save/reopen
and Cancel preserved the expected model. The saved `model.json` after Undo is
exactly equal to the original fixture, and after Cancel to the applied model.
This also completes the targeted Extrude recheck requested by the rebase record.

The standard document-tab trial retained click, rename and close, but document
switching cleared focus and broke consecutive arrow navigation. One small focus
repair passed its source fixture and still failed in the clean native build
`b9f1a556`. Final source `881ef366` therefore keeps all document tabs on their
original native path and removes the document-specific focus and double-click
adapters. The document-tab painter is byte-identical to the pre-migration source.
The final fallback has routing/source checks; no new live fallback pass is
claimed. Assembly tabs continue using Bevy's standard `Tab`/`TabList` directly.

Eight focused live behavior cases remain qualified for the adopted controls.
The document click case is historical and the document-arrow trial is recorded
as failed, not counted as a pass. This keeps the migration bounded instead of
adding more custom focus machinery.

![Slider pointer and keyboard workflow](qualification/bevy-controls-20261017/slider-workflow.gif)

![Feature dropdown keyboard selection and Escape](qualification/bevy-controls-20261017/dropdown-workflow.gif)

![Assembly tab navigation](qualification/bevy-controls-20261017/assembly-tabs.gif)

![Settings appearance before and after](qualification/bevy-controls-20261017/settings-comparison.png)

The GIFs are labeled sequences of native step captures, not real-time recordings.
The Settings comparison uses historical V8 `3a688963` beside `54c6a644`; layout
and theme remain comparable, with a slightly larger slider thumb. All live input
used the native app, with matched clean read-only MCP inspection/capture. No MCP
mutations or broad ledger changes were made. Other platforms, every feature form,
long dropdown lists and full screen-reader behavior remain unqualified. The prior
large Settings drag issue remains open; Snap focus is resolved in the follow-up below.
See the [focused evidence record](qualification/bevy-controls-20261017.json),
[input model](qualification/bevy-controls-20261017/standard-controls-input.limo)
and [applied model](qualification/bevy-controls-20261017/standard-controls-applied.limo).


## Settings dragging and Snap focus follow-up

Clean native macOS build `46ecca2a` fixes Snap focus through the temporary disabled
state used while a command runs. It keeps the existing Bevy `InputFocus` and
`InteractionDisabled` behavior: disabled controls cannot activate, while an
existing focus resumes when the control becomes available. Hidden, removed,
modal-covered and replaced-document targets still retire. There is no
Snap-specific restoration or second keyboard handler.

![Snap focus before and after](qualification/bevy-settings-snap-20261017/snap-focus-comparison.png)

![Snap pointer and keyboard workflow](qualification/bevy-settings-snap-20261017/snap-focus-workflow.gif)

Five focused live cases passed: clicking the displayed Snap square and Space;
production Tab/Shift+Tab and subsequent Space; Finish Sketch; dependent
Undo/Redo; and save, close and reopen. In this fixture Tab moves from Snap to
Constraint 2: horizontal; Shift+Tab returns to Snap. The saved `model.json` is
byte-identical to the input fixture, and editing the reopened sketch shows Snap
on. The palette layout remains comparable, with a visible retained focus ring.
These GIFs are labeled native step captures, not real-time recordings.

![Finish, Undo/Redo and reopen workflow](qualification/bevy-settings-snap-20261017/snap-model-workflow.gif)

The small dialog repair uses Bevy's existing press/drag ownership, including
child targets such as slider thumbs. A clipped or disabled sibling no longer
clears a dialog-owned pointer gesture. The regression exercises the actual
Bevy event schedules and dialog plugin, but **large native Settings drags still
fail**. On clean trial `a76749a7`, dragging the title from screenshot (1150,270)
to (550,270), with and without a prior title click, left the dialog unchanged
and focused its backdrop. The standard 60 Hz reactive-update trial did not help
and was removed in `46ecca2a`.

![Unresolved large Settings drag](qualification/bevy-settings-snap-20261017/settings-drag-blocker.png)

The repository's qualification rule limits one initial repair investigation to
two repair/rebuild cycles or 30 minutes. That limit was reached. A future bounded
investigation should compare native event delivery with Bevy's final-position
hit selection; this is a hypothesis, not a proven event trace. No extra pointer
recognizer, replay queue or Bevy fork was added. Prior small-dialog passes keep
their original revision identities; no full Settings drag/resize pass is claimed.

All 129 focused checks pass on final source: 109 native-interface, 13 native-host
and seven Settings tests, plus strict release Clippy and formatting. The new
regressions cover disabled-focus retention/activation gating and unrelated-control
changes during standard dialog dragging. The clean packaged GUI and read-only
MCP builds match. Broad UI/MCP coverage stays paused; there were no MCP mutations.
PR420 remains draft and the widget feature remains opt-in.

See the [focused evidence record](qualification/bevy-settings-snap-20261017.json)
and [saved fixture](qualification/bevy-settings-snap-20261017/snap-focus-saved.limo).
