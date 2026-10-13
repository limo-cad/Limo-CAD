# Sketch constraint pairwise permutation matrix

**Recorded:** 2026-08-29
**Executable source:**
[`crates/sketch/tests/constraint_pair_permutations.rs`](../crates/sketch/tests/constraint_pair_permutations.rs)

## Purpose

Constraint correctness is order-dependent. Testing `A` and `B` independently
does not prove that `A → B` behaves like `B → A`, that both equations remain
true, or that the second command avoids changing a property it does not own.
This matrix therefore treats every ordered pair as a separate public behavior.

The executable test is authoritative. This document records the matrix
definition, required invariants, expected exceptional cases, and reproduction
commands without duplicating a 625-cell result snapshot that could become
stale when a formerly rejected combination gains a correct solution.

## Operation vector

The same ordered vector supplies both the rows (first command) and columns
(second command).

### Geometric relationships — 16 operations

| ID | Operation | Selection form |
|---|---|---|
| G01 | Horizontal | line |
| G02 | Vertical | line |
| G03 | Horizontal points | two points |
| G04 | Vertical points | two points |
| G05 | Coincident | point and carrier |
| G06 | Tangent | line and circle |
| G07 | Equal lines | two lines |
| G08 | Equal curves | two circles/arcs |
| G09 | Parallel | two lines |
| G10 | Perpendicular | two lines |
| G11 | Fix | line/current pose |
| G12 | Midpoint | point and line |
| G13 | Concentric | two circles/arcs |
| G14 | Collinear | two lines |
| G15 | Symmetry points | two points and an axis |
| G16 | Symmetry lines | two lines and an axis |

### Dimensional relationships — 9 operations

| ID | Operation | Measured property |
|---|---|---|
| D01 | Line length | line length |
| D02 | Point distance | distance between two points |
| D03 | Point-line distance | perpendicular separation |
| D04 | Line offset | signed line-line separation |
| D05 | Curve offset | radial separation |
| D06 | Radius | circle/arc radius |
| D07 | Diameter | circle/arc diameter |
| D08 | Angle | angle between two lines |
| D09 | Axis angle | line angle from the sketch axis |

## Complete ordered matrix

Let `G = [G01 … G16]`, `D = [D01 … D09]`, and `O = G + D`. The matrix is the
Cartesian square `O × O`; rows are the first command and columns are the
second command.

| First ↓ / Second → | Geometric `G` (16) | Dimensional `D` (9) | Row total |
|---|---:|---:|---:|
| Geometric `G` (16) | 256 relationship/relationship | 144 relationship/dimension | 400 |
| Dimensional `D` (9) | 144 dimension/relationship | 81 dimension/dimension | 225 |
| Column total | 400 | 225 | **625** |

Every one of those 625 cells is repeated on two disconnected sketch islands.
That second matrix proves that solving the current island does not move an
already-solved, unrelated island. Total ordered pair scenarios: **1,250**.

### Higher-order dependency circuits

The pairwise matrix is exhaustive for two-command permutations, but it cannot
detect a relation implied only by two or more existing relations. For example,
`A ∥ B` followed by `B ⟂ C` already determines `A ⟂ C`; all three individual
relations are valid, but accepting the closing relation would make the graph
redundant.

The rank-based admission policy and the representative direction, equality,
incidence, tangency, center, symmetry, anchoring, and dimension circuits are
recorded in
[`SKETCH_OVERCONSTRAINT_HARDENING.md`](SKETCH_OVERCONSTRAINT_HARDENING.md)
and executed by
[`crates/sketch/tests/overconstraint_hardening.rs`](../crates/sketch/tests/overconstraint_hardening.rs).

The seven public dimension-selection forms also run through the command API in
both relation-first and dimension-first order: line length, point distance,
point-line distance, line angle, parallel-line offset, circle diameter, and
arc radius.

## Required result for every cell

The second command has only two valid outcomes:

1. **Exact success**
   - the first equation still holds;
   - the second equation holds;
   - all coordinates remain finite and bounded;
   - the second command respects its operation-ownership contract; and
   - unrelated geometry is unchanged.
2. **Atomic rejection**
   - entity geometry is byte-for-byte unchanged;
   - the constraint graph is byte-for-byte unchanged; and
   - the first equation still holds.

A solver report of convergence is insufficient by itself. The matrix also
checks the resulting geometry and retained properties.

## Operation-ownership contract

| Command family | May change | Must retain unless an earlier equation mathematically determines it |
|---|---|---|
| Horizontal, Vertical, Parallel, Perpendicular, Angle | direction | selected lengths |
| Horizontal/Vertical points | point-pair direction | pair distance |
| Coincident, Midpoint, point-line distance | placement | carrier size |
| Tangent | contact placement/direction | participating sizes |
| Equal | target size | reference size and line bearings |
| Length, point distance | measured size/separation | unmeasured bearing |
| Line/curve offset | measured separation | participating sizes |
| Radius/Diameter | selected curve size | center and unrelated arc state |
| Concentric, Collinear | relative placement | participating sizes |
| Symmetry | mirrored placement | datum-axis pose and reference size |
| Fix | nothing during application | complete captured pose |

