# Manufacturing evidence

The existing `solid_export_preflight` reports CAD layout diagnostics and proposed corrections. Its manufacturing summary identifies every included source body and intentional occurrence, hierarchy group, mesh hash, resolved placement hash, material/color, requested and inherited settings, effective values and their sources. Portable targets report process settings as unsupported; a request is not silently converted into a slicer override.

## Four artifacts with different meaning

- A **portable CAD model** carries tessellated geometry, grouping, transforms, materials and colors. Portable 3MF is the default. It does not promise Bambu process settings or a compatible machine profile.
- A **target-specific unsliced project** carries a complete reviewed target profile set and target metadata. The Bambu writer independently reads its output back, but export alone does not prove import or slicing.
- **Geometry-only import fallback** means the slicer loaded mesh geometry without accepting the project configuration. It does not qualify project export. Incomplete Bambu stub profiles are rejected, preserving the behavior fixed in #165.
- **Current sliced G-code** is tied to the exact source geometry, settings, layout, profiles and project artifact used to produce it. Editing those inputs makes prior evidence stale. Project refresh removes stale G-code, caches, thumbnails, estimates and verification metadata and requires reslicing.

## Evidence levels

Keep CAD layout checks, metadata write/readback, installed-slicer import, generated toolpaths and physical fit/load checks separate. Successful slicing does not qualify strength or dimensional accuracy. Six requested walls can produce fewer realized loops in a thin section; the preview states requests and native results without claiming six loops everywhere.

The explicit Bambu project report provides actual written transforms, plate indices, logical filament/support mappings, complete profile source/version, requested overrides, native inherited values and effective setting origins. Native saved-project readback records effective values and changes separately from generated toolpath evidence. Changes to printer/nozzle/process/filament/support mappings or volume quantities fail qualification while retaining any generated toolpath evidence. A changed effective normal-volume wall count, infill density/pattern or shell count also fails qualification; the report preserves the actual native value and toolpaths. Equivalent numeric formatting, such as `15%` and `15.0%`, does not create a mismatch. Missing or malformed native readback fails qualification while preserving generated toolpath hashes. Full normal-volume and modifier world triangles are compared within 0.001 mm, accounting for native mesh recentering; UUIDs, explicit instance identities, quantity, grouping and modifier attachment/settings must also match. Resource renumbering is allowed, but ambiguous or rewritten instance identities require explicit review instead of a name/order/geometry fallback. The local validation report adds executable and artifact hashes, runtime slicer version, exit status, elapsed time, per-plate generated-toolpath hashes and native estimates/material use when available. Missing or clamped values must be reported as unavailable or changed; metadata presence is insufficient.

Native readback additionally records read-only layer heights, widths, shell thickness, support and prime-tower metadata, including its effective process/object/volume source. Changes to those observed normal-volume settings fail handoff qualification while preserving generated toolpaths. Equivalent finite numeric spelling is accepted, with percentages and native vector shapes kept distinct. Native values outside typed process capability produce an explicit unqualified result; read-only inspection remains available. These checks compare metadata intent, not realized layer widths or support roles. Actual managed variable-layer schedules continue to use the separate height reader. Missing native fields are not invented, and unsupported templates are not automatically replaced.

## Opt-in local verification

`bambu_local_verification_start` prepares the reviewed project through the same Rust writer used by preview/export and returns a job ID. It requires the owning document's `expected_model_json` and an explicit absolute local Bambu Studio executable. The caller cannot supply CLI flags or output paths. Use `bambu_local_verification_poll` and `bambu_local_verification_cancel` for that job. Reports are restricted to the private owning engine/tab and source document. The same owning tab can cancel its child after replacing the document; cancellation returns only a job ID and cancellation receipt, without disclosing the previous document report. A separate engine with caller-supplied identical tab IDs cannot read or cancel the job.

The worker creates a fresh temporary directory, writes an input copy, and runs each plate with fixed import/slice/export flags. Each plate has a configurable 1–600 second limit, bounded captured logs and an output size limit. Cancellation kills and reaps only the tool's child. Results for completed plates remain available; remaining plates are marked cancelled. Temporary files are removed after results have been captured. Nothing replaces the user's active slicer project, starts a print, sends a printer command or transmits to a cloud service.

The current adapter qualifies Bambu Studio **02.08.02.61**. Generated G-code must report that version and native slicing must return success for the requested plate before toolpath evidence is accepted. A missing executable leaves verification failed/not run and does not prevent ordinary export. Orca needs a separate qualified adapter.

Current owning-model and resolved CAD-layout hashes are compared when polling. Native source geometry revisions detect recomputation even if model JSON did not change. Accepted owning-model edits also invalidate captured evidence immediately, so restoring the previous model before the next poll does not make it current again. Failed atomic requests leave the evidence unchanged. Model observation serializes only when the private owner has evidence; observation failure marks that evidence stale without changing a successful mutation result. Writing a different target artifact for the same owned document marks prior evidence stale and retains its original hashes and results. The separately recorded written-layout hash identifies the actual target plate assignments and transforms, including reviewed template placement. Geometry, print intent, profile provenance, appearance, joints or saved layout changes invalidate prior evidence conservatively. An external template change must be inspected and applied to the persistent handoff/profile before it can be described as the current project source. Reports retain original evidence and mark it stale rather than relabeling it.

