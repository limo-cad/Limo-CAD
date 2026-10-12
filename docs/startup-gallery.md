# Limo CAD startup and loading visuals

The initial application launch uses a **560 × 380** floating splash card. It shows the selected regional name, an engineering drawing, the real startup phase, and one stationary workshop note. It dismisses when the workspace is ready. Work in the ready application uses top-bar **Activity**.

## Startup splash recording

A native Windows launch on Thunder, captured as sampled window-only frames:

![Native startup splash recording](startup-measurements/captures/floating/native-loading.gif)

[One-third-speed playback](startup-measurements/captures/floating/native-loading-slow.gif) makes short phases easier to read. The recording places the small card on a neutral canvas, scales the workspace to fit, and holds the workspace for one second before looping. The [capture receipt](startup-measurements/captures/floating/native-loading-recording.json) records the executable, sample intervals, and playback delays. Performance measurements use separate unrecorded runs.

## English startup views

These five states use the actual source component with simulated native IPC to select the phase. They check layout and copy.

Preparing the first view with the workshop note:

![English splash composition](startup-measurements/captures/floating/en-floating-composition.png)

Preparing graphics:

![Graphics preparation](startup-measurements/captures/floating/en-graphics.png)

Application preparation and file geometry work:

![Application and file phases](startup-measurements/captures/floating/en-file.png)

Light appearance:

![Light splash](startup-measurements/captures/floating/en-light.png)

Recovery with readable details and actions:

![Recovery](startup-measurements/captures/floating/en-recovery.png)

## Four-language companion

The graphics-preparation view shares text-free artwork and layout. Names, statuses, notes, and actions use one selected language.

English — **Limo CAD**:

![English](startup-measurements/captures/floating/en-graphics.png)

Simplified Chinese — **砺模 CAD**:

![Simplified Chinese](startup-measurements/captures/floating/zh-CN-graphics.png)

Spanish — **Limo CAD**:

![Spanish](startup-measurements/captures/floating/es-graphics.png)

German — **Limo CAD**:

![German](startup-measurements/captures/floating/de-graphics.png)

## Native startup and recovery

The frameless startup card:

![Native startup splash](startup-measurements/captures/floating/native-standard-drag-loading.jpg)

A recoverable invalid project:

![Native recovery](startup-measurements/captures/floating/native-standard-drag-recovery.jpg)

Retry restores the sample in the same process, with four history entries and normal workspace controls:

![Recovered native workspace](startup-measurements/captures/floating/native-standard-drag-workspace.jpg)

## Loading feedback in the workspace

Top-bar Activity reports a completed file open over the rendered CAD document:

![Native Activity](startup-measurements/captures/runtime/native-activity-keyboard-final.jpg)

Expandable runtime error details preserve the loaded document:

![Native runtime error](startup-measurements/captures/runtime/native-runtime-failure.jpg)

The [four localized Activity views](runtime-activity.md#visuals) use the actual component with simulated operations for copy/layout checks. Native screenshots show the Windows application; simulated component screenshots do not establish native rendering or timings.

[Startup optimizations, measurements, and verification](startup-performance.md) · [Activity behavior](runtime-activity.md)
