# Native transition status

The native Bevy cutover merged in [PR #124](https://github.com/limo-cad/Limo-CAD/pull/124)
on October 13, 2026. Bevy owns the desktop interface; OCCT remains the geometry
engine. There is no React/Tauri desktop host.

The checkpoint diary is retained in the
[pre-merge source](https://github.com/limo-cad/Limo-CAD/blob/c537f0232ab86f28b1be3ee46fa6bf5ff46db5b9/docs/native-transition-status.md).
Source-bound qualification receipts remain in [qualification/](qualification/).
The [coverage ledger](qualification/surface-coverage.json) retains individual
passes, rechecks and the paused UI-first/MCP-second run.

## Qualification limits

Published packages and their source revisions are documented in
[installation](INSTALL.md) and [release notes](release-notes/README.md).
A source merge does not establish physical-device qualification.

- Fresh Windows/macOS Japanese IME evidence for the latest field implementation,
  candidate-popup placement and physical monitor/DPI transitions.
- Physical printing, macOS/Linux OS print dialogs, screen-reader speech,
  actual 6DoF hardware/driver behavior and macOS OS GetURL delivery.
- Broader real-input annotation/joint/gesture workflows. The joint fixture's
  read-only settlement correction has not been rerun; Scripts chooser gestures
  and physical multiline-editor IME are not established by source-level checks.
- Switching sputter attribution. The observed Windows tab/sheet irregularity
  has no matched current-source reproduction or latency benchmark. The optional
  comparison uses native Bevy builds; see [measurement scope](native-switching-measurement.md).

## Browser work still open

The replacement must reuse the desktop Bevy UI. The current Rust WASM engine
facade builds and has focused binding checks; it is not a browser CAD app.
The complete Bevy WASM host, file/storage/dialog services and geometry-service
transport remain unfinished. The planned first browser host offloads geometry
to native Rust/OCCT. The extracted native-engine host is its service-side
foundation. An optional in-browser OCCT WASM backend is separate work; the
native-service approach does not require that port. See [web/README.md](../web/README.md).

## Runtime and saved data

[Runtime identities and migration](limo-cad-runtime.md) documents installation,
preferences and compatibility paths. Projects, session inboxes, heartbeats and
recovery snapshots survive runtime replacement. Retained evidence and saved
models are not disposable transition artifacts.
