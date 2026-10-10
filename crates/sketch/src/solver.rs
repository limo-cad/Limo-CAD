//! Newton-based sketch constraint solver.
//!
//! Unknown layout: each `Point` contributes (x, y); each `Circle`
//! (cx, cy, r); each `Arc` (cx, cy, r, a0, a1); `Line`s contribute nothing
//! (their geometry lives in the shared endpoint points). Constraints
//! contribute residual equations with analytical Jacobians. The solver is
//! Levenberg–Marquardt damped Newton with an adaptive damping factor and
//! step acceptance by residual decrease; convergence tolerance is ~1e-9
//! (max |residual|), iteration-capped.
//!
//! DOF tracking: `unknowns − rank(J)` where the rank is computed by
//! Gaussian elimination with partial pivoting. Per-entity constraint state
//! also accounts for pivot variables that depend on free variables: an entity
//! is fully defined only when none of its parameters can vary in the nullspace.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::constraint::{ArcEndpoint, Constraint, ConstraintId};
use crate::entity::{Entity, EntityId, AXIS_SENTINEL};
use crate::geometry::Vec2;
use crate::geomops::fillet;
use crate::sketch::Sketch;

/// Convergence tolerance on max |residual| (mm / rad mixed residuals).
const TOL: f64 = 1e-9;
const MAX_ITERS: usize = 80;
/// Direction residuals are dimensionless. Scale them into the same numerical
/// neighborhood as ordinary millimetre equations so a distance relation
/// cannot dominate an angle relation by orders of magnitude.
const DIRECTION_SCALE: f64 = 100.0;
/// Below 0.1 micrometre a trimmed finite edge is treated as its persistent
/// support line. This avoids singular direction derivatives at the exact
/// fillet/chamfer consumption boundary while remaining far below modeling
/// and display tolerances.
const DEGENERATE_LINE_EPS: f64 = 1e-4;

const CONSUMED_CARRIER_EPS: f64 = 1e-3;

/// Preserve the external branch at its solved boundary. Reconstructing an
/// off-axis center at r1+r2 can round a few ULPs inside that distance; treating
/// that as containment switches a correctly seeded external tangent to the
/// internal equation on the next solve/analysis pass.
pub(crate) fn circle_tangent_is_external(
    distance: f64,
    first_radius: f64,
    second_radius: f64,
) -> bool {
    let sum = first_radius + second_radius;
    // Relative roundoff only: an absolute solver tolerance would swallow the
    // distinction between internal/external tangency for very small circles.
    let roundoff = 8.0 * f64::EPSILON * sum.abs();
    distance >= sum - roundoff
}

/// Outcome of one solve/analysis pass.
#[derive(Debug, Clone)]
pub struct Analysis {
    pub converged: bool,
    pub iterations: usize,
    /// max |residual| at the final state.
    pub residual: f64,
    pub unknowns: usize,
    pub equations: usize,
    pub rank: usize,
    /// `unknowns − rank` (≥ 0 by construction).
    pub dof: i32,
    /// Variable parameters per entity, including dependent pivots; 0 ⇔ fully defined.
    pub entity_free: HashMap<EntityId, usize>,
}

impl Analysis {
    pub fn fully_defined(&self, entity: EntityId) -> bool {
        self.entity_free.get(&entity).copied().unwrap_or(0) == 0
    }
}

/// Operation-local invariants used only while a newly requested relation is
/// being fitted to the authored geometry. These are deliberately excluded
/// from rank/DOF reporting and are never persisted in the sketch. They make
/// the initial application behave like a CAD tool instead of letting a
/// scale-invariant equation find a numerically cheap but surprising shape.
#[derive(Debug, Clone, Default)]
pub(crate) struct SolveStays {
    /// Hold these entities at their current parameter values for this solve,
    /// without adding authored Fix constraints or removing their reported DOF.
    pub(crate) rigid_entities: Vec<EntityId>,
    pub(crate) line_lengths: Vec<(EntityId, f64)>,
    pub(crate) line_angles: Vec<(EntityId, f64)>,
    pub(crate) line_midpoints: Vec<(EntityId, Vec2)>,
    pub(crate) point_pair_distances: Vec<(EntityId, EntityId, f64)>,
    pub(crate) point_pair_angles: Vec<(EntityId, EntityId, f64)>,
    pub(crate) point_pair_midpoints: Vec<(EntityId, EntityId, Vec2)>,
    pub(crate) point_positions: Vec<(EntityId, Vec2)>,
    pub(crate) curve_radii: Vec<(EntityId, f64)>,
    pub(crate) curve_centers: Vec<(EntityId, Vec2)>,
}

impl SolveStays {
    pub(crate) fn is_empty(&self) -> bool {
        self.rigid_entities.is_empty()
            && self.line_lengths.is_empty()
            && self.line_angles.is_empty()
            && self.line_midpoints.is_empty()
            && self.point_pair_distances.is_empty()
            && self.point_pair_angles.is_empty()
            && self.point_pair_midpoints.is_empty()
            && self.point_positions.is_empty()
            && self.curve_radii.is_empty()
            && self.curve_centers.is_empty()
    }
}

/// Variable indices of one point: `(x_var, y_var)`.
type Pt = (usize, usize);

/// A difference of two points: d = p2 − p1.
#[derive(Clone, Copy)]
struct Diff {
    x1: usize,
    y1: usize,
    x2: usize,
    y2: usize,
}

impl Diff {
    fn val(&self, x: &[f64]) -> (f64, f64) {
        (x[self.x2] - x[self.x1], x[self.y2] - x[self.y1])
    }
    /// Push the chain-rule expansion of `coef · d(dx)` and `coef · d(dy)`.
    fn push_deriv(&self, dx_coef: f64, dy_coef: f64, out: &mut Vec<(usize, f64)>) {
        out.push((self.x2, dx_coef));
        out.push((self.x1, -dx_coef));
        out.push((self.y2, dy_coef));
        out.push((self.y1, -dy_coef));
    }
}

/// One residual equation F(x) = 0 with an analytical Jacobian row.
enum Eq {
    ReferenceEdgeCoordinate {
        p: Pt,
        edge: crate::dto::ProjectedEdgeDto,
        axis: usize,
    },
    /// Σ aᵢ·xᵢ + c = 0 (H/V/Fix/Midpoint/Concentric/Equal-radius).
    Lin { terms: Vec<(usize, f64)>, c: f64 },
    /// cross(a, b) = a.x·b.y − a.y·b.x = 0 (Parallel, Collinear).
    Cross { a: Diff, b: Diff },
    /// dot(a, b) = 0 (Perpendicular, Symmetry legs).
    Dot { a: Diff, b: Diff },
    /// cross(d, p − base) = 0 (point-on-line incidence). A consumed fillet
    /// carrier uses its persistent support direction for the whole solve,
    /// just like line-circle tangency.
    CrossPt {
        d: Diff,
        p: Pt,
        base: Pt,
        support: (f64, f64),
        support_mode: bool,
    },
    /// T/|d| − sign·r = 0 with T = cross(d, center − line start) — signed
    /// perpendicular center-to-line distance (mm units), single-sided. The
    /// previous squared form (T² − r²|d|²) had two-sided phantom minima and
    /// mixed mm⁴/mm residual scales that stalled LM on coupled trims
    /// (2026-07-19 second-fillet false rejection). `sign` (+1/−1) is the
    /// center's side of the line at equation-build time.
    LineCircleTangent {
        d: Diff,
        c: Pt,
        base: Pt,
        r: usize,
        sign: f64,
        /// Last meaningful direction of the finite carrier segment. A
        /// fillet pair may consume an edge exactly (R1 + R2 == L), leaving
        /// coincident endpoint variables while its infinite support line is
        /// still geometrically well-defined.
        support: (f64, f64),
        /// Keep the support-line form for an entire solve that starts with a
        /// consumed carrier. Switching back to the finite-segment form as
        /// soon as an iteration separates the endpoints makes the Jacobian
        /// discontinuous exactly while a radius edit is reopening the edge.
        support_mode: bool,
    },
    /// |a| − |b| = 0 (Equal line lengths, in millimetres).
    EqualLength { a: Diff, b: Diff },
    /// |c1 − c2| − |r1 + sign·r2| = 0 (circle/arc↔circle/arc
    /// tangency, in millimetres; sign = +1 external, −1 internal).
    CircleCircle {
        c1: Pt,
        r1: usize,
        c2: Pt,
        r2: usize,
        sign: f64,
    },
    /// |p − c| − |r| = 0 (point on circle/arc, in millimetres).
    PointOnCircle { p: Pt, c: Pt, r: usize },
    /// p.x − c.x − r·cos a = 0 (arc-endpoint trim anchor, x component;
    /// Constraint::ArcEndpointCoincident).
    ArcEndX { p: Pt, c: Pt, r: usize, a: usize },
    /// p.y − c.y − r·sin a = 0 (arc-endpoint trim anchor, y component).
    ArcEndY { p: Pt, c: Pt, r: usize, a: usize },
    /// |a − b| − target = 0 (Distance between two points).
    DistPt { a: Pt, b: Pt, target: f64 },
    /// Signed perpendicular distance from q to the line through base with
    /// direction d, minus target (line↔line / point↔line distance).
    LineDist {
        d: Diff,
        q: Pt,
        base: Pt,
        target: f64,
    },
    /// r − target = 0 (Radius/Diameter).
    Radius { r: usize, target: f64 },
    /// Angle between two line directions in radians, wrapped to (−π, π].
    AngleLines { a: Diff, b: Diff, target: f64 },
    /// Angle between a line direction and the +u axis (auto axis dims).
    AngleAxis { a: Diff, target: f64 },
    /// cross(dax, (a+b)/2 − a1) = 0 (Symmetry: midpoint on the axis).
    SymmetryMid { a: Pt, b: Pt, axis: Diff },
}

