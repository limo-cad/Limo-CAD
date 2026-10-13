# Limo CAD startup and loading

Limo CAD presents a small startup splash, reports ongoing work through the UI, and prepares the workspace with less work on the startup critical path.

## Startup splash

A centered, frameless **560 × 380** card provides localized feedback while the application starts. It combines a restrained engineering drawing, the regional application name, the actual preparation phase, and one stationary workshop note. The artwork contains no embedded text. English uses **Limo CAD** and English copy throughout; Simplified Chinese uses **砺模 CAD**. Spanish and German use **Limo CAD** with localized copy. Saved language preferences and existing fallback rules apply from the first feedback.

The card closes as soon as required initialization and rendering succeed. Readiness requires the current document/session and geometry revision, assets and dependencies, GPU resources, successful pipelines for visible material meshes, and a presented frame at the workspace's actual viewport size. The viewport renders beneath the card so these resources can prepare. An empty pipeline queue alone does not establish readiness.

The window remains responsive during device and font preparation. A slow start exposes recovery controls. Document failures support retry or an empty workspace; renderer failures provide useful details and close/reopen guidance. Recovery hides the workshop note and keeps actions and details prominent.

## Loading feedback in the UI

Once the application is ready, file opens, imports, exports, saves, geometry rebuilding, drawing preparation, and CAM work use the top-bar **Activity** indicator. Its panel shows named operations, real phases, elapsed time, recent results, and error details. Application initialization is not repeated for these operations.

File work reports reading, validation, geometry preparation, document adoption, and rendering separately. Native 3D Open completes after a frame presents the current model and viewport. Geometry, drawing, and CAM tasks report their actual computation boundaries. File reading can overlap renderer preparation; there is no required sequence that reloads all CAD resources and drawing capabilities for every file.

Ordinary launches bring the existing window forward. Forwarded files wait for readiness and use the normal unsaved-change decisions, with document ownership checked before replacement. **File → New Window** creates an independent workspace.

[Activity behavior and reproduction](runtime-activity.md) · [Visual gallery](startup-gallery.md)

## Startup optimizations

The lightweight HTML/CSS bootstrap can paint before React and engine initialization. The existing Tauri task facility prepares the GPU device and regional fonts off the UI thread. Window ownership, Bevy installation, scene updates, and rendering stay on the main thread. Redundant initialization and optional shader warmup meshes are removed; only resources required for the initial view hold up readiness. The workshop note loads asynchronously and does not delay startup.

Both Cargo workspace roots use:

```toml
[profile.dev]
opt-level = 1

[profile.dev.package."*"]
opt-level = 3
debug = 0
```

The wildcard excludes members of the active workspace. Root engine workspace members therefore use level 1; path engine crates are dependencies in the `src-tauri` workspace and use level 3. The application retains debug information, debug assertions, overflow checks, and portable target settings. Dependency debug information is disabled to reduce compiler memory use. The shipping profile keeps Cargo's default optimization level 3; LTO, codegen units, panic behavior, stripping, and CPU target features are unchanged.

