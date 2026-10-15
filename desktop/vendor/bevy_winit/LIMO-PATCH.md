# Explicit service wakes without an OS redraw

This is the published `bevy_winit 0.20.0` source, from Bevy commit
`b3bdd75becaffe9a16677cf5bae5bd6d198f052b`, patched through the desktop's
`[patch.crates-io]`. The original MIT and Apache licenses are retained. The
normalized registry manifest is retained; the registry's separate lockfile and
original workspace-relative manifest are unnecessary here.

Original registry archive SHA256:
`1ef0a5a55524e793ab13536ef244229fa66a4839fae6564df2aa4b3c13956a82`.
The path dependency intentionally has no registry checksum in Cargo.lock; the
repository revision identifies the patched source.

The only source change is in `src/state.rs`: application updates may advance for
an explicit user-event wake even when a visible window has not delivered another
OS redraw. CAD uses these wakes for admitted controls, receipt publication,
background work, and render completion. The original gate otherwise permits a
live host to leave an admitted request unfinished indefinitely. Bevy 0.20.0
retains the same gate. Its upstream source files are identical to rc.2, so the
stable upgrade preserves this patch and refreshes only the registry manifest.

Ordinary window/device events retain the original redraw gate. The host's
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
