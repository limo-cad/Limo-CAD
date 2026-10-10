# Material presets

**Source of truth:** `catalog.json` in this folder.

The single catalog combines existing filament product/color records with imported engineering properties and print profiles. It is embedded by the shared Rust engine and used by the Bevy interface and MCP.

`sources.json` is the pinned import manifest. Use `cargo xtask materials --fetch` to regenerate, review and commit the catalog; use `cargo xtask materials --fetch --check` to verify a fresh fetch without writing. CI checks it before desktop packaging. Builds and runtime remain offline.

See [Materials](../../../docs/manufacturing/materials.md) for source context, units, provenance, persistence, and limitations, and [Third-party notices](../../../THIRD_PARTY_NOTICES.md) for attribution and licenses.
