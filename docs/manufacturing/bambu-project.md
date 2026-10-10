# Bambu project handoff

The portable 3MF scene remains the geometry and placement source. The Bambu adapter refreshes a complete saved Bambu Studio **02.08.02.61** project with explicit CAD definition/occurrence bindings. It preserves existing object grouping, intentional repeats, printer/process/filament configuration, volume UUIDs, plate assignments, and unrelated settings. It never creates a guessed machine or filament profile.

## Choose and review a source

Inspect a saved project before handoff. Its summary identifies the source SHA-256, printer/nozzles, process defaults, filament chemistry/colors, support mapping, plates, objects and normal volumes. Bind each CAD occurrence explicitly to the intended object/instance/volume; display names are labels, not identity. Every selected occurrence and target normal-volume instance must be bound exactly once. CAD hierarchy groups must match the target grouping. A mismatch requires another template or an explicitly reviewed restructuring of that template.

Read-only inspection retains the native infill option and reports a capability warning when the option cannot be represented by typed CAD print intent. Such a value remains in `native_process_settings`; the typed pattern is absent rather than substituted. This permits inspecting saved projects without pretending their process is editable. Applying the template as a resolved CAD process or managed export still rejects unsupported patterns explicitly. Bambu Studio 2.8.2.61's [upstream process enum](https://github.com/bambulab/BambuStudio/blob/v02.08.02.61/src/libslic3r/PrintConfig.cpp) spells its Rectilinear option `zig-zag`. A raw `rectilinear` value is retained with a warning, not silently equated to that enum or certified as a native-compatible option.

The read-only report also retains layer and first-layer heights, line widths, top/bottom shell thickness, support options and prime-tower enablement when present. Missing fields stay absent; native scalar/vector spelling and percentage widths are preserved. Object and volume reports resolve these fields through process/object/volume scopes and show the source. They remain metadata intent: a requested width is not an observed extrusion width, and a support flag is not evidence of emitted supports. Existing managed wall/infill/shell fields continue to distinguish CAD overrides from their inherited baseline. Inspection validates the adapter's package/profile contract; native import and toolpath generation remain separate acceptance evidence.

This read-only view has a fixed 23-key inventory, scalar strings up to 256 bytes, at most 128 scalar vector elements, and at most 4 KiB of selected process values. Oversized or complex values return a report-capacity error before per-part cloning; the saved project is unchanged. Unrelated complete-profile fields are preserved outside this bounded view.

`resolved_scene` placement uses the application's existing resolved assembly/named-view transforms. It currently requires a single-plate source because automatically assigning a chosen layout to several native plates is not qualified. Explicit `template` placement retains saved plate positions and centered volume transforms. Clear a selected named view when deliberately choosing template placement; the engine rejects conflicting placement instructions.

The adapter checks CAD appearance against each explicitly mapped template filament slot. A mismatch requires correcting the mapping or deliberately accepting the template's chemistry/colors. Bodies without authored CAD appearance also require that explicit review; acceptance retains the validated native filament/color mapping and reports the absence of CAD appearance without inventing one. The report preserves the chosen mapping and states the actual exported world transform and plate for every occurrence.

## Settings and precedence

The qualified keys are wall count, infill density/pattern, and top/bottom shell layers. Requested project defaults override the selected complete process's defaults; native object and inherited volume overrides remain effective, and an explicit CAD part override applies last. The report lists inherited values, written part overrides, effective values and each value's origin. A selected process snapshot must match its actual sourced template defaults. Put deliberate differences in project/part intent.

Rectilinear maps to Bambu's `zig-zag`. Incompatible 100% infill patterns are rejected; the adapter does not silently replace one. Unsupported scoped controls are not implied by arbitrary metadata keys. The local-modifier extension supports managed box/cylinder print-only zones; see [Local print modifiers](print-modifiers.md). Unmanaged print-only volumes and unsupported height-edit entries require explicit review or their coordinated adapter.

## Refresh after a slicer save

Keep `report.refresh_reference` with the CAD handoff. It records the document namespace, original template/profile lineage, target normal-volume UUID, per-instance `identify_id`, and bounded original/written values for the five supported settings. Supply that reference when refreshing a project saved by the slicer. Bambu removes custom CAD metadata and can renumber resource IDs; matching therefore uses UUID plus instance identity, never a name or guessed instance order.

