# Startup measurement

Startup is measured from executable launch to useful feedback and to an
initialized, rendered workspace separately. Compilation is outside both
intervals. The loading overlay has no minimum display duration.

The Windows x64 measurement host has an AMD Ryzen 7 5800X3D (16 logical CPUs),
32 GiB RAM, and AMD Radeon RX 9070 XT with driver 32.0.31036.15. Power settings
and OS/driver shader caches were not changed. Unrelated applications and
compiler jobs on the shared host were preserved; this task's own compilers and
preview servers must stop before a comparable launch batch.

[Visual review gallery](startup-gallery.md): five English views, a four-language
companion view, and actual native loading/recovery/workspace frames.

## Floating loading card — visual revision

The loading treatment now uses a centered, frameless **560 × 380** native card,
with a native shadow and Windows 11 rounded corners. The application title bar,
minimize and maximize controls return with the full workspace. The opaque card
keeps the existing real viewport rendering underneath; it adds no second native
window, transparency path, GPU scene, animation delay, or new renderer resource.
Readiness still requires the current document and the successful actual expanded
frame after normal decorations and workspace size have been restored.

Brand and a text-free machined-plate drawing occupy the left column. The right
column shows the actual application phase, independent file work when present,
and one stationary regional workshop note. The blank top edge uses Tauri's
standard drag region; the maintainer confirmed native movement on October 12.
A small localized close control remains available from early HTML feedback.
Recovery keeps useful details selectable and scrollable and fits four actions.

The [floating-card captures](startup-measurements/captures/floating) include
English, Simplified Chinese, Spanish and German, long filenames/notes, recovery,
and light mode. The [layout checks](startup-measurements/captures/floating/layout-checks.json)
passed 14 actual component-state captures at 560 × 380 or the stated larger
composition viewport. These are illustrative source-component captures with
simulated native IPC, not native-launch timing evidence. The thread's T3 browser
reported its automation host unavailable; an isolated headless browser supplied
these checks instead.

[Startup/control tests](startup-measurements/frontend-floating-startup-tests.log)
and the [desktop frontend build](startup-measurements/frontend-floating-startup-build.log)
passed. The frozen frontend is `index-CoxyHmt5.js`, SHA-256
`7e04de23b805938d45d7e205f993c47d5213d3ce97009e239e4252efc5142293`.
The numerical comparisons below identify their earlier immutable executables;
they are not silently relabeled as measurements of this visual revision.

The [native build](startup-measurements/floating-native-build.log) passed on
Windows. Its immutable executable is SHA-256
`f48b231c425644dfa901fc9f832aeaea40be3c7a93c48b4a3feca7fc7c11f0a7`;
the [staging receipt](startup-measurements/floating-staging.json) identifies it.
Actual [native loading pixels](startup-measurements/captures/floating/native-en-loading.jpg)
and [English recovery](startup-measurements/captures/floating/native-en-recovery.jpg)
show a 562 × 382 outer capture around the requested 560 × 380 frameless client
area, with no visible Windows title bar. Repairing only a private invalid ZIP
and choosing Try again retained PID 5048, loaded the sample's four history
entries, restored normal workspace chrome, and passed real CAD edits at
255.3/171.8 ms with exact restoration. The
[expanded native workspace](startup-measurements/captures/floating/native-en-workspace.jpg)
and [trace](startup-measurements/floating-native-recovery.jsonl) record this.
Human recovery waits are excluded from launch comparisons.

[Simplified Chinese native recovery](startup-measurements/captures/floating/native-zh-recovery.jpg)
verified regional naming/glyphs and the custom top-right close control; PID
15172 exited after the action. The two installed user CAD windows and original
sample were preserved. The [close stderr](startup-measurements/floating-native-close-stderr.log)
retains a WebView2 `Chrome_WidgetWin_0` unregister-class error 1412 during exit;
the process exited, but this shutdown log is not described as clean. The
[functional receipt](startup-measurements/floating-functional-receipt.json)
records the checks. Actual movement via the blank drag edge remains
unverified: two short synthetic drag inputs left the window origin unchanged.
Source review confirms shadow and decoration flags are independent; queued
native dragging requires a held primary press, which this automation API cannot
pause mid-drag. This is a hypothesis about the failed check, not proof of native
drag success. The maintainer later confirmed native dragging on October 12;
this earlier automated result is retained as historical evidence.

Two additional fresh-profile empty launches passed full expanded-frame,
camera/picking, first/repeated CAD and exact restoration checks. Their
[verification report](startup-measurements/floating-native-empty-verification.json)
retains feedback at 1021/525 ms, usable workspace at 2189/1476 ms, and first CAD
edits at 498/299 ms, repeats 187/163 ms. These two smoke runs are not a new matched
before/after comparison or verified boot-cold measurement. Compilation and own
preview/UI sessions were stopped before this batch; executable hashes were
verified after it. All task-owned native windows and the preview server were
closed afterward.

## Final native check and current executable

The final [native build](startup-measurements/standard-drag-native-build.log)
passed with the standard Tauri drag region and scoped main-window permission.
The custom dragging command was removed. The frozen frontend is
`index-uBMtl55s.js`, SHA-256
`67031d45f2d25521c4e397a6372a62f64b4a75636546b2721ded2a601f9436ec`.
The executable is SHA-256
`4c9a708a52b756965cb25abd7cbde71f469e2b8d804b286c6daa5576be0845af`;
the [staging receipt](startup-measurements/standard-drag-staging.json) records it.
Earlier captures and comparisons retain their actual earlier hashes.

The maintainer confirmed native card dragging on October 12, and the PR is ready
for review. The earlier [automated check receipt](startup-measurements/standard-drag-native-check.json)
is retained as a record of that automation attempt.

The final executable's [native recovery card](startup-measurements/captures/floating/native-standard-drag-recovery.jpg),
[retry loading phase](startup-measurements/captures/floating/native-standard-drag-loading.jpg),
and [expanded workspace](startup-measurements/captures/floating/native-standard-drag-workspace.jpg)
were captured on Thunder. Repairing only the private invalid fixture and choosing
Try again retained PID 24704, loaded all four sample history entries, and passed
the first/repeated real CAD diagnostic at 253.5/427.6 ms with exact model and
rendered-frame restoration. These deliberate recovery waits are not startup
timings. The [trace](startup-measurements/standard-drag-native-recovery.jsonl)
retains them. The native [front-view control](startup-measurements/captures/floating/native-standard-drag-front-view.jpg)
also changed the rendered view. The owned window exited afterward; the two
installed user CAD windows and original sample were preserved.

With this task's compilers and preview servers stopped, two empty launches and
two launches followed by runtime sample opens used distinct fresh WebView
profiles, no warmups, and after-batch hash verification. All four passed current
expanded-frame, camera/picking, first/repeated CAD and exact restoration checks.
These small final smoke batches establish current behavior, not a new matched
before/after comparison or verified post-reboot cold start. All first runs are
retained; OS and driver caches were untouched. The executable hash read after
the empty batch can warm pages for the subsequent runtime batch.

The [empty results](startup-measurements/standard-drag-empty-verification.json)
record feedback at 1277/649 ms, usable workspace at 2803/1639 ms, first real CAD
edits at 578/300 ms and repeated edits at 168/167 ms. The
[runtime-file results](startup-measurements/standard-drag-runtime-file-verification.json)
record feedback at 759/509 ms and app workspace at 1654/1338 ms. Opening the
sample afterward took 267/269 ms to its successful current frame; first CAD edits
took 210/203 ms and repeated edits 175/201 ms. Initial camera/picking took
16/11 ms for empty and 5/5 ms for the sample.

