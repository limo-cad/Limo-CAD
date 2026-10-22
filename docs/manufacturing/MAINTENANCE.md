# Maintaining manufacturing export

## Edit order

1. **Schema** — `crates/core/src/appearance.rs` (additive serde defaults only; no schema bump).
2. **Writers / catalog** — `crates/export/` (`lib`, `threemf`, `stl`, `slicer`, `materials`, `facade`).
3. **Catalog** — edit `crates/export/presets/catalog.json`; `crates/export/src/materials.rs` embeds it for all hosts. Run the export material tests after changing presets or their schema.
4. **Hosts** — `crates/sketch` persistence, `crates/occt` tessellate, `desktop`, `mcp-server`.
5. **UI** — the native body-appearance controller under `desktop/src/session_bridge/native_interface/controller/body_appearance/`, project file handling and localized labels.
6. **Docs** — update this folder’s INDEX/OKRs/VALIDATION when behavior changes.

## Invariants

- UI and MCP must call `ExportFacade` / shared `MeshExportRequest` — no third exporter.
- STEP must not invent colors.
- Orphan `body_appearances` scrubbed on save/commit.

## Tests to run

See [VALIDATION.md](VALIDATION.md).
