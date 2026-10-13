# limo-cad-export

Manufacturing mesh writers + filament catalog + slicer Metadata.

| Path | Role |
|------|------|
| [Manufacturing objectives](../../docs/manufacturing/OKRs.md) | Export, materials and slicer contracts |
| [presets/INDEX.md](presets/INDEX.md) | Catalog folder index |
| `build.rs` | Tracks changes to `presets/catalog.json` |
| `src/lib.rs` | Public API + tests |
| `src/facade.rs` | `ExportFacade` |
| `src/threemf.rs` | 3MF ZIP + slicer Metadata |
| `src/stl.rs` | Binary STL |
| `src/slicer.rs` | `SlicerTarget` |
| `src/materials.rs` | Catalog loader |
| `src/pip_demo.rs` | Print-in-place T-slot latch demo meshes |
| `presets/catalog.json` | Filament presets (source of truth) |
| [fixtures/INDEX.md](fixtures/INDEX.md) | Manual slicer smoke `.3mf` samples |

```sh
cargo test -p limo-cad-export --lib
```