Unexpected native changes to managed settings block refresh until explicitly reviewed. Accepting those changes adopts only changed inherited fields, then applies current CAD overrides. Later clearing a CAD override restores the reviewed baseline. Changes to complete profile hashes are reported; preserving hardware/process/material identifiers does not prove that every changed native setting remains qualified.

Bambu Studio 2.8.2.61's plate map preserves only one loaded instance identity when the same object has several instances on one plate. Native save retains their quantity and grouping, but can replace later `identify_id` values. Missing or ambiguous identities fail with source and native candidate IDs. Inspect and explicitly rebind such instances. Clearing an old reference and choosing exact new bindings adopts a new inherited baseline; it cannot recover the previous override-reset lineage automatically. No automatic fallback is used. A complete explicit binding set can deliberately reuse a foreign CAD-authored project as a fresh template; the report records a new reviewed lineage and retains its native settings as the baseline. An explicit foreign refresh reference remains rejected.

## Output and evidence

The writer replaces validated welded meshes, removes stale external-file reload matrices and source-offset provenance, clears old toolpaths, thumbnails, estimates and slice caches, and repairs package relationships/content types. Actual placement remains in 3MF components/build items; unrelated validated process settings remain intact. It independently parses the resulting ZIP/XML and checks mesh coordinates/topology, transforms, target UUIDs and effective settings before returning bytes. The report identifies source/output hashes, actual placements/materials, setting origins, invalidated entries and limitations.

`metadata_readback_verified` is distinct from installed-slicer import and generated toolpaths. Export alone leaves the latter fields false. Successful local slicing provides separate version, exit status, input/output hashes, per-plate results and G-code evidence. Neither a metadata check nor a generated toolpath demonstrates physical strength or dimensional accuracy.

## Reproducing qualification

Deterministic export contracts require no installed slicer:

```text
cargo test --locked -j1 -p limo-cad-export --lib bambu_project::tests
```

Opt-in tests take an operator-provided complete saved template through `LIMO_BAMBU_TEMPLATE` and write synthetic fixtures into a fresh absolute directory selected by `LIMO_BAMBU_QUALIFICATION_DIR`. They do not commit, modify or copy private model geometry. The five-object/four-plate fixture deliberately recognizes the reference acceptance names to assign test settings; production binding never uses names.

```text
cargo test --locked -j1 -p limo-cad-export --lib write_five_part_four_plate_native_qualification_fixtures -- --ignored --nocapture
cargo test --locked -j1 -p limo-cad-export --lib write_resolved_repeated_and_thin_native_qualification_fixtures -- --ignored --nocapture
```

Slice each generated project with the qualified local binary and an owned output directory:

```text
bambu-studio.exe --arrange 0 --slice 0 --outputdir <owned-output> --export-3mf <owned-saved-name.3mf> <synthetic-input.3mf>
```

The Windows launcher can reopen stdout/stderr to `CONOUT$`; empty redirected logs do not establish success. Require exit 0, native `result.json`, all requested plate G-code files and the runtime version in their headers. After saving the resolved repeat/thin fixtures to the documented `<case>-validated/<case>-sliced.3mf` paths, run the independent native geometry/group/material readback test.

Fresh qualification on 2026-10-04 passed baseline/configured five-object/four-plate slices and a subsequent native-save → explicit-reference refresh → four-plate re-slice. Separate resolved-scene qualification preserved two rotated multipart instances, each with two normal volumes, within 0.001 mm in independent native saved-project readback. Thick rectangular sections showed six wall loops at Z=5 mm versus two in the paired baseline. A 1.2 mm section still requested six walls but produced an outer perimeter and gap infill, without inner-wall paths at that layer. The unmodified sleeve's parsed linear-toolpath fingerprint was unchanged. These checks establish local import/toolpath behavior for these fixtures, not a strength claim or GUI Objects-panel inspection.

For an object with exactly one normal volume, explicitly requested CAD part keys are written at native object scope so Bambu Objects controls show the requested values. Those keys are removed from the normal volume to prevent hidden overrides from masking later native object edits. Original object and volume baselines are tracked and restored when CAD requests are reset; changed native object settings require review before refresh. Multipart requests remain volume settings, and the preview reports this native scope distinction without splitting CAD groups.
