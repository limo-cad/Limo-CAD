---
type: Concept
title: AM clamshell retainer
description: Slide-fit first, then optional detents — clamp faces carry strength; detents only retain the clip.
status: draft
updated: 2026-09-21
topics: dfam, am, snap-fit, joints, fdm
keywords: clamshell retainer, slide fit, detent, clamp face, retainer clip, C-clip, frame retainer, removable slide, retention bump, clamshell teeth cutout, matching cutouts clip, clip tooth receptacle, tooth count match
related_recipes: turbine-fit-coupons
sources: nwtc-guns-dfm, doe-3d
---

# AM clamshell retainer (slide first, detent second)

Use when an FDM/AM **retainer** holds two clamshell halves together. Prefer the simple clamp loop before inventing
snap geometry without this order.

## Print orientation

- Print the retainer **on its side** so layer lines run with the clamp load
  (stronger against the halves separating) — see
  [AM thin walls / anisotropy](am-thin-walls.md).
- Prefer a simple **rectangle with a center cutout** (channel / frame), not a
  decorative C or ornamental hooks, until the slide fit is proven.

## Function split (prefer separate roles)

1. **Clamp / strength** — faces that keep the two halves from separating. Sized
   for a clean removable **slide-in**.
2. **Retention** — small **detents on the inserted faces** only, added *after*
   the sliding fit is confirmed. Detents keep the clip from falling out; they
   are **not** the load path.

The surface that carries strength is not the surface that provides snap retention.
Friction detents without a distinct hook still belong in the retention role —
see [AM snap-fits](am-snap-fit.md) mechanism classes.

## Mating halves (clip teeth ↔ matching cutouts)

Prefer a **tooth ↔ receptacle** pair on every mating half before locking tooth
count or clip length.

1. **Matching cutouts** — every clip tooth / retention bump needs a matching
   receptacle or cutout in the mating half (same pitch and depth intent).
2. **Tooth count** — middle and end tooth count on the clip must match the
   cutout pattern on the halves.
3. **Envelope span** — when the retainer spans both halves, lengthen and center
   the clip on the **larger body envelope** so teeth land in their cutouts.

Prefer this page for retainer + matching cutout roles; cantilever barbs and deep
seats stay on [AM snap-fits](am-snap-fit.md).

## Order of work

1. Model a perfect removable **sliding** shape against the real pockets (dims,
   clearances, tolerances, mating surfaces).
2. Validate: slide in / slide out by hand intent, no overlap on the clamp
   faces, printable without supports in the side orientation.
3. Only then add **tiny detent** details on the insert faces.
4. Prefer a simple retainer while iterating; leave complex barbs, I-beams, or load-bearing snaps for a later pass
   after the slide fit.

## Engineering checklist

- [ ] Nominal + clearance / press **roles** called out (radial vs diametral)
      — [fits & clearances](fits-clearances.md)
- [ ] Mating surface finish / print orientation noted
- [ ] Lifetime: abrasion and plastic set on **detents**, not on the clamp faces
- [ ] Kid-removable when required: soft detent force; clamp still structural
- [ ] Locators separate from retainer — [alignment nubs vs pins](alignment-nubs-pins.md)
- [ ] Every clip tooth has a matching cutout / receptacle; tooth count matches
- [ ] Shot pack includes section through clamp + detent
      ([validate before show](../../concepts/validate-before-show.md))

## CAD / knowledge

Prefer local `cad_help` / `limo-cad://knowledge/...` before inventing geometry
([research before commit](../../concepts/research-before-commit.md)). Prove the
joint with section/inner shots and
[adversarial mesh audit](../../concepts/adversarial-mesh-audit.md) before export.
Inspect between mutates with `solid_scene`
([MCP workflow](../../concepts/agent-mcp-workflow.md)).
