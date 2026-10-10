---
type: Concept
title: Research before commit
description: Local help-first gate — VERIFY table, datasheets, and mechanism class before freezing mating geometry.
status: draft
updated: 2026-09-19
topics: research, workflow, help, hardware
keywords: VERIFY table, datasheet, hardware envelope, mechanism class, freeze geometry
related_recipes: turbine-fit-coupons, d-screw-vise-fit
---

# Research before commit

Before freezing mating geometry, hardware pockets, snap class, or fit
numbers, run a short research pass so the VERIFY table and mechanism class
are in place.

## Golden path before freeze

Prefer a green VERIFY checklist (or an explicit waiver with reason) for pocket
dims, fastener patterns, bearing seats, and snap class.

## Order of retrieval

1. **Local MCP help first** — `cad_help` `search`, then `get` on returned ids.
   Caps: search default 5 / max 10, snippet ~280 chars, get 12 KiB, topics
   page 50.
2. **Full page only after selection** — `resources/read` on the selected
   `limo-cad://knowledge/...` URI when complete markdown is needed; resources are
   not the first search surface.
3. **Product docs / recipes** — fit coupons (`turbine-fit-coupons`,
   `d-screw-vise-fit`), agent workflow ([MCP workflow](agent-mcp-workflow.md)).
   Prefer a **blank** document when replaying recipes.
4. **Primary datasheets / catalogs** for purchased parts (bearing, fastener,
   motor) — record nominal + tolerance **source**.
5. **Web search** as escape hatch after local miss; cite URL + date. Prefer distill over pasting closed
   standards tables (ASME/ISO body text) into model notes as if they were yours.

## Capture a VERIFY table before modeling

Write a short design brief (chat or doc):

| Item | CAD default / guess | Measured or datasheet | Source | Status |
|------|---------------------|------------------------|--------|--------|
| Critical envelope dim | … | … | URL / KB id | OK / WAIVE |
| Fit role (radial vs diametral) | … | … | … | … |
| Mechanism class | … | … | … | … |
| Purchased part id | … | … | catalog | … |

Also record: function + constraints, print process, open questions.

## Commit rule

- Changing a VERIFY row by more than process tolerance ⇒ regenerate affected
  features; keep edits on the intended faces.
- If research flips mechanism class (tabs vs clips, press vs slip), scrap the
  wrong embodiment — prefer a clean redesign over a patch.

Related: [Requirements → BOM hygiene](../machine-design/concepts/design-hygiene-requirements-bom.md),
[Hardware pocket research](../machine-design/concepts/am-hardware-pocket-research.md).
