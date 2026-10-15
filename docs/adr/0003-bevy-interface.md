# ADR 0003 — One Bevy application interface

- Status: Accepted and implemented; release evidence is recorded separately
- Date: 2026-09-13
- Tracking: [#38](https://github.com/limo-cad/Limo-CAD/issues/38)
- Supersedes: the React-shell and native-child-composition ownership decisions in [ADR 0002](0002-bevy-viewport.md)

The native cutover merged in [PR #124](https://github.com/limo-cad/Limo-CAD/pull/124).
Implementation checkpoints remain in the
[pre-merge record](https://github.com/limo-cad/Limo-CAD/blob/c537f0232ab86f28b1be3ee46fa6bf5ff46db5b9/docs/adr/0003-bevy-interface.md).
[Qualification receipts](../qualification/) record their source revisions and limits.

## Decision

Move the existing application's complete interface to Rust and Bevy. This is
one product and one command system: a person, an MCP client, and a replayed
recipe operate the same document services. OCCT remains the solid-modeling
authority. This decision does not replace B-rep geometry with rendering meshes.

The conversion includes the application shell, editable command forms, sketch
interaction, assemblies, drawings, CAM, scripts, presentation controls,
settings, file handling, and window lifecycle. Moving only the viewport HUD or
putting native buttons over the old web shell does not complete it.

Use Bevy/Winit for the native window, input and accessibility host.
Keep the existing binary and the ordinary desktop versus `--headless` choice;
do not introduce a second application or a permanent alternate-interface flag.
The production desktop uses that host directly. There is no embedded WebView
or alternate desktop host; native IME and accessibility use Winit and AccessKit
with the application's existing guarded editor/control adapters.

## Shared behavior

`interface/catalog.json` remains the product grouping authority. Native widgets
and MCP consume it; neither adds a competing operation taxonomy. Rendered
widgets register their actual entity identity, accessible name, computed
bounds, enabled state, input contract and modal ownership. Inspecting controls
must describe what a person can operate in the displayed interface.

Human input and MCP resolve through the same control validation and native
reducer. A queued control is bound to its document incarnation and semantic
binding, not merely a tab name or recycled widget position. Revalidate ownership
under the document's mutation lock. Reuse the existing engine dispatcher and
project-replacement path; do not translate native actions into hidden DOM clicks
or duplicate the modeling command switch.

The host owns presentation and file-dialog effects. Shared Rust services own
model changes, history, file encoding, validation and replay. Interactive
operations retain their draft, selection, preview, Apply/Cancel, error and undo
semantics. A generic JSON editor for API arguments is not a substitute for an
intuitive modeling form.

## Rendering and interaction

Use one composited Bevy surface for shell, floating controls and viewports.
Native layout must clip and hit-test menus, scroll areas and modal backdrops
consistently. Preserve accurate model/paper coordinate transforms in drawings
and viewport coordinates under DPI changes and panel resizing.

Retain widgets and geometry; update changed data rather than rebuild the entire
scene on pointer motion. Keep idle work event-driven. Long geometry and CAM
operations need cancellation, visible progress and owner-checked completion
without blocking navigation or assigning results to another document.

Distinguish accepted commands, completed model changes, published snapshots,
laid-out controls and submitted rendered frames. An action acknowledgement or
an inspected control tree is not proof that its pixels have been presented.
Background/headless operation must not wait indefinitely for a visible frame.

Native text entry must include selection, clipboard, shortcuts, composition
and IME. Focus traversal, modal trapping, keyboard activation and OS screen
reader access are part of the interface, not optional metadata. Winit and Bevy
integration still require real platform validation; their presence in the
dependency graph is not evidence that these behaviors work.
