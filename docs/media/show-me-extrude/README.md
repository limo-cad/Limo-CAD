# Extrude Show me evidence

The PNGs capture production native Bevy widgets and the isolated CAD renderer at 2× UI scale. `demo.gif` sequences the seven real lesson frames at their descriptor timings; it is a component capture, not an OS screen recording. Cropping and GIF downsampling do not change the UI.

Reproduce with a graphics adapter and the configured OCCT SDK:

```powershell
$env:LIMO_CAD_SHOW_ME_PROOF_DIR = '<fresh output directory>'
cargo xtask ci native-ignored-test show-me-ui
cargo xtask ci native-ignored-test extrude-preview
```

Use the registered Cargo build lane where available. The first command saves full-resolution captures and `performance.json`; CI retains them. Performance timings use the debug test profile and cover controller work, not whole-app CPU/GPU utilization. First opening still prepares geometry and renders frames; warm playback reuses textures. The 16 MiB budget covers decoded cached RGBA, not all renderer allocations. Close clears that cache.

`provenance.json` identifies the capture method and hashes the preview sources with LF line endings. These component checks do not replace broad live UI/MCP qualification.
