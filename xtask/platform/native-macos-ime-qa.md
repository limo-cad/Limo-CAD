# Owned Bevy macOS IME check

Run only on the disposable GitHub-hosted macOS job for repository ID `1313334315`.
The ID remains the same when the repository moves to the limo-cad organization.
The ordinary platform keyboard/clipboard check and Linux libpinyin path remain
unchanged. The explicit Japanese fixture requires
`LIMO_CAD_NATIVE_IME_TEST=macos-japanese`, `--desktop-input`, `--ime-japanese`, and
`--ime-stock-report ABSOLUTE_PASSED_REPORT`. Run the stock prerequisite in the
same job first; its run ID, clean source restoration, and successful exit must
match. Evidence and that report must live beneath `RUNNER_TEMP`.

Build the default native host. This fixture opens a private,
blank child document and uses the existing Rename field. It runs the ordinary
real Command-A, caret, clipboard and Unicode checks first. Japanese input then
uses the installed Apple Romaji/Hiragana source and real CoreGraphics key events:
HARU, Control-J, one Return, another HARU/Control-J, then Escape. If the first
Escape leaves marked yomi, the fixture sends one more Escape. It never sends
another Escape after composition ends, because that would cancel Rename.

The persistent input helper pins the child PID, canonical executable, and
visible normal OS window. It requires existing event-posting permission, keeps
the owned process frontmost before every key, and requires a fresh field/session
focus receipt before every key sequence. It enables only the exact installed
Apple method/mode. It restores the selected source and exact enabled-source set
on completion, error, or stdin EOF; lazily enabled Kana Palette entries are part
of that restoration. It does not change TCC, keyboard preferences, or any
developer desktop. Cleanup failure fails the check. A killed/hung helper cannot
be counted as successful cleanup.

The test-only host observer is enabled by the same disposable-runner opt-in.
`cad_interface inspect` includes at most 256 accepted native `WindowEvent::Ime`
events, each with its value, preedit cursor, editor composition state, document
owner, and control binding. It queries the owned Winit NSView's actual AppKit
input context for source ID, key-window/first-responder state, OS window number,
and backing scale. The observer does not inject events or alter input ordering;
ordinary product inspection contains no trace. Oversized text or event overflow
fails the fixture instead of silently truncating evidence.
Each fresh inspection ID is checked against its current focused control, while
the retained key/binding/document receipt pins the field across inspections;
temporary MCP target IDs are never reused as a persistent focus identity.

Assertions require real Hiragana Preedit while committed text remains empty,
exactly one real Commit after Return, and ended composition without submitting
Rename. A second preedit and Escape must preserve the first committed text and
add no Commit. Command-A must work after cancellation. The fixture restores the
original field, explicitly cancels Rename, and compares the entire shared
project model before/after. No second document or input model is introduced.

Review `ime-preedit.png`, `ime-committed.png`, and `ime-cancelled.png` with their
JSON receipts. `macos-ime-driver.json` retains actual posted keys and source
cleanup; `ime-result.json` includes host/helper hashes, stock-report provenance,
real Bevy events, and limits. Failure logs and cleanup evidence are retained.
Captures use the live product window endpoint only. This check does **not**
prove candidate-popup pixels/placement, physical keyboards, other IME engines,
or mixed-monitor DPI transitions. Those remain separate validation work.

The stock sequence and source lifecycle follow the passed stock probe in this
repository. Apple documents [Japanese conversion keys](https://support.apple.com/en-gb/guide/japanese-input-method/jpim10263/mac)
and [enabling installed input sources](https://developer.apple.com/library/archive/qa/qa1810/_index.html).