For the second runtime-file launch, the app's 1338 ms divides into 28 ms before
native main, 425 ms to Tauri setup, 55 ms to first feedback, 388 ms from feedback
to native installation, 385 ms from installation to expansion requested, and
56 ms for expansion and successful full-size frame feedback. The native worker
recorded GPU-device preparation at 255 ms, fonts at 8 ms, and renderer installation
at 119 ms within that 388 ms interval. Empty document/UI setup took 362 ms and
overlapped renderer work; it must not be added to those intervals. The remaining
installation-to-expansion interval includes document/layout completion, required
pipelines, GPU resources and the compact frame, rather than being attributed
entirely to shader compilation.

The separate file operation recorded frontend read 5.6 ms, validation 1.2 ms,
geometry preparation 91.6 ms and adoption 50.1 ms. Native project loading was
75.1 ms nested within geometry preparation. IPC receipts are not frontend span
endpoints; use the retained exact WebView span bounds, and do not sum nested
durations or subtract medians to infer an unmeasured phase.

The [full frontend suite](startup-measurements/pr-frontend-tests.log) passed.
After adopting the standard drag region, the
[startup/control tests](startup-measurements/frontend-standard-drag-tests.log)
and [desktop frontend build](startup-measurements/frontend-standard-drag-build.log)
passed again. The [native test build](startup-measurements/pr-native-test-build.log)
and isolated CommonControls-manifest test run passed: the viewport filter had
68 passes and 5 existing ignored tests, and the
[whole native artifact](startup-measurements/pr-native-all-tests.log) had
123 passes, 0 failures and 6 existing ignored tests (five GPU visual-matrix tests
and one private-file test). That test artifact precedes removal of the custom
drag command; the final native application build and checks above cover the
standard-region change. Rust formatting and whitespace checks passed.

The Windows checkout's raw version-sync check failed because its existing
newline-sensitive checks assumed LF in CRLF files. A read-only check of the same
31 version carriers normalized to their Git/LF representation
[passed at 0.2.2](startup-measurements/pr-version-check.log). The repository
version script and unrelated files were not changed to hide that limitation.

The recorded measurements do not cover actual post-reboot/driver-cold and
shipping-profile measurements, macOS/Linux native transitions, the six ignored
tests, and large-document responsiveness. Runtime progress is nonmodal, but
native project preparation and JavaScript archive decoding still contain
synchronous work; this is not a completed streaming geometry architecture.

Reproduce these final smoke batches after building and staging one immutable
executable. Substitute your own new output directories and runtime SDK path:

```powershell
node scripts/measure-startup.mjs --exe D:\limo-startup-candidate-standard-drag-runtime\nbcad.exe --output D:\limo-startup-standard-drag-empty-new --profile dev --cache first --runs 2 --warmups 0 --webview-profile fresh-per-run --verification after-batch --expected-exe-sha 4c9a708a52b756965cb25abd7cbde71f469e2b8d804b286c6daa5576be0845af --locale en --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --label final-standard-drag-empty
node scripts/measure-startup.mjs --exe D:\limo-startup-candidate-standard-drag-runtime\nbcad.exe --output D:\limo-startup-standard-drag-runtime-file-new --profile dev --cache first --runs 2 --warmups 0 --webview-profile fresh-per-run --verification after-batch --expected-exe-sha 4c9a708a52b756965cb25abd7cbde71f469e2b8d804b286c6daa5576be0845af --expected-document-sha d7e65fb222a0bb59f0ac82c3f3143a3437a487f3bbacf3a1f314b989b96de609 --document examples/testPiece.nbcad --document-mode runtime --locale en --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --label final-standard-drag-runtime-file
```

## Thunder cold-start definition and application/file boundary

