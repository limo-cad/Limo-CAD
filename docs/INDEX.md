# Documentation

## Start using CAD

- [Install and make your first part](INSTALL.md) — download, run the fillet lesson,
  edit a dimension, then save and reopen the project.
- [Connect an MCP agent](INSTALL.md#connect-an-mcp-agent) — packaged server setup
  and a concrete first task.
- [Recipe library](../examples/scripts/README.md) — short lessons, assemblies and
  print-fit coupons.
- [Assemblies](ASSEMBLIES.md) — reusable parts, joints, motion and interference.
- [Drawings](2D_DRAWINGS.md) — sheets, dimensions and export coverage.
- [CAM](cam/README.md) — toolpaths, stock simulation, posts and safety limits.
- [Flagship designs and validation](flagship-examples.md) — bench, vise and turbine.
- [Engineering knowledge](../knowledge/index.md) — materials, gears, bearings,
  workholding and printing guidance, also available offline through MCP.

## Automate and author recipes

- [MCP interface](../mcp-server/README.md) — operations and current boundaries.
- [Native scripts](native-scripts.md) — construction source and presentation controls.
- [Live document ownership](mcp-harness.md) — discovery, attachment and ordered edits.
- [Shared product interface](interface.md) — groups and operation contracts.
- [Native drawing export](native-drawing-export.md) — SVG/DXF payloads and references.
- [Manufacturing export](manufacturing/INDEX.md) — STL/3MF implementation and validation.

## Repository layout

| Path | Role |
|------|------|
| [src/](../src/INDEX.md) | Desktop UI entrypoints |
| [crates/](../crates/INDEX.md) | Host-neutral Rust crates |
| [desktop/](../desktop/INDEX.md) | Native desktop (Bevy + OCCT) |
| [mcp-server/](../mcp-server/INDEX.md) | MCP server surface and disclosure |
| [examples/scripts/](../examples/scripts/README.md) | Bundled recipes and lessons |
| [knowledge/](../knowledge/index.md) | Engineering knowledge served through MCP |
| [interface/](../interface/catalog.json) | Shared operation catalog for the UI and MCP |

## Contribute and develop

- [Developer setup](DEVELOPMENT.md) — the canonical build, native SDK and test guide.
- [Versioning and releases](RELEASING.md) — the one `VERSION` source, the guard
  that keeps every carrier honest, and how a release is tagged and published.
- [Release notes](release-notes/README.md) — one reviewed file per release tag,
  which the tag build publishes as the release description.
- [Contributing](../CONTRIBUTING.md) and [edge-case hunt](EDGE_CASE_HUNT.md) — focused
  improvements and useful bug reproductions.
- [Project direction](goals.md) — reliability, performance and ease of use.
- [Limo naming proposal](limo-naming-proposal.md) — accepted name, roots, four-language
  presentation, and learning direction.
- [Limo rename checklist](limo-rename-checklist.md) — sequenced cutover from Limo CAD,
  with what must stay stable.
- [Architecture proposals](proposed-architecture.md) — future approaches and rationale.
- [Agent and maintainer guidance](agentic/INDEX.md) — disclosure, source installation
  and implementation contracts.
- [Machine-design knowledge base](machine-design-kb.md) — OKF domain help, licenses, MCP plan.
- [Help search ADR](machine-design-help-search.md) — BM25 `cad_help` caps and growth bar.

<details>
<summary>Specialist implementation references</summary>

- [CAM foundation](CAM.md) and [High Speed Roughing](CAM_ADAPTIVE.md).
- [OCCT packaging](OCCT_PACKAGING.md), [Windows packaging](WINDOWS_PACKAGING.md) and
  [Ubuntu packaging](LINUX_PACKAGING.md).
- [Windows native viewport debugging](WINDOWS_NATIVE_VIEWPORT_DEBUGGING.md).
- [Sketch constraint matrix](SKETCH_CONSTRAINT_PAIRWISE_MATRIX.md),
  [modeling selection](MODELING_VIEWPORT_SELECTION.md) and
  [viewport interaction](VIEWPORT_INTERACTION_THEME.md).
- [Projected face-boundary profiles](SKETCH_FACE_BOUNDARY_PROFILES.md) — how a
  face sketch receives the support face's edges, and why a projected-only face
  never becomes a profile. Its hardening record is in the
  [projected face-boundary review](projected-face-boundary-review-2026-09-20.md).
- [Icon provenance](ICON_PROVENANCE.md), [MCP milestones](../mcp-server/OKRs.md) and
  [presentation review](demo-presentation.md).

</details>

Shared agent guidance belongs in `docs/agentic/`. Editor-specific `AGENTS.md`
and `.cursor/` files remain gitignored under the repository's existing policy.
