---
type: Concept
title: Constrain an angle bracket and recover a dimension edit
description: Separate location, shape and extrusion width in a six-edge L section, then edit one driving dimension.
status: draft
updated: 2026-10-11
topics: lessons, sketch, constraints, dimensions, recovery
keywords: angle bracket lesson, L section, horizontal vertical, driving dimension, overconstraint, closed profile, dimension recovery
related_recipes: angle-bracket, component-edit-recovery
---

# Constrain an angle bracket and recover a dimension edit

In **Scripts → Browse examples**, open **Dimensioned angle bracket** and use
**Run in new design**. Use presentation mode and Pause or Step at the chapters
to inspect the current section before it becomes a solid. Maximum rate builds
the same part and runs the same final checks.

## Constrain the section

The six edges trace a closed L through (0, 0), (40, 0), (40, 5), (5, 5),
(5, 30), (0, 30), then back to (0, 0) mm. Coincident endpoints close the
profile; horizontal and vertical constraints make its corners square. Fixing
the origin corner locates the section without freezing every edge.

Four driving lengths set the 40 × 30 mm outer envelope and the two 5 mm
walls. Pause at the completed sketch and distinguish these dimensions from
the unconstrained positions used when the lines were first drawn. A shape
that looks square can still move or change shape until its constraints remove
the remaining degrees of freedom.

## Give the bracket width

The extrusion is 20 mm perpendicular to the sketch. The 40 and 30 mm leg
lengths, 5 mm wall dimensions and 20 mm extrusion width describe different
directions. The finished part retains the driving section and its extrusion.
Inspect the inner corner and both flat ends in the framed solid view.

If retained sketch lines obscure the solid, **Reference → Construction** hides
finished references and can show them again. Keep the active sketch visible
while explaining its constraints. Visibility is presentation state; removing
the sketch from history would remove the design's driving construction.

## Edit the existing driver

Save a `.limo` copy. Open **Bracket / dimensioned L section** from the history
and change its 40 mm outer leg dimension to 45 mm. Keep the other outer leg
at 30 mm and both wall dimensions at 5 mm. Finish and recompute; the intended
part has a 45 × 30 mm section and the original 20 mm width. Restore the same
driver to 40 mm rather than adding another dimension to force the old shape.

An independent check for the original section is
`40 × 5 + (30 − 5) × 5 = 325 mm²`; its volume is `325 × 20 = 6500 mm³`.
For the proposed 45 mm edit, those values become 350 mm² and 7000 mm³.
The overlap at the inside corner is counted once.

## Recover a constraint or profile failure

If a new dimension conflicts with an existing driver, inspect the reported
constraints and edit the original driving value. Adding several competing
dimensions makes the cause harder to identify. Sketch Undo restores a
successful wrong edit while the sketch is active; document Undo outside
sketch mode affects the feature history.

If Extrude cannot find a closed profile, return to the section and inspect the
six endpoint connections. A tiny gap is not repaired by increasing extrusion
distance. After correcting the specific connection, finish and inspect the
feature result. Read each rejection and inspect the current model before
retrying; do not replace the section with an imported solid to conceal it.

Save and reopen the `.limo` project, then re-enter the driving sketch. The
[authored source](../../examples/scripts/angle-bracket.limo.jsonc) retains its
analytic and roundtrip checks. Teaching layout/pacing and the bracket's
physical load capacity require separate review.
