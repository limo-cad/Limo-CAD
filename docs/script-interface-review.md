# Script interface: validation and remaining work

PR [#99](https://github.com/jackControls/noBS-CAD/pull/99) is a saved, tested draft.
The construction engine is usable; its product and learning interface still needs
work. The Rust interpreter is a separate lower layer; bundled recipe sources,
their catalog and the example-dependent acceptance checks are a separate upper
layer. These are implementation gaps and release decisions, not failures hidden
by the example's passing geometry checks.

## Validated implementation

One Rust interpreter executes the commented command files through the shared
operation catalog. MCP and the native Scripts workspace use it directly. There is
no JavaScript modeling script or native sidecar interpreter. The native main
viewport renders through Bevy. The workspace loads, edits, saves and runs sources;
playback supports captions, speed, pause, step, stop and close/show.

The 2026-09-10 recording used rebuilt commit `59a27f4`. The complete live bench
passed all 595 construction steps and 47 final checks, then matched independent
reference model, sketch, assembly and geometry exports exactly. It contains 27
fully constrained sketches, 34 solved occurrences, 95 history features and 33
joints. Its saved native project matches that reference. This is repeatability
evidence for the tested binary/kernel build, not across different kernel versions.

The local recording was retimed to 26 seconds of chronological construction and
a four-second actual isometric capture. Hiding 27 sketches and one datum plane for
that closing shot changed visibility only. The separate recording window closed
through MCP, and both original user documents were preserved byte-for-byte.

The two manufacturing candidates also completed full live runs at `6f7e5e6`:
692 vise steps with 55 final checks in 237 seconds, and 1,622 turbine steps with
11 final checks in 959 seconds. Both exactly matched the independent headless
model, scene, sketches and solved assembly. The corresponding headless first
runs were 70 and 184 seconds. These are measured presentation runs on this
machine, not a maximum-rate benchmark or a profiler attribution.

PR112 makes material resolution identical through live and headless dispatch,
retains completed output before Save, and waits for the actual viewport and
camera after leaving Drawings. PR113 supplies the shared Show refs operation;
saved clean copies hide all 43/24 vise sketches/datums and 105/105 turbine
sketches/datums without changing any geometry or body visibility. PR114 corrects
the stale final step count and prevents a later Save from overwriting the final
camera feedback. PR108 owns export recovery after a proven unchanged Open
rejection, guarded STEP/mesh export and Save ownership, while retaining guards
after uncertain native replacement. The original PR115 recovery fix is now in
that owning layer. PR115 addresses document/session ownership during script
startup, inbox work, publication and delayed presentation controls.

## Before promoting the draft

- **Use the native renderer for feature previews.** The small preview currently
  projects immutable Rust-generated geometry with `src/scripts/previewGeometry.ts`
  and Canvas2D in `ScriptPreview.tsx`. That is a second renderer and remains a
  prototype. Reuse a bounded Rust/Bevy viewport without attaching to or changing
  the user's document. Retain one-shot playback, reduced-motion behavior, close,
  fit and inspect controls. Track teaching in #16 and native chrome in #29;
  full shell retirement remains #38.
- **Settle the run/inspect interaction.** File → Open Script, the Scripts dock,
  ribbon hover previews and the playback bar are implemented. Review them as
  one workflow: loading must not execute, the target design must be explicit,
  status and speed must agree, and controls must reclaim viewport space when
  closed. Keep keyboard dismissal and inspection available without hover timing
  traps or a popup that obstructs ordinary modeling. Passing control contracts
  alone does not approve this layout. During the latest attached fast-mode vise
  run, status still reported 0/692 while drawing operations were being applied;
  the final script report correctly completed all 692 steps. Expose inexpensive
  in-flight progress without adding a publication roundtrip for each step.
- **Stage construction for teaching.** The grouped closing-view operation now
  exists. During long builds, retained sketches/datums and overlapping part
  definitions can still obscure the current sketch before assembly placement.
  Review chapter-level visibility and camera framing without discarding the
  parametric references or silently hiding an unfinished sketch. Keep the final
  geometry and checks independent of presentation choices.
- **Consolidate presentation ownership.** Modeling and ordering are Rust;
  presentation timing and camera interpolation currently include frontend code in
  `operationPlayback.ts` and the viewport adapter. Move behavior into Rust/Bevy
  where the native renderer can own it, while keeping one ordered command path
  and equivalent maximum-rate results. Avoid adding parallel animation state.
  Profile attached maximum-rate and presented runs separately; do not infer that
  increasing presentation speed removes native publication and viewport work.
- **Complete the learning and manufacturing scope.** The short fillet lesson is
  not comprehensive feature coverage. Preserve #89's unique analytic/round-trip
  checks: its four small examples now have native JSONC sources and Rust stdio
  validation in the recipe-library layer. The bench remains a design
  candidate; selected-occurrence editing is #94 and complete drafting is #93.
  The printable windmill, bench and screw vise are iteration targets in
  [flagship-examples.md](flagship-examples.md), not released models.

Use focused regressions and live demonstrations to validate these changes.
Do not add a new CI matrix or tool-coverage percentage gate. Resolve the concrete
gaps before claiming the draft is ready; keep already reviewed export, drawing,
exit and bench fixes in their existing PRs.
