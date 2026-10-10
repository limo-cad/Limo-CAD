# Embedded printer geometry

The printer catalog is shared Rust data, alongside the existing unified material
catalog. It describes usable printer geometry; materials remain in the one
material catalog.

The default profile is Bambu X2D, using pinned Bambu Studio and OrcaSlicer
sources. Its main-nozzle envelope is 256 x 256 x 261 mm. The dual-nozzle shared
region starts at X = 20.5 mm, extends to X = 256 mm, and has height 256 mm.
These are slicer profile values. Saved named views retain their resolved printer
geometry and provenance until a user explicitly selects a profile again.

## Fetch and embed

`crates/core/data/printer-sources.json` pins each approved upstream repository,
full commit revision, and machine profile. Run:

```text
cargo xtask printer-profiles --fetch --check
```

The task downloads the pinned profile and its inheritance chain, validates convex
printable regions, nozzle mapping, dimensions, and exclusions, and compares the
result against `crates/core/data/printers.json`. CI runs this check before native
and browser builds. A fetch or normalization failure fails the check.

Rust embeds the committed catalog through `include_str!`. Local application
builds need no network. Without `--fetch`, the task uses its source cache at
`target/printer-profiles`.

To update profiles, change the pinned revisions, run
`cargo xtask printer-profiles --fetch`, review both source manifest and generated
catalog, then commit them together. Provenance records SHA-256 hashes for every
inherited source file. Only geometry facts and provenance are embedded; printer
G-code, process presets, and toolpaths are not included.

Sources: [Bambu Studio](https://github.com/bambulab/BambuStudio) and
[OrcaSlicer](https://github.com/OrcaSlicer/OrcaSlicer). Exact revisions and source
paths are recorded in the manifest and every sourced bed.
