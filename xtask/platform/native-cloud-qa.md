# Native revision-cloud authoring fixture

Build the default native host with exact Bevy
`=0.20.0-rc.1`. Use the fixture-owned host, a fresh blank session and fresh
absolute evidence directory. Set `LIMO_CAD_NATIVE_CLOUD_ONLY=1`, then run:

```
xtask test-mcp native-drawing-authoring --server <host> --session <blank-owned-session> --out <fresh-absolute-evidence-path>
```

The default path sends no OS input. It loads one saved seven-vertex cloud on
a sheet without views, with fractional coordinates, lowercase Unicode revision,
ANSI presentation, title/tolerance metadata, a Released receipt and unrelated
notes on two sheets. Published controls select its label, reject empty revision,
reset a draft, uppercase an edited revision, apply without changing vertices,
delete it, and restore exact complete models through Undo/Redo and native archive
save. It captures `cloud-loaded`, `cloud-edited`, `cloud-deleted` and
`cloud-restored` PNGs, plus the baseline model and `native-drawing-authoring.json`.

Triangle/quad creation and dragging require actual paper input. Only in an
authorized idle fixture-owned Windows desktop or disposable private Linux Xvfb,
also set `LIMO_CAD_NATIVE_CLOUD_INPUT=1`. This uses the existing bounded pointer
helper, verified host PID/window, published client bounds and actual logical
coordinates. It drags the loaded seven-vertex cloud, cancels partial placement,
authors a triangle by closing within four paper millimetres, and authors a quad
with a fourth corner. It verifies each exact saved vertex, no model/ID change
while staging, canceled drags, real scallop-path and label drags, and complete
delete/history/archive preservation. Requests, surfaces, input coordinates,
publication evidence and seven extra PNGs are retained. No retry-clicks or RPCs
run while a mouse button is held.

Inspect original PNGs to confirm scallops, open staged polyline and vertex dots,
revision labels, visible placement and dragged geometry. The semantic-only
report explicitly does not prove paper authoring or gestures. Neither mode
proves macOS input, IME or export. Do not run OS mode while the user is using
their desktop. Both flags are opt-in; default fixtures and release host remain
unchanged.

The opt-in Linux native-host job runs the same fixture at both scales through
`test-mcp native-cloud-platform --desktop-input --server <absolute-host> --out
<fresh-absolute-root>` under a private Xvfb. The launcher verifies the display
before starting its own blank host and enables both cloud flags only for that
fixture. A failure in another independent input family does not suppress this
family's retained evidence or change the job's failing status.
