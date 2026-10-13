# Native command scripts

Scripts build editable parts and assemblies, explain their construction and
present the result. Recipes are the bundled examples of those scripts, using the
same source editor, interpreter and runner. Start with
[your first part](INSTALL.md#make-your-first-part) or choose from the
[script examples](../examples/scripts/README.md).

A `.limo.jsonc` file is the reproducible construction source for a native design.
A `.limo` file is the editable project produced by those commands. Keep both when
publishing an example: one explains how it was made, the other opens directly for
further parametric work.

Select a part occurrence before re-entering its sketch to edit the shared
definition in place. Other occurrences fade while the selected part remains
opaque. Camera focus, picking and dimensions use that occurrence's placed frame;
saved sketch coordinates stay local to the definition. Finish the sketch and
recompute to update every occurrence and its joint references. MCP uses the same
operation: `sketch_edit` accepts `name` and optional `occurrence_id`. Placement and
assembly structure edits require finishing the active component edit first.

JSONC is deliberate. Long command sequences need comments, predictable quoting and
simple diffs. It uses the same JSON-shaped arguments as MCP without YAML’s implicit
types or indentation-dependent objects, and it avoids TOML’s repeated table syntax
for deep, ordered command lists. `//` and `/* ... */` comments and trailing commas
are supported. Comments inside strings are preserved. Version 1 contains data and
expressions only: no embedded JavaScript, shell commands or general programming
runtime.

## Open and run scripts in CAD

The **Scripts** button opens one workspace beside the current design. Choose
**Browse examples** for bundled lessons, complete designs and manufacturing
coupons, or **Open script...** for a `.limo.jsonc` file. **Path / Load script**
uses that same file loader when entering a path directly. **File → Open Script…**
also opens source in this workspace; the native macOS File menu uses the same
action as the in-window File menu.

Both an example and a file open the authored source editor without running
modeling commands or changing the current design. Bundled source has no
filesystem path until **Save script as...**. Lessons use this same select,
inspect, edit and run workflow.

**Run in new design** is available in the source editor. It creates a new retained
document tab before starting the shared live runner. The previous design stays
available in its tab. Execution uses the inspected, expanded source snapshot,
including its loaded includes; loading or validating again is required to pick
up later included-file edits. **Chapters** lets you inspect the teaching notes
before running, and the presentation and rate controls choose how the run is
shown. Pause, Step, Resume, Stop and rate controls use the native playback bar.
Save the result as an ordinary editable `.limo` project.

The playback bar docks below the viewport. **Close** hides it without changing
execution; **Show playback** restores it from the top bar, including after a run
has completed or stopped.

**Inspect / edit source** opens the authored JSONC in the existing native
multiline text field. Comments and `includes` remain intact. **Validate source**
uses the shared parser and resolves includes beside the displayed source path;
errors remain visible and Run stays unavailable until the current draft is
valid. Validation does not save edits. **Save script as...** explicitly writes
the authored text, including unfinished invalid drafts, through the existing
atomic file writer. It never substitutes expanded source or copies included
files. After saving to a new directory, Validate resolves includes there and
reports missing fragments. Opening another source requires saving or explicitly
discarding unsaved edits first. Closing the Scripts card retains its draft.
Application exit waits for script file operations and requires Save As or
Discard for an unsaved draft, including text still being edited in the field.

Recipe command-line URLs and `cad_interface` action `open_recipe` enter that
same source-only queue. Busy file/script work finishes first; unsaved source
requires Save As, Discard, or Cancel opening. A queued receipt acknowledges
delivery, not playback or replacement of the design. Native OS protocol
registration delivers recipe URLs to the Bevy Scripts editor.

For a preview-enabled example, **Preview lesson** renders its isolated captioned
teaching frames without accessing the current CAD document. Previous,
Next, explicit Replay/Stop, Fit, drag and arrow/Home keys use the preview camera.
It does not autoplay. Editing the bundled source disables its preview; use Run
in new design to inspect those edits. Closing the preview releases its retained
document, view and image. DPI-scaled requests use the shared bounded renderer.

Previews are bounded to short sources: at most 80 construction steps and checks
combined, and 2 MiB of source. They use a separate headless engine and the same
native interpreter. Ribbon hover help can show the same lesson for its associated
operation. Use **Run in new design** to build and inspect the editable model,
including larger assemblies.

The Bevy desktop uses native source editing, file dialogs, IME and lesson preview
rendering through this shared Rust workflow. Close the Scripts workspace to
restore viewport space, then use **Scripts** to reopen the retained draft.

## One execution path

The Rust `limo-cad-script` crate resolves references and sequences the existing grouped
interface operations. The MCP entry point is one call:

```json
{
  "action": "script",
  "path": "/absolute/path/to/design.limo.jsonc",
  "mode": "present",
  "validate": true
}
```

Send this to `cad_interface` after attaching to the intended session. Execution does
not return to an LLM for every modeling operation. Calls still happen in dependency
order, and a rejected operation or a geometry error stops the sequence immediately
with the failing step identified. An independent agent can use the ordinary live
presentation controls while the script is running.

For bundled recipes, `{"action":"recipes"}` returns the shared Rust catalog without
executing a design. Supply `"recipe":"mounting-plate"` instead of `path` or `source`
to run that committed source. Exactly one source selector is accepted. The app
uses the same collection; titles, chapters and actual operations are derived from
the script. A selected recipe is not the legacy `cad_script` trace-export command.

`cargo xtask run-script FILE --server CAD_EXECUTABLE --server-arg --headless` uses a Rust
MCP client to invoke the packaged server. A standalone `limo-cad-mcp` needs no
`--server-arg`; AppImage launch flags are also passed as separate server arguments.
See [developer replay setup](DEVELOPMENT.md#replay-a-recipe) for complete examples
and initialization deadlines. With no desktop session, replay runs headlessly
at maximum rate.
`--recipe ID` selects the shared bundled source instead of a file. A file or ID is
required; the runner does not silently select an example.
Add `--session UUID --new --present --speed 2` to create a blank design tab and
animate it in the existing window. Preserve the current document first. Omit
`--new` when the named session already contains the intended blank tab. Version 1
scripts require an empty document, including no active sketch, drawing or assembly.
`--save PATH` writes the resulting native file after completion.

`--repeat 2 --out DIRECTORY` starts independent headless MCP processes and compares
exported model, sketches, solved assembly and scene data. It writes the individual
run reports and native model JSON for inspection. A repeated run with identical
output is the determinism evidence; successful commands alone do not establish it.

## Reference visibility

The **Reference → Construction** button shows or hides retained sketches and datum
planes together. It uses the same `solid/reference` operation as a script:

```json
{"call":{"group":"solid/reference","operation":"construction_set_visibility","arguments":{"visible":false}}}
```

Omit both selectors to affect all current references. To select particular sets,
provide `sketch_names` and/or `datum_plane_ids`; an omitted category stays unchanged,
and an explicit empty array is a no-op. Unknown references reject the whole call.
Visibility is saved in the project and leaves body visibility and parametric
geometry unchanged. The active unfinished sketch stays visible, and references
created later start visible. Construction entities inside a sketch are a separate
sketch-editing setting.

The Browser's individual visibility choices are available in
`document/appearance`: `project_visibility` reads the saved snapshot and
`project_set_visibility` replaces its `hidden_body_ids`, `hidden_datum_plane_ids`
and `hidden_sketch_names` arrays. Preserve the other arrays when isolating a part,
and restore the original snapshot after a presentation or print-layout step.
These are the same project settings the Browser uses. Hiding a body changes its
display; it does not remove geometry or substitute for explicit export selection.
## File structure

A script has `version: 1`, a human-readable `name`, ordered `steps`, optional final
`checks`, optional named `views`, and `exports`. The optional `starting_state: "empty"` documents the required
blank starting state; omitting it has the same effect in version 1. The
[editor schema](../examples/scripts/limo-cad-script.schema.json) describes these fields.
Each step has exactly one of these actions:

- `call`: an existing `group`, `operation` and `arguments` object.
- `let`: bind an expression to a descriptive name.
- `assert` and `equals`: compare resolved values.
- `note`: persistent presentation text, with optional `chapter` and `duration_ms`.
- `view`: camera orientation and optional framing instructions.

A call’s `id` binds its complete returned result. Additional `bind` names can point
at paths within that result. Later arguments can use `{"$ref":"name"}` or add a
JSON `pointer`, for example `{"$ref":"picket","pointer":"/id"}`. A `let` uses
the same expressions to name a useful subset of an earlier result. Bindings are
immutable; use distinct steps when a binding depends on another binding.

This excerpt shows a named geometric selection followed by the actual feature:

```jsonc
{
  "let": {
    // Resolve the edge set from the completed stock operation, not recorded IDs.
    "crown_edges": {
      "$select": {
        "from": {"$ref": "picket_body_snapshot"},
        "path": "/edges",
        "where": {"$every": {"path": "/points", "where": {"/z": 315}}},
        "take": "all",
        "pointer": "/id"
      }
    }
  }
},
{
  "id": "round_crown",
  "call": {
    "group": "solid/refine",
    "operation": "solid_fillet",
    "arguments": {
      "body_id": {"$ref": "picket_body"},
      "edge_ids": {"$ref": "crown_edges"},
      "radius": 40,
      "tangent_chain": false
    }
  }
}
```

The complete bench also limits the crown edges to the two shoulders running through
its thickness. It is the executable reference for those full selectors; the excerpt
only illustrates expression structure.

A `$select` reads an array at `path`, filters by `where` and returns `one`, `first`,
`last` or `all`. Field predicates use JSON pointers. `$and`, `$or` and `$every`
combine geometric predicates. Floating-point comparisons allow a 1e-6 absolute
tolerance; unsigned integer identifiers compare exactly.
Use `take: "one"` when a reference must be unambiguous: zero or multiple matches
stop execution. Coplanar face fragments may intentionally use `first` when they
share the same machining plane.

`$count` counts an array. `$project` projects an authored 3D part coordinate into
a returned plane basis, producing the two-dimensional coordinates required by a
sketch or drilling operation. This prevents a hole layout from depending on a
recorded face ID or on a kernel-selected local face origin.

## Presentation without a second model

Fast mode runs all construction and validation with no authored presentation waits.
Presentation mode renders the same commands and consumes notes and camera steps.
The captions describe design decisions and persist until the next note. Short holds
at chapter boundaries allow the explanation to land without slowing every operation.

A view can use `current`, `isometric`, `front`, `back`, `left`, `right`, `top` or
`bottom`. `fit: true` frames the model; one optional focal target narrows it:
`body_id`, `component_id`, or `target: "active_sketch"`. `duration_ms` controls the
camera transition. Targets use the same named result expressions as modeling calls.

For a continuous turn around the finished part, use
`{"view":"current","orbit_degrees":120,"duration_ms":3000}`. The signed angle
can range from -360 to 360 degrees, including a complete revolution. The camera
keeps its distance, elevation and up direction around its current target. Optional
`fit` or a focal target establishes framing before the orbit; for a smooth framing
transition, put a normal view step before it. Orbit requires `view: "current"`.
The same fields work directly with `cad_interface` action `view` on a loaded
document; a script still starts from a blank document. Completion acknowledges the
actual camera animation. Presentation speed and reduced-motion preferences apply.

## Named view configurations

A top-level `views` array is stored in the project when the script finishes, in
both fast and presentation mode. Each view has a `name`, a `camera`
(`position`, `target`, `up`, in millimeters), `visible_body_ids`, and optional
`part_offsets` (`body_id` and a world-axis `translation` in millimeters).
Offsets change only the display. Body ids may be literals or result references;
duplicate resolved visible body ids are deduplicated before storage.
The Browser recalls a view by name.

Replacing `views` clears the active-view marker and display offsets; recall a
view to apply the replacement. A recalled view's camera and offsets stay with
its open project tab, including when an idle tab is released from memory.
Modeling picks exclude the display translation, so holes and move pivots keep
their model coordinates in an exploded view.
Use **Return to assembled view** in the Browser's Named Views folder to clear
display offsets and the active marker while preserving visibility and saved
configurations. Entering a sketch or feature edit, or changing the solid model,
also returns to the assembled pose. `clear_named_view` exposes the same explicit
reset through the engine and MCP interface.

Recall does not add a modeling Undo step. Ctrl+Z/Redo continues to change the
feature history and returns the model to assembled poses. Visibility choices
and saved named-view definitions survive solid Undo/Redo, including choices
made between Undo and Redo. Assembly edits and motion previews also return to
assembled poses before editing.

The shared presentation interface exposes `configure`, `note`, `pause`, `resume`,
`step`, `status`, `finish`, `stop`, `dismiss` and `show`. Configuration chooses `mode: "fast"` or
`"present"` and `speed` from 0.1 to 16. The on-screen controls operate the same state.
Pause preserves the remaining authored hold. Single step allows one modeling
mutation. Maximum rate skips presentation delays and camera animation. An authored
note’s duration is scaled by the current speed; it is not a mandatory sleep after
every operation.

Use steady geometry emphasis and purposeful camera moves. A shape update should
make the changed feature readable. Avoid full-viewport flashes, random effects and
rapid repeated zooming. The bench focuses the first stock construction, notch cuts,
crown, datum slot and arm noses, then returns to broader assembly views at chapter
boundaries.

## Validation and golden examples

Construction uses the authoritative snapshots already returned by mutations to
resolve the next command’s references. It does not need a separate inspection call
after every operation. Dependent operations still wait for successful completion;
maximum rate never means issuing commands against an unfinished model.

Place comprehensive model, solved assembly and interference checks in `checks`.
These run after the finished design is visible. Cheap error detection and required
reference/cardinality assertions remain at their point of use so a broken sequence
stops immediately. The CLI always validates. The MCP entry point also validates by
default, and the bench requires its final gate even if a caller asks to skip checks.

The garden-bench example declares `verification: "garden-bench"` and exports its
semantic manufacturing contracts alongside the finished native model and geometry.
Those contracts include member sizes and machining datums, expected occurrences,
bores and fastener connections. The Rust end gate checks the authored assembly
intent without converting the demonstration into a stream of temporary test edits.

Golden examples should export `final_model`, `final_scene`, `final_solution` and
`final_sketches` for independent comparison. Use physical dimensions, named features
and returned geometry references, never captured numeric entity IDs, cached solid
snapshots or imported tessellation as a construction shortcut.

Use `cargo xtask run-script FILE --server CAD_EXECUTABLE --server-arg --headless --repeat 2 --out DIRECTORY` to compare
independent fresh processes. A later live run can add `--compare DIRECTORY/run-1.json`;
the comparison excludes tool-disclosure hints, editing undo availability and
regenerated midpoint snap candidates. It retains persisted sketch constraints and
references, model history, geometry and assembly data. JSON boundaries preserve
floating-point round trips so tiny normals and solver residuals compare correctly.
This proves repeatability for the tested script and binary/kernel build; results are
not normalized across different kernel versions or platforms.
`--session UUID --new --present --speed 2` reuses an existing CAD
window and creates a blank design tab. Save or preserve the current document first.
Do not pass `--desktop` when the intended window is already open.

The regression harness commands below use the standalone `limo-cad-mcp` developer
server as `MCP`; see the [developer setup](DEVELOPMENT.md).

`cargo xtask test-mcp playback --server MCP --session UUID --out DIRECTORY` checks
the actual native caption, speed selector, pause, Step, Resume, Stop and Maximum
controls. It preserves the existing document, runs a small sketch exercise in the
same window and leaves a blank design for the next example. It never launches
another desktop window.

`cargo xtask test-mcp scripts-workspace --server MCP --session UUID --out DIRECTORY`
loads a small commented sketch-and-extrusion source through the visible file-path
control. Loading must preserve the original model and tabs; **Run in new design**
must retain the original and create an editable, fully constrained result in one
new tab. The scenario also closes and restores the Scripts dock and completed
playback controls, checking viewport space, retained source and completion state.
It writes its temporary source, native result and proof report to the output
directory. Finish active editing before running this check. It uses the named
existing window and has no dependency on the bundled example catalog.
Add `--script /absolute/path/source.limo.jsonc` to also check loading another
source before the built-in fixture; the additional source is never executed.

The recipe-library layer adds headless recipe and real-kernel preview checks
alongside their authored sources. These do not replace the live adapter checks
above or establish that the teaching interface has been validated on a new build.

## Script collections

Large designs can keep each part in its own JSONC fragment and compose them from
a root `.limo.jsonc`:

```jsonc
{
  "version": 1,
  "name": "Clamshell assembly",
  "includes": [
    "collections/base.collection.jsonc",
    { "path": "collections/lid.collection.jsonc" }
  ],
  "steps": [
    // mating / assembly calls after included part construction
  ]
}
```

A collection file is not a full script. It supplies `steps` and optional
`checks` (and may itself `includes` further fragments). A `.limo.jsonc` file
may also be included: its `steps` and `checks` are composed in, and its
`version`, `starting_state`, `verification`, `exports`, and `$schema` are
ignored. The root script keeps those fields.

The host expands `includes` when the root is loaded by **absolute `path`**, or
when inline `source` is paired with an absolute `include_base` directory (the
desktop editor uses that so an edited buffer still finds its fragments). Each
include path is relative to the file that declares it, not always the root
directory. Paths use forward slashes only, must not contain `.` or `..`, must
stay under the root file’s directory after canonicalization, and must end with
`.collection.jsonc` or `.limo.jsonc`.

Included steps share the root’s binding and step-id namespace. Prefix ids per part
(`base_…`, `lid_…`). After expansion, validation and execution are identical to a
monolithic script — including fast mode skipping presentation.

Inline `source` without `include_base`, and bundled `recipe` selectors, reject
unresolved includes. The desktop Scripts panel keeps the authored file in the
editor and expands includes only when validating or running.

## Exporting version-1 JSONC

`cad_interface` action `export_script` returns a version-1 `.limo.jsonc` `source`
string (and fidelity metadata). It is **not** `cad_script`.

- `cad_script` — forward dump of successful mutating MCP calls in this process as
  `{ calls: [{ name, arguments }] }`. Debugging aid; not a recipe format.
- `export_script` — emits replayable version-1 JSONC when possible.

| `fidelity` | Meaning |
|------------|---------|
| `lossless_authored` | Expanded source from the last successful `action: script` in this process. Modeling commands and result references match that run. Comments may be omitted after include flattening. The export is pretty-printed. `stale: true` means tools ran after that script; the source is still that script, not the later session. |
| `lossy_session_trace` | Synthetic script from the session tool trace with literal arguments. No `$select` / `$project`, no notes/views. Suitable for scratch replay of a blank-session MCP build, not for publishing. |

`from` selects the source:

- `auto` returns the authored script while no modeling tool has succeeded since that run, live desktop edits included. Once later tools have run, it returns the session trace instead of calling the old script lossless.
- `last_script` returns the authored script even when `stale` is true.
- `session_trace` always rebuilds from the tool trace.

Attach/refresh baselines (`cad_load_project_model`), pure UI feature edits, and
STEP imports without native history are **out of scope** for faithful JSONC
export in this release. Prefer keeping JSONC as the generator SoT.

### Agent rebuilds

Always use `"mode": "fast"` for agent and CI rebuilds. Fast mode skips `note` and
`view` (including those inside collections) while running the same modeling
commands and checks as presentation mode. See
[agentic JSONC workflow](agentic/jsonc-workflow.md).

