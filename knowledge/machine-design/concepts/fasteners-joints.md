---
type: Concept
title: Fasteners and joints
description: Purchased threaded joints — preload / clamp load, torque as install method vs proof, friction/lube sensitivity, grip/engagement, and VERIFY to datasheet — NASA RP-1228 golden path (no invented torque charts).
status: draft
updated: 2026-09-20
topics: fasteners, joints, machine-elements, hardware
keywords: bolt, preload, clamp load, torque, friction, lubricant, nut factor, K factor, proof load, thread, washer, locking, NASA RP-1228, screw joint, engagement, grip length, purchased hardware, BOM fastener, running torque
related_recipes: d-screw-vise, garden-bench, mounting-plate
sources: nasa-fastener
---

# Fasteners and joints

Most assemblies are held by **purchased fasteners**, not by modeled threads
alone. Call out the real hardware (size, grade/class, length, head, finish,
locking) on the drawing or BOM, and keep CAD threads as modeled geometry plus a
specification.

CAD owns **hole roles**, **grip stack**, and **named BOM lines**. Handbooks and
vendor datasheets own **preload targets**, **torque values**, and strength
allowables. Prefer a cited install method over inventing charts in Help.

**Attribution:** golden-path joint hygiene distilled from NASA Fastener Design
Manual RP-1228 (public domain — `nasa-fastener` in [SOURCES](../SOURCES.md)).
Numeric torque/preload tables stay **link-out / datasheet** — not reproduced
here.

## Vocabulary

| Term | Meaning |
|------|---------|
| **Preload (Fi)** | Initial tension put into the bolt at assembly |
| **Clamp load (Fc)** | Compression on the joint faces; equals Fi before external load |
| **External / working load (Fe)** | Service tension (or opening tendency) the joint must survive |
| **Grip** | Clamped thickness the shank spans (parts + washers) |
| **Engagement** | Female thread length that carries the male thread |
| **Torque** | Install *method* that approximates preload via friction — not a proof of Fi |
| **Nut factor / K** | Torque coefficient in *T ≈ K·F·d*; friction-dominated — VERIFY, do not invent |
| **Proof load** | Literature install ceiling (often ~¾ of theoretical yield) to leave margin for torque scatter |
| **Running torque** | Extra torque from a locking feature before clamp develops — add when the datasheet says so |

Modeled ISO/UN holes in Limo CAD are geometry aids. Prefer handbook/coupon
strength ratings; treat CAD as geometry, not a load certificate.

## Golden path (CAD-time)

1. **Name the purchased part** — size, grade/class, length, head, finish,
   locking, washer, nut/insert. Prefer
   [research before commit](../../concepts/research-before-commit.md) before
   freezing holes.
2. **Hole role** — clearance, counterbore, tap, or insert pilot
   ([fastener clearance & counterbore](fastener-clearance-counterbore.md)).
3. **Grip stack** — clamped thickness + washers; choose length so the nut/insert
   has full engagement and the shank (not threads) carries shear in the joint
   plane. Washers can tune grip when exact lengths are coarse (NASA grip
   practice).
4. **Preload intent** — decide that the joint must **stay closed** under working
   load. Preload clamps the faces so Fe does not open the joint; when the joint
   is much stiffer than the bolt, most of Fe goes into unloading the clamp, not
   into cycling the bolt (NASA joint-stiffness story). High preload also shrinks
   the alternating load the bolt sees in fatigue.
5. **Torque is install, not proof** — a torque wrench (or turn-of-nut, stretch,
   ultrasonic, load-indicating washer) is how you *approximate* Fi on the bench.
   It does **not** certify clamp load by itself. NASA lists friction under the
   head/nut, friction in the threads, coatings/lubricants, target % of strength,
   structure-vs-bolt stiffness, and locking running torque as the variables that
   dominate scatter.
