# Native command scripts

The interpreter is reviewed separately from the app/preview integration in
PR #99, which remains draft. Bundled construction recipes and their checks form
the next layer. See [the interface review](script-interface-review.md) for the
tested evidence and remaining work before promotion.

A `.nbcad.jsonc` file is the reproducible construction source for a native design.
A `.nbcad` file is the editable project produced by those commands. Keep both when
publishing an example: one explains how it was made, the other opens directly for
further parametric work.

JSONC is deliberate. Long command sequences need comments, predictable quoting and
simple diffs. It uses the same JSON-shaped arguments as MCP without YAML’s implicit
types or indentation-dependent objects, and it avoids TOML’s repeated table syntax
for deep, ordered command lists. `//` and `/* ... */` comments and trailing commas
are supported. Comments inside strings are preserved. Version 1 contains data and
expressions only: no embedded JavaScript, shell commands or general programming
runtime.

## Open and run scripts in CAD

The **Scripts** button opens the script workspace beside the current design.
Load a commented source file to inspect its chapter notes and grouped commands.
The recipe-library layer adds the bundled collection using this same adapter.

The Source tab supports editing, validation through the Rust parser, and **Save
script as…**. The file-path disclosure exposes the same loader to the semantic
MCP controls without having to operate an operating-system file picker.

Use **File → Open Script…** to load a `.nbcad.jsonc` file into that workspace. The
native macOS File menu routes this through the same action as the in-window File
menu. Opening a source file does not execute its modeling commands. Close the
script dock when you want the space back; use **Scripts** to show it again.

**Run in new design** creates a blank design in the existing CAD window and replays
the selected source there. The existing project remains separate. Presentation
controls provide Pause, Step, Resume, speed and Maximum rate during the run. Save
the resulting `.nbcad` project to retain its editable feature history.
The playback bar docks below the viewport. **Close** hides it without changing
execution; **Show playback** restores it from the top bar, including after a run
has completed or stopped.

Short lessons can also provide an isolated miniature preview by exporting
captioned scene frames. Preview execution
uses a separate headless engine, then displays its returned tessellation in the
small preview surface; it does not borrow the active document, camera or native
viewport’s feature-preview channel. Ribbon hover help can show the same lesson
where an example is associated with that operation. Hovering does not run the
lesson in the active design.

Preview is intentionally bounded to short source files: at most 80 construction
steps and checks combined, and 2 MiB of source. Use **Run in new design** for a
larger assembly. Both paths execute the same native command interpreter; a preview is
not a replacement for the editable `.nbcad` project or the final validation gate.

## One execution path

The Rust `nbcad-script` crate resolves references and sequences the existing grouped
interface operations. The MCP entry point is one call:

```json
{
  "action": "script",
  "path": "/absolute/path/to/design.nbcad.jsonc",
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

`cargo xtask run-script FILE --server MCP_EXECUTABLE` uses a Rust MCP client to invoke
the same entry point. With no desktop session it runs headlessly at maximum rate.
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

## File structure

A script has `version: 1`, a human-readable `name`, ordered `steps`, optional final
`checks`, and `exports`. The optional `starting_state: "empty"` documents the required
blank starting state; omitting it has the same effect in version 1. The
[editor schema](../examples/scripts/nbcad-script.schema.json) describes these fields.
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

Use `cargo xtask run-script FILE --server MCP --repeat 2 --out DIRECTORY` to compare
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
Add `--script /absolute/path/source.nbcad.jsonc` to also check loading another
source before the built-in fixture; the additional source is never executed.

The recipe-library layer adds real-kernel preview and bundled-lesson acceptance
scenarios alongside their authored sources, extending these adapter checks.
