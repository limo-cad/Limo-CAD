# Slicer targets

`MeshExportRequest.slicer_target` selects which extra metadata, if any, rides with the mesh.

| Target | Package contents |
|--------|------------------|
| `standard` (default) | `3D/3dmodel.model`: millimetres, `basematerials` name from filament type and color name, `displaycolor` for viewing. Application is `Limo CAD`. |
| `bambu_studio` | Same package as `standard`. |
| `orca_slicer` | Same package as `standard`. |
| `prusa_slicer` | + `Metadata/Slic3r_PE.config` filament arrays + `Slic3r_PE_model.config` object/volume extruder metadata (required — PS ignores basematerials) |
| `cura` | Consortium `basematerials` + `Metadata/cura_materials.json` hint list |

## What this is / is not

**Is:** a model file you can hand to someone else. Drag it onto the plate in Bambu Studio or Orca and it uses the printer already selected there. The base-material name is chemistry and color (`PETG, Jade White`), not a machine preset.

**Is not:** a sliced Bambu or Orca project. One 3MF cannot hold both of those profiles. Save the sliced project from inside each slicer when you want process, printer, and filament presets. Opening the CAD file with Open Project can still say it is not a Bambu Lab project; that is the model path, and Import / drag onto the plate is the handoff.

## Research notes

Bambu Studio treats a file as its own project when `Application` contains `BambuStudio`, then rejects the project when the embedded profile is not a real Bambu config (`invalid config, load geometry data only`). Orca recognizes the same `BambuStudio` and `OrcaSlicer` tokens. Stamping either name onto a CAD stub is the wrong file to distribute.

PrusaSlicer historically **ignores** consortium `basematerials` (see [prusa3d/PrusaSlicer#4503](https://github.com/prusa3d/PrusaSlicer/issues/4503)). Extruder assignment must come from `Metadata/Slic3r_PE_model.config` object/volume `metadata key="extruder"` entries, plus filament arrays in `Slic3r_PE.config`. Painting uses `slic3rpe:mmu_segmentation` (out of scope for body-level v1).

Cura primarily maps colors from consortium materials; `cura_materials.json` is a hint list for tooling. Other slicers ignore it.

Per-triangle `paint_color` is for painted multi-material — out of scope until face materials exist.
