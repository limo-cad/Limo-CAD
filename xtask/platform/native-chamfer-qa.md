# Native chamfer authoring fixture

Use the existing fixture-owned native host and fresh evidence directory. The
default Cargo build includes the native UI with exact Bevy `=0.20.0-rc.1`.

Set `LIMO_CAD_NATIVE_CHAMFER_ONLY=1`, then run the existing driver command:

```
xtask test-mcp native-drawing-authoring --server <host> --session <blank-owned-session> --out <fresh-absolute-evidence-path>
```

The normal authoring fixture is unchanged when the flag is absent. The narrow
path starts from the same blank-document guard, models a 40 × 30 × 10 mm solid,
applies a real 2 mm chamfer to a vertical corner, and loads a top sheet with
unrelated saved annotation intent and a Released receipt. Native controls then
select the actual chamfer edge, reset its disposable choice, create the note,
edit it, delete it, and check exact model/history/archive preservation. Its
reported 2 mm setback must differ from the selected edge's sqrt(8) mm length.

Default execution uses shared controls and never sends OS input. The report is
`native-drawing-authoring.json`; original captures are `chamfer-*.png`, and the
source projection and baseline model are retained as JSON. A successful report
still requires visual review of the original captures.

Only on an idle, explicitly authorized owned Windows desktop or a disposable
private Linux Xvfb, also set `LIMO_CAD_NATIVE_CHAMFER_INPUT=1`. This uses the existing
bounded `cam-row-drag` pointer helper with verified window PID, recipient,
foreground, client/DPI mapping and actual logical coordinates. It physically
picks the edge, separately clicks the paper, drags the label, and cancels picks
and drags with Escape. Each gesture retains request, surface, input and model
publication evidence. There are no retry-clicks or RPCs while the button is
held. This mode does not test IME, macOS, monitor transitions or export.

Do not use the OS mode while the user is actively using the desktop. Keep this
probe opt-in; it neither changes the default CI route nor promotes Bevy to the
release shell.

The opt-in Linux `desktop-input` CI route runs the owned launcher at both scales:

```
xvfb-run --auto-servernum --server-args='-screen 0 2560x1600x24' cargo xtask test-mcp native-chamfer-platform --desktop-input --server <absolute-host> --out <fresh-absolute-evidence-root>
```

This launcher verifies the private Xvfb, creates its own blank window and registry,
enables both chamfer flags for the fixture, and restores the previous environment.
