---
type: Concept
title: Shared reference geometry — surfaces that follow param changes
description: Prefer named shared references (planes, axes, sketches, faces) so related surfaces stay parallel/offset/coplanar when driving params change — not duplicated magic numbers.
status: draft
updated: 2026-09-20
topics: workflow, modeling, mcp, datums, parametric
keywords: shared reference geometry, named plane, named axis, offset from named plane, surfaces follow param change, coplanar, parallel offset, flush faces, concentric, construction plane, datum parent, stale numeric offset, JSONC reference first
---

# Shared reference geometry — surfaces that follow param changes

When two (or more) surfaces must stay related as the design evolves —
**parallel / offset / coplanar / flush / concentric** — drive them from the
**same named reference** (or an explicit offset/constraint from that
reference). Prefer that over two independent numeric offsets that go stale
when a driving param changes.

## Preferred packs

### Named shared references over duplicated numbers

Prefer **named shared references** as parents:

| Prefer | Examples |
|--------|----------|
| Named plane / axis / sketch / face | `mid_plane`, `motor_axis`, `seat_sketch`, `mount_face` |
| Explicit offset or constraint from that reference | face B = offset(`mid_plane`, 3 mm) |

Duplicated magic numbers (`extrude to Z=12` here and `plane at Z=12` there)
look fine until the height param moves and only one site updates.

### Same reference for related surfaces

When surfaces must stay related:

1. Create/name the **shared reference** first
2. Build each follower from that reference (or from a child that already cites it)
3. Change the driving param **once** on the reference (or its driver) so followers propagate

Leave the door open: **direct dimensions are OK** when faces are intentionally
independent (cosmetic offset, one-off clearance that should *not* track a
stack height).

### JSONC / history — reference first, children cite

In **`.limo.jsonc` / feature history**:

1. Create and **name** the reference entity first (plane, axis, sketch, face)
2. Children cite that **named entity** (or a stable id after inspect)
3. Param edits on the driver propagate; re-inspect ids after rebuilds

Align names with [geometry naming](geometry-naming.md) so script ↔ browser ↔
inspect share one vocabulary. Chunk scripts so the reference block stays
editable — [design VERSION / JSONC scripts](design-version-scripts.md).

Inspect between mutates ([MCP workflow](agent-mcp-workflow.md)).

### Assembly analog — locating schemes / DOF

At assembly level, the same idea is a **locating scheme**: one primary
reference, ordered secondary/tertiary roles, avoid fighting DOF — see
[locating schemes / DOF](../machine-design/concepts/locating-scheme-dof.md).
Part-level shared planes/axes and assembly-level locate features are siblings
in intent.

### Datums / sketch planes

Pick sketch and datum planes deliberately so followers have a stable parent —
[datum / sketch plane choice](../machine-design/concepts/datum-sketch-plane-choice.md).
Name those planes/faces when they are shared parents
([geometry naming](geometry-naming.md)).

Fit class and clearance still apply at mates — shared geometry does not
replace [fits & clearances](../machine-design/concepts/fits-clearances.md).

## VERIFY

After a driving **param change**:

1. `solid_scene` / document inspect — follower faces still match intent
   (parallel, offset distance, coplanar/flush, concentric)
2. Optional: `cad_compare_solids` / section views when the delta is subtle
3. Re-read topology ids after rebuilds — names and ids both matter
   ([MCP workflow](agent-mcp-workflow.md))

Pair visual claims with [validate before show](validate-before-show.md).

## Golden path checklist

1. Name the shared reference (plane / axis / sketch / face) before building
   followers.
2. Drive related surfaces from that reference (or an explicit offset from it)
   — not two independent magic numbers — unless independence is intentional.
3. In JSONC/history: reference first; children cite the named entity / stable
   id.
4. Assembly: map the same idea to locating-scheme / DOF roles.
5. VERIFY after param change that follower faces still match intent
   (`solid_scene` / compare / section).

Related: [geometry naming](geometry-naming.md),
[MCP workflow](agent-mcp-workflow.md),
[design VERSION / JSONC scripts](design-version-scripts.md),
[datum / sketch plane choice](../machine-design/concepts/datum-sketch-plane-choice.md),
[locating schemes / DOF](../machine-design/concepts/locating-scheme-dof.md),
[fits & clearances](../machine-design/concepts/fits-clearances.md).
