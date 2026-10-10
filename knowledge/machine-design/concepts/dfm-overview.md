---
type: Concept
title: DFM overview
description: Design-for-manufacturing mindset for CAD-time decisions; process selection before detail.
status: draft
updated: 2026-09-20
topics: dfm, dfa, manufacturing
keywords: DFM, DFA, DFMA, process selection, tolerance cost, standard parts, manufacturability, buy before invent
related_recipes: turbine-fit-coupons, d-screw-vise-fit, garden-bench, vertical-axis-turbine, d-screw-vise
sources: nwtc-guns-dfm, palni-dfma
---

# DFM overview

**Design for manufacturing (DFM)** means committing early to a process that can
make the geometry repeatably. **DFA** asks whether a person or robot can
grasp, orient, and join the parts without heroics. Tight tolerances and fancy
features that the process cannot hold are not “quality” — they are cost and
scrap.

**Attribution:** ideas adapted from Bryan Guns, NWTC LibreTexts
*[Design for Various Manufacturing Methods](https://eng.libretexts.org/Courses/Northeast_Wisconsin_Technical_College/Design_for_Various_Manufacturing_Methods)*
([Ch. 1](https://eng.libretexts.org/Courses/Northeast_Wisconsin_Technical_College/Design_for_Various_Manufacturing_Methods/01%3A_Design_for_Manufacturing_(DFM))),
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/);
and Gagnon & Bearman, *[Design for Manufacture and Assembly](https://pressbooks.palni.org/designmanufactureassembly/)* (PALNI),
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/).
Rewritten for Limo CAD help — not a chapter mirror. Prefer distill + link over copying Boothroyd
proprietary timing tables.

## CAD-time habits

- **Pick the process before the fillets.** Volume, material, and envelope drive
  CNC vs mold vs print vs sheet vs weld.
- **Buy before you invent.** Catalog fasteners and stock shapes beat one-off
  features when function allows ([fasteners](fasteners-joints.md)).
- **Merge parts only when motion, material, or service access allows.**
- **Make assembly boring.** Symmetry, chamfers that lead, and access for tools
  beat clever trapped fasteners.
- **Spend tolerance where function lives.** Use [GD&T](gdt-intro.md) for
  relationships that matter; leave the rest loose.
- **Design out secondary ops** when you can (extra grind, paint-critical seams,
  must-machine faces that could have been as-cast/as-printed).

## Process-selection checklist

1. **Volume & life** — one-off, dozens, thousands?
2. **Envelope & material** — size, temperature, chemicals.
3. **Critical features** — fits, sealing faces, optical, structural.
4. **Candidate processes** — shortlist 1–2; open
   [process guidelines](dfm-process-guidelines.md).
5. **Buy list** — fasteners, bearings, stock shapes named early.
6. **Coupon plan** — which joints need physical evidence first?
7. **Assembly story** — order, tools, rework path
   ([join choice](am-assembly-join-choice.md) for AM).

## Workflow

Concept → **process selection** → detail under that process’s rules →
coupon/prototype → feedback → release. Talk to manufacturing early; write down
trade-offs.

## In Limo CAD

Flagship recipes are manufacturing **candidates**. Replay and drawings are
software evidence. Fit coupons bridge to a specific printer and material
([fit coupons map](fit-coupons-recipes-map.md)).

## Further reading (link only)

- [UArk Jensen](https://uark.pressbooks.pub/mechanicaldesign/) — CC BY-NC
- [MIT OCW 2.008](https://ocw.mit.edu/courses/2-008-design-and-manufacturing-ii-spring-2025/) — CC BY-NC-SA

Related: [DFAM for FDM](dfam-fdm-overview.md), [Fits & clearances](fits-clearances.md), [Materials vocabulary](materials-vocabulary.md),
[SOURCES](../SOURCES.md).
