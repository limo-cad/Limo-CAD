# Materials model

## Persistence

`ProjectModelV4.body_appearances: Vec<BodyAppearance>` (additive, `#[serde(default)]`).

Fields:

| Field | Meaning |
|-------|---------|
| `body_id` | Stable solid body |
| `color` | RGBA8 viewport + 3MF displaycolor (alpha flattened opaque in 3MF) |
| `material_name` | Label. Prusa writes it as `filament_settings_id`. It is not the 3MF base-material name. |
| `filament_type` | PLA, PETG, ABS. This is the 3MF base-material name. |
| `brand` | Generic, Bambu Lab, Prusa, … |
| `color_name` | Appended to the 3MF base-material name. Display color stays on `color`. |
| `filament_id` | Optional vendor profile id |
| `preset_id` | Catalog key when chosen from presets |
| `density_g_cm3` | Optional |
| `diameter_mm` | Default 1.75 |

STEP never invents colors from this store.

## Catalog

Edit **`crates/export/presets/catalog.json`** as the source of truth.

Rust loads via `include_str!`. After changing the catalog, explicitly regenerate the Vite mirror with `cargo test -p nbcad-export regen_frontend_catalog_mirror -- --ignored --exact`; the normal test suite fails if the mirror drifts. Brands today: Generic, Bambu Lab, Prusa, Polymaker, Hatchbox, Overture, Elegoo, Creality, Sunlu, eSun, Anycubic.

Filament IDs are **best-effort** — vendors change SKUs; never block export if an id is stale.
