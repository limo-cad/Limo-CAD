---
type: Concept
title: Edit a shared component and recover a wrong dimension
description: Edit a selected occurrence in place, undo an incorrect driving value and save the shared definition.
status: draft
updated: 2026-10-06
topics: assembly, sketch, dimensions, undo, mcp
keywords: in place editing, selected occurrence, shared definition, driving dimension, wrong dimension, sketch undo, rotated part, local coordinates
related_recipes: component-edit-recovery, repeated-bracket-assembly
---

# Edit a shared component and recover a wrong dimension

Open **Scripts**, select **Edit a shared part in place and recover a wrong
dimension**, and choose **Run in new design**. Pause or Step at the chapter notes
to inspect the sketch, the rotated repeat and the changed dimension. Maximum
rate executes the same source and final checks without presentation waits.

An occurrence is a placed reference to a component definition. Select the part
occurrence, then edit its driving sketch. Other occurrences fade while that
part remains opaque. The camera, picking and dimension annotations use the
selected placement; the sketch solver and saved sources keep definition-local
coordinates. Finishing and recomputing updates every occurrence that shares the
definition, including its joint references.

If an entered driving value is valid but wrong, use sketch **Undo** before
finishing. It restores the previous solved geometry and dimension values.
Enter the intended expression, finish, and recompute. A failed expression or
constraint reports an error; inspect it before submitting another change.
Undo during an active sketch applies to that sketch. Document history outside
the sketch remains a separate control.

The lesson grounds the rotated occurrence. Grounding selects one root occurrence;
the original remains free at its authored placement. The final checks require
that specific warning. Use the repeated bracket assembly lesson to practice
constrained mating references; a solved display alone does not establish that
every component is constrained.

Save the `.limo` project, reopen it, select the repeated occurrence and re-enter
the sketch. Its local driving dimension and shared placements remain editable.
Keep the `.limo.jsonc` source to explain and reproduce the construction.

Agents use the same operations through `cad_interface` execute or ordinary MCP
tools. `sketch_edit` takes `name` and optional `occurrence_id`.
`sketch_edit_dimension` takes `constraint_id` and `text`; `sketch_undo` restores
the preceding sketch command. Finish the active component edit before changing
assembly placement or structure. Choosing an unrelated or missing occurrence
rejects before opening the sketch. Editing the definition changes all its
occurrences; it does not make the selected occurrence independent.

This lesson teaches native editing and recovery. It does not qualify a physical
part, fastener fit or load capacity. Linked external component files remain a
separate workflow.
