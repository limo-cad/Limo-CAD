The native entry now starts with the outcome (for example, "Round the corners"). Lessons wait for Next: one instruction, one primary action and a larger, quiet model stage. A clickable breadcrumb trail shows the previous, current and next named actions (the first/last three at the ends). All steps expands the complete numbered sequence in the same card; selecting one returns to its model frame. Expanding pauses playback without invalidating cached pixels. The second Cut outcome appears only after completion. Replay is optional; dragging and keyboard camera controls remain available through the model control. Help remains a secondary action beside the instruction.

Review update: the Sketch Plane step now shows a bounded outline and local-origin axes instead of an empty stage. `sketch.gif`, `sketch.png` and `sketch-plane.png` have fresh focused captures at source `f22848af`; `review-plane.json` records their separate provenance. Strict desktop Clippy, three targeted CPU tests and the native Sketch capture passed. Other media and the original `provenance.json` retain their prior capture identity. The PR remains draft: the Sketch Dimension step still needs native dimension leaders and values.

Four native Show me lessons share one editable angle bracket: Sketch, Extrude, Fillet and Cut. Fillet selects four outer corner edges and applies a 3 mm radius; the inner bend is supporting setup. Cut starts with the corners, bend, both base mounting holes and its upright circle already prepared. Its two choices share the first two steps: Through all opens both faces; Measured depth cuts 4 mm into the 8 mm wall and leaves a 4 mm floor. These are short alternative outcomes within one operation.

The proportions and orientation follow the README's AI-generated [branding concept](../../assets/branding/limo-proposal-concept-en.png): a broad upright, shorter base and large central opening. Inferred dimensions are 32 mm base length, 40 mm width, 44 mm height, 8 mm walls and a 26 mm upright hole centered 20 mm across and 27 mm above the origin. Two 7 mm base holes sit at X=24 mm, Y=8/32 mm. The bend radius is 4 mm, the four outer corner radii are 3 mm, and the side perimeter edge breaks are 0.6 mm. Preview yaw is -40 degrees and elevation 32 degrees, so the base projects toward the lower left. These are deliberate CAD dimensions inferred from the concept, not measurements of an exact source model.

The isolated preview uses opt-in satin aluminum shading (metallic 0.55, perceptual roughness 0.34). The full editable script assigns the aluminum 6061-T6 catalog appearance. Only the lesson palette quiets the ground grid; the main viewport and other preview users retain their existing palette.

The five GIFs sequence real production Bevy component captures of the user-paced steps at the authored replay timings: Sketch 5, Extrude 5, Fillet 4, through Cut 4 and measured Cut 4 frames. The complete-build preview stays within its original 16-frame bound. PNGs use the native CAD renderer, dark/light themes and supported 1200 x 760 / 175% layout. These are windowless component captures, not OS recordings or broad live UI/MCP qualification. The hint anchor is a native fixture; normal traversal for Extrude and Fillet is separately tested against the production ribbon.

Access: hover Create Sketch, Extrude or Fillet for 700 ms; keyboard focus reveals the hint immediately. Extrude offers both Extrude and Cut. The top-bar Help index uses the same teaching catalog, with written instructions in English, Simplified Chinese, Spanish and German. Help explains both Cut choices; returning to the demonstration preserves the selected choice. Full build opens the bundled `extrude-basics` recipe in Scripts using its existing save/run safeguards.

The outlined **All steps** button separates navigation from the step count and exposes its expanded state to accessibility tools. The chooser uses action labels, fixed 32 px targets and bounded pages for longer lessons. Playback controls disappear while choosing. Escape returns to the model with focus on All steps; a second Escape closes the lesson.

Performance: hover and read-only Help do no teaching CAD/GPU work. Built-in lessons load versioned CBOR geometry generated offline from the editable recipe; source-hash and geometry-budget checks reject stale or invalid assets. Edited scripts still use the existing isolated recipe execution path.

Teaching uses the existing CAD render worker, device, queue and pipelines. Its geometry, camera, lights, materials and gizmos occupy a separate render layer. The live document, selection, camera and undo state remain authoritative and untouched. Main CAD jobs take priority at scheduling boundaries; a synchronous shader compilation already in progress is not preemptible.

Only GPU texture handles cross to the UI: there is no production screenshot readback, PNG encoding/decoding, second upload, or per-frame waiting thread. The worker parks on its queue when idle. Closing cancels pending publication and clears UI texture imports. Worker failure closes the queue promptly; it does not silently create a second renderer.

The UI retains at most two frames / 2 MiB of color textures. Five separately leased teaching targets avoid blocking the main viewport's own target pool (at most 4.89 MiB of RGBA color storage at 640 x 400). These limits exclude depth/MSAA attachments, shared pipelines, retained meshes, driver allocations and allocator overhead. The authored CPU cache retains at most four entries / 16 MiB of serialized payload including source. All four embedded assets total 1,649,263 bytes. Scene assets are allocated on demand; no extra renderer is preallocated at startup.

`performance.json` records actual controller opening/reopening, whole-process memory, GPU job counts, and paused/closed polling in the debug test profile. Adjacent cached navigation adds no render jobs; replaying a sequence longer than the two-frame cache renders evicted frames again. `warm_gpu_jobs` covers three full replays; `warm_replay_ms` is their per-replay average. Memory excludes dedicated GPU allocations and is not a device-memory benchmark. Windows virtual-memory observations are private bytes. These are single-run observations, not release or constrained-device performance guarantees.

Reproduce with a graphics adapter, configured OCCT SDK and the registered Cargo build lane:

```powershell
$env:LIMO_CAD_SHOW_ME_PROOF_DIR = '<fresh UI evidence directory>'
cargo xtask ci native-ignored-test show-me-ui
$env:LIMO_VIEWPORT_RENDER_PROOFS = '<fresh isolation evidence directory>'
cargo xtask ci native-ignored-test show-me-lifecycle
cargo test --locked --manifest-path desktop/Cargo.toml --lib native_script_preview_bundled_assets
```

The UI fixture uses the real shared worker and imports its GPU textures. It captures all four operations and both Cut choices, dark/light themes and four locales at 175%, including clipping and missing-glyph checks. The separate worker test interleaves a distinct live solid with teaching, forces a fresh main render, checks exact pixel preservation, exhausts teaching leases while main orbit still works, and shuts down with leases retained.

GIFs assemble cropped native component captures at authored timings; they are not pointer recordings. `step-navigation.gif` illustrates the final Fillet frame, the expanded chooser and the selected-edge frame. `provenance.json` records source and executable hashes. Published media budget: 8 MiB. The capture checkout retains concurrent PR436 sketch parameter patches, recorded separately and excluded from this PR.

This Windows test build disables debug information only for the final desktop test crate (`-C debuginfo=0 -C link-arg=/DEBUG:NONE`) to fit the existing build disk. Rust debug assertions and optimization are unchanged. CI uses its normal build settings. Broad live UI/MCP qualification remains paused; no user document was used by these windowless fixtures.

Current observations: controller texture-ready first opens were 59–1,151 ms and same-lesson reopens 38–169 ms. Cold solid shader compilation remains visible. All 51 targeted CPU tests, four native GPU regressions, the native UI capture suite and 241 CI-tooling tests passed; two unrelated CI-tooling tests remain ignored. Strict desktop Clippy passed. See `renderer-lifecycle.json` for exact scope.
