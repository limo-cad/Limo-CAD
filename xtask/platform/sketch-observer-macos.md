# Original CAD cursor recorder

This separate macOS companion observes the unchanged **Limo CAD Sketch Fix Test**
application delivered from source `71d78963`. The newer in-process diagnostic app,
the recording-off copy, and the byte-identical original executable in the State
Check copy all behaved correctly for Jack. The original app still misbehaves on
the first rectangle. Those results do not establish a settings or source-code
cause.

Open **Limo CAD Original Cursor Recorder.app** and use **Open original app**.
Click **Start recording**. On the first run, allow the recorder in **System
Settings → Privacy & Security → Accessibility**, then click Start again. Return
to the original CAD app, draw the first rectangle, and reproduce the change in
dimension response. Return to the recorder and click **Stop recording**.
**Show recordings** reveals the local JSONL file in
`~/Desktop/Limo CAD Original Cursor Recording/`.

The recorder opens only the original app path. It checks the running process's
executable path and the original CAD executable's SHA-256 before recording. It
does not replace CAD files, change preferences, edit documents, send input,
request MCP actions, or install an event tap. It never stops CAD. It only records
cursor/field samples while that exact app is in the foreground. Other apps'
names, content, and cursor activity are not recorded. Files have mode 0600;
capture stops after 15 minutes or 128 MiB. Nothing is uploaded.

## Evidence and limits

- `cursor_screen_before/after` preserve the OS cursor's floating-point Quartz
  screen coordinates without rounding. These are screen points, not device
  motion or the cursor position processed by CAD. Display metadata and AX window
  and field bounds preserve the coordinate context.
- `dimensions` contain the original app's accessible `Drawing …` field values
  with each read's timing and screen bounds. They describe the app's published
  control values, not OCR of the visible raster. If that publication itself is
  stale, the recorder will retain the stale value. No value is inferred from
  rectangle geometry.
- Cursor and field reads come from an independent process. Start/end timestamps
  and cursor samples on both sides bound the polling interval; they do not claim
  one atomic CAD frame or one-to-one OS-event correlation. A slow accessibility
  query can lengthen the interval.
- An optional `viewport_derived_build_71d78963` uses an unambiguous AX content
  group and the known sketch chrome dimensions for this specific build. It is
  explicitly derived, not the engine's viewport measurement. Raw coordinates
  and candidate bounds remain available when this derivation is unavailable.
- `--self-test` checks subpixel/negative-screen coordinate conversion, the
  drawing-field filter, and JSON precision without opening or operating CAD.
  `--permission-status` checks trust without prompting. Passing these checks,
  compilation, static analysis, and signatures does not qualify live capture.

Build into a new directory with
`python3 xtask/platform/package-sketch-observer-macos.py /absolute/new/output`.
The fixed test-app path is relative to the current user's home directory. This
is an investigation helper, not a production CAD feature or a general UI driver.
