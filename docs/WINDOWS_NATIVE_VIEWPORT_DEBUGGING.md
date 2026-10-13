# Windows native viewport debugging

Status: Historical field notes from the August 2026 embedded viewport bring-up.
The desktop no longer uses Tauri, WebView2, child-window composition or these
legacy test commands. For the native-only build and current checks, use
[Development](DEVELOPMENT.md) and [transition status](native-transition-status.md).
The material below records the retired architecture, not current instructions.

This runbook records failure signatures, root causes, diagnostic techniques,
and validation commands that are specific to the Tauri + WebView2 + embedded
Bevy viewport on Windows. macOS remains the behavioral reference, but its
windowing and input paths are different. Do not copy the Win32 workarounds to
macOS.

## Architecture to keep in mind

The Windows desktop application has two adjacent native child windows:

- WebView2 owns the React shell, dialogs, accessibility tree, and transparent
  viewport interaction surface.
- Bevy owns an opaque Win32 child with class `noBS.CAD.BevyViewport` and renders
  the OCCT scene through wgpu.

The Bevy child is placed above the viewport part of WebView2. Its Win32 region
is cut around DOM overlays so React controls remain visible and interactive.
This is not the same compositor arrangement as macOS.

`HTTRANSPARENT` is insufficient here. It only walks sibling windows on the
same UI thread, while WebView2's renderer can live on another thread. The Bevy
child therefore owns viewport hits and relays pointer and wheel messages to the
page with `ICoreWebView2::PostWebMessageAsString`. The page recreates DOM
pointer events on the existing CAD interaction surface.

Primary code owners:

| Concern | Source |
|---|---|
| Win32 child, input relay, Bevy runtime | `desktop/src/native_viewport/platform.rs` |
| DOM/native layout and camera IPC | `src/components/viewport/nativeViewportBridge.ts` |
| Camera and interaction kernel | `src/components/viewport/Viewport.tsx` |
| Pointer jump filtering | `src/components/viewport/cadInteraction.ts` |
| SpaceMouse transport policy | `src/input/sixDofMouse.ts` |
| 3DxWare Navigation Library bridge | `src/input/threeDConnexionBridge.ts` |
| Native raw-HID fallback | `desktop/src/six_dof_mouse.rs` |
| Windows interaction regressions | `scripts/e2e-bevy-interaction-kernel.mjs` and `scripts/e2e-six-dof-mouse.mjs` |

## Reliable local launch

