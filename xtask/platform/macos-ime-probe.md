# Disposable macOS stock-control IME probe

Use the registered workflow on the reviewed feature branch:

```sh
gh workflow run native-host-tests.yml --repo limo-cad/Limo-CAD --ref feat/bevy-interface -f ime-probe-only=true -f ime-probe-macos=true
```

This defaults to inventory of installed TIS input sources, enabled/selected
state, image/session, and TCC preflights. It compiles a small AppKit helper on
`macos-15`; it builds no CAD code. All existing native/package jobs are skipped.
`ime-probe-macos` has no effect without `ime-probe-only`. macOS, Windows probe,
and ordinary native runs use different concurrency groups.

To additionally enable the installed Apple Japanese Romaji/Hiragana source and
exercise it on the disposable runner:

```sh
gh workflow run native-host-tests.yml --repo limo-cad/Limo-CAD --ref feat/bevy-interface -f ime-probe-only=true -f ime-probe-macos=true -f ime-provision-japanese=true -f ime-exercise=true
```

Unlike the Windows capability step, the macOS enable flag only enables the
installed `com.apple.inputmethod.Kotoeri.RomajiTyping` method and its
`com.apple.inputmethod.Kotoeri.RomajiTyping.Japanese` Hiragana mode. Missing or
ambiguous source identity fails with its inventory; it never installs an IME or
guesses another language. The selected source and exact enabled-state set are
restored, including a parent source or Kana Palette enabled as a side effect.
Restoration errors fail the probe. Input-source changes affect the disposable user/session and are
not claimed to be process-private.

Both method and mode state are checked. Run `36343710589` reached verified app
and field focus, but its mode was marked enabled while the containing method was
disabled, and selection returned OSStatus -50. Provisioning now enables the exact
parent first when necessary, retains every newly enabled source for cleanup, and
re-resolves the mode from the enabled-source list after app launch. Each enable
result and the available source IDs are retained; unavailable modes fail before
any key is posted.

The cleanup delta is sampled again after the exercise. Run `36344259249`
successfully enabled the parent and selected Hiragana, then Japanese use lazily
enabled `com.apple.50onPaletteIM` after the provisioning snapshot. Cleanup now
records and disables all newly enabled sources from the final snapshot, while
preserving every source enabled before the probe. Missing prior sources or
remaining new sources fail the exact-set check, with both differences retained.

Both script and executable require the expected GitHub-hosted macOS repository
and numeric run ID, with evidence inside canonical `RUNNER_TEMP`. The wrapper
requires a fresh directory. Do not run this on a personal Mac by spoofing those
guards. No TCC reset, permission prompt, preference-file write, or paste is used.

For the exercise, the wrapper packages the freshly compiled helper as
`StockIMEProbe.app` beneath that evidence directory. A supervisor launches that
exact URL through `NSWorkspace.openApplication`, with foreground activation
requested, a new instance required, substitution disabled, and no Recent Items
entry or permission UI. Only CI guard/provenance variables and the supervisor PID
are forwarded. The returned PID, bundle identifier, and executable path must match
the owned app before its result is accepted. Inventory-only runs remain command
line processes.

This fixes the launch mechanism used by the earlier failed attempts: those
command line helpers reached a running AppKit event loop and a first responder,
but never became the frontmost app or acquired a key window. `activate()` is a
cooperative request and does not guarantee focus. LaunchServices provides the
normal app-launch activation handoff; the probe still fails if macOS declines
activation. It never treats a successful launch or activation request as input
proof.

The shell-launched supervisor posts the real CoreGraphics virtual keys, as the
existing native macOS input driver does. It must already have event-posting
permission before launching the app; it neither requests nor changes TCC access.
The app bundle only receives input. Its separate TCC preflight is retained as
evidence, but the receiving app does not need permission to synthesize events.
This distinction matters on the hosted runner: the first bundled attempt compiled
and launched successfully, then correctly refused to post events from an app
that had no event-posting permission.

