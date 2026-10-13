# Sketch over-constraint hardening

**Recorded:** 2026-09-04

**Executable regressions:**
[`crates/sketch/tests/overconstraint_hardening.rs`](../crates/sketch/tests/overconstraint_hardening.rs)

**Pairwise matrix:**
[`docs/SKETCH_CONSTRAINT_PAIRWISE_MATRIX.md`](SKETCH_CONSTRAINT_PAIRWISE_MATRIX.md)

## Fusion reference behavior

Autodesk's public Fusion documentation establishes these product rules:

- Fusion exposes Horizontal/Vertical, Coincident, Tangent, Equal, Parallel,
  Perpendicular, Fix/UnFix, Midpoint, Concentric, Collinear, Symmetry, and
  Curvature constraint tools. See
  [Constraints in sketches](https://help.autodesk.com/cloudhelp/ENU/Fusion-Sketch/files/SKT-CONSTRAINTS.htm).
- Fusion's API documentation states that Fusion does not allow
  over-constrained sketches and that converting a Driven dimension to Driving
  can fail when it would over-constrain the sketch. See
  [SketchDimension.isDriving](https://help.autodesk.com/cloudhelp/ENU/Fusion-360-API/files/SketchDimension_isDriving.htm).
- A new Fusion dimension is Driving by default, but a dimension that would
  over-constrain the sketch is added as Driven. Driven dimensions are
  calculated, read-only measurements. See
  [Dimensions in sketches](https://help.autodesk.com/cloudhelp/ENU/Fusion-Sketch/files/SKT-SKETCH-CREATE-DIMENSIONS.htm).
- Fix/UnFix locks both size and location. See
  [Add a fixed constraint to a sketch](https://help.autodesk.com/cloudhelp/ENU/Fusion-Sketch/files/SKT-CONSTRAIN-FIX-UNFIX.htm).
- AutoConstrain presents candidate results for review and lets the user adjust
  their quantity and datum before accepting them. See
  [AutoConstrain](https://help.autodesk.com/cloudhelp/ENU/Fusion-Sketch/files/SKT-AUTO-CONSTRAIN-CONCEPT.htm).

The Autodesk documentation above does **not** publish an exhaustive list of
over-constraint combinations. It documents the constraint families and the
no-overconstraint policy. The taxonomy and complete test scope below are our
solver-level implementation of that policy, not a claim that Autodesk uses
the same internal algorithm.

Fusion also exposes Curvature for G2 spline continuity. noBS-CAD does not yet
expose that constraint, so it is outside the current matrix. Every geometric
and dimensional operation currently exposed by noBS-CAD is in scope.

## Failure taxonomy

| Class | Meaning | Example | Required result |
|---|---|---|---|
| Exact duplicate | The same relation is requested again, including reversed commutative operands or line-versus-endpoint aliases. | `Parallel(A,B)` after `Parallel(B,A)`; Radius after Diameter on the same curve | Reject atomically with a duplicate message. |
| Consistent dependency | The proposed relation is already implied by the retained relations. | `A ∥ C` after `A ∥ B` and `B ∥ C` | Reject a single-relation command atomically as **already determined**, naming the dependency circuit. Bulk actions follow the rules below. |
| Contradiction | The proposed relation cannot be true with the retained drivers. | Parallel and Perpendicular on the same nondegenerate line pair; incompatible fixed values | Reject atomically as a conflict, naming constraints whose removal restores a solution. |
| Degenerate/undefined | The requested equation has no stable geometric meaning. | Zero-length direction carrier or invalid entity-kind selection | Reject validation or construction without mutating the sketch. |

Dimensions follow Fusion's Driving/Driven distinction: an untyped dimension
whose measurement is already determined becomes a Reference dimension. A
typed value explicitly asks to drive the model, so the same dependency is
rejected instead of silently discarding the requested value.

## Dependency families

Pair tests cannot expose a circuit that needs three or more equations. The
focused hardening suite therefore covers representative generators for each
algebraic family:

- direction chains: Parallel transitivity, two Perpendicular relations
  implying Parallel, and Parallel plus Perpendicular implying another
  Perpendicular relation;
- axis implications: Horizontal plus Vertical implying Perpendicular, and a
  Parallel follower of a Horizontal line already being Horizontal;
- equality chains: transitive equal line lengths and equal curve radii feeding
  Reference dimensions;
- incidence chains: transitive point coincidence and Midpoint already
  including point-on-line coincidence;
- carrier chains: transitive Collinear relations and a Tangent relation
  propagating across collinear carriers;
- center chains: transitive Concentric relations and two curve centers acquired
  to one point already being concentric;
- symmetry: mirrored lines already have equal length, while mirrored endpoints
  make their carrier perpendicular to the mirror axis;
- anchoring: a relation already fixed by complete Fix geometry is redundant,
  and Fix on a point already tied to the origin adds no information;
- dimensions: Equal plus one length/diameter driver makes the peer measurement
  Reference, and Perpendicular makes a 90-degree angle dimension Reference;
- singular incidences: point-on-circle and point-on-tangent-line remain
  independent even when their first derivatives coincide, in both orders and
  at rotated/scaled poses;
- stationary measurements: a point's distance to a line can be a valid driver
  at an extremum of its travel around a circle; and
- atomic batches: useful H/V batches retain an independent subset; wholly
  implied or contradictory commands reject without partial changes. Connected
  rectangles support bulk Fix/Unfix and one-step Undo/Redo.

Internal construction relations such as trimmed arc endpoint coincidence,
span midpoint, and equal-distance chamfer support are created by their owning
tools rather than the Constraints panel. Their topology-specific combinations
remain covered by the modifier, fillet, slot, and spline suites. Two of them
carry authored geometry as well: center coincidence owns a drawn circle's
selectable center point, and a span midpoint binds a center rectangle's center
to a diagonal so the shape stays symmetric when a corner is dragged.

## Admission algorithm

For every newly requested persistent relation:

1. Validate entity kinds and reject an exact duplicate through normalized
   semantic matching.
2. Snapshot the sketch, add the proposed equation rows, and solve the command
   with the operation's normal ownership stays.
3. If the new relation cannot converge within tolerance, classify it as a
   contradiction when removing a related existing relation restores the
   solve; otherwise report numerical solve failure without inventing a
   blocker.
4. At the **same final geometric state**, compare Jacobian rank with and
   without the proposed relation. Comparing different poses is deliberately
   avoided because a nonlinear solve can cross a singular pose.
5. A positive rank gain accepts the relation. A rank tie in a nonlinear system
   also checks for finite permitted motion: perturb the proposed relation's
   variables in a private copy, remove the proposal, and re-solve. Only a
   converged configuration satisfying the retained equations while violating
   the proposal is evidence of independence. This avoids mistaking tangency
   or a measurement extremum for an implied relation. Off-manifold rank
   samples alone are not evidence; genuine dependency circuits can gain rank
   away from their solution manifold too.
6. Without rank gain or a finite-motion witness, reject a single relation as
   redundant and identify its dependencies. Any rejection restores the exact
   pre-command snapshot.

The finite-motion check is a bounded numerical safeguard, not a symbolic
proof over all possible sketch configurations. Probes are deterministic,
scaled by local feature size rather than world-coordinate magnitude, and
never change the authored geometry or undo history. Fully ranked and wholly
linear systems do not need probes. The check is shared by manual constraints,
automatic inference, and Driving/Reference dimension admission.

### Bulk operation rules

- Solve and validate the whole proposed batch before removing any redundant
  members. A contradiction must not be hidden by dropping an equation.
- For geometric batches, remove implied new members one at a time, starting
  with the last selection. This preserves an independent subset (for example,
  one Horizontal driver on two parallel lines). Never remove all initially
  dependent members simultaneously: a cycle may need some of them.
- Exact duplicates, an entirely implied batch, and a redundant explicit
  dimensional driver still reject atomically. Typed values are not discarded.
- Fix/Unfix is a per-entity toggle: assess new anchors as a group and retain
  their per-entity records even when they overlap at shared endpoints. This
  intentional overlap lets a second toggle of the same rectangle remove every
  anchor, rather than accidentally fixing previously skipped edges. Mixed
  Fix/Unfix selections may release existing anchors even if a newly added
  anchor is already implied.
- Each accepted batch or Fix/Unfix selection remains one undoable operation.

Selective automatic constraints pass through the same independence check, so
an inference is persisted only when it removes a real degree of freedom.

## Coverage boundary

The finite exhaustive layer is the Cartesian square of noBS-CAD's 25 exposed
operations: 625 ordered pairs, repeated on a disconnected solved island for
1,250 scenarios. It verifies both orderings, equation truth, property
ownership, bounded geometry, unrelated-island stability, and atomic rejection.

There is no finite enumeration of every longer constraint graph. Higher-order
coverage is therefore organized by the dependency families above rather than
claiming an impossible exhaustive permutation of all graph sizes. Each new
constraint type must add:

1. its rows and columns to the pairwise operation vector;
2. a focused test for any new dependency/contradiction family it introduces;
3. a Reference-dimension test when it can determine a measurable property;
   and
4. an atomic batch or tool-creation test when it can be emitted indirectly.

## Reproduce

```sh
cargo test -p limo-cad-sketch --test constraint_pair_permutations
cargo test -p limo-cad-sketch --test overconstraint_hardening
cargo test -p limo-cad-sketch
```