impl Eq {
    /// Residual plus sparse Jacobian row entries (var index, ∂F/∂xᵢ).
    fn eval(&self, x: &[f64]) -> (f64, Vec<(usize, f64)>) {
        match *self {
            Eq::ReferenceEdgeCoordinate { p, ref edge, axis } => {
                let point = Vec2::new(x[p.0], x[p.1]);
                let residual = |point: Vec2| {
                    let closest = edge.closest_point(point).unwrap_or(point);
                    if axis == 0 {
                        point.x - closest.x
                    } else {
                        point.y - closest.y
                    }
                };

                let h = 1e-5;
                let dx = (residual(point + Vec2::new(h, 0.0))
                    - residual(point - Vec2::new(h, 0.0)))
                    / (2.0 * h);
                let dy = (residual(point + Vec2::new(0.0, h))
                    - residual(point - Vec2::new(0.0, h)))
                    / (2.0 * h);
                (residual(point), vec![(p.0, dx), (p.1, dy)])
            }
            Eq::Lin { ref terms, c } => {
                let mut r = c;
                for &(i, a) in terms {
                    r += a * x[i];
                }
                (r, terms.clone())
            }
            Eq::Cross { a, b } => {
                let (ax, ay) = a.val(x);
                let (bx, by) = b.val(x);
                let la = (ax * ax + ay * ay).sqrt().max(1e-12);
                let lb = (bx * bx + by * by).sqrt().max(1e-12);
                let denom = la * lb;
                let cross = ax * by - ay * bx;
                let mut out = Vec::with_capacity(8);

                a.push_deriv(
                    DIRECTION_SCALE * (by / denom - cross * ax / (la * la * la * lb)),
                    DIRECTION_SCALE * (-bx / denom - cross * ay / (la * la * la * lb)),
                    &mut out,
                );
                b.push_deriv(
                    DIRECTION_SCALE * (-ay / denom - cross * bx / (la * lb * lb * lb)),
                    DIRECTION_SCALE * (ax / denom - cross * by / (la * lb * lb * lb)),
                    &mut out,
                );
                (DIRECTION_SCALE * cross / denom, out)
            }
            Eq::Dot { a, b } => {
                let (ax, ay) = a.val(x);
                let (bx, by) = b.val(x);
                let la = (ax * ax + ay * ay).sqrt().max(1e-12);
                let lb = (bx * bx + by * by).sqrt().max(1e-12);
                let denom = la * lb;
                let dot = ax * bx + ay * by;
                let mut out = Vec::with_capacity(8);

                a.push_deriv(
                    DIRECTION_SCALE * (bx / denom - dot * ax / (la * la * la * lb)),
                    DIRECTION_SCALE * (by / denom - dot * ay / (la * la * la * lb)),
                    &mut out,
                );
                b.push_deriv(
                    DIRECTION_SCALE * (ax / denom - dot * bx / (la * lb * lb * lb)),
                    DIRECTION_SCALE * (ay / denom - dot * by / (la * lb * lb * lb)),
                    &mut out,
                );
                (DIRECTION_SCALE * dot / denom, out)
            }
            Eq::CrossPt {
                d,
                p,
                base,
                support,
                support_mode,
            } => {
                let (dx, dy) = d.val(x);
                let (px, py) = (x[p.0], x[p.1]);
                let (bx, by) = (x[base.0], x[base.1]);
                let len = (dx * dx + dy * dy).sqrt();
                if support_mode || len < DEGENERATE_LINE_EPS {
                    let (ux, uy) = support;
                    let t = ux * (py - by) - uy * (px - bx);
                    return (t, vec![(p.0, -uy), (p.1, ux), (base.0, uy), (base.1, -ux)]);
                }
                let rx = px - bx;
                let ry = py - by;
                let cross = dx * ry - dy * rx;
                let mut out = Vec::with_capacity(10);

                d.push_deriv(
                    ry / len - cross * dx / len.powi(3),
                    -rx / len - cross * dy / len.powi(3),
                    &mut out,
                );
                out.push((p.0, -dy / len));
                out.push((p.1, dx / len));
                out.push((base.0, dy / len));
                out.push((base.1, -dx / len));
                (cross / len, out)
            }
            Eq::LineCircleTangent {
                d,
                c,
                base,
                r,
                sign,
                support,
                support_mode,
            } => {
                let (dx, dy) = d.val(x);
                let (cx, cy) = (x[c.0], x[c.1]);
                let (bx, by) = (x[base.0], x[base.1]);
                let raw_len = (dx * dx + dy * dy).sqrt();
                if support_mode || raw_len < DEGENERATE_LINE_EPS {
                    let (ux, uy) = support;
                    let t = ux * (cy - by) - uy * (cx - bx);
                    let f = t - sign * x[r];
                    return (
                        f,
                        vec![
                            (c.0, -uy),
                            (c.1, ux),
                            (base.0, uy),
                            (base.1, -ux),
                            (r, -sign),
                        ],
                    );
                }
                let t = dx * (cy - by) - dy * (cx - bx);
                let len = raw_len;

                let f = t / len - sign * x[r];
                let inv = 1.0 / len;
                let mut out = Vec::with_capacity(12);

                let mut t_terms = Vec::with_capacity(12);
                d.push_deriv(cy - by, -(cx - bx), &mut t_terms);
                t_terms.push((c.0, -dy));
                t_terms.push((c.1, dx));
                t_terms.push((base.0, dy));
                t_terms.push((base.1, -dx));
                for (i, v) in t_terms {
                    out.push((i, v * inv));
                }

                let s = -t / (len * len * len);
                d.push_deriv(s * dx, s * dy, &mut out);
                out.push((r, -sign));
                (f, out)
            }
            Eq::ArcEndX { p, c, r, a } => {
                let (ca, sa) = (x[a].cos(), x[a].sin());
                let f = x[p.0] - x[c.0] - x[r] * ca;
                (f, vec![(p.0, 1.0), (c.0, -1.0), (r, -ca), (a, x[r] * sa)])
            }
            Eq::ArcEndY { p, c, r, a } => {
                let (ca, sa) = (x[a].cos(), x[a].sin());
                let f = x[p.1] - x[c.1] - x[r] * sa;
                (f, vec![(p.1, 1.0), (c.1, -1.0), (r, -sa), (a, -x[r] * ca)])
            }
            Eq::EqualLength { a, b } => {
                let (ax, ay) = a.val(x);
                let (bx, by) = b.val(x);
                let a_length = (ax * ax + ay * ay).sqrt().max(1e-12);
                let b_length = (bx * bx + by * by).sqrt().max(1e-12);
                let mut out = Vec::with_capacity(8);
                a.push_deriv(ax / a_length, ay / a_length, &mut out);
                b.push_deriv(-bx / b_length, -by / b_length, &mut out);
                (a_length - b_length, out)
            }
            Eq::CircleCircle {
                c1,
                r1,
                c2,
                r2,
                sign,
            } => {
                let dx = x[c1.0] - x[c2.0];
                let dy = x[c1.1] - x[c2.1];
                let distance = (dx * dx + dy * dy).sqrt().max(1e-12);
                let signed_radius = x[r1] + sign * x[r2];
                let radius_sign = if signed_radius < 0.0 { -1.0 } else { 1.0 };
                let f = distance - signed_radius.abs();
                (
                    f,
                    vec![
                        (c1.0, dx / distance),
                        (c2.0, -dx / distance),
                        (c1.1, dy / distance),
                        (c2.1, -dy / distance),
                        (r1, -radius_sign),
                        (r2, -radius_sign * sign),
                    ],
                )
            }
            Eq::PointOnCircle { p, c, r } => {
                let dx = x[p.0] - x[c.0];
                let dy = x[p.1] - x[c.1];
                let distance = (dx * dx + dy * dy).sqrt().max(1e-12);
                let radius_sign = if x[r] < 0.0 { -1.0 } else { 1.0 };
                let f = distance - x[r].abs();
                (
                    f,
                    vec![
                        (p.0, dx / distance),
                        (c.0, -dx / distance),
                        (p.1, dy / distance),
                        (c.1, -dy / distance),
                        (r, -radius_sign),
                    ],
                )
            }
            Eq::DistPt { a, b, target } => {
                let dx = x[a.0] - x[b.0];
                let dy = x[a.1] - x[b.1];
                let d = (dx * dx + dy * dy).sqrt();
                if d < 1e-12 {
                    return (-target, Vec::new());
                }
                (
                    d - target,
                    vec![(a.0, dx / d), (b.0, -dx / d), (a.1, dy / d), (b.1, -dy / d)],
                )
            }
            Eq::Radius { r, target } => (x[r] - target, vec![(r, 1.0)]),
            Eq::LineDist { d, q, base, target } => {
                let (dx, dy) = d.val(x);
                let len = (dx * dx + dy * dy).sqrt();
                let (qx, qy) = (x[q.0], x[q.1]);
                let (bx, by) = (x[base.0], x[base.1]);
                let cross = dx * (qy - by) - dy * (qx - bx);
                if len < 1e-12 {
                    return (0.0, Vec::new());
                }
                let f = cross / len - target;

                let mut out = Vec::with_capacity(10);
                d.push_deriv((qy - by) / len, -(qx - bx) / len, &mut out);
                out.push((q.0, -dy / len));
                out.push((q.1, dx / len));
                out.push((base.0, dy / len));
                out.push((base.1, -dx / len));
                d.push_deriv(
                    -cross * dx / (len * len * len),
                    -cross * dy / (len * len * len),
                    &mut out,
                );
                (f, out)
            }
            Eq::AngleLines { a, b, target } => {
                let (ax, ay) = a.val(x);
                let (bx, by) = b.val(x);
                let cross = ax * by - ay * bx;
                let dot = ax * bx + ay * by;
                let r2 = (cross * cross + dot * dot).max(1e-24);
                let f = wrap_angle(cross.atan2(dot) - target);
                let dc = dot / r2;
                let dd = -cross / r2;
                let mut out = Vec::with_capacity(16);

                a.push_deriv(dc * by + dd * bx, -dc * bx + dd * by, &mut out);
                b.push_deriv(-dc * ay + dd * ax, dc * ax + dd * ay, &mut out);
                (
                    DIRECTION_SCALE * f,
                    out.into_iter()
                        .map(|(index, derivative)| (index, DIRECTION_SCALE * derivative))
                        .collect(),
                )
            }
            Eq::AngleAxis { a, target } => {
                let (ax, ay) = a.val(x);

                let r2 = (ax * ax + ay * ay).max(1e-24);
                let f = wrap_angle(ay.atan2(ax) - target);
                let mut out = Vec::with_capacity(4);

                a.push_deriv(-ay / r2, ax / r2, &mut out);
                (
                    DIRECTION_SCALE * f,
                    out.into_iter()
                        .map(|(index, derivative)| (index, DIRECTION_SCALE * derivative))
                        .collect(),
                )
            }
            Eq::SymmetryMid { a, b, axis } => {
                let (ux, uy) = axis.val(x);
                let len = (ux * ux + uy * uy).sqrt().max(1e-12);
                let mx = (x[a.0] + x[b.0]) / 2.0;
                let my = (x[a.1] + x[b.1]) / 2.0;
                let (a1x, a1y) = (x[axis.x1], x[axis.y1]);
                let rx = mx - a1x;
                let ry = my - a1y;
                let cross = ux * ry - uy * rx;
                let mut out = Vec::with_capacity(10);
                axis.push_deriv(
                    ry / len - cross * ux / len.powi(3),
                    -rx / len - cross * uy / len.powi(3),
                    &mut out,
                );

                out.push((a.0, -0.5 * uy / len));
                out.push((b.0, -0.5 * uy / len));
                out.push((a.1, 0.5 * ux / len));
                out.push((b.1, 0.5 * ux / len));

                out.push((axis.x1, uy / len));
                out.push((axis.y1, -ux / len));
                (cross / len, out)
            }
        }
    }
}

/// Unknown-index map for a sketch.
struct VarMap {
    points: HashMap<EntityId, Pt>,
    circles: HashMap<EntityId, (Pt, usize)>,
    arcs: HashMap<EntityId, (Pt, usize, usize, usize)>,
    /// Every fit point contributes an `(x, y)` pair. Splines do not yet
    /// participate in incidence/tangent constraints, but they must still
    /// carry real DOF so Fix/Unfix and transform solving are truthful.
    splines: HashMap<EntityId, Vec<Pt>>,
    /// Generated center handles whose coordinates *are* their curve's center.
    /// They alias into the curve's variables instead of adding two unknowns and
    /// two equations, which keeps a circle's cost at three unknowns whether or
    /// not it exposes a center (issue #151).
    aliases: HashSet<EntityId>,
    n: usize,
}

/// Center handles that can alias into the curve they belong to: a generated
/// point whose only relation is `CenterCoincident` to a curve and that no entity
/// references. Anything else — an acquired center the user attached to other
/// geometry, or a handle a line also uses — stays an independent variable so its
/// other relations keep their own freedom.
fn aliased_handles(
    sketch: &Sketch,
    excluded: &BTreeSet<ConstraintId>,
) -> HashMap<EntityId, EntityId> {
    let mut references: HashMap<EntityId, usize> = HashMap::new();
    let mut centers: HashMap<EntityId, EntityId> = HashMap::new();
    let mut excluded_points: HashSet<EntityId> = HashSet::new();
    for (id, constraint) in sketch.constraints() {
        for operand in constraint.referenced_entities() {
            *references.entry(operand).or_default() += 1;
        }
        if let Constraint::CenterCoincident { point, curve } = *constraint {
            if excluded.contains(&id) {
                excluded_points.insert(point);
            }
            centers.insert(point, curve);
        }
    }
    centers
        .into_iter()
        .filter(|(point, curve)| {
            !excluded_points.contains(point)
                && matches!(
                    sketch.entity(*curve),
                    Some(Entity::Circle { .. } | Entity::Arc { .. })
                )
                && sketch.is_generated_point(*point)
                && !sketch.is_referenced_by_entity(*point)
                && references.get(point) == Some(&1)
        })
        .collect()
}

fn build_var_map(sketch: &Sketch) -> VarMap {
    build_var_map_excluding(sketch, &BTreeSet::new())
}

/// Build the variable map while ignoring `excluded` relations when deciding
/// which center handles may alias. A relation under admission has to be judged
/// on its own equations, so the point it would bind keeps its two variables on
/// that side of the comparison instead of disappearing into the curve center.
fn build_var_map_excluding(sketch: &Sketch, excluded: &BTreeSet<ConstraintId>) -> VarMap {
    let mut map = VarMap {
        points: HashMap::new(),
        circles: HashMap::new(),
        arcs: HashMap::new(),
        splines: HashMap::new(),
        aliases: HashSet::new(),
        n: 0,
    };
    let aliased = aliased_handles(sketch, excluded);
    let mut alloc = |count: usize| {
        let start = map.n;
        map.n += count;
        start
    };
    for (id, entity) in sketch.entities() {
        match entity {
            Entity::Point { .. } => {
                if aliased.contains_key(&id) {
                    map.aliases.insert(id);
                } else {
                    let i = alloc(2);
                    map.points.insert(id, (i, i + 1));
                }
            }
            Entity::Circle { .. } => {
                let i = alloc(3);
                map.circles.insert(id, ((i, i + 1), i + 2));
            }
            Entity::Arc { .. } => {
                let i = alloc(5);
                map.arcs.insert(id, ((i, i + 1), i + 2, i + 3, i + 4));
            }
            Entity::Line { .. } => {}
            Entity::Spline { points } => {
                let mut vars = Vec::with_capacity(points.len());
                for _ in points {
                    let i = alloc(2);
                    vars.push((i, i + 1));
                }
                map.splines.insert(id, vars);
            }
        }
    }

    for (handle, curve) in aliased {
        let center = map
            .circles
            .get(&curve)
            .map(|(center, _)| *center)
            .or_else(|| map.arcs.get(&curve).map(|(center, ..)| *center));
        if let Some(center) = center {
            map.points.insert(handle, center);
        }
    }
    map
}

