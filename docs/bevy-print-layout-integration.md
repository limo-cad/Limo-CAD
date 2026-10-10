# Bevy named views and print layouts

The Bevy desktop now uses one named-view editor for presentation and printing.
Assembly 3MF exports retain the CAD component/occurrence hierarchy, alignment,
and every intentional repeated instance. This integrates the shared work from
[main PR #257](https://github.com/limo-cad/Limo-CAD/pull/257) into the native
host and Bevy controls. The feature targets `feat/bevy-interface` because that
interface replaces the legacy desktop.

## Use a layout

1. Open **Named Views** in the CAD browser and choose **Create named view**.
   Capture the current camera and visibility, name the view, and choose its
   purpose. Presentation and print views share the same placement model.
2. Select an existing CAD occurrence or body. Enter translations in document
   units and rotations in degrees. An occurrence offset moves its descendants
   together; body offsets apply to its repeated instances. Preview the draft
   without changing mechanical geometry or joints.
3. Choose the printer bed and **Check print layout**. Diagnostics identify
   affected occurrences and report exclusions, conservative overlaps, and bed
   violations. **Apply proposed group corrections to draft** preserves multipart
   alignment and quantity. Review the result, then save and recall the view.
4. Export 3MF using the current display, assembled placement, or any saved view.
   Presentation views can export too. Review export diagnostics and explicitly
   choose **Export despite layout issues** when the reported issues are intended.
   Save and recall a draft preview, or reset it, before exporting.

Print designation records the view's purpose without creating another hierarchy
or restricting export. Explicit checks are available in the editor and 3MF export
checks the selected placement again before writing. Selected-body assembly export
includes every occurrence of that body. Definition scope exports source bodies
once in their original coordinates and does not apply a layout.

Modeling tools use assembled geometry. Opening a source feature resets a recalled
view; pending layout previews and open source tools must be finished or canceled
before changing that context. Stale document/viewport captures and asynchronous
results are rejected. Saved views survive native tab retention and project
save/reopen; loading a project starts with assembled placement.

## Printer and material data

Bambu X2D is the default, with pinned Bambu Studio and OrcaSlicer profiles.
The main-nozzle profile envelope is 256 x 256 x 261 mm; the dual-nozzle shared
region is X = 20.5..256 mm, Y = 0..256 mm, Z = 0..256 mm. These are upstream
profile values, not a claim about physical printer calibration. Saved views
freeze resolved bed geometry and source revisions/hashes until a profile is
selected again. See [printer profiles](manufacturing/PRINTER_PROFILES.md).

Build CI runs `cargo xtask printer-profiles --fetch --check` alongside the existing
material fetch/check. Applications embed committed catalogs and work offline.
Engineering and filament properties remain in the existing unified material
surface; printer geometry does not introduce a second material catalog.

## Qualification and limits

The October 4 local qualification exercised the actual Windows Bevy controls:
capture, draft preview, diagnostics, corrections, save/recall, deliberate export,
presentation export, rename/delete/reset, and project save/reopen. An independent
3MF reader checked repeated quantity, multipart grouping, and world coordinates.
Native-host tests cover nested rotations, selected repeats, cold-tab restoration,
failed recall, source-edit reset, and Definition/Prusa adapters. Bambu Studio
**2.8.2.61** and OrcaSlicer **2.4.1** imported/re-exported the native-host fixture:
**3 instances in 2 groups**, preserving world vertices within **0.001 mm**.
Repeat with `scripts/test-3mf-slicers.ps1`; the owned live desktop scenario is
`cargo xtask test-mcp native-print-layout --server <limo-cad-binary> --session <owned-blank-session> --out <evidence>`. This scenario requires an isolated `LIMO_CAD_CONFIG_DIR` beside the evidence directory and a blank document. It drives the retained controls; OS file choosers and physical keyboard input are not qualified by this scenario.

Overlap checks use conservative bounds. Proposed corrections translate whole
groups and do not rotate parts or repair internal multipart intersections. Prusa's
existing metadata adapter remains flat. Physical printing, per-face painting,
qualified filament slots, printer/process presets and toolpaths remain outside
this portable-model milestone. Hosted Linux/macOS/package qualification and
independent PR approval remain separate merge/release gates.
