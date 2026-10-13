# Unified materials

The Bevy body appearance panel selects plastics, metals, and branded filaments from one embedded catalog. Select a body, choose its brand and material preset, then use **Properties** to inspect the merged engineering and printing data. Apply saves that resolved record with the body. Color edits retain its physical properties; changing the material family clears them. Reselecting a preset explicitly adopts the current catalog record.

## Catalog and build

`crates/export/presets/catalog.json` is the single runtime catalog, embedded by Rust with `include_str!`. Existing product/color IDs remain stable. `sources.json` specifies pinned upstream commits, source files, and explicit mappings into those records; it is an import manifest, not another selectable catalog. The Bevy interface and MCP `material_catalog` use the same Rust API.

Refresh and verify with:

```sh
cargo xtask materials --fetch
cargo xtask materials --fetch --check
```

The first command downloads pinned FreeCAD engineering cards and OrcaSlicer/Bambu Studio filament profiles, resolves inheritance, normalizes quantities, and rewrites the catalog. Review and commit the result. The second fetches fresh data and fails if it differs from the embedded catalog. Engine CI, repository tooling CI, and desktop packaging run that check. Ordinary builds and runtime use the committed catalog offline. Omit `--fetch` to regenerate from the source cache in `target/material-sources`.

Properties retain their source, unit, and context. Pressure/modulus quantities use Pa, density uses kg/m^3, and thermal expansion uses 1/K. Slicer profile temperatures use degC. Unknown quantities retain their source units and text instead of guessing a conversion. Engineering reference density and filament density remain separate; the legacy slicer density field prefers a sourced filament value. Print profiles retain their declared printer compatibility; generic defaults are not certified printer-specific settings. Source G-code is excluded.

Generic engineering properties are reference values, not a prediction of the strength of a printed part. Formulation, process, orientation, and sample conditions matter. Generic properties are not assigned to branded products. Unsourced legacy entries remain selectable and carry an explicit data note. Contradictory source strengths are retained and flagged for review, rather than silently corrected. Absence of a sourced print profile is visible for engineering-only materials.

Each source preserves repository, full commit, file path, SHA-256, author, license, and any supplier reference URL. Those facts are available in the Bevy Properties view and the shared catalog API. See [third-party notices](../../THIRD_PARTY_NOTICES.md) for attribution and licenses.

## Saved projects and API

`ProjectModelV9.body_appearances` persists the existing color, labels, vendor/profile identifiers, density, and diameter. Its optional additive `material` field stores the merged `MaterialDetails` snapshot: category, catalog identity, properties, print profiles, sources, and data notes. Old projects without that field remain readable. Catalog updates do not replace saved snapshots. Snapshot validation runs on assignment and project load, before mutating the document; malformed source references are rejected.

MCP `set_body_appearance` accepts `body_id + preset_id` to resolve a current catalog material, or a full appearance including `material` to preserve saved properties even if its preset was removed. Appearance mutations use the existing document ownership, undo/redo, and project persistence path. Existing filament field names remain for API and 3MF compatibility; metals use the same material selection and property model. Diameter is a legacy filament compatibility field and does not describe a metal stock dimension.

3MF continues to use appearance labels and display colors. Engineering values do not constitute a sliced print or configure a slicer automatically. STEP does not invent colors from this store. Filament vendor IDs remain best-effort and never block export when stale.