fn read_values(sketch: &Sketch, map: &VarMap) -> Vec<f64> {
    let mut x = vec![0.0; map.n];
    for (id, entity) in sketch.entities() {
        match entity {
            Entity::Point { position } => {
                if map.aliases.contains(&id) {
                    continue;
                }
                let p = map.points[&id];
                x[p.0] = position.x;
                x[p.1] = position.y;
            }
            Entity::Circle { center, radius } => {
                let (c, r) = map.circles[&id];
                x[c.0] = center.x;
                x[c.1] = center.y;
                x[r] = *radius;
            }
            Entity::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                let (c, r, a0, a1) = map.arcs[&id];
                x[c.0] = center.x;
                x[c.1] = center.y;
                x[r] = *radius;
                x[a0] = *start_angle;
                x[a1] = *end_angle;
            }
            Entity::Line { .. } => {}
            Entity::Spline { points } => {
                if let Some(vars) = map.splines.get(&id) {
                    for (point, var) in points.iter().zip(vars) {
                        x[var.0] = point.x;
                        x[var.1] = point.y;
                    }
                }
            }
        }
    }
    x
}

fn write_values(sketch: &mut Sketch, map: &VarMap, x: &[f64]) {
    let ids: Vec<EntityId> = sketch.entities().map(|(id, _)| id).collect();
    for id in ids {
        match sketch.entity_mut(id) {
            Some(Entity::Point { position }) => {
                let p = map.points[&id];
                *position = Vec2::new(x[p.0], x[p.1]);
            }
            Some(Entity::Circle { center, radius }) => {
                let (c, r) = map.circles[&id];
                *center = Vec2::new(x[c.0], x[c.1]);
                *radius = x[r];
            }
            Some(Entity::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            }) => {
                let (c, r, a0, a1) = map.arcs[&id];
                *center = Vec2::new(x[c.0], x[c.1]);
                *radius = x[r];
                *start_angle = x[a0];
                *end_angle = x[a1];
            }
            Some(Entity::Spline { points }) => {
                if let Some(vars) = map.splines.get(&id) {
                    for (point, var) in points.iter_mut().zip(vars) {
                        *point = Vec2::new(x[var.0], x[var.1]);
                    }
                }
            }
            _ => {}
        }
    }
}

impl VarMap {
    fn pt(&self, sketch: &Sketch, id: EntityId) -> Option<Pt> {
        self.points
            .get(&id)
            .copied()
            .or_else(|| match sketch.entity(id) {
                Some(Entity::Circle { .. }) => self.circles.get(&id).map(|(c, _)| *c),
                Some(Entity::Arc { .. }) => self.arcs.get(&id).map(|(c, ..)| *c),
                _ => None,
            })
    }

    fn line_diff(&self, sketch: &Sketch, id: EntityId) -> Option<Diff> {
        let (start, end) = sketch.line_endpoint_ids(id)?;
        let s = self.points.get(&start)?;
        let e = self.points.get(&end)?;
        Some(Diff {
            x1: s.0,
            y1: s.1,
            x2: e.0,
            y2: e.1,
        })
    }

    fn radius_var(&self, sketch: &Sketch, id: EntityId) -> Option<usize> {
        match sketch.entity(id) {
            Some(Entity::Circle { .. }) => self.circles.get(&id).map(|(_, r)| *r),
            Some(Entity::Arc { .. }) => self.arcs.get(&id).map(|(_, r, ..)| *r),
            _ => None,
        }
    }
}

/// When a line and arc already share an endpoint through an explicit
/// ArcEndpointCoincident relation, tangency is most accurately expressed as
/// line direction perpendicular to the endpoint radius. The generic
/// line-to-circle distance equation is geometrically correct, but its
/// Jacobian becomes first-order redundant exactly at an endpoint contact;
/// the directional equation keeps rank analysis and interactive solving
/// well-conditioned at that common sketch configuration.
fn shared_line_arc_endpoint_radial(
    sketch: &Sketch,
    map: &VarMap,
    line: EntityId,
    arc: EntityId,
) -> Option<Diff> {
    if !matches!(sketch.entity(arc), Some(Entity::Arc { .. })) {
        return None;
    }
    let (resolved_start, resolved_end) = sketch.resolved_line(line)?;
    if resolved_start.distance(resolved_end) < DEGENERATE_LINE_EPS {
        return None;
    }
    let (line_start, line_end) = sketch.line_endpoint_ids(line)?;
    let point = sketch
        .constraints()
        .find_map(|(_, constraint)| match *constraint {
            Constraint::ArcEndpointCoincident {
                point,
                arc: constrained_arc,
                ..
            } if constrained_arc == arc && (point == line_start || point == line_end) => {
                Some(point)
            }
            _ => None,
        })?;

    if trim_origin_for_endpoint(sketch, line, point).is_some() {
        return None;
    }
    let center = map.pt(sketch, arc)?;
    let endpoint = map.pt(sketch, point)?;
    Some(Diff {
        x1: center.0,
        y1: center.1,
        x2: endpoint.0,
        y2: endpoint.1,
    })
}