The native renderer uses Bevy **0.19.0**. The readiness implementation follows its version-matched [loading-screen example](https://github.com/bevyengine/bevy/blob/v0.19.0/examples/showcase/loading_screen.rs). Profile references: [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html) and [Bevy development optimizations](https://bevy.org/learn/quick-start/getting-started/setup/#compile-with-performance-optimizations).

## Measurement results

Measurements distinguish first useful feedback, usable workspace, and first CAD interaction. Compilation is excluded. The Windows x64 host is Thunder: Ryzen 7 5800X3D, 32 GiB RAM, Radeon RX 9070 XT, driver 32.0.31036.15. Hardware, development profile, locale, document, and cache preparation match within the comparison.

The [matched optimization benchmark](startup-measurements/no-warmup-paired-summary.json) contains five measured warm-cache launches per executable/workload, each following one excluded warmup:

- **Empty workspace:** feedback **1444 → 583 ms**; usable workspace **1678 → 1429 ms**, a **15%** improvement. Workspace ranges were 1498–1988 ms and 1310–1536 ms.
- **Representative document:** feedback **1553 → 531 ms**; usable workspace **2058 → 1391 ms**, a **32%** improvement. Workspace ranges were 1712–2590 ms and 1357–1854 ms.
- **First CAD edit:** empty **269 → 264 ms**; sample **244 → 234 ms**. The edit creates a real sketch and solid extrusion, verifies rendering, and restores the exact model before repeating. Camera/picking checks also pass.

These are descriptive results from five runs on a shared host. They show that the measured startup savings do not transfer a multi-second stall to the first CAD edit. The empty workspace's first solid frame retains a bounded preparation cost of 96–152 ms, compared with 27–68 ms for the repeated edit. Persistent shader caching is not part of this change.

Separate [empty](startup-measurements/standard-drag-empty-verification.json) and [application-then-file](startup-measurements/standard-drag-runtime-file-verification.json) batches use two fresh WebView profiles each, with no warmup:

- Empty launches: feedback **1277/649 ms**, workspace **2803/1639 ms**, first CAD edit **578/300 ms**, repeated edit **168/167 ms**.
- Application-then-file launches: feedback **759/509 ms**, application workspace **1654/1338 ms**; the separate sample open takes **267/269 ms** to its presented frame. First sample CAD edit takes **210/203 ms**, repeated edit **175/201 ms**.

The second application-then-file launch reaches the workspace in 1338 ms: 28 ms before native `main`, 425 ms to Tauri setup, 55 ms to feedback, 388 ms from feedback to renderer installation, 385 ms to expansion request, and 56 ms for expansion and the full-size frame. Within the 388 ms interval, the worker records GPU-device preparation at 255 ms, fonts at 8 ms, and renderer installation at 119 ms. Empty document/UI preparation overlaps this work. The separate file operation records reading at 5.6 ms, validation at 1.2 ms, geometry preparation at 91.6 ms, and adoption at 50.1 ms; native project loading is nested within geometry preparation. Nested spans must not be added or assigned entirely to shader compilation.

The reports identify each executable and its raw events. The matched benchmark uses baseline SHA256 `472a491839e47b529e9949c04d900ec56b575b06de76846a0b28832ee9f622af` and optimized SHA256 `6d6fc31655cb1e5bc1dc78fb6eccc0ffa74aaea9b3465d560291e15aabf8a67c`. The fresh-profile batches use SHA256 `4c9a708a52b756965cb25abd7cbde71f469e2b8d804b286c6daa5576be0845af`. These results measure the identified optimization and startup artifacts; they are not timing measurements of every build represented in the functional captures. The sample hash is `d7e65fb222a0bb59f0ac82c3f3143a3437a487f3bbacf3a1f314b989b96de609`.

## Measurement scope

A fresh process or WebView profile does not establish cold Windows filesystem or GPU-driver caches. OS/driver caches were untouched, and unrelated host applications were preserved. This task's compilers and preview servers were stopped for comparable timing batches. The fresh-profile batches retain their first launches and verify input hashes after the batch; a hash read between batches can warm input pages. They are functional/timing samples, not an additional causal comparison.

Verified native behavior is Windows x64. The records do not establish post-reboot or driver-cold timings, shipping-profile speedups, or native macOS/Linux transition behavior. Native project preparation and JavaScript archive decoding still contain synchronous work; Activity feedback does not imply a completed streaming geometry architecture or prove large-document responsiveness.

## Verification

The frontend suite, startup/runtime ownership and recovery tests, TypeScript check, and desktop build pass. Native library verification records **130 passed, 0 failed, 6 intentionally ignored**: five GPU visual-matrix cases and one private-fixture case. [Frontend tests](startup-measurements/runtime-frontend-tests.log), [frontend build](startup-measurements/runtime-frontend-build.log), [native tests](startup-measurements/runtime-native-tests.log), and [native application build](startup-measurements/runtime-native-app-build-keyboard-final.log) contain the results.

Windows checks cover startup, same-process recovery, empty recovery, representative-document rendering, first/repeated CAD edits and model restoration, viewport interaction, ordinary window reuse, forwarded-file decisions, explicit New Window, Activity keyboard/focus behavior, and runtime errors preserving the loaded document. Four-locale component checks cover naming, copy, glyphs, long filenames, recovery, and light appearance. Native and simulated component captures are labeled in the [gallery](startup-gallery.md); artifact receipts accompany them.

The [runtime verification receipt](startup-measurements/runtime-native-staging-keyboard-final.json) identifies executable SHA256 `996bb2421ed459296125725bdf89209c4cfb3848e23ae12091465facc345062f` and its embedded frontend. Raw measurement reports, experiment logs, and capture receipts remain in `startup-measurements/` for audit and reproduction.

## Reproduction

From a Visual Studio developer shell, build the desktop frontend and native executable with the checked OCCT SDK:

```powershell
$env:OCCT_ROOT = 'D:\limo-cad-maintenance\checked-occt-sdk'
$env:CARGO_TARGET_DIR = 'D:\limo-startup-target'
npm ci
npm run test:frontend
npm run build:desktop
cargo build --locked --manifest-path src-tauri/Cargo.toml --features custom-protocol --bin nbcad
```

Stage immutable executables before measuring. Use distinct private output directories, preserve the same SDK/profile/locale/document/cache conditions, and stop this task's build and preview work. The harness's `--profile` option records a label; it does not compile or select a Cargo profile.

```powershell
node scripts/measure-startup.mjs --exe D:\limo-startup-target\debug\nbcad.exe --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --output D:\limo-repro\empty --profile dev-app1-debug2-deps3-debug0 --cache warm --locale en --runs 5
node scripts/measure-startup.mjs --exe D:\limo-startup-target\debug\nbcad.exe --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --output D:\limo-repro\runtime-file --profile dev-app1-debug2-deps3-debug0 --cache warm --locale en --runs 5 --document examples/testPiece.nbcad --document-mode runtime
```

For a matched baseline, apply the observer-only patch to a clean checkout of `e4015de638b55823299d4a195e20a10ecbe9fae7`. Build in an isolated target with the same optimization settings:

```powershell
git worktree add --detach D:\limo-startup-baseline e4015de638b55823299d4a195e20a10ecbe9fae7
git -C D:\limo-startup-baseline apply "$PWD\scripts\startup-baseline-observers.patch"
cargo build --locked --manifest-path D:\limo-startup-baseline\src-tauri\Cargo.toml --features custom-protocol --bin nbcad --config "profile.dev.opt-level=1" --config "profile.dev.package.'*'.opt-level=3" --config "profile.dev.package.'*'.debug=0"
```

Measure both staged executables with the same harness arguments. For representative-document startup comparisons use `--document examples/testPiece.nbcad --document-mode startup` on both; use `runtime` when measuring usable-application time separately from file loading.

To retain first-use observations without harness warmup or pre-launch input reads, use `--cache first --warmups 0 --webview-profile fresh-per-run --verification after-batch --expected-exe-sha HASH`. Add `--expected-document-sha HASH` for a document. Boot-cold samples require a stated restart/cache procedure and one launch per preparation; these flags do not clear OS or driver caches.

The opt-in JSONL observer records native and WebView clocks, current-session/revision frame proof, camera/picking, and first/repeated real CAD diagnostics. It verifies exact parsed-model restoration and fails on missing milestones or diagnostic errors. Keep raw events, stderr, host state, artifact hashes, and all first runs. Recovery waits are excluded from launch comparisons.

Windows test artifacts require the application's Common Controls v6 manifest. [run-native-viewport-tests.ps1](../scripts/run-native-viewport-tests.ps1) runs the viewport filter from an isolated manifested copy, preserving Cargo's artifact. The complete library run is recorded in the native test log above.