For a distributable application, use `cargo xtask package` after the
[Windows SDK setup](WINDOWS_PACKAGING.md#local-windows-build). The command below
is a development launch for debugging native startup with a live frontend.

PowerShell may block `npm.ps1`. Use `npm.cmd` explicitly.

The development checkout used the release-style local vcpkg prefix below. If
the machine has the standard `x64-windows` prefix instead, substitute that
directory consistently.

```powershell
$env:OCCT_ROOT = (Resolve-Path 'vcpkg_installed\x64-windows-release').Path
$env:VCPKG_TARGET_TRIPLET = 'x64-windows-release'
$env:Path = "$env:OCCT_ROOT\bin;$env:Path"
npm.cmd run tauri dev -- --no-watch
```

Expected native startup output includes:

```text
native Bevy viewport installed (Bevy 0.19 / wgpu DX12-Vulkan / embedded HWND, 1.00x scale)
```

Source-only TypeScript changes can hot-load. Rust or Win32 changes require a
full application restart. Do not treat a hot-loaded frontend as proof that a
native fix is active.

## Failure signatures and findings

### Missing orientation dial or navigation bar

The orientation dial and bottom navigation bar are native-HUD proxies when the
opaque Bevy child is active. A DOM element can exist and still be hidden behind
the native child if its rectangle is not included in the native presentation
or overlay ownership data.

Check both sides:

1. The React elements exist and expose `data-native-hud` /
   `data-native-nav-id` metadata.
2. `nativeViewportBridge.ts` includes their exact rectangles and HUD state in
   the presentation sent to Bevy.
3. The Bevy HUD is respawned or updated when its revision changes.

The dial and bar should appear immediately. Waiting a minute is not an
acceptable recovery mechanism.

### Origin planes outline but do not fill on hover

This was an input-delivery problem, not a slow shader or delayed Bevy visual.
On Windows, passing `WM_NCHITTEST` through the opaque child did not reliably
reach the WebView2 DOM surface. Hover state consequently never reached the
interaction kernel.

The correct Windows path is:

```text
Win32 mouse message
  -> noBS.CAD.BevyViewport window procedure
  -> CoreWebView2 PostWebMessage
  -> nativeViewportBridge DOM event reconstruction
  -> existing hover/pick kernel
  -> native presentation update
  -> Bevy semi-transparent plane fill
```

Keep the finite plane footprint identical between React picking and Bevy
rendering. A large or infinite render plane paired with a smaller pick plane
creates apparent hover disagreement.

### Viewport flashes blank while orbiting

The preserved failure proved this was camera-state corruption rather than a
missed render. The live camera snapshot was approximately:

```json
{
  "position": [-102.16, 170.77, 32.85],
  "target": [-432.08, 622.12, 84.97],
  "up": [0.3961, -0.3977, 0.8276]
}
```

The camera remained near the model, but its look target had drifted hundreds
of model units away. The model origin was behind the camera. Focusing the
window and forcing a one-pixel resize did not recover it, which ruled out
focus, swapchain resize, and the one-versus-two Bevy update optimization.

Root cause: SpaceMouse depth input translated both `camera.position` and
`controls.target` through a fixed model. Repeated input could carry the whole
camera rig past the part.

Required invariant:

- lateral SpaceMouse motion may pan the camera rig;
- depth motion must dolly around a fixed target;
- dolly distance remains bounded (currently 2 to 5000 model units);
- the target must not cross behind the camera.

This camera invariant is shared with macOS. Only the Windows transport and
WebView2 input-delivery workarounds are platform-specific.

For a model center `m`, camera position `p`, and target `t`, this quick check
detects the blank-state geometry:

```text
forward = normalize(t - p)
model_depth = dot(m - p, forward)
model_depth <= 0  =>  model is on or behind the camera plane
```

### Exact orbit angles lose the render

Bevy's `Transform::looking_at` needs an up vector that is not parallel to the
view direction. Replacing every degenerate up vector with world Z is still
degenerate when looking exactly along positive or negative Z.

Use a stable orthogonal up vector:

1. Normalize the view direction.
2. Remove the view-parallel component from the up hint (Gram-Schmidt).
3. If the result is still near zero, use `any_orthonormal_vector()`.
4. Verify the resulting matrix is finite at both top and bottom views.

The same protection applies to camera-relative light transforms.

### Logitech wheel tilt makes the model fly

The Logitech center wheel can emit discrete horizontal wheel messages when it
is flipped left or right. These are not CAD pan or orbit gestures.

Windows input handling must:

- accept the center-wheel press for middle-button pan / Shift-middle orbit;
- ignore discrete horizontal wheel tilt;
- bound every wheel and pointer delta;
- reject implausible pointer jumps after capture loss;
- clear capture and mouse-button state on pointer cancel or window blur.

Do not let SpaceMouse motion and an active middle/right mouse drag write the
camera simultaneously.

### Sketch ribbon clips at a non-maximized width

The complete sketch command set must fit at the supported 1280-pixel test
width. The Finish Sketch command remains a fixed visible action, while command
groups compact before any button is allowed to become partially hidden.

The interaction regression checks that:

- the command strip has no horizontal overflow;
- Fix/Unfix is fully visible;
- Finish Sketch is fully visible.

### Sketch exit pauses halfway to isometric view

Camera restore animation and Bevy presentation work can compete if every
camera-frequency update renders twice. Structural changes need two Bevy
updates to settle the extracted render world; ordinary camera, annotation, and
style changes render once.

Keep camera IPC coalesced, but do not add animation-frame queues at every
layer. The current low-latency path combines matrix + target callbacks in a
microtask, permits one camera IPC request at a time, and retains only the most
recent pending state.

## SpaceMouse policy on Windows

### No startup input

The Windows desktop must not probe, connect, or register a motion path during
normal startup. The connection indicator starts gray and the camera remains
inert until the user deliberately clicks it.

This rule exists because motion during a gray or amber connection state is
indistinguishable from leaked input. It also prevents a displaced cap during
startup from nudging the initial view.

### Connection handshake gate

The installed-driver bridge may issue camera callbacks while it is still
creating the 3D mouse session. All mutating callbacks are gated until React has
painted the green connected indicator. A cap displaced during the handshake is
ignored.

The connection button is disabled while the state is `connecting`, preventing
overlapping connection attempts.

### Low-latency installed-driver path

3DxWare supports device-timed and application-timed frames. Application-timed
frames added a browser `requestAnimationFrame` and Navigation Library server
round trip before every camera update. Windows now requests device timing
(`timingSource: 0`) so fresh cap samples initiate updates.

Avoid this latency chain:

```text
device -> local driver -> browser rAF -> local driver -> camera setter
       -> viewport rAF -> bridge rAF -> Tauri IPC -> Bevy render
```

The intended path is:

```text
device -> local driver -> camera setters -> microtask coalescing
       -> Tauri IPC -> Bevy render
```

### Raw-HID fallback

Raw HID is an explicit fallback, never a silent Windows startup transport. Its
worker currently:

- reads with an 8 ms timeout;
- coalesces translation and rotation at a 16 ms emission interval;
- normalizes and dead-zones axes in the frontend;
- expires stale motion after 45 ms;
- emits only newly pressed hardware buttons.

Bluetooth SpaceMouse devices can send translation and rotation in one combined
report. Preserve support for both combined and split reports.

Canonical object-mode mapping is centralized in
`canonicalizeSixDofTranslation` and `canonicalizeSixDofRotation`. Do not add a
second platform-specific sign inversion elsewhere.

## Preserving and diagnosing a live failure

Do not immediately resize, fit, reload, or close the window. Preserve the bad
state long enough to separate rendering failure from camera failure. If the
document might be unsaved, get permission before terminating the process.

### Browser-side state

Open WebView2 DevTools with Inspect and query:

```js
JSON.stringify(window.__cameraApi.getSnapshot())

document.querySelector('[data-testid="six-dof-mouse-connect"]')?.title

document.querySelector('[data-native-hud="navigation"]')
  ?.getAttribute('data-native-six-dof-state')
```

Useful interpretations:

- DOM HUD present but not visible: native overlay/presentation ownership bug.
- Camera snapshot points away from the model: navigation-state bug.
- Button title names a device while the indicator is not green: connection
  state and transport are out of sync.
- Focus or a one-pixel resize recovers the view: investigate surface/present
  scheduling rather than camera math.

### Native window checks

`scripts/verify-windows-viewport.ps1` verifies a packaged launch, the native
child class, client size, visibility, and startup probe.

When querying a development process manually, inspect:

- top-level process title `Limo CAD`;
- child class `noBS.CAD.BevyViewport`;
- child visibility;
- client width and height greater than 100 pixels;
- Vite listening on port 5173.

If the application was launched with elevated permissions, a lower-integrity
PowerShell process can report `MainWindowHandle = 0` even though the window is
visible. Run the probe at the same integrity level before concluding that the
window is missing.

### Native metrics

`native_viewport_metrics` reports readiness, logical and physical size,
rendered frames, wakeups, average frame time, last pointer latency, body count,
and triangle count. Treat a responsive React shell plus a ready native child
as a multi-layer system: metrics from one layer do not prove the camera state
in another layer is correct.

## Validation commands

Production frontend build:

```powershell
npm.cmd run build
```

Focused browser regressions:

```powershell
npm.cmd run dev -- --host 127.0.0.1 --port 7199 --strictPort
node.exe scripts\e2e-bevy-interaction-kernel.mjs
node.exe scripts\e2e-six-dof-mouse.mjs
```

The first cold Vite load can spend more than 30 seconds initializing the CAD
engine. Tests should wait for `domcontentloaded` and then allow up to 90 seconds
for the application store and camera API rather than relying on
`networkidle`.

Native tests:

```powershell
$env:OCCT_ROOT = (Resolve-Path 'vcpkg_installed\x64-windows-release').Path
$env:VCPKG_TARGET_TRIPLET = 'x64-windows-release'
$env:Path = "$env:OCCT_ROOT\bin;$env:Path"
cargo.exe test --manifest-path desktop\Cargo.toml --lib
```

Regression coverage should include:

- pointer jump rejection and bounded wheel deltas;
- horizontal Logitech wheel-tilt suppression;
- all six canonical SpaceMouse axes;
- hundreds of maximum depth packets without target drift or target crossing;
- driver motion injected during the gray/amber handshake;
- device-timed driver configuration;
- no raw Bluetooth HID connection on Windows startup;
- finite camera transforms at exact positive/negative up-axis alignment;
- compact sketch ribbon visibility;
- sketch-exit camera restoration;
- native HUD proxies and origin-plane hover ownership.

Expected non-blocking output includes existing unused native-menu warnings,
Vite chunk-size warnings, and Git's LF-to-CRLF notices. New compiler errors,
page errors, camera drift, or a non-green input path are failures.

## Windows-specific cautions

- Preserve unrelated worktree changes and local `.cache/` or vcpkg overlay
  directories; they are not automatically part of a fix or documentation
  commit.
- Stop only validated process IDs. Do not kill every Node, WebView2, or Cargo
  process on the machine.
- A one-frame performance optimization must be tested against both ordinary
  camera updates and structural Bevy changes.
- Do not infer that a blank native viewport is a GPU problem until camera
  position, target, up, and model depth have been inspected.
- Do not infer that a visual will eventually appear. Hover, HUD, and connection
  state should be deterministic within the next rendered frame.