The October 11 cold-start audit confirms the measurement host is **Thunder**,
running Windows 11 Pro 10.0.26200. Its last boot was October 8 at
02:01:41 UTC. A fresh process is not proof of cold Windows filesystem or
driver caches. A fresh WebView2 user-data folder isolates application browser
storage and disk cache; it does not clear Windows or GPU-driver caches.
See [Microsoft's WebView2 user-data folder documentation](https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/user-data-folder).

The old harness read the entire executable and sample for hash verification
before launches. It also reused one private WebView profile after a warmup
within each batch. Earlier results are correctly labeled warm-cache observations.
New opt-in flags support `--webview-profile fresh-per-run` and
`--verification after-batch`, with trusted expected input hashes. They avoid
harness input pre-reads, retain the first launch, and record profile policy,
hostname, uptime, and untouched/unverified OS/driver cache conditions. The
`--cache` label describes an operator's conditions; it does not itself make
a launch cold.

An initial audit of the immutable compact executable `f9d476…` retained five
empty launches and five startup-file launches, without warmups. All ten fresh
processes used distinct initially empty WebView profiles; input hashes were
checked after each batch. The first empty launch showed feedback at **1508 ms**
and a usable workspace at **2557 ms**. It is a useful first-use observation,
not a verified first launch after reboot. Empty medians were **539 ms** feedback
and **1517 ms** workspace; sample-at-startup medians were **508 ms** and **1500 ms**.
Do not subtract those workload medians to estimate file time: the workloads ran
in different batches, and document/native work overlaps.

The retained first empty launch consisted of 289 ms before Rust `main`, feedback
at 1508 ms, GPU-device preparation 476 ms ending at 1985 ms, native font
preparation 6 ms ending at 1992 ms, and renderer installation 112 ms ending
at 2106 ms. Window expansion took 69 ms and ended near the 2557 ms workspace
marker. Initial empty engine/tab preparation took 131 ms and overlapped GPU
preparation. The older executable has no early setup landmark, so the remaining
pre-feedback interval cannot be attributed exclusively to WebView2. These
are intervals from one real launch, not a sum of medians.

The [first-use summary](startup-measurements/thunder-fresh-f9d-summary.json),
[boot evidence](startup-measurements/thunder-fresh-f9d-host-boot.json), and
[stderr audit](startup-measurements/thunder-fresh-f9d-stderr-audit.json) preserve
all launches, including the slow first launch. No reboot, global cache purge,
driver modification, or installed user-window termination was performed.

### Honest phase contract

Application initialization and file preparation are separate operations.
The empty application initializes the native window/WebView, CAD engine/session,
GPU device, fonts, renderer, and its actual first-view assets and pipelines.
Frontend engine/session work overlaps background GPU preparation. Native fonts
follow device creation, then Bevy installation runs on the main thread.

An actual file open reads bytes, validates/decodes the archive, waits for existing
document snapshots, prepares geometry through the CAD engine, then adopts the
document's metadata and workspace state. A successful displayed file additionally
requires its current session/revision to be presented. File reading does not
require the GPU device to have finished; presentation requires both valid
geometry and the renderer. There is no mandatory sequence that reloads CAD
resources, geometry capabilities, and drawing capabilities for every file.

The initial compact screen labels application preparation explicitly and shows
a separate filename/operation when a real file is opening. An empty launch
does not pretend to be reading a drawing. Later opens use a small document-local
status while the workspace remains visible; they do not reset the startup screen.
Reading, validating, waiting, geometry preparation, and document adoption are
reported at actual operation boundaries, without invented percentages.

The measurement-only `--document-mode runtime` opens the sample after the initial
workspace marker. It records file preparation and waits for the actual current
session/revision frame before `fileReady`, then runs the camera and first/repeated
CAD diagnostics. This separates usable-application time from later file-open
time. The historical `startup` mode remains available for direct comparison.

### Runtime direction and remaining work

The product target is responsive interaction with resources loaded as needed,
and no global loading screen after initial startup. Existing retained project
sessions, ownership/version checks, optional workspace facilities, and
asynchronous pipeline compilation provide foundations. Optional work should
be scheduled behind visible geometry, with bounded resource budgets and stale
results discarded before publication. Requested expensive outputs can remain
pending locally while unrelated interactions continue; approximate preview
geometry must never be represented as final CAD geometry.

This change establishes honest progress and measurement boundaries, not universal
streaming or instant geometry. Archive decoding is still synchronous JavaScript;
native file reads and project preparation/replay/commit still execute on
synchronous Tauri commands. Project replacement holds the publisher/engine
transaction, so simply moving it to a worker can still block getters and model
snapshots. A future measured large-document change should keep the same
transaction and ownership invariants, stage expensive preparation off the UI
thread, prevent UI getters waiting on its lock, and publish a complete result
atomically. No claim of fully responsive large-file loading is made here.

A first launch after a real Windows restart is still outstanding. Run exactly
one launch with no warmup, a fresh private profile, and after-batch verification
before opening or hashing the application/sample elsewhere. Record boot time
and uptime. A Windows restart resets more state than Fast Startup shutdown;
persistent driver caches can remain, and must still be labeled accurately.
See [Microsoft's full-boot versus Fast Startup guidance](https://learn.microsoft.com/en-us/windows-hardware/test/assessments/onoff-transition-performance).
Do not label five consecutive launches as five boot-cold samples: each such
sample requires its own stated preparation.

### Reproduce separate application and file measurements

The application/file-boundary build is staged immutably at
`D:\limo-startup-candidate-separated-runtime\nbcad.exe`, SHA-256
`e77ea3a6a09da71be2862e4c17d517fd2c71a6206254243a230801bb6688d308`.
It embeds `index-3K4FOBp_.js`, SHA-256
`4952c99ba7254c0cd3a744351733df048eba6a9cfeb8b15833ff6586c01f3735`.
The checked SDK, development profile, English locale, and original sample hash
remain the same. The [staging receipt](startup-measurements/thunder-separated-staging.json)
and [native application build log](startup-measurements/thunder-native-application-build.log)
identify the executable. This measures the repository candidate, not the separately
installed Bevy 0.20 application or a shipping-profile build.

Build and stage first, then stop this task's compilers, native UI checks, and
preview servers. Use a new output directory for each batch. These commands do
not restart Windows or purge caches; `first` means no harness warmup.

```powershell
node scripts/measure-startup.mjs --exe D:\limo-startup-candidate-separated-runtime\nbcad.exe --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --output D:\limo-repro-separated\empty --profile dev-app1-debug2-deps3-debug0 --cache first --locale en --runs 5 --webview-profile fresh-per-run --verification after-batch --expected-exe-sha e77ea3a6a09da71be2862e4c17d517fd2c71a6206254243a230801bb6688d308
node scripts/measure-startup.mjs --exe D:\limo-startup-candidate-separated-runtime\nbcad.exe --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --output D:\limo-repro-separated\runtime-file --profile dev-app1-debug2-deps3-debug0 --cache first --locale en --runs 5 --webview-profile fresh-per-run --verification after-batch --expected-exe-sha e77ea3a6a09da71be2862e4c17d517fd2c71a6206254243a230801bb6688d308 --document examples/testPiece.nbcad --document-mode runtime --expected-document-sha d7e65fb222a0bb59f0ac82c3f3143a3437a487f3bbacf3a1f314b989b96de609
```

For a first launch after Windows restarts, prepare these commands and immutable
inputs beforehand, use `--runs 1`, and launch before reading/hashing the inputs
elsewhere. Record the boot timestamp, cache conditions, and other host work.
Repeat with a new restart for each boot-cold sample. Persistent driver caches
remain a separate condition; these commands do not invalidate them.

### Current Thunder measurements — separate application and file

The final immutable `e77ea3…` candidate ran at 21:16–21:18 UTC on October 11.
Five empty launches and five launches that opened the sample after workspace
readiness passed, with no warmups, distinct initially empty browser profiles,
and after-batch input verification. Own compilation and preview servers were
stopped. Hash verification after the empty batch read the executable before
the later runtime-file batch, potentially warming its filesystem pages. Each
harness invocation avoided its own input pre-reads; the whole experiment did
not control file/OS/driver coldness. Unrelated work was preserved: the final compiler snapshot recorded
10 Cargo, 13 rustc, and two linker processes. The start snapshot timed out with
partial output retained. This is a shared-host observation under substantial
contention, not an isolated performance-regression experiment.

- Empty application: median useful feedback **871 ms**, range **651–6545 ms**;
  usable workspace **2279 ms**, range **1935–12015 ms**.
- Application in the runtime-file batch, before any file open: feedback
  **728 ms**, range **665–943 ms**; workspace **1891 ms**, range **1724–2128 ms**.
- Opening the unchanged 2,322-byte, four-history-entry sample after readiness:
  **370 ms** median, range **331–397 ms**, through a successful new frame for
  the current session and geometry revision. This is a directly measured file
  operation, not a subtraction of workload medians.
- Empty camera/picking median **20.8 ms**, range **6.6–76.3 ms**; first real CAD
  edit **391 ms**, range **376–811 ms**, repeat **248 ms**, range **219–279 ms**.
  Runtime-file camera/picking median **7.1 ms**, range **6.8–13.3 ms**; first CAD
  edit **296 ms**, range **262–344 ms**, repeat **292 ms**, range **266–1074 ms**.
  The 1,074 ms repeated edit remains in the report; 600 ms was its solid-frame
  interval. No multi-second startup stall was transferred to the first edit,
  but interaction is not uniformly instant under this host load.

The slow first empty launch is retained: **6.545 s** feedback and **12.015 s**
workspace. Launch-to-Rust-main took **2.645 s**; main-to-setup took **3.766 s**;
setup-to-feedback took **135 ms**. The eager native CAD engine setup inside
main-to-setup was only **3.23 ms**. Window/WebView/framework preparation occupies
the remaining early interval, but this telemetry does not split it further or
establish why it was slow. GPU-device preparation took **3.225 s**, native fonts
**80 ms**, and Bevy renderer installation **646 ms**. Installed renderer to
expansion request took **1.277 s**, then the expanded workspace marker arrived
**233 ms** later. Initial engine/session getters took **161 ms** and overlapped
GPU work. These exact operation spans have small dispatch gaps and must not be
summed as if they were exclusive phases. Cache state, scheduling, antivirus,
and individual driver operations were not independently traced.

A representative actual runtime-file launch, run 4, gives a nonoverlapping
application timeline that sums to **1891 ms**:

- Process/DLL load before Rust main: **67 ms**.
- Main to native setup, including window/WebView/framework preparation:
  **583 ms**. Native CAD-engine setup within it was **0.115 ms**.
- Setup to useful feedback receipt: **78 ms**.
- Feedback to installed renderer: **487 ms**. Within this interval, GPU-device
  preparation was **323 ms**, native fonts **9 ms**, and renderer installation
  **150 ms**. These nested spans and dispatch gaps explain the interval.
- Installed renderer to expansion request: **568 ms**. This joint interval
  includes remaining session/first-view resources, uploads, successful required
  pipelines, the compact presented frame, and polling/IPC; those costs are not
  individually attributed by the current instrumentation.
- Expansion request to usable expanded workspace receipt: **108 ms**.

The same launch's **369.7 ms** file operation is partitioned on one WebView
clock: preparation/metrics **8.6 ms**, read including IPC **45.2 ms**, archive
validation/decoding **1.6 ms**, dispatch gap **2.6 ms**, geometry/replay including
IPC **110.3 ms**, document adoption **56.2 ms**, and final current-frame proof
**145.2 ms**. Actual native file reading was only **0.187 ms**, nested within
the read/IPC interval; native project loading was **86.2 ms**, nested within
the geometry interval. The final proof interval includes model publication,
render resources/pipelines/presentation and observation; it is not a pure GPU
time. Median native project loading across the five file opens was **103 ms**,
range **73–156 ms**. CAD diagnostic restoration events are excluded from these
file-only native summaries.

The [raw empty report](startup-measurements/thunder-separated-empty-en.json),
[runtime-file report](startup-measurements/thunder-separated-runtime-document-en.json),
[summary](startup-measurements/thunder-separated-summary.json),
[representative timeline](startup-measurements/thunder-separated-representative-timeline.json),
[retained slow first launch](startup-measurements/thunder-separated-first-slow-launch-timeline.json),
and [stderr/hash audit](startup-measurements/thunder-separated-stderr-audit.json)
retain all firsts, ranges, clocks and unchanged inputs. All ten launches passed
camera/picking, real first/repeated sketch/extrusion, exact document restoration,
and applicable current-file-frame proof. No detected stderr warnings, panics,
device loss, or GPU validation failures occurred.

Earlier that hour, `f9d476…` under the same fresh-profile/post-hash policy had
an empty workspace median of 1517 ms, versus this batch's 2279 ms. The earlier
compiler endpoints sampled four Cargo processes and no rustc; the final new
batch had many active compilers. Different binary, launch order, cache history,
and changing contention prevent attributing the difference to code. The matched
original-baseline improvements retained below remain historical evidence;
they do not prove the current candidate is faster under these changed conditions.
Verified first-after-Windows-restart and shipping-profile numbers remain outstanding.

### Application/file-boundary verification

The final frontend suite, added file-picker ordering/cancellation tests, targeted
startup tests, and desktop asset build passed. Native viewport tests passed
68 cases, and the two new diagnostic-clock/scope tests passed; five existing
GPU visual-matrix cases remain ignored. Results are retained in
[frontend tests](startup-measurements/frontend-file-progress-tests.log),
[targeted tests](startup-measurements/frontend-file-progress-targeted.log),
[frontend build](startup-measurements/frontend-file-progress-build.log),
[native viewport tests](startup-measurements/thunder-native-viewport-tests.log), and
[native telemetry tests](startup-measurements/thunder-startup-metrics-tests.log).
The final native application build includes the additional engine-setup and
window-expansion-request landmarks. Repository diff whitespace checks passed.

Selected real component states were captured in
[English](startup-measurements/captures/startup-file-en.png),
[Simplified Chinese](startup-measurements/captures/startup-file-zh-CN.png),
[Spanish](startup-measurements/captures/startup-file-es.png), and
[German](startup-measurements/captures/startup-file-de.png).
All fit the 520 × 440 compact client area with a long filename. These are
illustrative captures, not native timed-launch screenshots. The
[runtime status capture](startup-measurements/captures/workspace-file-progress-en.png)
shows a local nonmodal indicator; hit testing passed through it to the canvas.
The [recovery layout](startup-measurements/captures/startup-file-failure-en.png)
fits its controls and hides the workshop note. Locale assets share the same
text-free artwork and use their selected language exclusively.

Actual Windows checks on this executable also verified an ordinary launch
without diagnostic environment overrides and an actual native empty grid. Its
[native normal-startup capture](startup-measurements/captures/native-en-separated-normal.jpg)
uses an isolated fresh profile. A separate invalid private ZIP showed the
[compact English recovery controls](startup-measurements/captures/native-en-separated-recovery.jpg).
Repairing only that private fixture and choosing Try again retained PID 50536
and its document session, rendered the sample with all four history entries,
and passed real CAD edits at 415.5/276.5 ms plus exact restoration. The
[actual retry frame](startup-measurements/captures/native-en-separated-retry-loading.jpg)
shows "CAD initialized." independently of "Updating the document…".
[Face picking](startup-measurements/captures/native-en-separated-face-pick.jpg)
reported approximately 29 × 14 mm, 339.97 mm² and 126.789 mm perimeter.
The [recovery trace](startup-measurements/thunder-separated-recovery.jsonl)
includes deliberate human waits and is excluded from comparable launch timing.
Simplified Chinese independently verified its native title, glyphs, compact
[recovery controls](startup-measurements/captures/native-zh-separated-recovery.jpg),
and Open an empty workspace in the same PID 50864/session. The
[recovered native grid](startup-measurements/captures/native-zh-separated-empty-recovered.jpg)
had an empty history and passed CAD edits at 460.3/350.7 ms with exact restoration.
Its [trace](startup-measurements/thunder-separated-empty-recovery.jsonl) likewise
includes human waits. All task-owned native UI processes were retired, and
the original sample and two installed user CAD windows were preserved.
The native file-dialog smoke check reached the picker but automated filename
focus could not be established, so that current-build manual path is not claimed
as completed. The ten controlled launch/file workflows and prior-build actual
file-dialog checks remain separate evidence.
The [functional receipt](startup-measurements/thunder-separated-functional-receipt.json)
records these current executable/PID checks and their limits.

## Compact native window follow-up — October 11, 2026

The existing native window now opens centered with a 520 × 440 logical-pixel
client area. It cannot be maximized or resized during preparation. The shared
text-free mechanical drawing, localized status, and one stationary workshop
note fit in a 432-pixel card. English startup, notes, and recovery controls stay
entirely English. No animation or minimum display time delays readiness.

The real native viewport uses the full compact client area beneath the overlay,
independent of optional workspace panels. Saved CAD page zoom is deferred until
the window expands, keeping compact feedback readable. After the current
document's compact frame succeeds, the same window returns to the existing
1440 × 900 workspace size and 1200 × 760 minimum. Before revealing the workspace,
the frontend matches the current DOM viewport in native logical units, while
Bevy independently proves presentation at the new layout generation. Both the
acquired window dimensions and the actual GPU texture must match the required
physical extent. A stale compact frame cannot release the overlay.

Native phases distinguish device setup, font preparation, drawing resources,
GPU preparation, pipeline creation, and presentation. The frontend's 60-second
deadline covers document preparation and the final expanded frame. It retains
failure even when a pending native command completes later. Slow-start wording
is separate from the localized timeout failure.

The new immutable executable has SHA-256
`f9d476b25c3d5a37effd2ff3990d0c93bd5d22c3a71fe847102cd6307a220ebb`.
Its embedded frontend bundle is `index-DG1a8uxa.js`, SHA-256
`7b7f1b82ea90be81450489206195a8a1d499276b359c2e5e9fc072cd52050a65`.
The immediately previous executable is the v3 build with SHA-256
`6d6fc31655cb1e5bc1dc78fb6eccc0ffa74aaea9b3465d560291e15aabf8a67c`.
Both use the same development profile, checked OCCT SDK, and no sibling DLL
copies. Earlier original-baseline comparisons below are historical evidence;
the follow-up measures these two executables afresh.

### Latest matched launches

The final October 11 batch contains five measured launches and one excluded
warmup for each executable/workload combination: 20 recorded launches and four
warmups. English, the development profile, hardware, checked SDK, repeated-launch
conditions, and the unchanged sample document were matched. OS/driver caches
were not reset or verified identical. Each workload batch
created one private WebView profile, reused by its warmup and five measured
launches. The harness verified hashes before launches, reading the entire
executable and optional document into filesystem caches. These are warm-cache,
fresh-process observations, not boot-cold or fresh-profile observations.
Compilation time is excluded. Unrelated compiler jobs
on the shared host continued throughout and are recorded in start/end snapshots.

Medians from the previous v3 executable to the final compact executable:

- Empty launch: first feedback **565 → 566 ms**; usable workspace
  **1519 → 1490 ms**; first CAD edit **320 → 266 ms**; repeated edit **198 → 166 ms**.
- `examples/testPiece.nbcad`: first feedback **552 → 618 ms**; usable workspace
  **1522 → 1670 ms**; first CAD edit **259 → 220 ms**; repeated edit **231 → 220 ms**.
- Final feedback ranges: **535–696 ms** empty and **498–797 ms** sample.
  Final workspace ranges: **1349–1778 ms** empty and **1415–1922 ms** sample.
  First camera response medians were **9.9 ms** empty and **13.2 ms** sample.

Total startup results are mixed: empty-workspace median improved 1.9%, while
the sample-workspace median increased 9.7%. These five-run batches with shared
host contention do not establish a uniform v3-to-compact speedup. Earlier
original-baseline gains below remain historical evidence and are not substituted
for these current measurements. First and repeated CAD trials both restored
the exact document state; the first interaction did not inherit a multi-second
startup delay. All 24 launches completed with no detected stderr warnings,
panics, device loss, or GPU validation failures.

The initial compact experiment preserved executable SHA-256
`73143d60a2322e5fdd5ebdb86984d7c954b5d3943da93c522e8c9523230160e7`.
It measured a 104 ms empty / 89 ms sample window-expansion median. The final
change polls readiness every 16 ms after expansion, retaining the 50 ms cadence
during initial preparation. It still requires the actual expanded frame and
has no minimum dwell. Final expansion medians were **62 ms** empty (58–90 ms)
and **70 ms** sample (55–82 ms). That bounded transition experiment succeeded;
optional optimization stopped after it.

After-only instrumentation measured GPU-device preparation medians of
**266 / 257 ms**, native fonts **8 / 7 ms**, main-thread renderer installation
**144 / 135 ms**, and document workflow **440 / 589 ms** (empty / sample).
Document work overlaps native preparation. These spans must not be added to
form startup time or compared to nonexistent baseline spans.

The [paired summary](startup-measurements/compact-poll-paired-summary.json),
[phase summary](startup-measurements/compact-poll-phase-summary.json), and
[stderr/hash audit](startup-measurements/compact-poll-stderr-audit.json) retain
all values. Raw `compact-poll-before-v3-*` and `compact-poll-after-compact-poll-*`
reports and compiler snapshots are beside them. The initial compact experiment
is preserved separately as `compact-*` reports.

To reproduce on this host, run the harness below for each immutable executable
and each workload, using a new output directory every time. The baseline path
is `D:\limo-startup-candidate-no-warmup-runtime\nbcad.exe`; the final candidate
path is `D:\limo-startup-candidate-compact-poll-runtime\nbcad.exe`. Omit
`--document` for the empty launch. Stop this task's own compilers, UI sessions,
and preview servers before the four batches; preserve other host applications.

```powershell
node scripts/measure-startup.mjs --exe D:\limo-startup-candidate-compact-poll-runtime\nbcad.exe --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --output D:\limo-repro-compact\after-document --profile dev-app1-debug2-deps3-debug0 --cache warm --locale en --label after-compact-poll --runs 5 --document examples/testPiece.nbcad
```

The exact four-batch host command is retained at
`D:\limo-startup-baseline-e4015de\measure-paired-compact-poll.cmd`; its original
output directories already exist and must be changed before rerunning.

### Compact-window verification

The final frontend suite and desktop asset build passed. Browser checks covered
all 28 phase/locale combinations with the longest workshop notes, all four
document-recovery layouts and long renderer errors, light mode, and the expanded
card. Long details scroll within 72 pixels while recovery controls stay visible.
The [English compact capture](startup-measurements/captures/loading-en-compact.png)
shows the regular loading treatment; equivalent Chinese, Spanish, and German
captures are retained beside it.

Five current English examples and the first example's four-language companions
are retained in [the examples directory](startup-measurements/captures/examples).
Their [caption metadata](startup-measurements/captures/examples/captions.json)
records the deliberately selected real phase and existing workshop note for
each 520 × 440 capture. These are illustrative captures of the actual UI,
not timed native-launch screenshots. Artwork and layout are shared; all text
is typeset in the selected language. A real launch retains one note while
its measured phase changes.

The final native suite passed **68 tests**, including three new layout-generation
regressions. Five existing GPU visual-matrix tests remain ignored.
[Native test output](startup-measurements/compact-native-viewport-tests.log) and
[the final native build log](startup-measurements/compact-poll-native-build.log)
are retained. The final frontend-only polling change also passed the
[startup tests](startup-measurements/compact-poll-frontend-startup-tests.log)
and [desktop asset build](startup-measurements/compact-poll-frontend-build.log).

Actual Windows checks verified a 522 × 472 outer-window capture, including
decorations around the requested 520 × 440 client area. An owned invalid ZIP
produced the [English compact recovery window](startup-measurements/captures/native-en-compact-recovery.jpg).
Repairing only its private copy and choosing Try again retained PID 41416,
loaded all four sample history entries, expanded the same window, and passed
first/repeated CAD edits plus exact restoration. The
[native loading frame](startup-measurements/captures/native-en-compact-retry-loading.jpg)
and [face selection](startup-measurements/captures/native-en-compact-face-pick.jpg)
record actual pixels. Selection reported approximately 29 × 14 mm, 339.97 mm²,
and 126.789 mm perimeter.

An ordinary launch without any `NBCAD_STARTUP_*` variables restored a saved
175% CAD UI zoom after compact preparation and revealed the real empty grid.
The [final normal-startup capture](startup-measurements/captures/native-en-compact-final-normal-saved-zoom.jpg)
records that result. Simplified Chinese separately verified the regional native
title, compact loading note, glyphs, and recovery actions. Open empty workspace
retained PID 24876, displayed an empty rendered history, and passed the same CAD
diagnostic. [Chinese loading](startup-measurements/captures/native-zh-compact-loading.jpg)
and [recovered empty workspace](startup-measurements/captures/native-zh-compact-empty-recovered.jpg)
captures are retained. These recovery timings include human waits and some own
compilation, so they are excluded from comparable launch timings. All owned UI
processes and compilers were stopped before the fresh comparison; the two
installed user CAD windows and original sample file were preserved.

Native macOS/Linux window transitions, cold OS/driver caches, shipping-profile
timings, and the five ignored visual-matrix tests remain unverified.

## Audited baseline

The baseline is `e4015de638b55823299d4a195e20a10ecbe9fae7`. The native
application has a React 18.3.1 shell, Zustand 5.0.14 state, and an embedded
Bevy viewport. The desktop lockfile resolves Tauri 2.11.5, Bevy 0.19.0 and
wgpu 29.0.4; the frontend build resolves Vite 6.4.3. Browser preview uses the
WASM host and is a separate measurement surface.

The repository has separate root engine and `src-tauri` Cargo workspaces.
Before this change neither root declared compiler profiles. The checked-in
`.cargo/config.toml` declared only an `xtask` alias. On the measurement host
there was no user Cargo config and no `CARGO_*`, `RUST*`, `OCCT_*`, or `VCPKG_*`
environment override in the invoking shell. Thus the original desktop dev
build used Cargo's default `opt-level = 0`; release used its default level 3.

The original startup path installed the native child view, constructed Bevy's
plugins, finished GPU initialization, installed the scene, and eagerly ran two
frames inside Tauri's main-thread `with_webview` callback. The callback could
delay the webview's first feedback. The frontend then fetched the document,
its geometry and dependent workspace state, registered its first retained
session, and exported its recovery snapshot. Existing native `ready` meant
the runtime had been installed; it did not prove its required pipelines had
compiled or its initial geometry had appeared.

Both workspace roots now use Bevy's documented `opt-level = 1` application /
level 3 dependencies starting point for the development-profile experiment.
The wildcard excludes members of the active workspace;
`src-tauri` contains only `nbcad`, so its path engine dependencies qualify as
dependencies for that workspace. The root engine workspace members need
their own explicit package overrides if level 3 is desired; they retain level 1
until engine CPU timing justifies a stronger override. Development debug
assertions, overflow checks, unwinding, and portable target settings retain
Cargo defaults. The shipping profile was audited and retained at its original
default level 3. No measured evidence justified changing LTO, codegen units,
panic behavior, stripping, or CPU target features.

The first level 1 / level 3 development build with full dependency debug
information failed with LLVM out of memory while compiling `bevy_reflect`.
The host had about 1 GB of available physical memory at that point. The
experiment therefore sets dependency `debug = 0`, while the application and
active workspace members retain their default level 2 debug information.
This reduces compiler memory and dependency symbols; it does not change Rust
debug assertions or overflow checks. That failed compile produced no startup
measurement, and compilation time remains outside all launch intervals.

References: [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)
and [Bevy development optimizations](https://bevy.org/learn/quick-start/getting-started/setup/#compile-with-performance-optimizations).
The readiness observer adapts the version-matched
[Bevy 0.19 loading-screen example](https://github.com/bevyengine/bevy/blob/v0.19.0/examples/showcase/loading_screen.rs).

## Implemented startup behavior

The small HTML/CSS bootstrap provides localized feedback before React and the
engine initialize. After that feedback can paint, the existing Tauri blocking
task facility prepares the GPU device and regional font collection. Window
ownership, Bevy installation, scene updates, and rendering remain on the main
thread. The native viewport stays active beneath the overlay.

Readiness requires the current document/session and geometry revision, loaded
assets and dependencies, prepared GPU resources, successful pipelines for visible
material meshes, and a presented real-size frame. An empty pipeline queue is
insufficient. The overlay displays real preparation phases without percentages
or a minimum display duration. Only the actual initial viewport's resources and
pipelines hold up readiness. Optional first-edit shader warmups were removed
after measuring their startup cost against their interaction benefit.

Document failures offer retry and an empty workspace within the existing native
session. Empty recovery clears the failed file's save destination. Renderer
failures preserve useful details and offer close/reopen guidance; a caught render
panic stops further access to the potentially damaged Bevy app. A slow-start
notice makes recovery controls available without pretending work has completed.

English, Spanish, and German use Limo CAD; Simplified Chinese uses 砺模 CAD.
Saved locale preferences and existing browser-language fallback rules apply from
the bootstrap onward. The shared mechanical drawing contains no embedded text;
localized names, published regional taglines, statuses, and recovery actions are
typeset separately. The four `loading-*-final.png` captures verify the web shell;
the `native-*` captures separately record actual viewport pixels and interaction.

A locally bundled pool contains 100 original playful engineering asides per supported
locale. A launch selects one note and keeps it stationary beneath the real
phase. The English screen uses English copy throughout. The text-free sketch
is a static SVG, with no additional GPU scene or mandatory animation. The
optional note script loads asynchronously; missing or late notes do not hold
up feedback, native initialization, or readiness. Recovery and slow-start
screens hide the note so the cause and available actions remain prominent.

## Native verification

The final native implementation passed 65 `native_viewport::` Rust tests, with
five existing GPU visual tests ignored and 54 unrelated tests filtered out.
The [test log](startup-measurements/native-viewport-tests.log) includes readiness
stages, an empty pipeline cache, stale document frames, the asynchronous report
publication race, startup timeouts, panic quarantine,
session recovery, and actual OCCT geometry picking.

On this Windows host, the compiled test artifact initially stopped before the
harness with `STATUS_ENTRYPOINT_NOT_FOUND`: it imports
`comctl32.dll!TaskDialogIndirect` but lacks the application's common-controls v6
manifest. The application's normal executable and CAD diagnostics ran correctly.
The [Windows test helper](../scripts/run-native-viewport-tests.ps1) copies the test
artifact into a new directory and embeds the application's existing manifest.
It preserves Cargo's original test executable and restores its process-local PATH.
Build the test artifact with `cargo test --locked --manifest-path
src-tauri/Cargo.toml --features custom-protocol --lib native_viewport:: --no-run`,
then use its reported path in this command:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/run-native-viewport-tests.ps1 `
  -TestExecutable "PATH_FROM_CARGO_TEST_NO_RUN" `
  -AppExecutable "PATH_TO_BUILT_NBCAD_EXE" `
  -SdkBin "PATH_TO_OCCT_SDK/bin" `
  -ManifestTool "PATH_TO_WINDOWS_SDK/x64/mt.exe" `
  -OutputDirectory "NEW_PRIVATE_TEST_DIRECTORY"
```

The command enables the repository script only in that child PowerShell process;
it does not alter the host's persistent execution policy. Actual desktop checks
separately verify native GPU pixels and interaction; the ignored visual-test
matrix is not reported as having run.

Desktop functional checks on the optimized native implementation used a private
WebView2 profile and preserved the two installed CAD windows. A normal English
launch without benchmark environment variables opened the real grid. Opening
`examples/testPiece.nbcad` through the actual file dialog rendered the chamfered
two-hole solid with all four history entries. A real top-face selection displayed
approximately 29 × 14 mm, area 339.97 mm², and perimeter 126.789 mm; the
[English native capture](startup-measurements/captures/native-en-v3-face-pick.jpg)
records the result. Settings verified regional window titles and glyphs for
English, Simplified Chinese, Spanish, and German.

An owned invalid ZIP fixture produced the
[English recovery screen](startup-measurements/captures/native-en-final-recovery.jpg)
with no secondary note. Repairing only that fixture and choosing Try again
restored the sample's four-entry document in the same process and native session.
A separate Open empty workspace recovery presented an actual native grid and
an empty history in the same process. Both recoveries then passed the real CAD
diagnostic and exact restoration check. Their elapsed startup times include
intentional human interaction and are excluded from performance comparisons.
All three test-owned UI processes were retired; the original sample file and
installed CAD windows were preserved. The four-locale native settings checks
precede the user-selected playful-copy update; regional naming and font code are
unchanged. The final executable separately repeated normal English startup,
actual file-dialog Open, face picking and both document recovery actions.

The final native
[loading capture](startup-measurements/captures/native-en-v3-loading-playful.jpg)
shows the actual English startup phase and stationary playful note. Its
[failure capture](startup-measurements/captures/native-en-v3-recovery.jpg)
shows actionable recovery with the note hidden. Repairing an owned invalid fixture
and choosing Try again restored the four-entry solid in the same process;
first/repeated CAD diagnostics and exact restoration passed at 271/167 ms.
The separate Open empty workspace recovery showed a real grid and empty history,
then passed the same diagnostics at 327/238 ms. Captures retain the
[restored document](startup-measurements/captures/native-en-v3-retry-restored.jpg)
and [recovered empty workspace](startup-measurements/captures/native-en-v3-empty-recovered.jpg).
These intentional recovery waits are excluded from launch measurements.
All final test-owned processes were retired; the original fixture and two
installed CAD windows remain untouched.

## Reproduction

Build both source revisions before measuring. Keep the hardware, SDK, profile,
locale, document, driver, power settings, and cache condition matched. The
harness runs one discarded warm-up for `--cache warm`. Label the first launch
separately with `--cache first --runs 1`. Do not call a launch cold merely because it is a fresh
process: OS and GPU-driver caches survive process exit.

For a Windows packaged frontend development binary, from a Visual Studio
developer shell:

```powershell
$env:OCCT_ROOT = 'D:\limo-cad-maintenance\checked-occt-sdk'
$env:CARGO_TARGET_DIR = 'D:\limo-startup-target'
npm ci
npm run build:desktop
cargo build --locked --manifest-path src-tauri/Cargo.toml --features custom-protocol --bin nbcad
node scripts/measure-startup.mjs --exe D:\limo-startup-target\debug\nbcad.exe --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --output D:\limo-startup-results\candidate-empty-en --profile dev-app1-debug2-deps3-debug0 --cache warm --locale en --runs 5
```

For a comparison that holds the original compiler settings fixed despite the
new profiles, pass Cargo configuration overrides to the candidate build:

```powershell
cargo build --locked --manifest-path src-tauri/Cargo.toml --features custom-protocol --bin nbcad --config "profile.dev.opt-level=0" --config "profile.dev.package.'*'.opt-level=0" --config "profile.dev.package.'*'.debug=2"
```

Keep the optimized development experiment in a separate target directory and
record `dev-app1-debug2-deps3-debug0` in its harness `--profile` label.
The original baseline has no profile declarations; build its observer checkout
with these overrides to match that optimized candidate:

```powershell
cargo build --locked --manifest-path D:\limo-startup-baseline\src-tauri\Cargo.toml --features custom-protocol --bin nbcad --config "profile.dev.opt-level=1" --config "profile.dev.package.'*'.opt-level=3" --config "profile.dev.package.'*'.debug=0"
```

Use distinct output/runtime directories for baseline and candidate. Separate
Cargo targets are simplest; these measurements instead reused one exclusively
owned target serially, copying each completed executable into an immutable
runtime directory before building the other revision. Do not replace the
installed application or share an in-progress Cargo target with another agent.
`--profile` records the effective profile label and does not
compile the executable. Use `--label baseline` or `--label candidate` to name
the measurement. The output directory must have no existing `run-NN` folders
or WebView2 data folder. Each executable uses a dedicated
`WEBVIEW2_USER_DATA_FOLDER` inside its results directory. Baseline and
candidate begin with separately created empty folders, then retain them
across warm-up and repeated runs. This isolates preferences, emergency
snapshots and benchmark document state from the installed application.

The harness uses opt-in `NBCAD_STARTUP_REPORT_FILE` JSONL events. Each event
contains its phase, native process elapsed time, webview elapsed time, and PID.
The native `processStartedUnixMs` timestamp anchors the main-entry interval to
the harness's launch timestamp, including time loading the executable and its
DLLs before Rust `main`. The phases are `feedback`, `workspace`, `interaction`,
and `cadInteraction`. The latter two measure camera rendering plus picking and
a real first sketch rectangle and solid extrusion respectively. The CAD observer
repeats the edit to compare first-use and warm interaction. It restores the
original parsed JSON model and frontend state after each trial before
publishing its result; only JSON object key order is immaterial, and every
array, scalar, and metadata value remains exact. A successful restored-model
frame is required before starting the repeated trial. The event's bounded
detail includes sketch commands, sketch frame preparation, solid commands,
solid frame preparation, and repeated-edit timing. Both
interaction durations must be supplied. Null interaction
latency is outstanding verification, never a zero-latency claim. The harness
stops on the first missing milestone or explicit diagnostic `error` event and
retains the error detail, rather than publishing partial runs as successful.
Under the default `before-each` verification policy, it hashes the executable
before and after measurements and records hardware,
document hash, locale, cache label, stderr, and all raw events. Executable and
fixture hashes must remain unchanged before every run and after the batch.
The opt-in `after-batch` policy instead uses trusted expected input hashes and
verifies unchanged files after the batch, avoiding harness pre-launch input reads.
It retires only
the process it spawned.

The baseline must receive the same telemetry and readiness observer without
changing its initialization, scheduling, or optimization behavior. Comparing
the old runtime-installed probe with the new rendered-workspace gate is not a
valid comparison.

The exact observer-only backport is retained in
`scripts/startup-baseline-observers.patch`. Apply it to a clean checkout of the
baseline revision before building:

```powershell
git worktree add --detach D:\limo-startup-baseline e4015de638b55823299d4a195e20a10ecbe9fae7
git -C D:\limo-startup-baseline apply "$PWD\scripts\startup-baseline-observers.patch"
```

For representative-document measurements use `examples/testPiece.nbcad`
(or the smaller retained-tab fixture `xtask/fixtures/switching/part-a.nbcad`).
Use `--document PATH` to hash the fixture and forward `NBCAD_STARTUP_DOCUMENT`.
The opt-in observer uses the existing `openProject` file workflow after first
tab registration, then measures readiness for the loaded document. The same
observer is in both baseline and candidate. The normal production command
line continues to accept only recipe URLs and `--headless`, not document paths.
A representative document must not be silently replaced by an empty-workspace
measurement.

## Verification record

### Final matched launches

The [final paired summary](startup-measurements/no-warmup-paired-summary.json)
and [empty baseline](startup-measurements/no-warmup-baseline-empty-en.json),
[empty candidate](startup-measurements/no-warmup-candidate-empty-en.json),
[document baseline](startup-measurements/no-warmup-baseline-document-en.json),
and [document candidate](startup-measurements/no-warmup-candidate-document-en.json)
record five warm measured launches per workload/executable, each following one
discarded warm-up. All 20 measured launches and four warm-ups passed readiness,
camera/picking, first and repeated sketch/extrusion, and exact model restoration.
No panic, device-loss or GPU-validation errors were found in their stderr logs.

- Empty project: median feedback **1,444 → 583 ms** (60% earlier); workspace
  **1,678 → 1,429 ms** (15% faster). Workspace ranges were 1,498–1,988 ms before
  and 1,310–1,536 ms after.
- Representative document: median feedback **1,553 → 531 ms** (66% earlier);
  workspace **2,058 → 1,391 ms** (32% faster). Workspace ranges were 1,712–2,590 ms
  before and 1,357–1,854 ms after.
- First CAD edit: empty **269 → 264 ms**, document **244 → 234 ms**. Candidate
  first-edit ranges were 214–323 ms and 164–247 ms; repeated-edit ranges were
  132–224 ms and 163–424 ms. Candidate camera/picking took 4.5–11.5 ms.

The faster startup did not move a multi-second stall into initial CAD interaction.
It does not eliminate every first-use cost: the empty project's first solid frame
took 96–152 ms versus 27–68 ms on the repeated edit, about 80 ms of bounded
residual shader preparation. Its total first-edit time remained comparable to
baseline. The optional warmup removal is adopted, and optional optimization stops
here; persistent shader caching was not justified by this remaining cost.

These are descriptive medians from five runs on a shared Windows host, not a
statistical significance claim. Startup ranges partly overlap. The
[start process snapshot](startup-measurements/no-warmup-background-compilers-start.json)
found no compilers. The end snapshot saw three transient Cargo processes with
zero CPU time and no rustc/native linker; they were already gone on the immediate
follow-up query. Unrelated applications were preserved. OS/driver caches were
warm and left intact. Full raw events, fixture/executable hashes and first/repeated
edit subdivisions are retained in the linked reports.

Reproduce the final measurements after building and staging both revisions:

```powershell
node scripts/measure-startup.mjs --exe D:\limo-startup-baseline-optimized-runtime\nbcad.exe --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --output D:\limo-repro\baseline-empty --profile dev-app1-debug2-deps3-debug0 --cache warm --locale en --label baseline --runs 5
node scripts/measure-startup.mjs --exe D:\limo-startup-candidate-no-warmup-runtime\nbcad.exe --runtime-dir D:\limo-cad-maintenance\checked-occt-sdk\bin --output D:\limo-repro\candidate-empty --profile dev-app1-debug2-deps3-debug0 --cache warm --locale en --label candidate --runs 5
```

Repeat those two commands with fresh output directories and
`--document "$PWD\examples\testPiece.nbcad"` for the representative document.
Use unchanged immutable executables, keep all of this task's compilers/preview
servers stopped, and retain the matched profile, locale, hardware and cache label.
Run `npm run test:frontend` and `npm run build:desktop` for the frontend checks;
both passed on the final playful-copy assets. Native checks use the manifest-copy
helper described above.

Outstanding verification: native macOS/Linux launches, cold OS/driver-cache
launches, shipping-profile timings, and the five existing ignored GPU visual-matrix
tests were not run successfully. The verified native
platform is Windows x64; earlier web loading layouts were captured at 1280 × 800, and
native captures record the actual desktop window. No claim
is made for those unverified conditions.

### Retained experiments and historical checks

The retained [first optimized paired comparison](startup-measurements/optimized-paired-summary.json)
contains five recorded warm launches per executable and workload, plus one
discarded warm-up per batch. All 20 recorded runs and four warm-ups passed the
camera, first/repeated CAD, and exact document-restoration diagnostics. The
baseline and candidate shared the optimized development profile, English locale,
SDK, hardware, document and cache preparation. Compilation was excluded.

In that comparison, median feedback improved from 1,436 to 682 ms for an empty
project and from 1,549 to 665 ms for the sample document. Workspace readiness
was mixed: empty 1,628 to 1,788 ms, sample document 1,975 to 1,774 ms. The ranges
overlapped, so the empty-workspace regression and noisy document improvement
were retained rather than described as a completed readiness optimization.
First CAD edits were 304 to 287 ms for empty projects and 262 to 280 ms for
the sample; candidate repeated edits were 228 and 244 ms respectively.

This exposed a poor tradeoff in the optional two zero-area shader-warmup meshes:
they had been added after an unoptimized first edit took 2,149 ms, but the
optimized comparison showed only a 17 ms median empty first-edit benefit alongside
a 160 ms startup cost. They were removed in a small reversible experiment.
The required assets, actual visible-mesh successful pipelines, and presented-frame
observer were unchanged. The final experiment uses a fresh paired launch batch;
the earlier reports remain as evidence of the rejected hypothesis.

The final executable is immutable SHA-256
`6d6fc31655cb1e5bc1dc78fb6eccc0ffa74aaea9b3465d560291e15aabf8a67c`;
the baseline is
`472a491839e47b529e9949c04d900ec56b575b06de76846a0b28832ee9f622af`.
Both resolve the same checked SDK DLLs through the SDK `bin` directory in PATH.
Neither runtime folder contains additional DLL copies. The original sample hash
is `d7e65fb222a0bb59f0ac82c3f3143a3437a487f3bbacf3a1f314b989b96de609`.

The completed original empty-workspace baseline is retained in
[`startup-measurements/baseline-empty-en.json`](startup-measurements/baseline-empty-en.json).
Five warm launches with the original `dev` optimization level 0 / debug level 2
had median first feedback 4,856 ms and rendered workspace 5,930 ms. The full
ranges were 4,212–27,365 ms and 4,846–27,778 ms respectively. Initial camera
rendering and picking took a median 11.7 ms; the first real sketch and solid
extrusion took 1,350 ms (range 1,151–1,601 ms). This host had unrelated active
applications and compiler processes, which were preserved. The broad startup
spread must be retained when assessing candidate results. No compilation from
this task ran during these timed launches.
The final comparisons use the same optimized profile and stop this task's
compilers and development servers during timed launches. Unrelated host
workloads remain in place. The original unoptimized median cannot establish
the causal improvement of a differently optimized candidate.

These original-profile measurements are historical discovery runs, not the
matched before/after comparison. The final comparisons are recorded separately
below, including representative-document restoration.

The first unoptimized candidate discovery is retained in
[`startup-measurements/candidate-dev0-empty-discovery.json`](startup-measurements/candidate-dev0-empty-discovery.json):
first feedback 2,297 ms, workspace 6,387 ms, camera 25.4 ms, and first CAD edit
2,149 ms. This single discovery run is not the final repeated comparison. It
identified first-interaction work that still needed pipeline preparation and
the optimized development-profile experiment.

The representative-document discovery in
[`startup-measurements/candidate-dev0-document-diagnostic.json`](startup-measurements/candidate-dev0-document-diagnostic.json)
reached a rendered workspace and camera interaction, then explicitly rejected
the diagnostic's strict restoration check. It is retained as a failed
diagnostic, with its error detail; it is not a successful CAD verification run.

The optimized candidate subsequently passed the final strict diagnostic for
both [empty](startup-measurements/candidate-optimized-empty-diagnostic.json)
and [representative-document](startup-measurements/candidate-optimized-document-diagnostic.json)
launches, including exact parsed-model restoration and a successful restored
frame before the repeated edit. The empty launch took 1,970 ms to feedback and
4,363 ms to workspace; first/repeated CAD edits took 423/320 ms. The document
launch took 574 ms to feedback and 1,697 ms to workspace; CAD edits took
282/339 ms. These are single discovery launches on the immutable executable
with SHA-256 `6bdbc53f89f273e8c7d924cf09f0b633b5218411a3c62a4a591290ecbd865bf3`.
They confirm the diagnostic succeeds; the matched five-run comparisons below
provide the before/after measurements.

Native Windows functional checks also exercised a Simplified Chinese invalid
ZIP launch and the localized Open empty workspace recovery. Recovery retained
the same application process and session, presented a real Bevy frame, and
completed the initial CAD diagnostic in 1,966 ms in that unoptimized build.
Native captures are
[`native-zh-recovery.png`](startup-measurements/captures/native-zh-recovery.png),
[`native-zh-sketch.png`](startup-measurements/captures/native-zh-sketch.png), and
[`native-de-document.png`](startup-measurements/captures/native-de-document.png).
These captures verify native pixels and regional glyphs on Windows; they do
not replace the final timing comparison.

A Spanish invalid-ZIP check repaired only its private fixture copy and then
used Reintentar. The same process and session loaded the representative
document with all four history entries. Native face picking displayed localized
dimensions of 29 × 9 mm, area 261 mm², and perimeter 76 mm. The original fixture
remained unchanged with SHA-256
`d7e65fb222a0bb59f0ac82c3f3143a3437a487f3bbacf3a1f314b989b96de609`.
Captures show the real retry
[`loading phase`](startup-measurements/captures/native-es-loading.png),
[`recovery actions`](startup-measurements/captures/native-es-recovery.png), and
[`face selection`](startup-measurements/captures/native-es-selection.png).
These are functional checks on the unoptimized candidate; they are not final
performance measurements.

Record normal and repeated launches, the representative document, recoverable
failure, supported locales, viewport interaction, and actual packaged native
pixels. Browser screenshots validate the web loading shell only; they do not
validate Bevy output. Results and any unverified criterion are recorded in the
task handoff rather than inferred from successful compilation.
