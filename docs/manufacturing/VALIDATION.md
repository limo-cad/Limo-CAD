# Manufacturing export validation

## Automated (required before merge)

```sh
cargo test -p limo-cad-core -p limo-cad-export -p limo-cad-sketch --lib
```

Expect:

- appearance serde defaults + round-trip
- STL header + triangle count
- 3MF `unit="millimeter"` + basematerials name (filament type, color name) and display color
- Standard, Bambu, and Orca packages have Application `Limo CAD` and no `project_settings.config`
- Prusa `Metadata/Slic3r_PE.config`
- Cura `Metadata/cura_materials.json` + basematerials
- catalog JSON parse + Bambu/Prusa/Sunlu/eSun/Anycubic presets (≥40 entries)
- project round-trip scrubbing orphan appearances

For native OCCT/MCP coverage, use the runtime setup and sequential native test
command in [DEVELOPMENT.md](../DEVELOPMENT.md#verify-changes).

## Manual slicer smoke (KR3.6)

Regenerate fixtures: `cargo test -p limo-cad-export --lib tests::regen_manual_smoke_fixtures -- --ignored --exact`

The committed smoke fixtures under `crates/export/fixtures/smoke/` were regenerated
for the portable-model export (no `project_settings.config`; Application
`Limo CAD`). The slicer imports below have not been re-run against them yet;
record what each slicer shows rather than the expected result.

Then open from `crates/export/fixtures/smoke/`:

1. [ ] **`print_in_place_cam_bolt_bambu.3mf`** → **Bambu Studio** (Import / drag onto plate, not Open Project) — four-body cam bolt; record observed filament slots / colours. The file is a model, so Studio keeps the printer you already have selected.
2. [ ] **`print_in_place_cam_bolt_orca.3mf`** → **Orca Slicer** (same drag-onto-plate path) — record observed filament slots / colours.
3. [ ] **`print_in_place_latch_prusa.3mf`** → **PrusaSlicer** — latch with PE metadata.
4. Optional: latch/clip `*_bambu.3mf` / `*_orca.3mf` / `*_cura.3mf`, or simple `cube_*.3mf` colour checks.
5. App path: Extrude box → Bambu PLA Basic Red → Export 3MF; Export STL (appearance warning); Export STEP (no color expectation).

Record date/app versions and observed filament slots when checking off GitHub issue #13.