/// Build the equation set: one row per residual, tagged by constraint.
fn build_equations(
    sketch: &Sketch,
    map: &VarMap,
    pins: &[(EntityId, Vec2)],
) -> Vec<(Option<ConstraintId>, Eq)> {
    fn push_lin(
        eqs: &mut Vec<(Option<ConstraintId>, Eq)>,
        cid: ConstraintId,
        terms: Vec<(usize, f64)>,
        c: f64,
    ) {
        eqs.push((Some(cid), Eq::Lin { terms, c }));
    }

    let mut eqs: Vec<(Option<ConstraintId>, Eq)> = Vec::new();

    for (cid, constraint) in sketch.constraints() {
        if sketch.is_reference_dimension(&cid) {
            continue;
        }
        match *constraint {
            Constraint::Horizontal { entity } => {
                if let Some(d) = map.line_diff(sketch, entity) {
                    push_lin(&mut eqs, cid, vec![(d.y2, 1.0), (d.y1, -1.0)], 0.0);
                }
            }
            Constraint::Vertical { entity } => {
                if let Some(d) = map.line_diff(sketch, entity) {
                    push_lin(&mut eqs, cid, vec![(d.x2, 1.0), (d.x1, -1.0)], 0.0);
                }
            }
            Constraint::HorizontalPoints { a, b } => {
                if let (Some(a), Some(b)) = (map.pt(sketch, a), map.pt(sketch, b)) {
                    push_lin(&mut eqs, cid, vec![(b.1, 1.0), (a.1, -1.0)], 0.0);
                }
            }
            Constraint::VerticalPoints { a, b } => {
                if let (Some(a), Some(b)) = (map.pt(sketch, a), map.pt(sketch, b)) {
                    push_lin(&mut eqs, cid, vec![(b.0, 1.0), (a.0, -1.0)], 0.0);
                }
            }
            Constraint::Fix { entity } => {
                if let Some(targets) = sketch.fix_targets(&cid) {
                    let mut vars: Vec<usize> = Vec::new();
                    match sketch.entity(entity) {
                        Some(Entity::Point { .. }) => {
                            let p = map.points[&entity];
                            vars.extend([p.0, p.1]);
                        }

                        Some(Entity::Line { start, end }) => {
                            for pid in [start, end] {
                                let p = map.points[pid];
                                vars.extend([p.0, p.1]);
                            }
                        }
                        Some(Entity::Circle { .. }) => {
                            let (c, r) = map.circles[&entity];
                            vars.extend([c.0, c.1, r]);
                        }
                        Some(Entity::Arc { .. }) => {
                            let (c, r, a0, a1) = map.arcs[&entity];
                            vars.extend([c.0, c.1, r, a0, a1]);
                        }
                        Some(Entity::Spline { .. }) => {
                            if let Some(points) = map.splines.get(&entity) {
                                for point in points {
                                    vars.extend([point.0, point.1]);
                                }
                            }
                        }
                        None => {}
                    }
                    for (var, target) in vars.into_iter().zip(targets.iter()) {
                        push_lin(&mut eqs, cid, vec![(var, 1.0)], -*target);
                    }
                }
            }
            Constraint::OriginCoincident { entity } => {
                if let Some(center) = map.pt(sketch, entity) {
                    push_lin(&mut eqs, cid, vec![(center.0, 1.0)], 0.0);
                    push_lin(&mut eqs, cid, vec![(center.1, 1.0)], 0.0);
                }
            }
            Constraint::CenterCoincident { point, curve } => {
                if let (Some(point), Some(center)) = (map.pt(sketch, point), map.pt(sketch, curve))
                {
                    if point != center {
                        push_lin(&mut eqs, cid, vec![(center.0, 1.0), (point.0, -1.0)], 0.0);
                        push_lin(&mut eqs, cid, vec![(center.1, 1.0), (point.1, -1.0)], 0.0);
                    }
                }
            }
            Constraint::Coincident { a, b } => {
                let pa = map.pt(sketch, a);
                let pb = map.pt(sketch, b);
                match (sketch.entity(a), sketch.entity(b)) {
                    (Some(Entity::Point { .. }), Some(Entity::Point { .. })) => {
                        if let (Some(pa), Some(pb)) = (pa, pb) {
                            push_lin(&mut eqs, cid, vec![(pa.0, 1.0), (pb.0, -1.0)], 0.0);
                            push_lin(&mut eqs, cid, vec![(pa.1, 1.0), (pb.1, -1.0)], 0.0);
                        }
                    }
                    (Some(Entity::Point { .. }), Some(Entity::Line { .. })) => {
                        if let (Some(p), Some(d)) = (pa, map.line_diff(sketch, b)) {
                            eqs.push((
                                Some(cid),
                                Eq::CrossPt {
                                    d,
                                    p,
                                    base: (d.x1, d.y1),
                                    support: line_support(sketch, b).unwrap_or((1.0, 0.0)),
                                    support_mode: line_is_degenerate(sketch, b),
                                },
                            ));
                        }
                    }
                    (Some(Entity::Line { .. }), Some(Entity::Point { .. })) => {
                        if let (Some(p), Some(d)) = (pb, map.line_diff(sketch, a)) {
                            eqs.push((
                                Some(cid),
                                Eq::CrossPt {
                                    d,
                                    p,
                                    base: (d.x1, d.y1),
                                    support: line_support(sketch, a).unwrap_or((1.0, 0.0)),
                                    support_mode: line_is_degenerate(sketch, a),
                                },
                            ));
                        }
                    }
                    (
                        Some(Entity::Point { .. }),
                        Some(Entity::Circle { .. } | Entity::Arc { .. }),
                    ) => {
                        if let (Some(p), Some(c), Some(r)) =
                            (pa, map.pt(sketch, b), map.radius_var(sketch, b))
                        {
                            eqs.push((Some(cid), Eq::PointOnCircle { p, c, r }));
                        }
                    }
                    (
                        Some(Entity::Circle { .. } | Entity::Arc { .. }),
                        Some(Entity::Point { .. }),
                    ) => {
                        if let (Some(p), Some(c), Some(r)) =
                            (pb, map.pt(sketch, a), map.radius_var(sketch, a))
                        {
                            eqs.push((Some(cid), Eq::PointOnCircle { p, c, r }));
                        }
                    }

                    (
                        Some(Entity::Circle { .. } | Entity::Arc { .. }),
                        Some(Entity::Circle { .. } | Entity::Arc { .. }),
                    ) => {
                        if let (Some(ca), Some(cb)) = (pa, pb) {
                            push_lin(&mut eqs, cid, vec![(ca.0, 1.0), (cb.0, -1.0)], 0.0);
                            push_lin(&mut eqs, cid, vec![(ca.1, 1.0), (cb.1, -1.0)], 0.0);
                        }
                    }
                    _ => {}
                }
            }
            Constraint::Midpoint { a, b } => {
                if let (Some(p), Some(d)) = (map.pt(sketch, a), map.line_diff(sketch, b)) {
                    push_lin(
                        &mut eqs,
                        cid,
                        vec![(p.0, 1.0), (d.x1, -0.5), (d.x2, -0.5)],
                        0.0,
                    );
                    push_lin(
                        &mut eqs,
                        cid,
                        vec![(p.1, 1.0), (d.y1, -0.5), (d.y2, -0.5)],
                        0.0,
                    );
                }
            }
            Constraint::ReferenceMidpoint {
                point, position, ..
            } => {
                if let Some(p) = map.pt(sketch, point) {
                    push_lin(&mut eqs, cid, vec![(p.0, 1.0)], -position.x);
                    push_lin(&mut eqs, cid, vec![(p.1, 1.0)], -position.y);
                }
            }
            Constraint::ReferenceOnEdge { point, edge } => {
                if let (Some(p), Some(carrier)) =
                    (map.pt(sketch, point), sketch.reference_edge(edge))
                {
                    for axis in 0..2 {
                        eqs.push((
                            Some(cid),
                            Eq::ReferenceEdgeCoordinate {
                                p,
                                edge: carrier.clone(),
                                axis,
                            },
                        ));
                    }
                } else {
                    push_lin(&mut eqs, cid, vec![], 1.0);
                }
            }
            Constraint::SpanMidpoint { point, start, end } => {
                if let (Some(p), Some(a), Some(b)) = (
                    map.pt(sketch, point),
                    map.pt(sketch, start),
                    map.pt(sketch, end),
                ) {
                    push_lin(
                        &mut eqs,
                        cid,
                        vec![(p.0, 1.0), (a.0, -0.5), (b.0, -0.5)],
                        0.0,
                    );
                    push_lin(
                        &mut eqs,
                        cid,
                        vec![(p.1, 1.0), (a.1, -0.5), (b.1, -0.5)],
                        0.0,
                    );
                }
            }
            Constraint::Equal { a, b } => match (sketch.entity(a), sketch.entity(b)) {
                (Some(Entity::Line { .. }), Some(Entity::Line { .. })) => {
                    if let (Some(da), Some(db)) =
                        (map.line_diff(sketch, a), map.line_diff(sketch, b))
                    {
                        eqs.push((Some(cid), Eq::EqualLength { a: da, b: db }));
                    }
                }
                _ => {
                    if let (Some(ra), Some(rb)) =
                        (map.radius_var(sketch, a), map.radius_var(sketch, b))
                    {
                        push_lin(&mut eqs, cid, vec![(ra, 1.0), (rb, -1.0)], 0.0);
                    }
                }
            },
            Constraint::Parallel { a, b } => {
                if let (Some(da), Some(db)) = (map.line_diff(sketch, a), map.line_diff(sketch, b)) {
                    eqs.push((Some(cid), Eq::Cross { a: da, b: db }));
                }
            }
            Constraint::Perpendicular { a, b } => {
                if let (Some(da), Some(db)) = (map.line_diff(sketch, a), map.line_diff(sketch, b)) {
                    eqs.push((Some(cid), Eq::Dot { a: da, b: db }));
                }
            }
            Constraint::Tangent { a, b } => {
                let kinds = (sketch.entity(a), sketch.entity(b));
                match kinds {
                    (
                        Some(Entity::Line { .. }),
                        Some(Entity::Circle { .. } | Entity::Arc { .. }),
                    ) => {
                        if let (Some(d), Some(radial)) = (
                            map.line_diff(sketch, a),
                            shared_line_arc_endpoint_radial(sketch, map, a, b),
                        ) {
                            eqs.push((Some(cid), Eq::Dot { a: d, b: radial }));
                            continue;
                        }
                        if let (Some(d), Some(c), Some(r)) = (
                            map.line_diff(sketch, a),
                            map.pt(sketch, b),
                            map.radius_var(sketch, b),
                        ) {
                            let support = tangent_support(sketch, a, b);
                            let sign = tangent_sign(sketch, map, a, c, support);
                            let base = tangent_base(sketch, map, a, b, d);
                            let support_mode = line_is_degenerate(sketch, a);
                            eqs.push((
                                Some(cid),
                                Eq::LineCircleTangent {
                                    d,
                                    c,
                                    base,
                                    r,
                                    sign,
                                    support,
                                    support_mode,
                                },
                            ));
                        }
                    }
                    (
                        Some(Entity::Circle { .. } | Entity::Arc { .. }),
                        Some(Entity::Line { .. }),
                    ) => {
                        if let (Some(d), Some(radial)) = (
                            map.line_diff(sketch, b),
                            shared_line_arc_endpoint_radial(sketch, map, b, a),
                        ) {
                            eqs.push((Some(cid), Eq::Dot { a: d, b: radial }));
                            continue;
                        }
                        if let (Some(d), Some(c), Some(r)) = (
                            map.line_diff(sketch, b),
                            map.pt(sketch, a),
                            map.radius_var(sketch, a),
                        ) {
                            let support = tangent_support(sketch, b, a);
                            let sign = tangent_sign(sketch, map, b, c, support);
                            let base = tangent_base(sketch, map, b, a, d);
                            let support_mode = line_is_degenerate(sketch, b);
                            eqs.push((
                                Some(cid),
                                Eq::LineCircleTangent {
                                    d,
                                    c,
                                    base,
                                    r,
                                    sign,
                                    support,
                                    support_mode,
                                },
                            ));
                        }
                    }
                    (
                        Some(Entity::Circle { .. } | Entity::Arc { .. }),
                        Some(Entity::Circle { .. } | Entity::Arc { .. }),
                    ) => {
                        if let (Some(c1), Some(r1), Some(c2), Some(r2)) = (
                            map.pt(sketch, a),
                            map.radius_var(sketch, a),
                            map.pt(sketch, b),
                            map.radius_var(sketch, b),
                        ) {
                            let d = sketch_point(sketch, map, c1)
                                .distance(sketch_point(sketch, map, c2));
                            let sign = if circle_tangent_is_external(
                                d,
                                current_r(sketch, a),
                                current_r(sketch, b),
                            ) {
                                1.0
                            } else {
                                -1.0
                            };
                            eqs.push((
                                Some(cid),
                                Eq::CircleCircle {
                                    c1,
                                    r1,
                                    c2,
                                    r2,
                                    sign,
                                },
                            ));
                        }
                    }
                    _ => {}
                }
            }
            Constraint::ArcEndpointCoincident { point, arc, end } => {
                if let (Some(p), Some((c, r, a0, a1))) =
                    (map.points.get(&point).copied(), map.arcs.get(&arc).copied())
                {
                    let a = match end {
                        crate::constraint::ArcEndpoint::Start => a0,
                        crate::constraint::ArcEndpoint::End => a1,
                    };
                    eqs.push((Some(cid), Eq::ArcEndX { p, c, r, a }));
                    eqs.push((Some(cid), Eq::ArcEndY { p, c, r, a }));
                }
            }
            Constraint::EqualDistance { origin, a, b } => {
                if let (Some(o), Some(pa), Some(pb)) = (
                    map.points.get(&origin).copied(),
                    map.points.get(&a).copied(),
                    map.points.get(&b).copied(),
                ) {
                    eqs.push((
                        Some(cid),
                        Eq::EqualLength {
                            a: Diff {
                                x1: o.0,
                                y1: o.1,
                                x2: pa.0,
                                y2: pa.1,
                            },
                            b: Diff {
                                x1: o.0,
                                y1: o.1,
                                x2: pb.0,
                                y2: pb.1,
                            },
                        },
                    ));
                }
            }
            Constraint::Concentric { a, b } => {
                if let (Some(ca), Some(cb)) = (map.pt(sketch, a), map.pt(sketch, b)) {
                    push_lin(&mut eqs, cid, vec![(ca.0, 1.0), (cb.0, -1.0)], 0.0);
                    push_lin(&mut eqs, cid, vec![(ca.1, 1.0), (cb.1, -1.0)], 0.0);
                }
            }
            Constraint::Collinear { a, b } => {
                if let (Some(da), Some(db)) = (map.line_diff(sketch, a), map.line_diff(sketch, b)) {
                    eqs.push((Some(cid), Eq::Cross { a: da, b: db }));
                    eqs.push((
                        Some(cid),
                        Eq::CrossPt {
                            d: da,
                            p: (db.x1, db.y1),
                            base: (da.x1, da.y1),
                            support: line_support(sketch, a).unwrap_or((1.0, 0.0)),
                            support_mode: line_is_degenerate(sketch, a),
                        },
                    ));
                }
            }
            Constraint::Symmetry { a, b, axis } => {
                if let Some(axd) = map.line_diff(sketch, axis) {
                    match (sketch.entity(a), sketch.entity(b)) {
                        (Some(Entity::Point { .. }), Some(Entity::Point { .. })) => {
                            if let (Some(pa), Some(pb)) = (map.pt(sketch, a), map.pt(sketch, b)) {
                                eqs.push((
                                    Some(cid),
                                    Eq::SymmetryMid {
                                        a: pa,
                                        b: pb,
                                        axis: axd,
                                    },
                                ));
                                eqs.push((
                                    Some(cid),
                                    Eq::Dot {
                                        a: Diff {
                                            x1: pa.0,
                                            y1: pa.1,
                                            x2: pb.0,
                                            y2: pb.1,
                                        },
                                        b: axd,
                                    },
                                ));
                            }
                        }
                        (Some(Entity::Line { .. }), Some(Entity::Line { .. })) => {
                            if let (Some(da), Some(db)) =
                                (map.line_diff(sketch, a), map.line_diff(sketch, b))
                            {
                                for (p, q) in [
                                    ((da.x1, da.y1), (db.x1, db.y1)),
                                    ((da.x2, da.y2), (db.x2, db.y2)),
                                ] {
                                    eqs.push((
                                        Some(cid),
                                        Eq::SymmetryMid {
                                            a: p,
                                            b: q,
                                            axis: axd,
                                        },
                                    ));
                                    eqs.push((
                                        Some(cid),
                                        Eq::Dot {
                                            a: Diff {
                                                x1: p.0,
                                                y1: p.1,
                                                x2: q.0,
                                                y2: q.1,
                                            },
                                            b: axd,
                                        },
                                    ));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            Constraint::Distance { from, to, value } => {
                let target = sketch.dim_value(&cid, value);
                match (sketch.entity(from), to.map(|t| sketch.entity(t))) {
                    (Some(Entity::Line { .. }), None) => {
                        if let Some(d) = map.line_diff(sketch, from) {
                            eqs.push((
                                Some(cid),
                                Eq::DistPt {
                                    a: (d.x1, d.y1),
                                    b: (d.x2, d.y2),
                                    target,
                                },
                            ));
                        }
                    }

                    (Some(Entity::Point { .. }), Some(Some(Entity::Point { .. }))) => {
                        if let (Some(pa), Some(pb)) =
                            (map.pt(sketch, from), to.and_then(|t| map.pt(sketch, t)))
                        {
                            eqs.push((
                                Some(cid),
                                Eq::DistPt {
                                    a: pa,
                                    b: pb,
                                    target,
                                },
                            ));
                        }
                    }

                    (Some(Entity::Point { .. }), Some(Some(Entity::Line { .. }))) => {
                        if let (Some(p), Some(d)) = (
                            map.pt(sketch, from),
                            to.and_then(|t| map.line_diff(sketch, t)),
                        ) {
                            eqs.push((
                                Some(cid),
                                Eq::LineDist {
                                    d,
                                    q: p,
                                    base: (d.x1, d.y1),
                                    target,
                                },
                            ));
                        }
                    }
                    (Some(Entity::Line { .. }), Some(Some(Entity::Point { .. }))) => {
                        if let (Some(p), Some(d)) = (
                            to.and_then(|t| map.pt(sketch, t)),
                            map.line_diff(sketch, from),
                        ) {
                            eqs.push((
                                Some(cid),
                                Eq::LineDist {
                                    d,
                                    q: p,
                                    base: (d.x1, d.y1),
                                    target,
                                },
                            ));
                        }
                    }

                    (Some(Entity::Line { .. }), Some(Some(Entity::Line { .. }))) => {
                        if let (Some(da), Some(db)) = (
                            map.line_diff(sketch, from),
                            to.and_then(|t| map.line_diff(sketch, t)),
                        ) {
                            let target = if let Some(side) = sketch.offset_side(cid) {
                                target * side
                            } else {
                                let x = read_values(sketch, map);
                                let (dx, dy) = da.val(&x);
                                let len = (dx * dx + dy * dy).sqrt().max(1e-12);
                                let qx = x[db.x1] - x[da.x1];
                                let qy = x[db.y1] - x[da.y1];
                                let cur = (dx * qy - dy * qx) / len;
                                if cur < 0.0 {
                                    -target.abs()
                                } else {
                                    target.abs()
                                }
                            };
                            eqs.push((
                                Some(cid),
                                Eq::LineDist {
                                    d: da,
                                    q: (db.x1, db.y1),
                                    base: (da.x1, da.y1),
                                    target,
                                },
                            ));
                        }
                    }

                    (
                        Some(Entity::Circle { .. } | Entity::Arc { .. }),
                        Some(Some(Entity::Circle { .. } | Entity::Arc { .. })),
                    ) => {
                        if let (Some(r_from), Some(r_to)) = (
                            map.radius_var(sketch, from),
                            to.and_then(|id| map.radius_var(sketch, id)),
                        ) {
                            push_lin(&mut eqs, cid, vec![(r_to, 1.0), (r_from, -1.0)], -target);
                        }
                    }
                    _ => {}
                }
            }
            Constraint::Radius { entity, value } => {
                if let Some(r) = map.radius_var(sketch, entity) {
                    let target = sketch.dim_value(&cid, value);
                    eqs.push((Some(cid), Eq::Radius { r, target }));
                }
            }
            Constraint::ArcAngle { entity, value } => {
                if let Some(&(_, _, start_angle, end_angle)) = map.arcs.get(&entity) {
                    let target = sketch.dim_value(&cid, value).to_radians().abs();
                    push_lin(
                        &mut eqs,
                        cid,
                        vec![(end_angle, 1.0), (start_angle, -1.0)],
                        -target,
                    );
                }
            }
            Constraint::Diameter { entity, value } => {
                if let Some(r) = map.radius_var(sketch, entity) {
                    let target = sketch.dim_value(&cid, value);
                    eqs.push((
                        Some(cid),
                        Eq::Radius {
                            r,
                            target: target / 2.0,
                        },
                    ));
                }
            }
            Constraint::Angle { a, b, value } => {
                let target = sketch.dim_value(&cid, value).to_radians();
                if b.0 == AXIS_SENTINEL.0 {
                    if let Some(d) = map.line_diff(sketch, a) {
                        eqs.push((Some(cid), Eq::AngleAxis { a: d, target }));
                    }
                } else if let (Some(da), Some(db)) =
                    (map.line_diff(sketch, a), map.line_diff(sketch, b))
                {
                    eqs.push((
                        Some(cid),
                        Eq::AngleLines {
                            a: da,
                            b: db,
                            target,
                        },
                    ));
                }
            }
        }
    }

    for (id, target) in pins {
        if let Some(p) = map.points.get(id) {
            eqs.push((
                None,
                Eq::Lin {
                    terms: vec![(p.0, 1.0)],
                    c: -target.x,
                },
            ));
            eqs.push((
                None,
                Eq::Lin {
                    terms: vec![(p.1, 1.0)],
                    c: -target.y,
                },
            ));
        }
    }

    eqs
}

fn sketch_point(sketch: &Sketch, map: &VarMap, p: Pt) -> Vec2 {
    let x = read_values(sketch, map);
    Vec2::new(x[p.0], x[p.1])
}

fn has_point_line_incidence(sketch: &Sketch, point: EntityId, line: EntityId) -> bool {
    sketch.constraints().any(|(_, constraint)| {
        matches!(
            constraint,
            Constraint::Coincident { a, b }
                if (*a == point && *b == line) || (*a == line && *b == point)
        )
    })
}

/// Recover the persistent pre-trim corner associated with one finite carrier
/// endpoint. Chamfer stores it directly as `EqualDistance::origin`; Fillet
/// stores enough topology to recover it as the point incident to the carrier
/// and the arc's other tangent line.
fn trim_origin_for_endpoint(
    sketch: &Sketch,
    carrier: EntityId,
    endpoint: EntityId,
) -> Option<Vec2> {
    for (_, constraint) in sketch.constraints() {
        match *constraint {
            Constraint::EqualDistance { origin, a, b } if a == endpoint || b == endpoint => {
                return sketch.point_position(origin);
            }
            Constraint::ArcEndpointCoincident { point, arc, .. } if point == endpoint => {
                let adjacent_lines: Vec<EntityId> = sketch
                    .constraints()
                    .filter_map(|(_, tangent)| match *tangent {
                        Constraint::Tangent { a, b } if a == arc && b != carrier => Some(b),
                        Constraint::Tangent { a, b } if b == arc && a != carrier => Some(a),
                        _ => None,
                    })
                    .filter(|id| matches!(sketch.entity(*id), Some(Entity::Line { .. })))
                    .collect();
                for adjacent in adjacent_lines {
                    for (candidate, entity) in sketch.entities() {
                        if matches!(entity, Entity::Point { .. })
                            && has_point_line_incidence(sketch, candidate, carrier)
                            && has_point_line_incidence(sketch, candidate, adjacent)
                        {
                            return sketch.point_position(candidate);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// Directed support from the line's original start-side trim corner toward
/// its original end-side trim corner. This preserves all four rectangle edge
/// orientations when a zero-span carrier later reopens.
fn trimmed_carrier_direction(sketch: &Sketch, line: EntityId) -> Option<Vec2> {
    let (start, end) = sketch.line_endpoint_ids(line)?;

    let start_origin =
        trim_origin_for_endpoint(sketch, line, start).or_else(|| sketch.point_position(start))?;
    let end_origin =
        trim_origin_for_endpoint(sketch, line, end).or_else(|| sketch.point_position(end))?;
    let direction = end_origin - start_origin;
    (direction.length() >= DEGENERATE_LINE_EPS).then_some(direction)
}

fn line_is_degenerate(sketch: &Sketch, line: EntityId) -> bool {
    sketch
        .resolved_line(line)
        .is_some_and(|(start, end)| start.distance(end) < CONSUMED_CARRIER_EPS)
}

/// Choose the carrier endpoint owned by this fillet as the support-line
/// origin. When two fillets exactly consume a line, using the line's start
/// for both tangent equations makes the opposite drawing orientation
/// asymmetric and can pin the zero-span edge closed. Ordinary, untrimmed
/// line-circle tangencies retain the historical line-start origin.
fn tangent_base(sketch: &Sketch, map: &VarMap, line: EntityId, curve: EntityId, d: Diff) -> Pt {
    if let Some((start, end)) = sketch.line_endpoint_ids(line) {
        for (_, constraint) in sketch.constraints() {
            if let Constraint::ArcEndpointCoincident { point, arc, .. } = *constraint {
                if arc == curve && (point == start || point == end) {
                    if let Some(base) = map.pt(sketch, point) {
                        return base;
                    }
                }
            }
        }
    }
    (d.x1, d.y1)
}

/// Stable unit direction for a finite line or a fully consumed trim carrier.
/// Axis constraints are a compatibility fallback for older sketches that do
/// not carry the persistent corner topology needed to recover orientation.
fn line_support(sketch: &Sketch, line: EntityId) -> Option<(f64, f64)> {
    let (a, b) = sketch.resolved_line(line)?;
    let direction = b - a;
    let length = direction.length();
    if length >= CONSUMED_CARRIER_EPS {
        return Some((direction.x / length, direction.y / length));
    }

    if let Some(direction) = trimmed_carrier_direction(sketch, line) {
        let length = direction.length();
        return Some((direction.x / length, direction.y / length));
    }

    if length >= DEGENERATE_LINE_EPS {
        return Some((direction.x / length, direction.y / length));
    }

    if sketch.has_constraint_on(
        line,
        |constraint| matches!(constraint, Constraint::Horizontal { entity } if *entity == line),
    ) {
        return Some((1.0, 0.0));
    }
    if sketch.has_constraint_on(
        line,
        |constraint| matches!(constraint, Constraint::Vertical { entity } if *entity == line),
    ) {
        return Some((0.0, 1.0));
    }

    None
}

/// Which side of `line` the circle/arc center `c` sits on (for the signed
/// tangent residual): cross(line_dir, center − line_start) ≥ 0 → +1, else −1.
fn tangent_support(sketch: &Sketch, line: EntityId, curve: EntityId) -> (f64, f64) {
    if let Some(support) = line_support(sketch, line) {
        return support;
    }

    if let Some((a, _)) = sketch.resolved_line(line) {
        let center = match sketch.entity(curve) {
            Some(Entity::Circle { center, .. }) | Some(Entity::Arc { center, .. }) => *center,
            _ => Vec2::ZERO,
        };
        let tangent = (a - center).perp();
        let tangent_length = tangent.length();
        if tangent_length >= DEGENERATE_LINE_EPS {
            return (tangent.x / tangent_length, tangent.y / tangent_length);
        }
    }

    (1.0, 0.0)
}

fn tangent_sign(sketch: &Sketch, map: &VarMap, line: EntityId, c: Pt, support: (f64, f64)) -> f64 {
    let t = sketch
        .resolved_line(line)
        .map(|(a, b)| {
            let raw = b - a;
            let d = if raw.length() < CONSUMED_CARRIER_EPS {
                Vec2::new(support.0, support.1)
            } else {
                raw
            };
            let cc = sketch_point(sketch, map, c);
            d.x * (cc.y - a.y) - d.y * (cc.x - a.x)
        })
        .unwrap_or(1.0);
    if t >= 0.0 {
        1.0
    } else {
        -1.0
    }
}

fn current_r(sketch: &Sketch, id: EntityId) -> f64 {
    match sketch.entity(id) {
        Some(Entity::Circle { radius, .. }) | Some(Entity::Arc { radius, .. }) => *radius,
        _ => 0.0,
    }
}

fn eval_all(
    eqs: &[(Option<ConstraintId>, Eq)],
    x: &[f64],
    n: usize,
) -> (Vec<f64>, Vec<Vec<(usize, f64)>>) {
    let mut f = Vec::with_capacity(eqs.len());
    let mut rows = Vec::with_capacity(eqs.len());
    for (_, eq) in eqs {
        let (r, mut row) = eq.eval(x);

        row.sort_by_key(|(i, _)| *i);
        let mut merged: Vec<(usize, f64)> = Vec::with_capacity(row.len());
        for (i, v) in row {
            if let Some(last) = merged.last_mut() {
                if last.0 == i {
                    last.1 += v;
                    continue;
                }
            }
            merged.push((i, v));
        }
        debug_assert!(merged.iter().all(|(i, _)| *i < n));
        f.push(r);
        rows.push(merged);
    }
    (f, rows)
}

fn max_abs(f: &[f64]) -> f64 {
    f.iter().fold(0.0_f64, |m, v| m.max(v.abs()))
}

/// Find the constraint/variable island that currently needs work.
///
/// Sketches commonly contain several disconnected groups of geometry. Once
/// one group is solved, including its tiny floating-point residuals in every
/// later LM step can make an otherwise simple operation on another group
/// stall or move the wrong geometry. Start with equations that are outside
/// tolerance, then follow the equation-variable graph to include every
/// equation coupled to those variables. Satisfied, disconnected islands stay
/// exactly where the user left them and do not enlarge the linear solve.
fn active_solve_component(
    f: &[f64],
    jac: &[Vec<(usize, f64)>],
    variable_count: usize,
) -> (Vec<bool>, Vec<bool>) {
    let mut active_rows: Vec<bool> = f.iter().map(|value| value.abs() > TOL).collect();
    let mut active_variables = vec![false; variable_count];

    loop {
        let mut changed = false;

        for (row_index, row) in jac.iter().enumerate() {
            if !active_rows[row_index]
                && row.iter().any(|(variable, _)| active_variables[*variable])
            {
                active_rows[row_index] = true;
                changed = true;
            }
        }

        for (row_index, row) in jac.iter().enumerate() {
            if !active_rows[row_index] {
                continue;
            }
            for &(variable, _) in row {
                if !active_variables[variable] {
                    active_variables[variable] = true;
                    changed = true;
                }
            }
        }

        if !changed {
            break;
        }
    }

    if active_rows.iter().any(|active| *active) && !active_variables.iter().any(|active| *active) {
        active_rows.fill(true);
        active_variables.fill(true);
    }

    (active_rows, active_variables)
}

fn selected_squared_norm(f: &[f64], active_rows: &[bool]) -> f64 {
    f.iter()
        .zip(active_rows)
        .filter_map(|(value, active)| active.then_some(value * value))
        .sum()
}

/// Wrap an angle residual to (−π, π] (branch-safe for Newton steps).
fn wrap_angle(a: f64) -> f64 {
    const TAU: f64 = std::f64::consts::TAU;
    let mut w = a % TAU;
    if w <= -std::f64::consts::PI {
        w += TAU;
    } else if w > std::f64::consts::PI {
        w -= TAU;
    }
    w
}

/// Distance between two points given their variable pairs.
fn a2dist(x: &[f64], a: Pt, b: Pt) -> f64 {
    ((x[b.0] - x[a.0]).powi(2) + (x[b.1] - x[a.1]).powi(2)).sqrt()
}

/// Solve A·x = b with Gaussian elimination + partial pivoting.
/// Returns false on singularity.
fn solve_square(a: &mut [Vec<f64>], b: &mut [f64]) -> bool {
    let n = a.len();
    for col in 0..n {
        let mut pivot = col;
        for r in col + 1..n {
            if a[r][col].abs() > a[pivot][col].abs() {
                pivot = r;
            }
        }
        if a[pivot][col].abs() < 1e-14 {
            return false;
        }
        if pivot != col {
            a.swap(pivot, col);
            b.swap(pivot, col);
        }
        let d = a[col][col];
        for r in col + 1..n {
            let factor = a[r][col] / d;
            if factor == 0.0 {
                continue;
            }
            let (before, current) = a.split_at_mut(r);
            for (value, pivot_value) in current[0][col..n].iter_mut().zip(&before[col][col..n]) {
                *value -= factor * pivot_value;
            }
            b[r] -= factor * b[col];
        }
    }
    for col in (0..n).rev() {
        let mut s = b[col];
        for c in col + 1..n {
            s -= a[col][c] * b[c];
        }
        b[col] = s / a[col][col];
    }
    true
}

struct JacobianEchelon {
    rows: Vec<Vec<f64>>,
    pivots: Vec<usize>,
}

/// Row echelon form with the same partial-pivot tolerance used for DOF counting.
fn jacobian_echelon(jac: &[Vec<(usize, f64)>], n: usize) -> JacobianEchelon {
    let m = jac.len();
    if m == 0 || n == 0 {
        return JacobianEchelon {
            rows: Vec::new(),
            pivots: Vec::new(),
        };
    }
    let mut a: Vec<Vec<f64>> = jac
        .iter()
        .map(|row| {
            let mut r = vec![0.0; n];
            for &(i, v) in row {
                r[i] = v;
            }
            r
        })
        .collect();
    let max_el = a
        .iter()
        .flat_map(|r| r.iter())
        .fold(0.0_f64, |acc, v| acc.max(v.abs()));
    let eps = 1e-9 * max_el.max(1.0);
    let mut pivots = Vec::with_capacity(m.min(n));
    let mut rank = 0;
    for col in 0..n {
        if rank >= m {
            break;
        }
        let mut pivot = rank;
        for r in rank + 1..m {
            if a[r][col].abs() > a[pivot][col].abs() {
                pivot = r;
            }
        }
        if a[pivot][col].abs() <= eps {
            continue;
        }
        a.swap(pivot, rank);
        let d = a[rank][col];
        for r in rank + 1..m {
            let factor = a[r][col] / d;
            if factor == 0.0 {
                continue;
            }
            let (before, current) = a.split_at_mut(r);
            for (value, pivot_value) in current[0][col..n].iter_mut().zip(&before[rank][col..n]) {
                *value -= factor * pivot_value;
            }
            current[0][col] = 0.0;
        }
        pivots.push(col);
        rank += 1;
    }
    JacobianEchelon { rows: a, pivots }
}

fn rank_of(jac: &[Vec<(usize, f64)>], n: usize) -> usize {
    jacobian_echelon(jac, n).pivots.len()
}

/// A parameter is fixed only when its reduced equation has no free dependence.
fn defined_variables(jac: &[Vec<(usize, f64)>], n: usize) -> (usize, Vec<bool>) {
    let JacobianEchelon { mut rows, pivots } = jacobian_echelon(jac, n);
    for (pivot_row, &col) in pivots.iter().enumerate().rev() {
        let (upper, pivot) = rows.split_at_mut(pivot_row);
        let divisor = pivot[0][col];
        for row in upper {
            let factor = row[col] / divisor;
            if factor != 0.0 {
                for (value, pivot_value) in row[col..n].iter_mut().zip(&pivot[0][col..n]) {
                    *value -= factor * pivot_value;
                }
                row[col] = 0.0;
            }
        }
    }
    let mut defined = vec![false; n];
    for (row, &col) in rows.iter().zip(&pivots) {
        defined[col] = row
            .iter()
            .enumerate()
            .all(|(other, value)| other == col || (value / row[col]).abs() <= 1e-9);
    }
    (pivots.len(), defined)
}

fn trim_reference_segment(sketch: &Sketch, line: EntityId) -> Option<fillet::LineSeg> {
    let (start, end) = sketch.line_endpoint_ids(line)?;
    let a =
        trim_origin_for_endpoint(sketch, line, start).or_else(|| sketch.point_position(start))?;
    let b = trim_origin_for_endpoint(sketch, line, end).or_else(|| sketch.point_position(end))?;
    (a.distance(b) >= DEGENERATE_LINE_EPS).then_some(fillet::LineSeg { a, b })
}

/// Finite trimmed fillets also need an exact seed at their singular endpoint
/// tangency; accept only their explicit endpoint and shared-corner topology.
fn finite_trim_radius_seed_is_supported(
    sketch: &Sketch,
    arc: EntityId,
    lines: &[EntityId],
) -> bool {
    let bindings: Vec<_> = sketch
        .constraints()
        .filter_map(|(_, constraint)| match *constraint {
            Constraint::ArcEndpointCoincident {
                point,
                arc: owner,
                end,
            } if owner == arc => Some((point, end)),
            _ => None,
        })
        .collect();
    if bindings.len() != 2 || bindings[0].0 == bindings[1].0 || bindings[0].1 == bindings[1].1 {
        return false;
    }
    let endpoints: Vec<_> = lines
        .iter()
        .filter_map(|line| {
            let (start, end) = sketch.line_endpoint_ids(*line)?;
            let owned: Vec<_> = bindings
                .iter()
                .filter(|(point, _)| *point == start || *point == end)
                .collect();
            (owned.len() == 1).then(|| owned[0].0)
        })
        .collect();
    if endpoints.len() != 2 || endpoints[0] == endpoints[1] {
        return false;
    }
    let corners: Vec<_> = sketch
        .entities()
        .filter_map(|(id, entity)| {
            (matches!(entity, Entity::Point { .. })
                && !endpoints.contains(&id)
                && lines
                    .iter()
                    .all(|line| has_point_line_incidence(sketch, id, *line)))
            .then_some(id)
        })
        .collect();
    if corners.len() != 1 {
        return false;
    }
    let Some(corner) = sketch.point_position(corners[0]) else {
        return false;
    };
    lines
        .iter()
        .zip(endpoints)
        .all(|(line, endpoint)| trim_origin_for_endpoint(sketch, *line, endpoint) == Some(corner))
}

/// A consumed fillet carrier starts from a singular zero-span configuration.
/// Proven finite trimmed fillets can also have singular endpoint tangency.
/// When either radius changes, reconstruct the exact local fillet from the
/// persistent pre-trim corners before LM begins. This is only an initial
/// guess; the complete constraint system still determines the final geometry
/// and the crossed-carrier guard rejects values beyond the topology boundary.
fn seed_consumed_radius_edit(sketch: &Sketch, map: &VarMap, x: &mut [f64]) {
    let radius_edits = sketch
        .constraints()
        .filter_map(|(cid, constraint)| match *constraint {
            Constraint::Radius { entity, value } if !sketch.is_reference_dimension(&cid) => {
                let radius_var = map.radius_var(sketch, entity)?;
                let current = x[radius_var].abs();
                let target = sketch.dim_value(&cid, value).abs();
                ((current - target).abs() > TOL).then_some((entity, radius_var, target))
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    for (arc, radius_var, target) in radius_edits {
        let mut tangent_lines = sketch
            .constraints()
            .filter_map(|(_, constraint)| match *constraint {
                Constraint::Tangent { a, b }
                    if a == arc && matches!(sketch.entity(b), Some(Entity::Line { .. })) =>
                {
                    Some(b)
                }
                Constraint::Tangent { a, b }
                    if b == arc && matches!(sketch.entity(a), Some(Entity::Line { .. })) =>
                {
                    Some(a)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        tangent_lines.sort_unstable();
        tangent_lines.dedup();
        if tangent_lines.len() != 2 {
            continue;
        }
        let consumed = tangent_lines
            .iter()
            .any(|line| line_is_degenerate(sketch, *line));
        if !consumed && !finite_trim_radius_seed_is_supported(sketch, arc, &tangent_lines) {
            continue;
        }

        let (Some(first), Some(second)) = (
            trim_reference_segment(sketch, tangent_lines[0]),
            trim_reference_segment(sketch, tangent_lines[1]),
        ) else {
            continue;
        };
        let Ok(result) = fillet::fillet_lines(&first, &second, target) else {
            continue;
        };
        let Some(&(center, _, start_angle, end_angle)) = map.arcs.get(&arc) else {
            continue;
        };
        x[center.0] = result.arc.center.x;
        x[center.1] = result.arc.center.y;
        x[radius_var] = target;

        for (line, tangent) in tangent_lines
            .into_iter()
            .zip([result.tangent_on_l1, result.tangent_on_l2])
        {
            let endpoint = sketch.constraints().find_map(|(_, constraint)| {
                let Constraint::ArcEndpointCoincident {
                    point,
                    arc: owner,
                    end,
                } = *constraint
                else {
                    return None;
                };
                (owner == arc
                    && sketch
                        .line_endpoint_ids(line)
                        .is_some_and(|(start, finish)| point == start || point == finish))
                .then_some((point, end))
            });
            let Some((point, end)) = endpoint else {
                continue;
            };
            let point_vars = map.points[&point];
            x[point_vars.0] = tangent.x;
            x[point_vars.1] = tangent.y;
            let angle = (tangent.y - result.arc.center.y).atan2(tangent.x - result.arc.center.x);
            match end {
                ArcEndpoint::Start => x[start_angle] = angle,
                ArcEndpoint::End => x[end_angle] = angle,
            }
        }
    }
}

/// Damped Newton (LM) solve; writes the solution back into the sketch.
pub fn solve(sketch: &mut Sketch, pins: &[(EntityId, Vec2)]) -> Analysis {
    solve_with_stays(sketch, pins, &SolveStays::default())
}

/// Seed a point drag on an isolated, already-collinear line group by moving
/// its support perpendicular to itself. Along-support freedom belongs only to
/// the dragged endpoint. This avoids asking LM to discover a common support
/// translation through the singular aligned start, without adding constraints.
fn seed_free_collinear_drag(
    sketch: &Sketch,
    map: &VarMap,
    pins: &[(EntityId, Vec2)],
    x: &mut [f64],
) {
    let [(point, target)] = pins else { return };
    let Some(origin) = sketch.point_position(*point) else {
        return;
    };
    if !target.x.is_finite() || !target.y.is_finite() {
        return;
    }
    let Some(first) = sketch
        .entities()
        .filter_map(|(id, entity)| match entity {
            Entity::Line { start, end } if start == point || end == point => Some(id),
            _ => None,
        })
        .find(|id| {
            sketch.constraints().any(|(_, constraint)| {
            matches!(constraint, Constraint::Collinear { a, b } if a == id || b == id)
        })
        })
    else {
        return;
    };
    let mut lines = BTreeSet::from([first]);
    loop {
        let before = lines.len();
        for (_, constraint) in sketch.constraints() {
            if let Constraint::Collinear { a, b } = constraint {
                if lines.contains(a) || lines.contains(b) {
                    lines.extend([*a, *b]);
                }
            }
        }
        if lines.len() == before {
            break;
        }
    }
    if lines.len() < 2 {
        return;
    }
    let mut points = BTreeSet::new();
    for line in &lines {
        let Some((start, end)) = sketch.line_endpoint_ids(*line) else {
            return;
        };
        points.extend([start, end]);
    }
    // Shared points or any other relation make this a wider constrained graph;
    // retain its general solve rather than inventing a rigid translation.
    if sketch.entities().any(|(id, entity)| matches!(entity,
        Entity::Line { start, end } if !lines.contains(&id) && (points.contains(start) || points.contains(end)))) {
        return;
    }
    for (_, constraint) in sketch.constraints() {
        if !constraint
            .referenced_entities()
            .iter()
            .any(|id| lines.contains(id) || points.contains(id))
        {
            continue;
        }
        match constraint {
            Constraint::Horizontal { entity } | Constraint::Vertical { entity }
                if lines.contains(entity) => {}
            Constraint::Collinear { a, b } if lines.contains(a) && lines.contains(b) => {}
            _ => return,
        }
    }
    let Some((a, b)) = sketch.resolved_line(*lines.first().unwrap()) else {
        return;
    };
    let direction = b - a;
    let length = direction.length();
    if !length.is_finite() || length < DEGENERATE_LINE_EPS {
        return;
    }
    let direction = direction * (1.0 / length);
    // Only an already-satisfied common support qualifies for this exact seed.
    if points.iter().any(|id| {
        sketch.point_position(*id).is_none_or(|p| {
            let offset = p - a;
            !p.x.is_finite()
                || !p.y.is_finite()
                || (direction.x * offset.y - direction.y * offset.x).abs() > TOL
        })
    }) {
        return;
    }
    let delta = *target - origin;
    let translation = delta - direction * delta.dot(direction);
    let mut candidate = x.to_vec();
    for id in points {
        let Some(position) = sketch.point_position(id) else {
            return;
        };
        let Some(vars) = map.points.get(&id) else {
            return;
        };
        let position = if id == *point {
            *target
        } else {
            position + translation
        };
        candidate[vars.0] = position.x;
        candidate[vars.1] = position.y;
    }
    // A collapsed seed must not preempt the general solver's ability to move
    // the other endpoint. Retain the original guess in that boundary case.
    for line in lines {
        let (start, end) = sketch.line_endpoint_ids(line).unwrap();
        let (a, b) = (map.points[&start], map.points[&end]);
        let pre = a2dist(x, a, b);
        let post = a2dist(&candidate, a, b);
        if !post.is_finite() || post < (pre * 0.01).min(0.1) || post < 1e-9 {
            return;
        }
    }
    x.copy_from_slice(&candidate);
}

/// Solve while retaining selected authored properties.
///
/// These stays are operation-local stabilization equations, not persistent
/// sketch constraints. They let direction-only tools choose the nearest
/// rigid rotation instead of stretching a free endpoint toward infinity,
/// and let size-only tools resize without rotating their carriers. The
/// returned rank/DOF still describes only the actual sketch constraints.
pub(crate) fn solve_with_stays(
    sketch: &mut Sketch,
    pins: &[(EntityId, Vec2)],
    stays: &SolveStays,
) -> Analysis {
    let map = build_var_map(sketch);
    let mut eqs = build_equations(sketch, &map, pins);
    let hard_equation_count = eqs.len();
    for &(line, target) in &stays.line_lengths {
        let Some((start, end)) = sketch.line_endpoint_ids(line) else {
            continue;
        };
        let (Some(a), Some(b)) = (map.pt(sketch, start), map.pt(sketch, end)) else {
            continue;
        };
        if target.is_finite() && target >= DEGENERATE_LINE_EPS {
            eqs.push((None, Eq::DistPt { a, b, target }));
        }
    }
    for &(line, target) in &stays.line_angles {
        if target.is_finite() {
            if let Some(a) = map.line_diff(sketch, line) {
                eqs.push((None, Eq::AngleAxis { a, target }));
            }
        }
    }
    for &(line, target) in &stays.line_midpoints {
        let Some((start, end)) = sketch.line_endpoint_ids(line) else {
            continue;
        };
        let (Some(a), Some(b)) = (map.pt(sketch, start), map.pt(sketch, end)) else {
            continue;
        };
        if target.x.is_finite() && target.y.is_finite() {
            eqs.push((
                None,
                Eq::Lin {
                    terms: vec![(a.0, 0.5), (b.0, 0.5)],
                    c: -target.x,
                },
            ));
            eqs.push((
                None,
                Eq::Lin {
                    terms: vec![(a.1, 0.5), (b.1, 0.5)],
                    c: -target.y,
                },
            ));
        }
    }
    for &(first, second, target) in &stays.point_pair_distances {
        if !target.is_finite() || target < DEGENERATE_LINE_EPS {
            continue;
        }
        if let (Some(a), Some(b)) = (map.pt(sketch, first), map.pt(sketch, second)) {
            eqs.push((None, Eq::DistPt { a, b, target }));
        }
    }
    for &(first, second, target) in &stays.point_pair_angles {
        if !target.is_finite() {
            continue;
        }
        if let (Some(a), Some(b)) = (map.pt(sketch, first), map.pt(sketch, second)) {
            eqs.push((
                None,
                Eq::AngleAxis {
                    a: Diff {
                        x1: a.0,
                        y1: a.1,
                        x2: b.0,
                        y2: b.1,
                    },
                    target,
                },
            ));
        }
    }
    for &(first, second, target) in &stays.point_pair_midpoints {
        if !target.x.is_finite() || !target.y.is_finite() {
            continue;
        }
        if let (Some(a), Some(b)) = (map.pt(sketch, first), map.pt(sketch, second)) {
            eqs.push((
                None,
                Eq::Lin {
                    terms: vec![(a.0, 0.5), (b.0, 0.5)],
                    c: -target.x,
                },
            ));
            eqs.push((
                None,
                Eq::Lin {
                    terms: vec![(a.1, 0.5), (b.1, 0.5)],
                    c: -target.y,
                },
            ));
        }
    }
    for &(point, target) in &stays.point_positions {
        if !target.x.is_finite() || !target.y.is_finite() {
            continue;
        }
        if let Some(point) = map.pt(sketch, point) {
            eqs.push((
                None,
                Eq::Lin {
                    terms: vec![(point.0, 1.0)],
                    c: -target.x,
                },
            ));
            eqs.push((
                None,
                Eq::Lin {
                    terms: vec![(point.1, 1.0)],
                    c: -target.y,
                },
            ));
        }
    }
    for &(curve, target) in &stays.curve_radii {
        if !target.is_finite() || target < DEGENERATE_LINE_EPS {
            continue;
        }
        if let Some(r) = map.radius_var(sketch, curve) {
            eqs.push((None, Eq::Radius { r, target }));
        }
    }
    for &(curve, target) in &stays.curve_centers {
        if !target.x.is_finite() || !target.y.is_finite() {
            continue;
        }
        if let Some(center) = map.pt(sketch, curve) {
            eqs.push((
                None,
                Eq::Lin {
                    terms: vec![(center.0, 1.0)],
                    c: -target.x,
                },
            ));
            eqs.push((
                None,
                Eq::Lin {
                    terms: vec![(center.1, 1.0)],
                    c: -target.y,
                },
            ));
        }
    }
    let mut rigid_values = BTreeMap::new();
    for &id in &stays.rigid_entities {
        match sketch.entity(id) {
            Some(Entity::Point { position }) => {
                let p = map.points[&id];
                rigid_values.extend([(p.0, position.x), (p.1, position.y)]);
            }
            Some(Entity::Line { start, end }) => {
                for point in [start, end] {
                    if let Some(position) = sketch.point_position(*point) {
                        let p = map.points[point];
                        rigid_values.extend([(p.0, position.x), (p.1, position.y)]);
                    }
                }
            }
            Some(Entity::Circle { center, radius }) => {
                let (c, r) = map.circles[&id];
                rigid_values.extend([(c.0, center.x), (c.1, center.y), (r, *radius)]);
            }
            Some(Entity::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            }) => {
                let (c, r, a0, a1) = map.arcs[&id];
                rigid_values.extend([
                    (c.0, center.x),
                    (c.1, center.y),
                    (r, *radius),
                    (a0, *start_angle),
                    (a1, *end_angle),
                ]);
            }
            Some(Entity::Spline { points }) => {
                for (position, p) in points.iter().zip(&map.splines[&id]) {
                    rigid_values.extend([(p.0, position.x), (p.1, position.y)]);
                }
            }
            None => {}
        }
    }
    eqs.extend(rigid_values.into_iter().map(|(variable, target)| {
        (
            None,
            Eq::Lin {
                terms: vec![(variable, 1.)],
                c: -target,
            },
        )
    }));
    let n = map.n;
    let m = hard_equation_count;

    if n == 0 {
        return Analysis {
            converged: true,
            iterations: 0,
            residual: 0.0,
            unknowns: 0,
            equations: m,
            rank: 0,
            dof: 0,
            entity_free: HashMap::new(),
        };
    }

    let mut x = read_values(sketch, &map);
    seed_consumed_radius_edit(sketch, &map, &mut x);

    let pre_line_len: Vec<(usize, usize, usize, usize, f64, bool)> = sketch
        .entities()
        .filter_map(|(id, e)| match e {
            Entity::Line { start, end } => {
                let a = map.points[start];
                let b = map.points[end];
                Some((
                    a.0,
                    a.1,
                    b.0,
                    b.1,
                    a2dist(&x, a, b),
                    line_has_trimmed_endpoint(sketch, id, *start, *end),
                ))
            }
            _ => None,
        })
        .collect();
    let pre_radius: Vec<(usize, f64)> = sketch
        .entities()
        .filter_map(|(id, e)| match e {
            Entity::Circle { .. } => {
                let r = map.circles[&id].1;
                Some((r, x[r].abs()))
            }
            Entity::Arc { .. } => {
                let r = map.arcs[&id].1;
                Some((r, x[r].abs()))
            }
            _ => None,
        })
        .collect();

    // Capture collapse guards before the drag seed, so a pin cannot make a
    // collapsed original carrier become its own accepted baseline.
    if eqs.len() == hard_equation_count {
        seed_free_collinear_drag(sketch, &map, pins, &mut x);
    }
    let (mut f, mut jac) = eval_all(&eqs, &x, n);
    let mut residual = max_abs(&f);
    let (active_rows, active_variables) = active_solve_component(&f, &jac, n);
    let mut cost = selected_squared_norm(&f, &active_rows);

    let mut movement_weight = vec![1.0; n];
    let prefers_preserved_radius = |entity: EntityId| {
        let tangent_participant = sketch.constraints().any(|(_, constraint)| {
            matches!(constraint, Constraint::Tangent { a, b } if *a == entity || *b == entity)
        });
        let directly_dimensioned = sketch.constraints().any(|(cid, constraint)| {
            if sketch.is_reference_dimension(&cid) {
                return false;
            }
            matches!(
                constraint,
                Constraint::Radius { entity: target, .. }
                    | Constraint::Diameter { entity: target, .. }
                    if *target == entity
            )
        });
        tangent_participant && !directly_dimensioned
    };
    for (entity, (_, radius)) in &map.circles {
        if prefers_preserved_radius(*entity) {
            movement_weight[*radius] = 1024.0;
        }
    }
    for (entity, (_, radius, ..)) in &map.arcs {
        if prefers_preserved_radius(*entity) {
            movement_weight[*radius] = 1024.0;
        }
    }
    let mut lambda = 1e-3;
    let mut iterations = 0;

    while residual > TOL && iterations < MAX_ITERS {
        iterations += 1;

        let mut ata = vec![vec![0.0; n]; n];
        let mut jtf = vec![0.0; n];
        for ((row, &fr), active) in jac.iter().zip(f.iter()).zip(&active_rows) {
            if !active {
                continue;
            }
            for &(i, vi) in row {
                jtf[i] -= vi * fr;
                for &(j, vj) in row {
                    ata[i][j] += vi * vj;
                }
            }
        }
        for i in 0..n {
            if active_variables[i] {
                ata[i][i] += lambda * movement_weight[i] * ata[i][i].max(1e-12);
            } else {
                ata[i][i] = 1.0;
            }
        }
        let mut rhs = jtf;
        if !solve_square(&mut ata, &mut rhs) {
            lambda *= 8.0;
            continue;
        }

        let mut accepted = None;
        for backtrack in 0..8 {
            let scale = 0.5_f64.powi(backtrack);
            let mut candidate = x.clone();
            for i in 0..n {
                if active_variables[i] {
                    candidate[i] += rhs[i] * scale;
                }
            }
            let (candidate_f, candidate_jac) = eval_all(&eqs, &candidate, n);
            let candidate_cost = selected_squared_norm(&candidate_f, &active_rows);
            if candidate_cost < cost {
                accepted = Some((candidate, candidate_f, candidate_jac, candidate_cost, scale));
                break;
            }
        }
        if let Some((x_new, f_new, jac_new, cost_new, scale)) = accepted {
            x = x_new;
            f = f_new;
            jac = jac_new;
            cost = cost_new;
            residual = max_abs(&f);
            lambda = if scale == 1.0 {
                (lambda / 4.0).max(1e-12)
            } else {
                (lambda / 2.0).max(1e-12)
            };
        } else {
            lambda *= 6.0;
        }
    }

    let converged = residual <= TOL;

    if converged {
        let mut ata = vec![vec![0.0; n]; n];
        let mut jtf = vec![0.0; n];
        for ((row, &fr), active) in jac.iter().zip(f.iter()).zip(&active_rows) {
            if !active {
                continue;
            }
            for &(i, vi) in row {
                jtf[i] -= vi * fr;
                for &(j, vj) in row {
                    ata[i][j] += vi * vj;
                }
            }
        }
        for (i, active) in active_variables.iter().enumerate() {
            if !active {
                ata[i][i] = 1.0;
            }
        }
        let mut rhs = jtf;
        if solve_square(&mut ata, &mut rhs) {
            let mut x_new = x.clone();
            for i in 0..n {
                if active_variables[i] {
                    x_new[i] += rhs[i];
                }
            }
            let (f_new, jac_new) = eval_all(&eqs, &x_new, n);
            let res_new = max_abs(&f_new);
            if res_new < residual {
                x = x_new;
                f = f_new;
                jac = jac_new;
                residual = res_new;
            }
        }
    }

    let converged = residual <= TOL;

    let crossed_trimmed_carrier = sketch.entities().any(|(id, entity)| {
        let Entity::Line { start, end } = *entity else {
            return false;
        };
        if !line_has_trimmed_endpoint(sketch, id, start, end) {
            return false;
        }
        let Some(support) = trimmed_carrier_direction(sketch, id) else {
            return false;
        };
        let a = map.points[&start];
        let b = map.points[&end];
        let post = Vec2::new(x[b.0] - x[a.0], x[b.1] - x[a.1]);
        post.dot(support) / support.length() < -DEGENERATE_LINE_EPS
    });

    let invalid_geometry = crossed_trimmed_carrier
        || pre_line_len
            .iter()
            .any(|&(x1, y1, x2, y2, pre, intentional_trim)| {
                let post = ((x[x2] - x[x1]).powi(2) + (x[y2] - x[y1]).powi(2)).sqrt();
                !intentional_trim && (post < (pre * 0.01).min(0.1) || post < 1e-9)
            })
        || pre_radius
            .iter()
            .any(|&(r, pre)| x[r].abs() < (pre * 0.01).min(0.1) || x[r].abs() < 1e-9);
    let converged = converged && !invalid_geometry;
    if converged {
        write_values(sketch, &map, &x);
    }

    let hard_residual = max_abs(&f[..hard_equation_count]);
    finish_analysis(
        sketch,
        &map,
        &eqs[..hard_equation_count],
        &jac[..hard_equation_count],
        converged,
        iterations,
        hard_residual,
    )
}

/// Only a retained trim corner permits a carrier to collapse. An ordinary
/// line joined tangentially to an arc must keep the normal collapse guard.
fn endpoint_is_trimmed(sketch: &Sketch, line: EntityId, point: EntityId) -> bool {
    trim_origin_for_endpoint(sketch, line, point).is_some()
}

/// Internal trim ownership distinguishes a valid topology transition from an
/// accidental solver collapse of an ordinary line. One owned endpoint is
/// sufficient: an exact one-sided fillet/chamfer can consume the full edge.
fn line_has_trimmed_endpoint(
    sketch: &Sketch,
    line: EntityId,
    start: EntityId,
    end: EntityId,
) -> bool {
    endpoint_is_trimmed(sketch, line, start) || endpoint_is_trimmed(sketch, line, end)
}

/// Presentation predicate for a stable carrier whose visible span is fully
/// consumed. Keeping the entity lets later dimension edits reopen it.
pub(crate) fn line_is_consumed_trim_carrier(sketch: &Sketch, line: EntityId) -> bool {
    let Some((start, end)) = sketch.line_endpoint_ids(line) else {
        return false;
    };
    line_has_trimmed_endpoint(sketch, line, start, end)
        && sketch
            .resolved_line(line)
            .is_some_and(|(a, b)| a.distance(b) < CONSUMED_CARRIER_EPS)
}

/// Analysis without mutation (residual at the current state).
pub fn analyze(sketch: &Sketch) -> Analysis {
    let map = build_var_map(sketch);
    let eqs = build_equations(sketch, &map, &[]);
    let x = read_values(sketch, &map);
    let (f, jac) = eval_all(&eqs, &x, map.n);
    let residual = max_abs(&f);
    finish_analysis(sketch, &map, &eqs, &jac, residual <= TOL, 0, residual)
}

/// Residual of one constraint's own equations at the sketch's current
/// state — used by over-constraint rejection (D4.2).
pub fn constraint_residual(sketch: &Sketch, cid: ConstraintId) -> f64 {
    let map = build_var_map(sketch);
    let eqs = build_equations(sketch, &map, &[]);
    let x = read_values(sketch, &map);
    eqs.iter()
        .filter(|(owner, _)| *owner == Some(cid))
        .map(|(_, eq)| eq.eval(&x).0.abs())
        .fold(0.0_f64, |a, b| a.max(b))
}

/// Jacobian rank of the current solved geometry while omitting every row
/// owned by `excluded` constraints.
///
/// Rank comparisons for constraint admission must be made at one geometric
/// state. Comparing the old sketch's rank with the newly solved sketch's rank
/// can misclassify nonlinear relations when the solve moves through a
/// singular pose. This helper keeps the final geometry fixed and changes only
/// the equation set.
pub(crate) fn rank_excluding_constraints(sketch: &Sketch, excluded: &[ConstraintId]) -> usize {
    let map = build_var_map_excluding(sketch, &excluded.iter().copied().collect());
    let eqs = build_equations(sketch, &map, &[]);
    let x = read_values(sketch, &map);
    let (_, jac) = eval_all(&eqs, &x, map.n);
    let retained = eqs
        .iter()
        .zip(jac)
        .filter_map(|((owner, _), row)| {
            let omitted = owner.is_some_and(|cid| excluded.contains(&cid));
            (!omitted).then_some(row)
        })
        .collect::<Vec<_>>();
    rank_of(&retained, map.n)
}

/// Decide admission at the solved pose, without mistaking a singular
/// Jacobian for a geometric implication. At a line/circle tangency, for
/// example, the two incidence rows have the same first derivative although
/// a point on the circle can still move away from the line.
pub(crate) fn constraints_are_redundant(sketch: &Sketch, proposed: &[ConstraintId]) -> bool {
    let map = build_var_map_excluding(sketch, &proposed.iter().copied().collect());
    let eqs = build_equations(sketch, &map, &[]);
    let x = read_values(sketch, &map);
    let (_, jac) = eval_all(&eqs, &x, map.n);
    let is_proposed =
        |owner: &Option<ConstraintId>| owner.is_some_and(|cid| proposed.contains(&cid));
    let retained = eqs
        .iter()
        .zip(&jac)
        .filter(|((owner, _), _)| !is_proposed(owner))
        .map(|(_, row)| row.clone())
        .collect::<Vec<_>>();
    let reduced_rank = rank_of(&retained, map.n);
    if rank_of(&jac, map.n) > reduced_rank {
        return false;
    }
    if reduced_rank == map.n
        || eqs
            .iter()
            .all(|(_, eq)| matches!(eq, Eq::Lin { .. } | Eq::Radius { .. }))
    {
        return true;
    }

    let variables = eqs
        .iter()
        .zip(&jac)
        .filter(|((owner, _), _)| is_proposed(owner))
        .flat_map(|(_, row)| row.iter().map(|(variable, _)| *variable))
        .collect::<BTreeSet<_>>();
    let fixed_variables = eqs
        .iter()
        .filter(|(owner, _)| !is_proposed(owner))
        .filter_map(|(_, eq)| match eq {
            Eq::Lin { terms, .. } if terms.len() == 1 && terms[0].1 != 0.0 => Some(terms[0].0),
            Eq::Radius { r, .. } => Some(*r),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    let angular_variables = map
        .arcs
        .values()
        .flat_map(|(_, _, a0, a1)| [*a0, *a1])
        .collect::<BTreeSet<_>>();
    let step = independence_probe_step(sketch, proposed);
    let mut reduced = sketch.clone();
    for cid in proposed {
        reduced.remove_constraint(*cid);
    }
    for variable in variables.difference(&fixed_variables) {
        let delta = if angular_variables.contains(variable) {
            0.01
        } else {
            step
        };
        for sign in [-1.0, 1.0] {
            let mut trial = reduced.clone();
            let mut seed = x.clone();
            seed[*variable] += sign * delta;
            write_values(&mut trial, &map, &seed);
            let analysis = solve(&mut trial, &[]);
            if !analysis.converged || analysis.residual > TOL * 10.0 {
                continue;
            }
            let witness = read_values(&trial, &map);
            let (residuals, _) = eval_all(&eqs, &witness, map.n);

            let retained_satisfied = eqs.iter().zip(&residuals).all(|((owner, _), residual)| {
                is_proposed(owner) || (residual.is_finite() && residual.abs() <= TOL * 10.0)
            });
            let proposal_violated = eqs.iter().zip(&residuals).any(|((owner, _), residual)| {
                is_proposed(owner) && residual.is_finite() && residual.abs() > 1e-6
            });
            if retained_satisfied && proposal_violated {
                return false;
            }
        }
    }
    true
}

/// Use local feature size rather than absolute coordinates, so translating
/// a sketch does not change the admission test. Keep probes small and bound
/// them in millimetres; arc angle variables use radians separately.
fn independence_probe_step(sketch: &Sketch, proposed: &[ConstraintId]) -> f64 {
    let entities = sketch
        .constraints()
        .filter(|(cid, _)| proposed.contains(cid))
        .flat_map(|(_, constraint)| constraint.referenced_entities())
        .collect::<BTreeSet<_>>();
    let mut positions = Vec::new();
    let mut scale: f64 = 0.0;
    for entity in entities {
        match sketch.entity(entity) {
            Some(Entity::Point { position }) => positions.push(*position),
            Some(Entity::Line { .. }) => {
                if let Some((a, b)) = sketch.resolved_line(entity) {
                    positions.extend([a, b]);
                }
            }
            Some(Entity::Circle { center, radius } | Entity::Arc { center, radius, .. }) => {
                positions.push(*center);
                scale = scale.max(radius.abs());
            }
            Some(Entity::Spline { points }) => positions.extend(points),
            None => {}
        }
    }
    if let Some(first) = positions.first().copied() {
        let mut min = first;
        let mut max = first;
        for point in positions {
            min.x = min.x.min(point.x);
            min.y = min.y.min(point.y);
            max.x = max.x.max(point.x);
            max.y = max.y.max(point.y);
        }
        scale = scale.max(max.x - min.x).max(max.y - min.y);
    }
    (scale * 0.01).clamp(1e-4, 1.0)
}

fn finish_analysis(
    sketch: &Sketch,
    map: &VarMap,
    eqs: &[(Option<ConstraintId>, Eq)],
    jac: &[Vec<(usize, f64)>],
    converged: bool,
    iterations: usize,
    residual: f64,
) -> Analysis {
    let (rank, defined) = defined_variables(jac, map.n);
    let mut entity_free: HashMap<EntityId, usize> = HashMap::new();
    for (id, entity) in sketch.entities() {
        let count_free = |variables: &[usize]| variables.iter().filter(|&&v| !defined[v]).count();
        let free = match entity {
            Entity::Point { .. } => {
                let p = map.points[&id];
                count_free(&[p.0, p.1])
            }
            Entity::Circle { .. } => {
                let (c, r) = map.circles[&id];
                count_free(&[c.0, c.1, r])
            }
            Entity::Arc { .. } => {
                let (c, r, a0, a1) = map.arcs[&id];
                count_free(&[c.0, c.1, r, a0, a1])
            }
            Entity::Line { .. } => 0,
            Entity::Spline { .. } => map
                .splines
                .get(&id)
                .into_iter()
                .flatten()
                .flat_map(|point| [point.0, point.1])
                .filter(|&v| !defined[v])
                .count(),
        };
        entity_free.insert(id, free);

        if let Entity::Line { start, end } = entity {
            let s = entity_free.get(start).copied().unwrap_or(0);
            let e = entity_free.get(end).copied().unwrap_or(0);
            entity_free.insert(id, s + e);
        }
    }
    Analysis {
        converged,
        iterations,
        residual,
        unknowns: map.n,
        equations: eqs.len(),
        rank,
        dof: map.n.saturating_sub(rank) as i32,
        entity_free,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraint::Constraint;
    use crate::geometry::Vec2;

    #[test]
    fn free_collinear_horizontal_endpoints_follow_pins_without_null_direction_drift() {
        // Retained UI fixture: two separated lines, one Horizontal and one
        // Collinear relation. All x derivatives vanish at its aligned start.
        for dragged in 0..4 {
            for delta in [
                Vec2::new(0.0, 5.0),
                Vec2::new(5.0, 5.0),
                Vec2::new(5.0, 0.0),
            ] {
                let mut sketch = Sketch::new();
                let positions = [
                    Vec2::new(-20.0, 10.0),
                    Vec2::new(-10.0, 10.0),
                    Vec2::new(7.9289321881345245, 10.0),
                    Vec2::new(22.071067811865476, 10.0),
                ];
                let points = positions.map(|p| sketch.add_entity(Entity::point(p.x, p.y)));
                let a = sketch.add_entity(Entity::line(points[0], points[1]));
                let b = sketch.add_entity(Entity::line(points[2], points[3]));
                sketch.add_constraint(Constraint::Horizontal { entity: a });
                sketch.add_constraint(Constraint::Collinear { a, b });
                let target = positions[dragged] + delta;
                let result = solve(&mut sketch, &[(points[dragged], target)]);
                assert!(
                    result.converged,
                    "endpoint {dragged}, delta {delta:?}: {result:?}"
                );
                assert!(
                    sketch
                        .point_position(points[dragged])
                        .unwrap()
                        .distance(target)
                        < TOL
                );
                for (index, point) in points.iter().enumerate() {
                    let position = sketch.point_position(*point).unwrap();
                    assert!((position.y - target.y).abs() < 1e-7);
                    // Free length coordinates must not fly away merely because
                    // their first-order gradient was zero at the initial state.
                    assert!((position.x - positions[index].x).abs() <= delta.x.abs() + 0.1);
                }
                assert_eq!(analyze(&sketch).dof, 5);
                assert!(analyze(&sketch).converged);

                let before = serde_json::to_value(sketch.snapshot()).unwrap();
                let incompatible = (dragged + 1) % points.len();
                let result = solve(
                    &mut sketch,
                    &[
                        (points[dragged], target),
                        (points[incompatible], target + Vec2::new(0.0, 5.0)),
                    ],
                );
                assert!(!result.converged, "conflicting pins must still reject");
                assert_eq!(
                    serde_json::to_value(sketch.snapshot()).unwrap(),
                    before,
                    "rejection must be atomic"
                );
            }
        }
    }

    #[test]
    fn defined_variables_distinguish_fixed_parameters_from_dependent_pivots() {
        for (jacobian, unknowns, rank, defined) in [
            (vec![vec![(0, 1.), (1, -1.)]], 2, 1, vec![false, false]),
            (
                vec![vec![(0, 1.), (1, -1.)], vec![(1, 1.), (2, -1.)]],
                3,
                2,
                vec![false, false, false],
            ),
            (
                vec![vec![(0, 1.), (1, 1.), (2, 1.)], vec![(1, 1.), (2, 1.)]],
                3,
                2,
                vec![true, false, false],
            ),
            (vec![vec![(1, 1.)]], 2, 1, vec![false, true]),
            (vec![vec![(0, 1.)], vec![(1, 1.)]], 2, 2, vec![true, true]),
        ] {
            assert_eq!(rank_of(&jacobian, unknowns), rank);
            assert_eq!(defined_variables(&jacobian, unknowns), (rank, defined));
        }
    }

    #[test]
    fn an_owned_center_handle_adds_no_unknowns() {
        let mut sketch = Sketch::new();
        let circle = sketch.add_entity(Entity::circle(20.0, 10.0, 5.0));
        let handle = sketch.add_generated_point(Vec2::new(20.0, 10.0));
        sketch.add_constraint(Constraint::CenterCoincident {
            point: handle,
            curve: circle,
        });
        assert_eq!(build_var_map(&sketch).n, 3);
    }

    #[test]
    fn an_acquired_center_keeps_its_own_variables() {
        let mut sketch = Sketch::new();
        let circle = sketch.add_entity(Entity::circle(20.0, 10.0, 5.0));
        let acquired = sketch.add_entity(Entity::point(20.0, 10.0));
        sketch.add_constraint(Constraint::CenterCoincident {
            point: acquired,
            curve: circle,
        });
        assert_eq!(build_var_map(&sketch).n, 5);
    }

    #[test]
    fn a_center_relation_naming_a_non_curve_keeps_the_point_real() {
        let mut sketch = Sketch::new();
        let a = sketch.add_entity(Entity::point(0.0, 0.0));
        let b = sketch.add_entity(Entity::point(10.0, 0.0));
        let line = sketch.add_entity(Entity::line(a, b));
        let handle = sketch.add_generated_point(Vec2::new(5.0, 0.0));
        sketch.add_constraint(Constraint::CenterCoincident {
            point: handle,
            curve: line,
        });
        assert_eq!(build_var_map(&sketch).n, 6);
        assert!(solve(&mut sketch, &[]).converged);
    }
}
