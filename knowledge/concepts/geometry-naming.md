---
type: Concept
title: Geometry naming — bodies, features, faces, scripts, STEP
description: Prefer role nouns and one shared vocabulary across project browser, JSONC scripts, and STEP/export so humans and agents can inspect, cite, and hand off complex models.
status: draft
updated: 2026-09-20
topics: workflow, modeling, mcp, export, sessions
keywords: geometry naming, body name, face name, edge name, datum name, set_name, display name, STEP names, JSONC names, browser tree, servo_cradle, clamshell_a, lid, stable labels, shared reference
---

# Geometry naming — bodies, features, faces, scripts, STEP

Always name bodies, surfaces, and other geometry so users and agents can
understand a complex **project file**, **STEP file**, and especially **script
files** (JSONC). Prefer one vocabulary that survives inspect, edit, compare,
and handoff.

## Preferred packs

### Bodies — role nouns

Name **bodies** with role nouns the design story already uses:

| Prefer | Examples |
|--------|----------|
| Role noun (snake_case or product convention) | `servo_cradle`, `clamshell_a`, `lid`, `bearing_cap`, `base_plate` |

Anonymous defaults (`Body1`, `Solid2`) leave agents and reviewers without a
citeable handle. Role names make `solid_scene` / browser / STEP the same map.

### Features / history nodes

Name **features and history nodes** in the browser tree with the **same
vocabulary** as the script steps that create them. If the JSONC section or
command builds `servo_cradle`, the extrude / combine / fillet node that
produces it should read as that role — not a disconnected `Extrude12`.

### Faces, edges, datums (when critical)

Name **faces / edges / datums** when they are mating, datum, or
export-critical. Stable labels let agents cite topology after inspect
(`solid_scene`, document inspect) without guessing which Face id is the
seat or split plane.

### JSONC scripts

In **`.limo.jsonc` scripts**:

- Put the **same names** in command arguments, comments, and `set_name` (or
  equivalent) ops the product supports
- Keep **`VERSION`** + **section headers** so named geometry is easy to find
- **Chunk** scripts (or clear sections) so named bodies/features are editable
  in small hunks — see [design VERSION / JSONC scripts](design-version-scripts.md)

### STEP / export

Prefer exports that **preserve display names** when the format allows. Treat
names as part of the handoff package alongside mesh / print checks
([export and print](export-print.md), [adversarial mesh audit](adversarial-mesh-audit.md)).

### One vocabulary across surfaces

Align **script names ↔ project browser ↔ STEP** so compare / diff / inspect
stays one vocabulary. Agents cite the role noun; humans see the same label in
the tree and in the interchange file.

## VERIFY

After each mutate that creates or renames geometry:

1. Call `solid_scene` / `cad_document` (or document inspect)
2. Confirm the **intended names** appear before the next write
3. Re-read topology ids after rebuilds — names and ids both matter
   ([MCP workflow](agent-mcp-workflow.md))

Pair visual “looks good” claims with
[validate before show](validate-before-show.md).

## Golden path checklist

1. Plan role nouns for bodies (and critical faces/edges/datums) before or as
   you model.
2. Mirror those names in JSONC args / comments / `set_name` ops and in
   browser feature nodes.
3. VERIFY names via `solid_scene` / `cad_document` before the next mutate.
4. Prefer name-preserving STEP/export; include names in the handoff story
   with mesh/print preflight.
5. Keep script ↔ browser ↔ STEP vocabulary aligned when cutting a revision.

Related: [MCP workflow](agent-mcp-workflow.md) (inspect between mutates),
[design VERSION / JSONC scripts](design-version-scripts.md) (JSONC chunks),
[shared reference geometry](shared-reference-geometry.md) (named parents so
followers track param changes),
[export and print](export-print.md),
[validate before show](validate-before-show.md).