## Validation lanes

Ordinary tests cover serialization, effective-settings reports, writer/readback contracts, stale ownership, missing executables, cancellation, timeouts and failing plates. They do not require a slicer:

```text
cargo test --locked -j1 -p limo-cad-export --lib slicer_verification -- --test-threads=1
cargo test --locked -j1 -p limo-cad-export --lib manufacturing_report
```

The installed-tool lane accepts a writer-produced owned synthetic project and its matching JSON report; it never uses the active user project:

```text
LIMO_BAMBU_VERIFY_PROJECT=<owned synthetic 3mf>
LIMO_BAMBU_VERIFY_REPORT=<matching writer report json>
LIMO_BAMBU_EXECUTABLE=<absolute local Bambu executable>
LIMO_BAMBU_VERIFY_EVIDENCE=<owned evidence json>
cargo test --locked -j1 -p limo-cad-export --lib slicer_verification::tests::installed_bambu_verifies_each_owned_plate -- --ignored --exact
```

The fixture records per-plate native results and hashes. GUI Objects controls and physical prints are separate acceptance tasks and must not be inferred from this CLI lane.

## Recorded local qualification

The application verifier sliced all four plates of the owned five-part fixture in Bambu Studio 02.08.02.61. All child exit statuses were zero; every plate had native success, generated moves, versioned G-code and a toolpath hash. Native saved-project readback retained the reviewed logical printer/nozzle/process/filament/support mappings, selected-plate quantities and effective per-volume settings, and verified every normal volume against its explicit CAD source binding and full world geometry. A separate two-zone project passed the same service gate, including modifier geometry, pose, attachment and settings readback. No per-object setting changes were observed. Native slice estimates and material use are retained in the structured report.

The qualified CLI exports only the selected plate when slicing one plate. Expected readback therefore uses that plate's explicit object-instance assignments, rather than incorrectly expecting all four plates in each output. Windows canonical extended paths are converted to ordinary absolute argument paths for Bambu's parser. Inputs remain owned temporary copies and the worker deletes only its own temporary directory after capturing results.

Managed height metadata is checked through the shared Bambu height reader. The report records written and native ranges, requested speeds and variable-layer samples separately from toolpaths. Native object ordinals may change; stable volume and instance identities resolve the corresponding object. For an object repeated across plates, the report retains the original height source bindings and the selected-plate projection. Each selected instance must still include every normal multipart sibling. A changed or missing managed range or layer schedule fails readback while preserving any generated toolpath evidence. Portable preflight includes each body's bound height requests and explicit target-support results; these requests do not become portable 3MF settings.

The local report retains the shared writer's limitations, including warnings that unmanaged native height metadata was preserved unchanged. Such metadata is outside the managed CAD height comparison. Warning text/counts are bounded; a truncation notice directs the caller to the complete export report.

Installed range/speed and variable-layer fixtures retain two intentional roots of the same multipart object. Both sliced successfully and retain versioned G-code hashes, but Bambu changed the second instance's `identify_id` from `121` to `9` when saving. Their verifier results are **failed/unqualified**, with the original source bindings and actionable review/rebind errors preserved. The verifier does not infer a new binding from geometry or order, remove an intentional repeat, or present these toolpaths as fully qualified. The independently qualified height adapter/toolpath evidence remains distinct from this strict saved-instance identity check.

Deterministic export tests passed 84 tests with eleven explicit installed/manual fixtures ignored. Two native ownership/staleness tests and the equivalent MCP snapshot-restore regression passed separately. The current writer's five-object, four-plate project also passed the installed verifier after the object-controls scope repair and strict effective-setting checks. Bevy live verification controls remain a separate review gate; this CLI result does not claim GUI Objects inspection or physical qualification.

Actual saved-target Z preflight groups normal volumes by native object, instance and plate, retaining their reviewed source bindings and full world bounds. It reuses the existing above/below-bed diagnostic and proposes a Z-only translation of the entire multipart group. Repeated groups remain separate; print-only modifiers are excluded. Explicit template placement identifies corrections as targeting the saved slicer template: CAD offsets do not apply there. Proposals are never applied automatically and do not qualify XY clearance, support geometry or first-layer realization.

Failed native exits retain the bounded selected-plate slice result and its actionable warning text, while remaining failed with no toolpath claim. The negative `bambu-project-live-06` fixture replaced taller template geometry with centered 10 mm boxes, leaving housing halves, auger and adapter above their plates. Bambu rejected plates 1-3 for empty initial layers; plate 4 sliced. The original failed artifact/report are preserved, with independent bounds and native diagnostics in `native-verifier-live06-diagnosis-01`. This failure does not qualify that layout.

The failed-plate reporting regression and actual group Z tests pass on both branches. Re-running the unchanged negative fixture through the service retains all three native empty-layer diagnostics in selected-plate results and failure messages, while plate 4 keeps its real toolpath evidence (`native-verifier-live06-diagnosis-01/service-actionable-failure.json`). The negative run remains failed; it is not counted as successful installed qualification.
