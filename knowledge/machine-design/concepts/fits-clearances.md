---
type: Concept
title: Fits and clearances — clearance, transition, and interference fits
description: Clearance fit, transition fit, and interference fit (press fit) as hole/shaft design intent; distinct from assembly interference checks.
status: draft
updated: 2026-09-20
topics: fits, gdt, manufacturing, clearances
keywords: clearance fit, transition fit, interference fit, press fit, allowance, radial clearance, coupon
related_recipes: turbine-fit-coupons, d-screw-vise-fit
sources: nist-gdt-2, iso-286, asme-b4
---

# Fits and clearances — clearance, transition, and interference fits

A **fit** is the designed relationship between two mating features of size
(typically **hole and shaft**). Choose a **fit class** from **function**,
**process capability**, and whether the joint should slide, locate, or press.

This page is about **clearance fit / transition fit / interference fit**
intent — **not** multi-body overlap probes. For geometric collisions at solved
poses, use [assembly interference check](../../concepts/assembly-interference.md).

Teaching rewrite informed by NIST / Berez GD&T Part II limits-and-fits review
([Zenodo](https://zenodo.org/records/8237278), **CC BY 4.0**). This page is
**not** ISO 286 or ASME B4.x.

## Three classes (say the class out loud)

| Class | Intent | Everyday names |
|-------|--------|----------------|
| **Clearance fit** | Always a gap (running or sliding). | running fit, sliding fit, free fit |
| **Transition fit** | May be slight clearance or slight interference; usually locates rather than heavy press. | locational fit, snug fit |
| **Interference fit** | Always overlap; press, shrink, or freeze assembly. | press fit, force fit, shrink fit |

Say whether a number is **radial** or **diametral**. Mixing those is a common
print-to-part bug. Process matters: a reamed hole and an FDM hole leave different
clearances even at the same nominal — pick the allowance for the process pair.

## Checklist before locking a fit class

1. **Name the function** — rotate freely, locate once, or transmit torque by
   friction/press?
2. **Name the process pair** — CNC ream + ground shaft ≠ FDM hole + printed pin.
3. **Write radial vs diametral** next to every clearance number.
4. **Separate locators from clamps** — pins locate; screws clamp
   ([locating schemes](locating-scheme-dof.md)).
5. **Coupon the critical joint** before flagship commit
   (`turbine-fit-coupons`, `d-screw-vise-fit`) — see
   [fit coupons map](fit-coupons-recipes-map.md).
6. **Keep distinct** an **interference fit** (press class) versus an
   **assembly interference** report (bodies overlapping in space).

## Preferred-fit designations (link-out only)

Standards publish **preferred fits** (ISO hole-basis pairs such as `H7/g6`, or
ANSI/ASME B4 inch families). NIST Part II summarizes ASME B4.2-era
**purpose language** for teaching — that is **not** redistributed here as a
selection table.

For contractual designations and deviation tables, open:

- [ISO 286](https://www.iso.org/) / GPS fits documentation (purchase)
- [ASME B4.x](https://www.asme.org/codes-standards) (purchase)
- NIST Part II deck (CC BY) as a teaching companion — not a substitute

Prefer purchased ISO/ASME charts for official fits; treat in-app help lists as teaching distill.

## AM / FDM habits

- Printed holes shrink; treat first articles as process evidence, not a chart.
- Prefer a printed **clearance fit** coupon beside the part over guessing
  “0.2 mm all around.”
- Press-fit plastic onto metal is a different system than metal-on-metal —
  measure both after cool-down ([AM thin walls](am-thin-walls.md)).

## In Limo CAD

Recipes `turbine-fit-coupons` and `d-screw-vise-fit` print dimensioned
specimens of the **actual** mating geometry before committing the flagship.
Coupon results are printer/material evidence, not a universal fit table.
Software replay ≠ physical qualification.

Related: [GD&T intro](gdt-intro.md), [DFM overview](dfm-overview.md),
[DFM process guidelines](dfm-process-guidelines.md),
[tolerance stack-up](tolerance-stackup-intro.md),
[bearings / hubs / seats](bearings-hubs-seats.md),
[assembly interference check](../../concepts/assembly-interference.md).
