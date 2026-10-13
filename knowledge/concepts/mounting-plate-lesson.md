---
type: Concept
title: Drill a mounting plate and edit its thickness
description: Locate four through holes, distinguish face coordinates from part coordinates, and recover a thickness edit.
status: draft
updated: 2026-10-11
topics: lessons, sketch, holes, parameters, recovery
keywords: mounting plate lesson, four holes, top face, through all, thickness edit, stale face, hole recovery
related_recipes: mounting-plate, fillet-basics
---

# Drill a mounting plate and edit its thickness

In **Scripts → Browse examples**, open **Four-hole mounting plate**. Opening
the source preserves your current design; **Run in new design** starts the
lesson in a separate blank design. Choose presentation mode and use Pause or
Step to read the two construction chapters. Maximum rate uses the same
commands and final checks.

## Locate the plate

The current part starts as a 60 × 40 mm rectangle on XY. Its fixed lower-left
corner is (−30, −20), so the sketch origin is at the centre. Width and height
dimensions control its shape; the fixed corner removes the remaining motion.
Before finishing, identify the origin and the four corners. A fully constrained
sketch has zero degrees of freedom; being close to the intended position is
not the same as being constrained.

The extrusion makes 5 mm stock. The hole-entry face is therefore at Z = 5 mm.
Pause after the stock appears and identify its top face and flat bottom.

## Drill from current geometry

The four bore centres are (−20, −10), (20, −10), (20, 10) and (−20, 10) mm in
the part's XY coordinates. Each centre is 10 mm from its two adjacent edges.
All holes have diameter 5 mm and **Through All** extent.

A face has its own origin and axes. The authored lesson projects each part
coordinate into the current face basis and resolves that face again after each
cut. When using the UI, select the current top face and inspect the preview;
an old face reference or a number copied from another face can locate a bore
incorrectly. The four-hole result still has its original sketch, extrusion and
four hole features in the editable history.

## Change one parameter

After completion, save a `.limo` copy. Edit the original extrusion's finite
distance from 5 to 8 mm. The intended result is 60 × 40 × 8 mm stock, with the
same four 5 mm holes passing through it. Thickness belongs to the extrusion;
hole diameter and centre position belong to the hole features. Inspect both
entry and exit faces, then restore 5 mm through the same feature edit.

For the original design, an independent volume check is
`60 × 40 × 5 − 4 × π × 2.5² × 5 = 12000 − 125π mm³`. This is a useful
geometry check, not a fastener-fit or load rating.

## Recover without replacing the part

If the entered thickness is valid but wrong, Undo that successful feature edit
outside sketch mode or reopen the same extrusion and restore its distance.
If a hole reports an unresolved face after an upstream change, inspect the
failing feature and reselect the intended current entry face. Preserve the
other holes and the sketch; replacing the body with an import discards the
lesson's editable construction.

For a rejected value or feature error, read the message and inspect the current
model before submitting another change. A rejected request and a successful
wrong edit require different recovery decisions. Save and reopen the native
project, then check that the original parameters can still be edited.

The [authored source](../../examples/scripts/mounting-plate.limo.jsonc) retains
its existing analytic and roundtrip acceptance. These exercises are teaching
prompts; human pacing/layout review and physical part qualification remain open.
