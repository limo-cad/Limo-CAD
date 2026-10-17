Three native Show me examples share one editable angle bracket: Sketch, Extrude and Cut. The Cut starts with rounded corners, the bend, both base mounting holes and its upright circle already prepared; it links back to the earlier operations.

The proportions and orientation follow the README's AI-generated [branding concept](../../assets/branding/limo-proposal-concept-en.png): a broad upright, shorter base and large central opening. Inferred dimensions are 32 mm base length, 40 mm width, 44 mm height, 5 mm walls and a 26 mm upright hole centered 20 mm across and 27 mm above the origin. Two 7 mm base holes sit at X=24 mm, Y=8/32 mm. The bend radius is 4 mm, the four outer corner radii are 3 mm, and the side perimeter edge breaks are 0.6 mm. Preview yaw is -40 degrees and elevation 32 degrees, so the base projects toward the lower left. These are deliberate CAD dimensions inferred from the concept, not measurements of an exact source model.

The isolated preview uses opt-in satin aluminum shading (metallic 0.55, perceptual roughness 0.34). The full editable script assigns the aluminum 6061-T6 catalog appearance. The main viewport renderer is unchanged.

The GIFs sequence real production Bevy component captures at their authored timings (5/5/4 frames). PNGs use the native CAD renderer, dark/light themes and supported 1200 x 760 / 175% layout. These are windowless component captures, not OS recordings or broad live UI/MCP qualification. The hint anchor is a native fixture; normal traversal is separately tested against the production ribbon.

Access: hover Create Sketch or Extrude for 700 ms; keyboard focus reveals the hint immediately. Extrude offers both Extrude and Cut. The top-bar Help index uses the same teaching catalog, with written instructions in English, Simplified Chinese, Spanish and German. Full build opens the bundled `extrude-basics` recipe in Scripts using its existing save/run safeguards.

Performance: no CAD or preview GPU work starts on hover or read-only Help. First opening prepares isolated geometry and renders on demand; warm replay reuses textures. `performance.json` records debug controller timings, not whole-app utilization. The 16 MiB cap covers decoded cached RGBA, not every renderer allocation. Close clears the image cache.

Reproduce with a graphics adapter, the configured OCCT SDK and the registered Cargo build lane:

```powershell
$env:LIMO_CAD_SHOW_ME_PROOF_DIR = '<fresh directory>'
cargo xtask ci native-ignored-test show-me-ui
$env:LIMO_CAD_PREVIEW_PROOF_DIR = '<fresh geometry directory>'
cargo xtask ci native-ignored-test extrude-preview
```

The first command captures all three operations in dark/light English views and four localized light views, including high-scale Help clipping, scroll-to-end and missing-glyph checks. The geometry check also compares the prepared circle with the same solid without a profile, requiring a visible accent outline, and compares the aluminum finish against the default matte control. CI retains full-resolution captures. Committed images are cropped; GIFs are downsampled to 504 x 570 using a common palette per clip. `provenance.json` records LF-normalized source hashes. Media budget: 8 MiB.