Each key is an individually acknowledged, monotonically numbered request, bounded
to 64 events from the probe's fixed virtual-key set. The app verifies its own
foreground PID, key window, and exact first responder immediately before each
request. The supervisor accepts only that PID/window's focus receipt, less than
one second old, and rechecks its live bundle identity, foreground state, and TCC
permission before posting to that PID. Replies record actual OS posting; requests
alone are not input proof. The small JSON mailboxes contain virtual-key codes and
modifier flags, never text or fabricated IME callbacks. A driver failure asks the
app to unwind its normal cleanup and waits up to five seconds; it does not kill
the receiving process.

The owned `NSTextView` logs real `keyDown`, `setMarkedText`, `insertText`,
`unmarkText`, and first-rectangle callbacks while forwarding normal AppKit
behavior. Real CoreGraphics virtual keys type `haru`; Control-J normalizes the
provisional conversion to `はる`. The test requires marked text with no committed
content, one Return commit, a second composition, and at most two Escapes with no
extra commit or remaining marked text. Apple documents Escape both reverting a
conversion to yomi and deleting text awaiting conversion. The actual first
Escape in run `36344259249` left the yomi marked. One further Escape is allowed
only after the field processed the first and its marked/committed text remains
exactly as expected; both real key receipts and the intermediate state are kept.
NSTextView storage includes provisional text, so the committed value is computed
by excluding its marked range. Empty callback
notifications are retained but do not count as text commits.

The field's own input context must acknowledge the selected Japanese source;
each key checks frontmost PID/key window/first responder. The input state machine
has a 30-second deadline. Unexpected Escape/live-conversion behavior is retained
as a failure, never repaired by manually clearing the field. All callback/key
sequences and cleanup status are retained in `report.json`; the wrapper retains
compile/run logs and hashes even on failure. `launch.json` records the owned
process, foreground PID/bundle samples, launch errors, and final child result;
`report.json` records bundle provenance and the field/window activation samples.
`launch.log` holds supervisor output and `probe.log` holds the app's output.
Both reports retain actual-post receipts; the last request and reply remain in
`key-request.json` and `key-reply.json`. Early child failures retain their own
error even if their process has exited before launch identity properties can be
read; no keys are sent on that failure path.
The supervisor retains the independently verified live PID because
`NSRunningApplication` may no longer expose its PID or URLs after exit. A child
that exits before verification can contribute only a failure diagnostic, never
an input or success result. The app records its actual activation policy before
and after any necessary change; an already regular app needs no policy switch.
Launch completion is bounded at 20 seconds and app supervision at 60 seconds,
including its 30-second input deadline. The supervisor requires a completed
success report from that PID after the app exits; a launcher exit alone is not
a pass. The job is bounded at ten minutes.

**`stock-control-ime-feasible` proves only this stock control and IME session.**
It does not validate Bevy, candidate-popup ownership/placement/pixels, physical
keyboard hardware, or DPI transitions. First-rect callbacks are anchor evidence,
not a capture of the real candidate window. No screenshots are taken by this
prerequisite probe.

The Swift/AppKit source cannot be compiled on the Windows development host;
the short CI job must establish SDK compilation and runtime feasibility.

References: [Apple text-input protocol](https://developer.apple.com/documentation/appkit/nstextinputclient),
[input-source enable/disable APIs](https://developer.apple.com/library/archive/qa/qa1810/_index.html),
[input context](https://developer.apple.com/documentation/appkit/nstextinputcontext),
[cooperative activation](https://developer.apple.com/documentation/appkit/passing-control-from-one-app-to-another-with-cooperative-activation),
[workspace app launching](https://developer.apple.com/documentation/appkit/nsworkspace/openapplication(at:configuration:completionhandler:)),
[launch configuration](https://developer.apple.com/documentation/appkit/nsworkspace/openconfiguration),
[Japanese conversion keys](https://support.apple.com/en-gb/guide/japanese-input-method/jpim10263/mac).
