# Explicit service wakes and ordered pointer-button updates

This is the published `bevy_winit 0.20.0` source, from Bevy commit
`b3bdd75becaffe9a16677cf5bae5bd6d198f052b`, patched through the desktop's
`[patch.crates-io]`. The original MIT and Apache licenses are retained. The
normalized registry manifest is retained; the registry's separate lockfile and
original workspace-relative manifest are unnecessary here.

Original registry archive SHA256:
`1ef0a5a55524e793ab13536ef244229fa66a4839fae6564df2aa4b3c13956a82`.
The path dependency intentionally has no registry checksum in Cargo.lock; the
repository revision identifies the patched source.

The service-wake change is in `src/state.rs`: application updates may advance for
an explicit user-event wake even when a visible window has not delivered another
OS redraw. CAD uses these wakes for admitted controls, receipt publication,
background work, and render completion. The original gate otherwise permits a
live host to leave an admitted request unfinished indefinitely. Bevy 0.20.0
retains the same gate. Its upstream source files are identical to rc.2, so the
stable upgrade preserves this patch and refreshes only the registry manifest.

Without the pointer-button opt-in below, ordinary window/device events retain the original redraw gate. The host's
indefinite idle wait is unchanged. This patch does not mark a frame as submitted
or presented: CAD's GPU receipt still comes only from its render submission
callback. The regression executes application schedules through repeated explicit
wakes with no intervening OS redraw, then verifies that servicing stops at idle.

Remove this vendored patch when the pinned upstream runner services these wakes
independently of compositor redraw delivery. Recheck the regression when changing
Bevy versions; do not silently refresh these sources.

Scoped validation:

```powershell
cargo test --locked --release --manifest-path desktop/Cargo.toml -p bevy_winit explicit_service_wakes_advance_app_without_os_redraw_and_return_to_idle -- --test-threads=1
```

## Primary pointer-button ordering (experimental widgets only)

`WinitPointerButtonUpdates` is a small opt-in resource, enabled by the desktop
only with `experimental-ui-widgets`. The window runner flushes pending ordinary
app input before a primary-button transition and processes that transition
before accepting later movement. This uses the existing app schedules; it adds
no hit testing, gesture recognizer, synthetic input, replay queue or timer.
Default runners and secondary camera buttons keep the previous batching.

Bevy 0.20 explicitly documents that picking uses only the final hover location
for all events in an input batch. A native trace showed a title press at
(140,103) delivered with a later movement to (720,435.5): the backdrop received
the press and the dialog closed on release. A large resize can similarly hit a
radio option. Processing the button boundary preserves Bevy's standard picking
and gesture ownership; flushing movement before release also keeps its previous
hover map at the release position.

The regression `pointer_button_updates_separate_motion_and_transitions_without_idle_polling`
checks actual runner message delivery, default batching, both button boundaries,
and no idle updates. There is bounded extra schedule work at button transitions;
no zero-cost or broad performance claim is made. Remove this extension when the
pinned Bevy picking pipeline resolves button positions within input batches.