6. **Friction / lube sensitivity** — the same fastener dry vs oiled/waxed/plated
   needs **different** install torque for the same Fi. Prefer one finish+lube
   story on the BOM and use the **matching** datasheet or NASA RP-1228 method;
   do not mix a dry table with a lubricated joint. *T ≈ K·F·d* is a role reminder
   that *K* moves with μ — not a license to invent *K* charts in CAD notes.
7. **Locate vs clamp** — pins/nubs locate; screws clamp
   ([alignment nubs](alignment-nubs-pins.md),
   [locating schemes](locating-scheme-dof.md)). Avoid prying/bending on the bolt.
8. **Locking choice** — prevailing torque, chemical, or mechanical — named for
   vibration/thermal cycles; include running torque in the install method when
   required.
9. **AM path** — heat-set insert vs hex trap vs printed thread
   ([heat-set inserts](am-heat-set-inserts.md),
   [captive nut trap](captive-nut-hex-trap.md),
   [cosmetic threads](cosmetic-threads-vs-clearance.md)).
10. **VERIFY gate** — torque/tension numbers, proof load, and engagement rules
    come from the **hardware datasheet**, a cited handbook method, or NASA
    RP-1228 — never from invented Help charts. Confirm finish/lube/locking match
    the cited table. Prefer tension or stretch methods when clamp load is
    critical.

## Prefer these patterns

| Need | Prefer |
|------|--------|
| Keep joint closed under Fe | Named preload intent + stiff clamp path; redesign if joint opens |
| Set install torque | Vendor / NASA RP-1228 table for **this** finish & lube — cite, do not invent |
| Critical clamp load | Tension, stretch, or load-indicating method over wrench-feel alone |
| Shear in the joint plane | Grip so threads are out of bearing; washers to tune length |
| Vibrating / thermal duty | Named locking + re-torque/inspect policy after first cycle |
| Soft / plastic female thread | Insert or nut with vendor engagement length |

## Hole and insert roles

| Need | Page |
|------|------|
| Clearance / counterbore / tap vs insert | [Fastener clearance & counterbore](fastener-clearance-counterbore.md) |
| FDM bosses and crush ribs | [AM heat-set inserts](am-heat-set-inserts.md) |
| Hex nut traps / captive nuts | [Captive nut and hex nut trap](captive-nut-hex-trap.md) |
| Cosmetic CAD threads vs real holes | [Cosmetic threads vs modeled clearance](cosmetic-threads-vs-clearance.md) |
| Hole feature vs sketch pattern | [Hole wizard vs modeled hole](hole-wizard-vs-modeled.md) |

## Checklist before locking hardware

1. **BOM line** — size, grade/class, length, head, finish, locking — not “M3 screw.”
2. **Hole role** frozen ([clearance & counterbore](fastener-clearance-counterbore.md)).
3. **Grip + engagement** — full nut/insert engagement; no blind bottoming.
4. **Preload / clamp intent** named; joint-open under load → redesign, not only a longer bolt.
5. **Install method** — torque (with finish/lube), turn-of-nut, or tension — and who verifies.
6. **Locate vs clamp** separated from fasteners.
7. **Access** — tool clearance, one-side assembly, captive hardware if needed.
8. **VERIFY table** for vendor envelope and torque/tension
   ([research before commit](../../concepts/research-before-commit.md)).

## In this product

`d-screw-vise` uses an interrupted helical screw, wear nut, and purchased M3
keepers. `garden-bench` is timber stock plus fasteners as open inputs.
`mounting-plate` exercises clearance patterns. Keep hardware as named purchased
parts.

## Further reading (link only)

- NASA Fastener Design Manual RP-1228 (`nasa-fastener`) — preload, torque
  derivation, lubricants, grip, inserts: https://ntrs.nasa.gov/citations/19900009424
- Hole roles: [fastener clearance & counterbore](fastener-clearance-counterbore.md)

Related: [Materials vocabulary](materials-vocabulary.md),
[AM assembly join choice](am-assembly-join-choice.md),
[springs and couplings](springs-couplings.md),
[drawing vs MBD / PMI](drawing-vs-mbd-pmi.md),
[taxonomy](../taxonomy.md).
