---
type: Concept
title: Revolve an annular spacer and edit its length
description: Read a radial section, choose its axis, and preserve the bore while editing the spacer length.
status: draft
updated: 2026-10-11
topics: lessons, sketch, revolve, parameters, recovery
keywords: revolved spacer lesson, annulus, radial section, revolve axis, bore radius, length edit, revolve recovery
related_recipes: revolved-spacer
---

# Revolve an annular spacer and edit its length

In **Scripts → Browse examples**, open **Revolved annular spacer**, then use
**Run in new design**. Presentation mode provides chapter captions and camera
framing; Pause or Step when reading the section. Opening a source does not run
it, and maximum rate performs the same construction and checks without waits.

## Read the radial section

The current sketch is on XY. Its rectangle extends from X = 5 to X = 10 mm,
and from Y = 0 to Y = 12 mm. The fixed corner locates the section; its width
and height dimensions remove the remaining motion. The Y axis at X = 0 is
the future centreline, outside the rectangular material.

Distance from that axis is a **radius**: the inner edge at 5 mm gives a 10 mm
bore diameter, and the outer edge at 10 mm gives a 20 mm outside diameter.
The rectangle's 5 mm width is the radial wall thickness. Its 12 mm height is
the spacer's axial length. Pause before finishing and identify these three
different quantities in the sketch.

## Turn the section around its axis

The Revolve uses an axis origin of (0, 0), a direction along sketch Y and a
360° angle. One full turn produces one annular solid with an open bore. A
partial angle makes a sector; selecting an axis through the rectangular
material changes the intended geometry.

After the solid appears, inspect the opening from both ends. The retained
sketch and Revolve are still editable. **Reference → Construction** can hide
finished references for a clearer solid view, then show them again. An active
unfinished sketch remains visible; do not hide unfinished work to make a
lesson appear complete.

## Change length while keeping the radial design

Save a `.limo` copy, open **Spacer / bore radius and length section** from
the history and edit its 12 mm driving length to 16 mm. Keep the fixed corner,
5 mm radial width and Y revolution axis. Finish and recompute. The intended
result has a 10 mm bore, 20 mm outside diameter and 16 mm axial length.
Restore the length to 12 mm through the same sketch edit.

The reference volume is `π × (10² − 5²) × 12 = 900π mm³`; after the proposed
16 mm edit it is `1200π mm³`. These independent numbers distinguish a length
change from accidentally filling the bore or changing the radial dimensions.

## Recover an axis or dimension mistake

If a driving length was accepted but wrong, sketch Undo restores the preceding
sketch edit while the sketch is active. Outside sketch mode, use document
history Undo or edit the same feature again. For a rejected edit, inspect the
message and the existing constraints before adding another dimension.

For a wrong revolution, reopen the original Revolve and compare its axis and
angle with the radial section. Restore the intended axis instead of making a
second overlapping body. Save and reopen the native project, then confirm that
the section dimensions and revolution remain editable.

The [authored source](../../examples/scripts/revolved-spacer.limo.jsonc) keeps
its analytic and roundtrip checks. The guide describes intended practice;
human layout/pacing review and a manufactured spacer's fit remain separate.