Temporary numerical stays enforce this application behavior only while the
new command is fitted. They are not persisted as hidden constraints and do not
reduce reported degrees of freedom.

## Deliberate exceptions and contradictions

- Horizontal-points plus Vertical-points on the same pair makes the two points
  coincident, so their former nonzero separation cannot also be retained.
- A point fixed to a line midpoint plus an explicit distance to one endpoint
  can mathematically determine the carrier length.
- A point at a line midpoint cannot simultaneously have nonzero perpendicular
  distance from that same line; the second command must be rejected.
- Parallel and Perpendicular on the same line pair are contradictory.
- Radius and Diameter are two presentations of the same radial measurement;
  an equivalent second driver is rejected or represented as a reference
  dimension through the dimension workflow.

## Important order reversals

These named pairs remain as focused regressions in addition to the full loop:

| First | Second | Expected ownership behavior |
|---|---|---|
| Parallel | Equal | Equal changes target length; bearings stay parallel |
| Equal | Parallel | Parallel changes direction; both equal lengths remain |
| Perpendicular | Equal | Equal changes target length; the 90° relation remains |
| Equal | Perpendicular | Perpendicular changes direction; equal lengths remain |
| Angle | Equal | Equal changes target length; the angle remains |
| Equal | Angle | Angle changes direction; equal lengths remain |
| Horizontal | Line length | length changes; horizontal bearing remains |
| Line length | Horizontal | bearing changes; dimensioned length remains |

## Implemented selective automatic-constraint policy

Automatic constraints now record only high-confidence intent established by
the active creation gesture. The implemented inference set is deliberately
small:

- exact existing-point and curve-intersection acquisition creates an explicit
  associative coincidence;
- exact origin acquisition creates an explicit origin-coincident relation;
- midpoint acquisition creates an explicit midpoint relation;
- circle and arc center acquisition creates an explicit center-coincident
  relation;
- a new line within 3° of a sketch axis creates Horizontal or Vertical;
- two connected lines within 3° of a right angle create Perpendicular;
- an arc endpoint placed on an existing point remains associated with that
  point; and
- an arc endpoint drawn from a topologically connected line within 3° of
  tangency creates Tangent.

Each inferred persistent relation is rendered using the same selectable and
removable indication as a manually applied relation. An inferred relation is
committed only when the solver converges to the exact relation, its residual is
within tolerance, and the shared admission check finds independent information:
normally a rank increase, or a finite-motion witness at a singular pose (see the
hardening document above). A failed or redundant inference leaves the authored
geometry intact.

Holding Control on Windows/Linux or Command on macOS suppresses relation
inference for the current gesture. Grid quantization remains a separate user
setting. Suppression does not create duplicate vertices: an endpoint that is
already at exactly the same coordinates as an existing vertex reuses that
topology without adding a persistent inferred relation.

Grid quantization is magnetic, not unconditional: the nearest adaptive minor
grid intersection captures only inside a small screen-space radius. Away from
that radius, continuous cursor coordinates remain available. Grid acquisition
never adds a relation. Point-axis and feature-extension tracking likewise
remain dotted, operation-local guides; they can establish an exact placement
for the current gesture but do not persist Horizontal/Vertical point-pair
relations.

For two-line direction operations, the first selection is the stable direction
reference. The second selection is the follower: its length is retained and it
rotates about a shared endpoint, otherwise its most connected endpoint, with a
midpoint fallback for disconnected geometry. The focused three-line regression
asserts that applying Parallel to the top and bottom carriers changes only the
top carrier's direction; the reference line, shared upright, and all three
lengths remain unchanged.

Automatic **Fix** is different and should not be the default. Fix locks size
and location, masks under-constrained design intent, and easily turns later
dimensions into conflicts. A stable sketch should instead use:

1. explicit coincidence to the origin or another chosen datum;
2. inferred geometric relations that match the drawing gesture;
3. driving dimensions for remaining size and position; and
4. explicit Fix only when the user intentionally wants frozen geometry.

No dimension and no Fix relation is created automatically. For numerical
stability, the solver may use temporary nearest-pose stays while committing an
operation. Those stays never appear as persistent or hidden design
constraints. A future batch AutoConstrain workflow should preview candidate
constraints/dimensions, let the user choose a datum and quantity, make geometry
adjustment opt-in and tolerance-bounded, and commit only after review.

## Reproduce

```sh
cargo test -p limo-cad-sketch --test constraint_pair_permutations
cargo test -p limo-cad-sketch --test overconstraint_hardening
```

Related UI verification:

```sh
npm run e2e:constraint-hardening
npm run e2e:reference-dimensions
```
