# What we are building

Shared directions for people and contributors. Our priorities, in order, are
**reliability, performance, and ease of use**. Implementation proposals live in
[proposed-architecture.md](proposed-architecture.md); current behavior is described
in the product guides linked below.

Limo CAD is **local** mechanical CAD. Files stay on your machine. There is no
required cloud account or cloud control plane.

## Accepted high-level directions

These broaden the original Limo CAD goal; they do not replace it.

- **Mechanical design:** dependable sketches, features, history, drawings,
  assemblies and project files, with responsive interaction and clear workflows.
- **Additive manufacturing:** native **3MF** export with per-body materials and
  colors, plus **STL** for mesh interchange and **STEP** for exact CAD geometry.
  Export metadata supports the slicer handoff; it does not qualify a material or
  replace slicing and physical testing.
- **Local automation:** MCP uses the same product groups and native operations
  as the desktop. Agents can build headlessly or drive an explicitly selected
  live document. Bring an MCP-compatible agent and model; keeping that interface
  useful as frontier models and clients evolve is an ongoing priority.
- **Build, teach and demonstrate:** one Rust-interpreted construction source
  supports maximum-rate execution, step-through inspection and paced presentation.
  Bundled recipes and offline engineering guidance provide the foundation for
  more feature lessons and, eventually, conversational design wizards.
- **CAM:** a careful path toward functional, modern **3-axis** CAM, developed
  with machining feedback. An early toolpath, stock simulation and machine-aware
  post foundation exists; it is not production-safe CAM. See the
  [implementation and safety limits](cam/README.md).
- **Simulation / analysis:** extend the existing fit and motion tools in stages;
  strength analysis requires a separately validated solver stack.

## Simulation in stages

Do not treat fit, motion, and strength as one deliverable:

1. **Geometric fit / interference:** native solid and assembly checks exist;
   broaden their reliability on real designs.
2. **Motion:** assemblies, joints and deterministic kinematic previews exist.
   Continue hardening mechanisms and coupled motion; this is not a dynamics engine.
3. **Strength / FEA:** future work requiring validated meshing, material, load
   and solver behavior. A material assignment is not a strength calculation.

## Near-term engineering priorities

1. **Reliability:** make sketching, solid modeling, drawings, assemblies, history,
   undo, project files and export dependable. Preserve explicit live-document
   ownership and turn reported failures into focused regression tests.
2. **Performance:** improve preview, selection, recompute, rendering and MCP
   execution without weakening validation or introducing a second modeling path.
3. **Ease of use:** simplify installation, navigation and feature workflows across
   desktop and automation. Improve authored lessons, captions and camera guidance
   using the existing Rust script format and shared product interface.

These priorities apply to both interactive work and automation. Examples should
retain editable parametric history and identify their build and validation
evidence. Digital replay and geometry checks do not establish physical fit,
strength, durability or generator output.

## Related reading

The next interface investigation is the [Bevy shell proposal](https://github.com/limo-cad/Limo-CAD/issues/38),
starting with [one native pane](https://github.com/limo-cad/Limo-CAD/issues/29).
Use the existing Bevy UI builders and shared command identities, with explicit
ownership for focus, text input, accessibility and MCP control. Remove each
replaced path only after its replacement works. Measure clean and incremental
build time and dependency changes; fewer dependencies alone do not prove a faster
build. The [accepted viewport boundary](adr/0002-bevy-viewport.md) still applies
until a reviewed decision changes it. This front-door work does not migrate the
shell or replace the current packaging workflow.

- [README.md](../README.md) — public product overview
- [interface.md](interface.md) — shared desktop, MCP and API contract
- [mcp-harness.md](mcp-harness.md) — current headless and live ownership behavior
- [native-scripts.md](native-scripts.md) — Rust construction and presentation scripts
- [flagship-examples.md](flagship-examples.md) — examples and qualification boundaries
- [proposed-architecture.md](proposed-architecture.md) — architectural proposals
- [mcp-server/README.md](../mcp-server/README.md) — current server and setup
