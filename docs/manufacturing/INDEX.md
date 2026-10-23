# Manufacturing export — index

Manufacturing documentation. This directory currently covers additive export
(STEP / 3MF / STL), materials, and slicer targets. The subtractive 3-axis CAM
documentation starts at [../cam/README.md](../cam/README.md).

[Persistent print intent](print-intent.md) documents shared process settings,
definition identity, schema migration, ownership, and target capability limits.

| Doc | Purpose |
|-----|---------|
| [OKRs.md](OKRs.md) | Objectives & key results for this subsystem |
| [materials.md](materials.md) | BodyAppearance + brand filament catalog |
| [slicer-targets.md](slicer-targets.md) | Standard / Bambu / Orca / Prusa / Cura 3MF |
| [VALIDATION.md](VALIDATION.md) | Automated + manual validation checklist |
| `../../crates/export/fixtures/` | KR3.6 smoke `.3mf` samples |
| [MAINTENANCE.md](MAINTENANCE.md) | Contributor edit order and validation |

## Code map

| Path | Role |
|------|------|
| `crates/core/src/appearance.rs` | `BodyAppearance` / `Rgba8` (persisted in `.limo`) |
| `crates/export/` | Writers + catalog + `ExportFacade` + slicer metadata |
| `crates/export/presets/catalog.json` | **Source of truth** for filament presets |
| `crates/export/src/materials.rs` | Embedded catalog shared by native UI and MCP |
| `desktop/src/session_bridge/native_interface/controller/body_appearance/` | Native material selection and properties |
| `desktop/src/session_bridge/native_interface/controller/files/io.rs` | Desktop export orchestration |
| `mcp-server` | `solid_export_3mf` / `_stl`, `material_catalog`, appearance tools |

## Non-goals (v1)

- 3MF **import**
- Face-level paint / AMS brush painting inside Limo CAD
- Full sliced G-code.3mf project authoring (temps, wipe tower, AMS machine pairing)
- Claiming vendor filament IDs are always current — treat as best-effort hints
