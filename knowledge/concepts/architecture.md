---
type: Concept
title: Architecture
description: Kernel, shell, viewport, and project-file boundaries in Limo CAD.
status: stable
updated: 2026-10-02
---

# Architecture

## Kernel

- Rust crates: `core`, `sketch`, `solid` (host-neutral model logic)
- `occt` — native geometry adapter; `wasm` — browser adapter path
- Same planner code for desktop and MCP; live edits are routed to the owning
  desktop document through its session inbox

## Shells

- Desktop UI and viewport: Rust/Bevy; isolated script previews reuse its scene systems
- Browser replacement: the same Bevy UI, with the OCCT WASM and browser host-service
  ports still unfinished. The React/Three.js application is retired.

## Files

- `.limo` — editable project archive (may change in pre-alpha)
- STEP import / AP242 STEP export — CAD interchange
- 3MF print export with appearance/material metadata, plus STL fallback

These capabilities describe the development branch that bundles this knowledge;
older release snapshots can predate them. Ideal assembly constraints and rendered
geometry leave physical strength and printable fit for coupons and analysis.

Related: [Export & print](export-print.md), [MCP harness](mcp-harness.md),
and the longer [proposed architecture](../../docs/proposed-architecture.md).
