Four native Show me lessons share one editable angle bracket: Sketch, Extrude, Fillet and Cut. Fillet selects four outer corner edges and applies a 3 mm radius; the inner bend is supporting setup. Cut starts with the corners, bend, both base mounting holes and its upright circle already prepared. Its two choices share the first two steps: Through all opens both faces; Measured depth cuts 4 mm into the 8 mm wall and leaves a 4 mm floor. These are short alternative outcomes within one operation.

The proportions and orientation follow the README's AI-generated [branding concept](../../assets/branding/limo-proposal-concept-en.png): a broad upright, shorter base and large central opening. Inferred dimensions are 32 mm base length, 40 mm width, 44 mm height, 8 mm walls and a 26 mm upright hole centered 20 mm across and 27 mm above the origin. Two 7 mm base holes sit at X=24 mm, Y=8/32 mm. The bend radius is 4 mm, the four outer corner radii are 3 mm, and the side perimeter edge breaks are 0.6 mm. Preview yaw is -40 degrees and elevation 32 degrees, so the base projects toward the lower left. These are deliberate CAD dimensions inferred from the concept, not measurements of an exact source model.

The isolated preview uses opt-in satin aluminum shading (metallic 0.55, perceptual roughness 0.34). The full editable script assigns the aluminum 6061-T6 catalog appearance. The main viewport renderer is unchanged.

The five GIFs sequence real production Bevy component captures at their authored timings: Sketch 5, Extrude 5, Fillet 4, through Cut 4 and measured Cut 4 frames. The complete-build preview stays within its original 16-frame bound. PNGs use the native CAD renderer, dark/light themes and supported 1200 x 760 / 175% layout. These are windowless component captures, not OS recordings or broad live UI/MCP qualification. The hint anchor is a native fixture; normal traversal for Extrude and Fillet is separately tested against the production ribbon.

Access: hover Create Sketch, Extrude or Fillet for 700 ms; keyboard focus reveals the hint immediately. Extrude offers both Extrude and Cut. The top-bar Help index uses the same teaching catalog, with written instructions in English, Simplified Chinese, Spanish and German. Help explains both Cut choices; returning to the demonstration preserves the selected choice. Full build opens the bundled `extrude-basics` recipe in Scripts using its existing save/run safeguards.

Performance: no CAD or preview GPU work starts on hover or read-only Help. First opening prepares isolated geometry and renders on demand; warm replay reuses textures. Switching Cut choices reuses the retained model, with six distinct cached frames after both choices are warmed. A 100-round switch/replay check submits zero additional render jobs. `performance.json` records debug controller timings, not whole-app utilization; `cached_rgba_bytes` describes the initially captured choice, and `scenario_cached_frames` describes the warmed pair. The 16 MiB cap covers decoded cached RGBA, not every renderer allocation. Close clears the image cache.

Reproduce with a graphics adapter, the configured OCCT SDK and the registered Cargo build lane:

```powershell
$env:LIMO_CAD_SHOW_ME_PROOF_DIR = '<fresh directory>'
cargo xtask ci native-ignored-test show-me-ui
$env:LIMO_CAD_PREVIEW_PROOF_DIR = '<fresh geometry directory>'
cargo xtask ci native-ignored-test extrude-preview
```

The first command captures all four operations and both Cut choices in dark/light English views and four localized light views, including high-scale Help clipping, scroll-to-end and missing-glyph checks. The geometry check compares selected Fillet edges against an unselected control, blind and through outcomes, the prepared circle against the same solid without a profile, and aluminum against default matte shading. CI retains full-resolution captures. Committed images are cropped; GIFs are downsampled to 504 x 570 using a common palette per clip. `provenance.json` records LF-normalized source hashes. Media budget: 8 MiB.

This Windows capture used `cargo rustc --locked --manifest-path desktop/Cargo.toml --lib --profile test -- -C link-arg=/DEBUG:NONE` to avoid writing the large optional MSVC debug database on the constrained build disk. The compiled test executable ran 22 preview/ribbon tests and both ignored GPU suites directly, after exact 22/1/1 inventory checks. Rust debug assertions and optimization are unchanged. The standard CI commands above continue to use their normal build settings.
