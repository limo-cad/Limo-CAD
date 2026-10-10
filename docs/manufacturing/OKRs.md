# Manufacturing export OKRs

Horizon: ship reliable local additive export that opens colored in major slicers
without weakening STEP or inventing cloud lock-in.

## Objective 1 — Honest multi-format export

**KR1.1** Same OCCT tessellation feeds STL + 3MF; STEP remains AP242 B-rep only.
**KR1.2** 3MF always declares `unit="millimeter"` and consortium `basematerials` when appearance is included.
**KR1.3** STL export warns that materials/colors are dropped.
**KR1.4** CI unit tests cover STL triangle counts + 3MF XML/Metadata for Standard, Bambu, Prusa, Orca, Cura.

## Objective 2 — Comprehensive materials (CAD-side)

**KR2.1** `BodyAppearance` stores color, filament type, brand, color name, optional vendor id, density, diameter, preset id.
**KR2.2** Built-in catalog covers Generic + Bambu Lab + Prusa + Polymaker + Hatchbox + Overture + Elegoo + Creality + Sunlu + eSun + Anycubic (PLA/PETG/ABS/ASA/TPU/PA/PC families as applicable).
**KR2.3** Catalog JSON is single-sourced (`crates/export/presets/catalog.json`); an explicit ignored regeneration test mirrors it to `src/materials/catalog.json`, and a normal test detects drift.
**KR2.4** UI can assign presets or free-form custom materials; orphans scrubbed on save.

## Objective 3 — Direct slicer brand integration

**KR3.1** Export target enum: `standard` \| `bambu_studio` \| `orca_slicer` \| `prusa_slicer` \| `cura`.
**KR3.2** Bambu/Orca: same portable 3MF as `standard` (no slicer project profile, Application `Limo CAD`).
**KR3.3** PrusaSlicer: embed `Metadata/Slic3r_PE.config` + `Slic3r_PE_model.config` hints.
**KR3.4** Cura: consortium basematerials + `Metadata/cura_materials.json` hints (not a full Cura project).
**KR3.5** Default export target is `standard`. Users can still select Prusa or Cura metadata.
**KR3.6** Manual smoke: colored multi-body 3MF opens with colors in Bambu Studio + PrusaSlicer (documented in VALIDATION.md).

## Objective 4 — Agent-maintainable subsystem

**KR4.1** Folder indexes + OKR pointers at `docs/`, `docs/manufacturing/`, `crates/`, `crates/export/`, `crates/export/presets/`, `src/`, `src/materials/`.
**KR4.2** `MAINTENANCE.md` + Cursor rule describe edit order and sync obligations.
**KR4.3** MCP exposes `material_catalog` + export tools with `slicer_target`.

## Out of scope this OKR cycle

Live AMS sync, MakerWorld upload, proprietary Bambu machine auth, resin printer MSLA stacks.
