# Guided examples

Sketch, Extrude, Fillet and Cut operate on the same angle bracket. Cut offers separate Through all and Measured depth outcomes. Long-hover or keyboard-focus Create Sketch, Extrude or Fillet to open a **Guided examples** area with Book icons and explicit example names. Modeling commands keep their existing role.

Opening a guide starts automatic playback. Sketch curves draw progressively, extrusion grows along its authored axis, and selection/depth guides explain the Cut and Fillet actions before revealing their exact authored results. Pause/Resume preserves progress; Next and clickable steps provide manual navigation; Replay restarts. The model remains visible while a replacement texture is pending, with captions and breadcrumbs tied to the displayed geometry.

**Instructions** expands within the guide and pauses playback. Wide layouts use a side column; narrow layouts use a bounded, scrollable lower region. Closing it retains the step and camera. Escape dismisses All steps, then Instructions, then the guide. Help opens the same lesson catalog. There are no duplicate Show me/Back-to-example destinations. English, Simplified Chinese, Spanish and German share the catalog.

## Rendering and reliability

The guide uses the existing CAD worker, device and queue, on an isolated render layer. Built-in CBOR geometry is prepared offline; playback performs no CAD operations. Animation reuses meshes and updates curves/transforms/guides; Cut and Fillet switch to exact endpoint geometry once. Main CAD jobs have priority at queue boundaries. Cold synchronous shader compilation cannot be preempted.

Sketch-bearing steps reuse the document viewport's continuous adaptive grid on the authored sketch plane. It remains visible while drawing and dimensioning, with the same depth fading and bounded line count. This adds line drawing during active renders without creating another grid implementation, GPU device or CAD operation.

There is one pending teaching request, one parked deadline scheduler, and at most two imported textures / 2 MiB in the UI cache. Motion is capped at 20 samples per second; static cues render once and hold. The worker color pool remains bounded at five targets, at most 4.89 MiB at the maximum size, excluding depth/MSAA, driver allocations and shared resources. Paused/closed guides schedule no continuing animation work. This is a bounded design, not a constrained-device performance guarantee.

Shader-error recovery rebuilds contaminated renderer caches once on the existing device, restores live state and queued deltas, and disables teaching for that worker. Genuine main/device failures retain their fatal handling.

## Evidence

`guided-experience.json` records source hashes, capture scope and timings. GIFs sample real native component captures at authored pauses, including intermediate motion. They are not OS pointer recordings. The native suite covers all lessons and both Cut outcomes, four locales at 175%, dimension raster ink, instruction clipping, a full automatic Sketch playback without blank model frames, and bounded texture imports. CPU regressions cover pause/resume, stale replies, deadline ordering and authored geometry. Independent code/performance and visual reviews informed the fixes.

Current review captures come from the successful [native visual job](https://github.com/limo-cad/Limo-CAD/actions/runs/39359657089/job/118422580828) at `2e313e7a4069ab194ec99202916c0c0ab399bfda`. `review-captures-20261020.json` records their exact paths and hashes. The Spanish guide and German instructions both use a 1200 by 760 surface at 175% scale. The instructions fit this capture; scrolling behavior is verified separately by tests. These are native component captures, not recordings of the packaged application.

Existing GIFs and `guided-experience.json` retain their original capture provenance. Broad live UI/MCP qualification remains paused. Published media budget: 8 MiB.

Use the registered Cargo lane, matching OCCT SDK, and a fresh evidence directory:

```powershell
$env:LIMO_CAD_SHOW_ME_PROOF_DIR = '<fresh directory>'
cargo xtask ci native-ignored-test show-me-ui
$env:LIMO_VIEWPORT_RENDER_PROOFS = '<fresh directory>'
cargo xtask ci native-ignored-test show-me-lifecycle
```

The reference proportions remain 32 mm base length, 40 mm width, 44 mm height, 8 mm walls and a 26 mm upright hole. These are inferred CAD dimensions based on the README branding concept. No user document is used by the windowless fixtures.
