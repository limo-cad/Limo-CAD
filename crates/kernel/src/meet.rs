//! Intersection curves of surface pairs that have no closed form.
//!
//! One surface is the base. In its parameter plane the curve is where the
//! other surface's signed distance is zero. That distance changes no faster
//! than the point moves, so a cell whose centre is further from the other
//! surface than the cell reaches cannot hold any of the curve, and neither
//! can one that far outside the region asked about. The window is halved on
//! that test until what is left is a thin band of small cells.
//! Each is crossed like a contour map, the crossings are joined into chains,
//! and the chains are cut into pieces a [`Curve::meet`] can follow. A piece
//! is a graph over one parameter, so a chain is cut wherever the curve turns
//! back in the parameter it is followed along. It may do that between two
//! crossings, inside one cell: the lean of the other surface's distance at
//! the two of them shows it.
//!
//! A closed piece of the curve can be smaller than a cell and go round none
//! of its corners, where one surface only just dips through the other. The
//! corners then all lie on one side. Such cells are searched down the slope
//! of the distance for a place on the other side, and the piece is drawn
//! round it from there.
//!
//! Tangential contact changes no sign and is not reported, as for the
//! closed-form pairs. Where the surfaces touch and each is on both sides of
//! the other around the place, the curve crosses itself there: a body
//! against its own mirror image does it, and so does a rod through a ball
//! that it touches from the inside. No contour map gets that right, as
//! the four ways out of the crossing look like two curves that happen to
//! pass close by. The crossing is found for what it is, a place on both
//! surfaces where the distance stops changing either way, and each of
//! the four arms is walked out from it to the first place the grid knows.
//! The pieces then end at the crossing, all four of them.
//!
//! Where the surfaces only come close to touching, the curve can turn more
//! tightly than the cells are wide. Such a stretch shows up as a piece that
//! will not follow its points. The search is then repeated on finer cells;
//! a piece that still does not follow is handed over marked as doubtful
//! (see [`doubtful`]), for the caller to refuse if anything turns out to
//! depend on it.

use std::collections::VecDeque;

use crate::fast::Map;
use crate::geom::{Curve, SurfKind, Surface};
use crate::math::*;

/// Finest cells across the window, at most.
const FINEST: usize = 512;

/// Marks the key of an arm of a crossing among the keys of grid edges.
const ARM: u64 = 1 << 63;

/// One of the four ways out of a place where the curve crosses itself.
struct Arm {
    /// The crossing it leaves.
    node: V2,
    /// The way it leaves, in the parameters.
    heading: V2,
    /// Its points in order from the crossing, which is not one of them.
    points: Vec<V2>,
}

/// A crossing of the curve with itself, and for each of its four arms a
/// first point and the way it leaves.
type Crossing = (V2, [(V2, V2); 4]);

/// A grid line a step went over: how far along the step, the grid edge it
/// crossed there, and the cell it left.
type Over = (f64, (i64, i64, bool), (i64, i64));

/// What an arm runs into.
enum Reach {
    /// A crossing of a grid edge that the grid knows of, come upon from
    /// this cell.
    Edge(u64, (usize, usize)),
    /// A place where the curve crosses itself: another, or the same again.
    Node(usize, V2),
    /// The edge of the window.
    Open,
}

/// What a look over the grid finds.
struct Scan {
    /// Contour segments, as pairs of grid edges.
    links: Vec<(u64, u64)>,
    /// The cell each segment lies in.
    owners: Vec<(usize, usize)>,
    /// Closed pieces too small for the grid to see.
    islands: Vec<Vec<V2>>,
}

/// The better surface to write the curve on comes first: its parameters
/// stay regular for longer.
fn rank(kind: SurfKind) -> u8 {
    match kind {
        // A surface that winds along its axis has one sheet after another;
        // only its own parameters tell them apart.
        SurfKind::Helicoid => 0,
        SurfKind::Coil => 1,
        // A spline's distance costs a search for the nearest point; its
        // points cost next to nothing. Better to walk on it than to ask it.
        SurfKind::Spline => 2,
        // The same goes for a pipe, whose distance means finding the
        // nearest point of its spine.
        SurfKind::Pipe => 3,
        SurfKind::Plane => 4,
        SurfKind::Cylinder => 5,
        SurfKind::Torus => 6,
        SurfKind::Cone => 7,
        SurfKind::Sphere => 8,
    }
}

/// The radius a surface bends with, if it has one.
fn bend(s: &Surface) -> f64 {
    match s.kind {
        SurfKind::Plane | SurfKind::Cone => f64::INFINITY,
        SurfKind::Cylinder | SurfKind::Sphere => s.r,
        SurfKind::Torus => s.s,
        // Half the way from one turn to the next, square to the flank.
        SurfKind::Helicoid => PI * s.t.abs() * s.s.abs().sin(),
        SurfKind::Coil => s.s.abs(),
        SurfKind::Spline => s.spline_form().radius,
        SurfKind::Pipe => s.r,
    }
}

/// How sharply a surface bends at a point of it: one over the smallest
/// radius it curves with there.
fn curvature(s: &Surface, at: V3) -> f64 {
    match s.kind {
        // Round the axis, tighter towards the tip.
        SurfKind::Cone => {
            let local = s.f.local(at);
            s.s.cos() / local.x.hypot(local.y).max(1.0e-9)
        }
        _ => 1.0 / bend(s),
    }
}

/// The part of the base's parameter plane to search, as a grid.
struct Grid<'a> {
    base: &'a Surface,
    other: &'a Surface,
    region: Aabb,
    lo: V2,
    step: V2,
    n: [usize; 2],
    /// A whole turn of the parameter: the last column or row meets the first.
    wrap: [bool; 2],
    /// Largest model length of a unit step in each parameter.
    scale: V2,
    /// Distance at every node looked at so far; `NAN` for the rest.
    values: Vec<f64>,
}

/// A grid edge: from node `(i, j)` to the next node across (`upright`
/// false) or up.
type Edge = (usize, usize, bool);

impl Grid<'_> {
    fn new<'a>(
        base: &'a Surface,
        other: &'a Surface,
        region: &Aabb,
        finer: u32,
    ) -> Option<Grid<'a>> {
        let mut local = [V3::ZERO; 8];
        for (i, slot) in local.iter_mut().enumerate() {
            *slot = base.f.local(v3(
                if i & 1 == 0 { region.lo.x } else { region.hi.x },
                if i & 2 == 0 { region.lo.y } else { region.hi.y },
                if i & 4 == 0 { region.lo.z } else { region.hi.z },
            ));
        }
        let range = |pick: fn(&V3) -> f64| {
            local
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
                    (lo.min(pick(p)), hi.max(pick(p)))
                })
        };
        // A whole turn starts off the usual symmetry directions, so nodes
        // rarely land on the curve.
        let turn = |phase: f64| (-PI + phase, PI + phase);
        // The angles about the axis that the region takes up: all of them
        // if the axis runs through it, otherwise the arc its corners span.
        let around = || {
            let mut angles: Vec<f64> = local.iter().map(|p| p.y.atan2(p.x)).collect();
            angles.sort_by(f64::total_cmp);
            let mut widest = (angles[0] + TAU - angles[7], 7);
            for k in 0..7 {
                if angles[k + 1] - angles[k] > widest.0 {
                    widest = (angles[k + 1] - angles[k], k);
                }
            }
            if widest.0 <= PI {
                return (turn(0.0123), true);
            }
            let start = angles[(widest.1 + 1) % 8];
            ((start, start + TAU - widest.0), false)
        };
        let (u, v, wrap, scale) = match base.kind {
            SurfKind::Plane => (range(|p| p.x), range(|p| p.y), [false, false], v2(1.0, 1.0)),
            SurfKind::Cylinder => {
                let (u, whole) = around();
                (u, range(|p| p.z), [whole, false], v2(base.r, 1.0))
            }
            SurfKind::Cone => {
                let (lo, hi) = range(|p| p.z);
                // Stay off the apex, where the angle means nothing.
                let lo = lo.max(1.0e-6 * hi.abs().max(1.0));
                if hi <= lo {
                    return None;
                }
                let widest = hi * base.s.tan();
                let (u, whole) = around();
                (u, (lo, hi), [whole, false], v2(widest, 1.0 / base.s.cos()))
            }
            SurfKind::Sphere => {
                let (u, whole) = around();
                (
                    u,
                    (-0.5 * PI + 1.0e-3, 0.5 * PI - 1.0e-3),
                    [whole, false],
                    v2(base.r, base.r),
                )
            }
            SurfKind::Torus => {
                let (u, whole) = around();
                (u, turn(0.0217), [whole, true], v2(base.r + base.s, base.s))
            }
            // The turns that pass through the heights of the region, out to
            // as far from the axis as the region goes.
            SurfKind::Helicoid => {
                let (lo, hi) = range(|p| p.z);
                let far = local.iter().map(|p| p.x.hypot(p.y)).fold(0.0, f64::max);
                // As far along a ruling as it takes to get that far out,
                // and what the ruling has climbed by then.
                let (sideways, up) = base.ruling();
                let out = far / sideways;
                let rise = up * out;
                let turns = [lo, hi, lo - rise, hi - rise].map(|z| z / base.t);
                let first = turns.iter().copied().fold(f64::INFINITY, f64::min);
                let last = turns.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                (
                    (first - PI, last + PI),
                    (1.0e-6 * out.max(1.0e-6), out),
                    [false, false],
                    v2(far.hypot(base.t), 1.0),
                )
            }
            SurfKind::Coil => {
                let (lo, hi) = range(|p| p.z);
                let tube = base.s.abs();
                let ends = [(lo - tube) / base.t, (hi + tube) / base.t];
                (
                    (ends[0].min(ends[1]) - PI, ends[0].max(ends[1]) + PI),
                    (-PI + 1.0e-3, PI - 1.0e-3),
                    [false, false],
                    v2((base.r + tube).hypot(base.t), tube),
                )
            }
            // All the way along the spine and all the way round it.
            SurfKind::Pipe => {
                let pipe = base.pipe_form();
                let (u, whole) = if pipe.closed {
                    (turn(0.0123), true)
                } else {
                    ((pipe.span[0], pipe.span[1]), false)
                };
                let mut fastest = 0.0f64;
                for i in 0..=12 {
                    for v in [0.0, 0.5 * PI, PI, -0.5 * PI] {
                        let at = u.0 + (u.1 - u.0) * i as f64 / 12.0;
                        fastest = fastest.max(base.du(at, v).len());
                    }
                }
                (u, turn(0.0217), [whole, true], v2(fastest, base.r))
            }
            // The part of the spline's own rectangle that comes near the
            // region, by the net of points it keeps.
            SurfKind::Spline => {
                let form = base.spline_form();
                let mut within = Aabb::EMPTY;
                for p in &local {
                    within.add(*p);
                }
                let (mut lo, mut hi) = form.window(&within)?;
                let hand = base.hand();
                if hand < 0.0 {
                    (lo.x, hi.x) = (-hi.x, -lo.x);
                }
                let whole = form.closed[0] && hi.x - lo.x >= TAU - 1.0e-9;
                let u = if whole { turn(0.0123) } else { (lo.x, hi.x) };
                let fast = form
                    .bend(v2(hand * u.0, lo.y), v2(hand * u.1, hi.y))
                    .fastest;
                (u, (lo.y, hi.y), [whole, false], fast)
            }
        };
        // An open range is widened unevenly for the same reason.
        let widen = |(lo, hi): (f64, f64), whole: bool| {
            if whole {
                (lo, hi)
            } else {
                let size = (hi - lo).max(1.0e-6);
                (lo - 0.0137 * size, hi + 0.0171 * size)
            }
        };
        let (u, v) = (widen(u, wrap[0]), widen(v, wrap[1]));
        let size = v2((u.1 - u.0) * scale.x, (v.1 - v.0) * scale.y);
        let longest = size.x.max(size.y);
        if !(longest.is_finite() && longest > 0.0) {
            return None;
        }
        let most = FINEST << finer;
        let fine = (bend(base).min(bend(other)) / 4.0)
            .min(longest / 16.0)
            .max(longest / FINEST as f64)
            / (1 << finer) as f64;
        let cells = |length: f64| ((length / fine).ceil() as usize).clamp(2, most);
        let n = [cells(size.x), cells(size.y)];
        Some(Grid {
            base,
            other,
            region: *region,
            lo: v2(u.0, v.0),
            step: v2((u.1 - u.0) / n[0] as f64, (v.1 - v.0) / n[1] as f64),
            n,
            wrap,
            scale,
            values: vec![f64::NAN; (n[0] + 1) * (n[1] + 1)],
        })
    }

    #[inline]
    fn distance(&self, q: V2) -> f64 {
        self.other.sdist(self.base.point(q.x, q.y))
    }

    /// How the other surface's distance changes along `u` and along `v`.
    /// The curve runs square to this.
    fn lean(&self, q: V2) -> V2 {
        let n = self.other.normal(self.base.point(q.x, q.y));
        v2(n.dot(self.base.du(q.x, q.y)), n.dot(self.base.dv(q.x, q.y)))
    }

    /// A point of the curve between two of its points that it passes
    /// heading opposite ways, as on the two sides of a hairpin: where it
    /// crosses the line midway between them. `None` unless it crosses that
    /// line exactly once within reach.
    fn between(&self, p: V2, q: V2) -> Option<V2> {
        let mid = (p + q) * 0.5;
        let m = self.base.scale(mid.x, mid.y);
        // In model lengths, where a quarter turn is a quarter turn.
        let chord = v2((q.x - p.x) * m.x, (q.y - p.y) * m.y);
        let len = chord.len();
        if len < 1.0e-6 || m.x <= 0.0 || m.y <= 0.0 {
            return None;
        }
        let across = v2(-chord.y / (len * m.x), chord.x / (len * m.y));
        let reach = len + (self.step.x * m.x).hypot(self.step.y * m.y);
        let at = |w: f64| mid + across * w;
        const STEPS: usize = 48;
        let width = 2.0 * reach / STEPS as f64;
        let mut found: Option<f64> = None;
        let mut before = self.distance(at(-reach)) > 0.0;
        for i in 1..=STEPS {
            let w = -reach + width * i as f64;
            let now = self.distance(at(w)) > 0.0;
            if now != before {
                if found.is_some() {
                    return None;
                }
                found = Some(w);
            }
            before = now;
        }
        let (mut lo, mut hi) = (found? - width, found?);
        let below = self.distance(at(lo)) > 0.0;
        for _ in 0..60 {
            let w = 0.5 * (lo + hi);
            if (self.distance(at(w)) > 0.0) == below {
                lo = w;
            } else {
                hi = w;
            }
        }
        let found = at(0.5 * (lo + hi));
        let apart = |a: V2, b: V2| ((b.x - a.x) * m.x).hypot((b.y - a.y) * m.y);
        (apart(p, found) > 1.0e-6 && apart(found, q) > 1.0e-6).then_some(found)
    }

    #[inline]
    fn node(&self, i: usize, j: usize) -> V2 {
        v2(
            self.lo.x + i as f64 * self.step.x,
            self.lo.y + j as f64 * self.step.y,
        )
    }

    /// Distance at a node, the same for both ends of a whole turn.
    fn value(&mut self, i: usize, j: usize) -> f64 {
        let i = if self.wrap[0] && i == self.n[0] { 0 } else { i };
        let j = if self.wrap[1] && j == self.n[1] { 0 } else { j };
        let slot = j * (self.n[0] + 1) + i;
        let known = self.values[slot];
        if !known.is_nan() {
            return known;
        }
        let g = self.distance(self.node(i, j));
        self.values[slot] = g;
        g
    }

    fn key(&self, (i, j, upright): Edge) -> u64 {
        let i = if upright && self.wrap[0] && i == self.n[0] {
            0
        } else {
            i
        };
        let j = if !upright && self.wrap[1] && j == self.n[1] {
            0
        } else {
            j
        };
        (i as u64) << 33 | (j as u64) << 1 | u64::from(upright)
    }

    /// Where the curve crosses a grid edge whose ends are on opposite sides.
    fn crossing(&mut self, key: u64) -> V2 {
        let (i, j, upright) = (
            (key >> 33) as usize,
            ((key >> 1) & 0xffff_ffff) as usize,
            key & 1 == 1,
        );
        let a = self.node(i, j);
        let b = if upright {
            self.node(i, j + 1)
        } else {
            self.node(i + 1, j)
        };
        let (mut lo, mut hi) = (0.0, 1.0);
        let (mut glo, mut ghi) = (self.distance(a), self.distance(b));
        // As close to the surface as its distance can be told from zero.
        let exact = 4.0 * f64::EPSILON * (1.0 + glo.abs().max(ghi.abs()));
        // Illinois: fast, and it never leaves the bracket.
        let mut side = 0;
        for _ in 0..80 {
            if hi - lo <= 1.0e-15 {
                break;
            }
            let mut t = (lo * ghi - hi * glo) / (ghi - glo);
            if !t.is_finite() || t <= lo || t >= hi {
                t = 0.5 * (lo + hi);
            }
            let g = self.distance(a + (b - a) * t);
            if g.abs() <= exact {
                lo = t;
                hi = t;
                break;
            }
            if (g > 0.0) == (ghi > 0.0) {
                hi = t;
                ghi = g;
                if side == -1 {
                    glo *= 0.5;
                }
                side = -1;
            } else {
                lo = t;
                glo = g;
                if side == 1 {
                    ghi *= 0.5;
                }
                side = 1;
            }
        }
        a + (b - a) * (0.5 * (lo + hi))
    }

    /// The contour segments through one finest cell, as pairs of grid edges.
    /// Returns whether there are any.
    fn cell(&mut self, i: usize, j: usize, links: &mut Vec<(u64, u64)>) -> bool {
        let above = [
            self.value(i, j) > 0.0,
            self.value(i + 1, j) > 0.0,
            self.value(i + 1, j + 1) > 0.0,
            self.value(i, j + 1) > 0.0,
        ];
        let mut sides = [
            (above[0] != above[1], (i, j, false)),
            (above[1] != above[2], (i + 1, j, true)),
            (above[3] != above[2], (i, j + 1, false)),
            (above[0] != above[3], (i, j, true)),
        ];
        // Between two turns of a helicoid its distance changes sign without
        // passing through zero. A real crossing lies between two values no
        // further apart than their nodes; that jump is half a turn wide.
        if self.other.kind == SurfKind::Helicoid {
            let ends = [(0, 1), (1, 2), (3, 2), (0, 3)];
            let corner = [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)];
            for (side, (a, b)) in sides.iter_mut().zip(ends) {
                if !side.0 {
                    continue;
                }
                let (ga, gb) = (
                    self.value(corner[a].0, corner[a].1),
                    self.value(corner[b].0, corner[b].1),
                );
                let reach = if side.1 .2 {
                    self.step.y * self.scale.y
                } else {
                    self.step.x * self.scale.x
                };
                side.0 = ga.abs() + gb.abs() <= 2.0 * reach;
            }
        }
        let cut: Vec<u64> = sides
            .iter()
            .filter(|(crossed, _)| *crossed)
            .map(|(_, edge)| self.key(*edge))
            .collect();
        match cut.len() {
            2 => links.push((cut[0], cut[1])),
            4 => {
                // A saddle: the middle says which two corners are joined.
                let middle = self.distance(self.node(i, j) + self.step * 0.5) > 0.0;
                if middle == above[0] {
                    links.push((cut[0], cut[1]));
                    links.push((cut[2], cut[3]));
                } else {
                    links.push((cut[0], cut[3]));
                    links.push((cut[1], cut[2]));
                }
            }
            _ => {}
        }
        !cut.is_empty()
    }

    /// A closed piece of the curve too small to cross a side of the grid:
    /// it lies in the cell at `(i, j)` and goes round none of its corners.
    /// The other surface dips through this one there and comes straight
    /// back, as the tip of a cone does, or a ball that only just reaches.
    ///
    /// All four corners are on one side. The search goes down the slope of
    /// the distance from the middle of the cell, and where it comes out on
    /// the other side it feels its way round from there. `found` are the
    /// pieces already drawn, by the cell next door perhaps.
    fn island(
        &mut self,
        i: usize,
        j: usize,
        (here, middle): (V3, f64),
        found: &[(V2, Vec<V2>)],
    ) -> Option<(V2, Vec<V2>)> {
        let (lo, hi) = (self.node(i, j), self.node(i + 1, j + 1));
        let centre = (lo + hi) * 0.5;
        let m = self.base.scale(centre.x, centre.y);
        if m.x <= 0.0 || m.y <= 0.0 {
            return None;
        }
        let (wide, tall) = (self.step.x * m.x, self.step.y * m.y);
        let corners = [
            self.value(i, j),
            self.value(i + 1, j),
            self.value(i + 1, j + 1),
            self.value(i, j + 1),
        ];
        let outside = corners[0] > 0.0;
        // No place in the cell is further than half its longer side from
        // the nearest of these five, and the distance changes no faster
        // than the place does. Where both surfaces bend gently it cannot
        // even change that fast: a dip between the five is a shallow bowl,
        // no deeper than the bend allows across it. That leaves only the
        // cells that all but touch, and those next to something sharp.
        let least = corners.iter().fold(middle.abs(), |l, g| l.min(g.abs()));
        let bend = curvature(self.base, here) + curvature(self.other, here);
        let span = 0.75 * wide.max(tall);
        if (middle > 0.0) == outside && least > span.min(0.5 * bend * span * span) {
            return None;
        }
        let slope_at = |grid: &Grid, q: V2| {
            let lean = grid.lean(q);
            v2(lean.x / m.x, lean.y / m.y)
        };
        let mut seed = centre;
        if (middle > 0.0) == outside {
            // A smooth dip has a level floor, which a steep slope across
            // the cell rules out; the corners show the slope for nothing.
            // The tip of a cone has no floor.
            let across = v2(
                (corners[1] + corners[2] - corners[0] - corners[3]) / (2.0 * wide),
                (corners[3] + corners[2] - corners[0] - corners[1]) / (2.0 * tall),
            );
            if across.len() > 0.7 && bend * wide.max(tall) < 1.0 {
                return None;
            }
            let (mut q, mut g) = (centre, middle);
            let mut through = false;
            for _ in 0..6 {
                let slope = slope_at(self, q);
                let steep = slope.len();
                if steep < 1.0e-12 {
                    break;
                }
                // As far as a straight slope would need, and a little over.
                let reach = (1.25 * g.abs() / steep).min(0.5 * wide.max(tall));
                let way = slope * (-g.signum() * reach / steep);
                let next = v2(
                    (q.x + way.x / m.x).clamp(lo.x, hi.x),
                    (q.y + way.y / m.y).clamp(lo.y, hi.y),
                );
                let there = self.distance(next);
                if (there > 0.0) != outside {
                    seed = next;
                    through = true;
                    break;
                }
                if there.abs() >= g.abs() {
                    break;
                }
                (q, g) = (next, there);
            }
            if !through {
                return None;
            }
        }
        let within = |ring: &[V2], q: V2| {
            let mut inside = false;
            for w in ring.windows(2) {
                let (a, b) = (w[0], w[1]);
                if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x)
                {
                    inside = !inside;
                }
            }
            inside
        };
        // Drawn already, from the cell next door? The outline it left is
        // sixteen straight sides, which cut the corners off a place that
        // is inside by a hair. Two places on the far side with nothing but
        // far side between them are in the same piece, whatever its drawn
        // outline says.
        let joined = |from: V2, to: V2| {
            const LOOKS: usize = 24;
            (1..LOOKS).all(|k| {
                let q = from + (to - from) * (k as f64 / LOOKS as f64);
                (self.distance(q) > 0.0) != outside
            })
        };
        if found
            .iter()
            .any(|(entered, ring)| within(ring, seed) || joined(seed, *entered))
        {
            return None;
        }
        // Out from there every way, to where the distance changes side.
        // A way that never does leads into something larger, which the
        // grid sees for itself.
        const WAYS: usize = 16;
        const STEPS: usize = 16;
        let far = 2.0 * (wide + tall);
        let mut ring: Vec<V2> = Vec::with_capacity(WAYS + 1);
        for k in 0..WAYS {
            let turn = TAU * k as f64 / WAYS as f64;
            let way = v2(turn.cos() / m.x, turn.sin() / m.y);
            let at = |w: f64| seed + way * w;
            let mut near = 0.0;
            let mut beyond = (1..=STEPS)
                .map(|s| far * s as f64 / STEPS as f64)
                .find(|&w| {
                    let out = (self.distance(at(w)) > 0.0) == outside;
                    if !out {
                        near = w;
                    }
                    out
                })?;
            for _ in 0..60 {
                let w = 0.5 * (near + beyond);
                if (self.distance(at(w)) > 0.0) == outside {
                    beyond = w;
                } else {
                    near = w;
                }
            }
            ring.push(at(0.5 * (near + beyond)));
        }
        ring.push(ring[0]);
        // Round a corner of the grid it is no island: the cells there have
        // it already.
        let (mut low, mut high) = (ring[0], ring[0]);
        for q in &ring {
            low = v2(low.x.min(q.x), low.y.min(q.y));
            high = v2(high.x.max(q.x), high.y.max(q.y));
        }
        let column = |x: f64| ((x - self.lo.x) / self.step.x).floor();
        let row = |y: f64| ((y - self.lo.y) / self.step.y).floor();
        for a in column(low.x) as i64..=column(high.x) as i64 + 1 {
            for b in row(low.y) as i64..=row(high.y) as i64 + 1 {
                let node = v2(
                    self.lo.x + a as f64 * self.step.x,
                    self.lo.y + b as f64 * self.step.y,
                );
                if within(&ring, node) {
                    return None;
                }
            }
        }
        Some((seed, ring))
    }

    /// Contour segments of every cell the curve can be in, and the closed
    /// pieces of it that are too small for the grid to see.
    fn scan(&mut self) -> Scan {
        let mut islands: Vec<(V2, Vec<V2>)> = Vec::new();
        let mut links = Vec::new();
        let mut owners: Vec<(usize, usize)> = Vec::new();
        let mut pending = vec![[0, self.n[0], 0, self.n[1]]];
        while let Some([i0, i1, j0, j1]) = pending.pop() {
            let wide = (i1 - i0) as f64 * self.step.x * self.scale.x;
            let tall = (j1 - j0) as f64 * self.step.y * self.scale.y;
            let middle = v2(
                self.lo.x + 0.5 * (i0 + i1) as f64 * self.step.x,
                self.lo.y + 0.5 * (j0 + j1) as f64 * self.step.y,
            );
            let centre = self.base.point(middle.x, middle.y);
            let reach = 0.5 * (wide + tall);
            let off = self.other.sdist(centre);
            if off.abs() > reach || self.region.distance(centre) > reach {
                continue;
            }
            if i1 - i0 == 1 && j1 - j0 == 1 {
                if self.cell(i0, j0, &mut links) {
                    owners.resize(links.len(), (i0, j0));
                } else if let Some(island) = self.island(i0, j0, (centre, off), &islands) {
                    islands.push(island);
                }
            } else if j1 - j0 == 1 || (i1 - i0 > 1 && wide >= tall) {
                let m = (i0 + i1) / 2;
                pending.push([i0, m, j0, j1]);
                pending.push([m, i1, j0, j1]);
            } else {
                let m = (j0 + j1) / 2;
                pending.push([i0, i1, j0, m]);
                pending.push([i0, i1, m, j1]);
            }
        }
        Scan {
            links,
            owners,
            islands: islands.into_iter().map(|(_, ring)| ring).collect(),
        }
    }

    /// Whether the grid sees the curve cross one of its edges: the same
    /// test the cells are drawn by.
    fn crossed(&mut self, (i, j, upright): Edge) -> bool {
        let (i1, j1) = if upright { (i, j + 1) } else { (i + 1, j) };
        let (ga, gb) = (self.value(i, j), self.value(i1, j1));
        if (ga > 0.0) == (gb > 0.0) {
            return false;
        }
        if self.other.kind == SurfKind::Helicoid {
            let reach = if upright {
                self.step.y * self.scale.y
            } else {
                self.step.x * self.scale.x
            };
            return ga.abs() + gb.abs() <= 2.0 * reach;
        }
        true
    }

    /// How far apart two places of the parameter plane are on the surface,
    /// near enough, for places close together.
    fn apart(&self, a: V2, b: V2) -> f64 {
        let m = self.base.scale(0.5 * (a.x + b.x), 0.5 * (a.y + b.y));
        ((b.x - a.x) * m.x).hypot((b.y - a.y) * m.y)
    }

    /// The places where the curve crosses itself, each with the four arms
    /// that leave it, and the places that are one by every sign but could
    /// not be drawn. `starts` are points of the curve to look from.
    ///
    /// Such a place is on both surfaces, and their normals agree there:
    /// the distance from the other surface is nought and has stopped
    /// changing either way. Around it the distance is positive in two
    /// opposite quarters and negative in the other two.
    fn nodes(&self, starts: &[V2]) -> (Vec<Crossing>, Vec<V2>) {
        let mut found: Vec<Crossing> = Vec::new();
        let mut unclear: Vec<V2> = Vec::new();
        let mut tried: Vec<V2> = Vec::new();
        let top = v2(
            self.lo.x + self.n[0] as f64 * self.step.x,
            self.lo.y + self.n[1] as f64 * self.step.y,
        );
        // The lean of the distance, and how the lean itself changes: across
        // a short way either side.
        let bowl = |q: V2| -> (V2, [f64; 3]) {
            let (hu, hv) = (1.0e-4 * self.step.x, 1.0e-4 * self.step.y);
            let (a, b) = (self.lean(v2(q.x + hu, q.y)), self.lean(v2(q.x - hu, q.y)));
            let (c, d) = (self.lean(v2(q.x, q.y + hv)), self.lean(v2(q.x, q.y - hv)));
            (
                self.lean(q),
                [
                    (a.x - b.x) / (2.0 * hu),
                    0.5 * ((a.y - b.y) / (2.0 * hu) + (c.x - d.x) / (2.0 * hv)),
                    (c.y - d.y) / (2.0 * hv),
                ],
            )
        };
        for &start in starts {
            let m = self.base.scale(start.x, start.y);
            if m.x <= 0.0 || m.y <= 0.0 {
                continue;
            }
            // How steeply the surfaces cross at a place: nought where the
            // curve crosses itself.
            let steep = |q: V2| {
                let lean = self.lean(q);
                (lean.x / m.x).hypot(lean.y / m.y)
            };
            // No further than the cells next door: a place further off has
            // points of the curve nearer to it.
            let near = |q: V2| {
                ((q.x - start.x) / self.step.x).abs() <= 3.0
                    && ((q.y - start.y) / self.step.y).abs() <= 3.0
            };
            let mut q = start;
            let mut now = steep(q);
            // Newton on the lean, taking less of a step where the whole of
            // it does not help; and where no part of it helps, feeling the
            // way down. Across the edge of a patch the surface goes on
            // straight and the lean changes differently on the two sides,
            // so a step reckoned on one side can be wrong on the other.
            // The steepness itself has no such break: it is a cone round
            // the place looked for, creased along the patch edge, and a
            // cone can be walked down blind until the reckoning works again.
            let mut blind = 0;
            for _ in 0..60 {
                if now < 1.0e-11 {
                    break;
                }
                let (g, [uu, uv, vv]) = bowl(q);
                let det = uu * vv - uv * uv;
                let mut next = None;
                if det.abs() > 1.0e-300 {
                    let step = v2((vv * g.x - uv * g.y) / det, (uu * g.y - uv * g.x) / det);
                    let mut share = 1.0;
                    while share > 0.01 {
                        let to = q - step * share;
                        if near(to) {
                            let there = steep(to);
                            if there < (1.0 - 0.25 * share) * now {
                                next = Some((to, there));
                                break;
                            }
                        }
                        share *= 0.5;
                    }
                }
                if let Some((to, there)) = next {
                    let moved =
                        ((to.x - q.x) / self.step.x).abs() + ((to.y - q.y) / self.step.y).abs();
                    (q, now) = (to, there);
                    if moved < 1.0e-12 {
                        break;
                    }
                    continue;
                }
                blind += 1;
                if blind > 4 {
                    break;
                }
                let mut reach = v2(0.1 * self.step.x, 0.1 * self.step.y);
                let from = now;
                for _ in 0..40 {
                    if now < 0.2 * from || reach.x < 1.0e-13 * self.step.x {
                        break;
                    }
                    let mut best = (q, now);
                    for (du, dv) in [
                        (1.0, 0.0),
                        (-1.0, 0.0),
                        (0.0, 1.0),
                        (0.0, -1.0),
                        (1.0, 1.0),
                        (1.0, -1.0),
                        (-1.0, 1.0),
                        (-1.0, -1.0),
                    ] {
                        let to = v2(q.x + du * reach.x, q.y + dv * reach.y);
                        if near(to) {
                            let there = steep(to);
                            if there < best.1 {
                                best = (to, there);
                            }
                        }
                    }
                    if best.1 < now {
                        (q, now) = best;
                    } else {
                        reach = reach * 0.5;
                    }
                }
            }
            // As flat as rounding lets it get, or it is no such place.
            if now > 1.0e-9 || q.x < self.lo.x || q.y < self.lo.y || q.x > top.x || q.y > top.y {
                continue;
            }
            if tried.iter().any(|t| self.apart(*t, q) < 1.0e-7) {
                continue;
            }
            tried.push(q);
            let m = self.base.scale(q.x, q.y);
            if m.x <= 0.0 || m.y <= 0.0 {
                continue;
            }
            let (_, [uu, uv, vv]) = bowl(q);
            let (a, b, c) = (uu / (m.x * m.x), uv / (m.x * m.y), vv / (m.y * m.y));
            // A saddle, or the surfaces only touch and there is no curve.
            // And a saddle with some shape to it: two surfaces that lie
            // in one another over a stretch are flat against each other
            // everywhere there, and what rounding leaves of the bowl is
            // as often a saddle as not. (The straight run-on of a patch
            // past its edge is such a stretch against that of the same
            // patch moved along it.)
            let det = a * c - b * b;
            if det.is_nan() || det >= -1.0e-12 {
                continue;
            }
            // Off the other surface here, the two branches pass each other
            // without meeting: two curves, for the grid. How far off is
            // only known to rounding, though, and where the surfaces cross
            // at a shallow angle a distance that is all rounding would put
            // the branches a thousandth apart. So it is the distance that
            // is asked, not the gap it would make: closer than anything
            // could tell from touching, they touch.
            let off = self.distance(q).abs();
            let gap = (2.0 * off / (-det).sqrt()).sqrt();
            if gap > TOL && off > 1.0e-12 * (1.0 + self.region.diagonal()) {
                continue;
            }
            match self.star(q) {
                Some(arms) => found.push((q, arms)),
                None => unclear.push(q),
            }
        }
        (found, unclear)
    }

    /// Points of a chain to look for a crossing of the curve with itself
    /// from. Towards one the surfaces cross at an ever shallower angle, so
    /// along the chain the steepness dips to nearly nothing and comes back;
    /// and whichever way the chain was joined through it, the slope swings
    /// round from one side of it to the other.
    fn dips(&self, points: &[V2], closed: bool, out: &mut Vec<V2>) {
        let n = if closed {
            points.len() - 1
        } else {
            points.len()
        };
        if n < 3 {
            return;
        }
        let slopes: Vec<V2> = points[..n]
            .iter()
            .map(|q| {
                let m = self.base.scale(q.x, q.y);
                let lean = self.lean(*q);
                v2(lean.x / m.x.max(1.0e-300), lean.y / m.y.max(1.0e-300))
            })
            .collect();
        let steep: Vec<f64> = slopes.iter().map(|g| g.len()).collect();
        // The point `by` places along, if the chain goes that far.
        let at = |k: usize, by: i64| -> Option<usize> {
            let to = k as i64 + by;
            if closed {
                Some(to.rem_euclid(n as i64) as usize)
            } else {
                (to >= 0 && (to as usize) < n).then_some(to as usize)
            }
        };
        for k in 0..n {
            if steep[k] > 0.3 {
                continue;
            }
            let (before, after) = (at(k, -1), at(k, 1));
            let lowest = [before, after]
                .iter()
                .flatten()
                .all(|&j| steep[j] >= steep[k]);
            if !lowest {
                continue;
            }
            let further: Vec<usize> = [at(k, -2), at(k, 2)].into_iter().flatten().collect();
            // Flat all along is two surfaces lying in each other, which is
            // no crossing: the steepness comes back within a cell or two
            // of a real one.
            let comes_back = [before, after]
                .into_iter()
                .flatten()
                .chain(further.iter().copied())
                .any(|j| steep[j] > 1.0e-7);
            if !comes_back {
                continue;
            }
            let dips = !further.is_empty() && further.iter().all(|&j| steep[k] < 0.6 * steep[j]);
            let swings = match (before, after) {
                (Some(a), Some(b)) => slopes[a].dot(slopes[b]) < 0.7 * steep[a] * steep[b],
                _ => false,
            };
            if dips || swings {
                out.push(points[k]);
            }
        }
    }

    /// The four arms of the crossing at `at`: for each a point of it a
    /// short way out, and the way it leaves. Found by going round the
    /// crossing and seeing where the distance changes side: four times, or
    /// it is not a crossing that can be drawn.
    fn star(&self, at: V2) -> Option<[(V2, V2); 4]> {
        let m = self.base.scale(at.x, at.y);
        const LOOKS: usize = 72;
        let mut radius = 0.2 * (self.step.x * m.x).min(self.step.y * m.y);
        for _ in 0..8 {
            let on =
                |turn: f64, r: f64| v2(at.x + r * turn.cos() / m.x, at.y + r * turn.sin() / m.y);
            let side = |turn: f64, r: f64| self.distance(on(turn, r)) > 0.0;
            let angle = |k: usize| TAU * (k as f64 + 0.37) / LOOKS as f64;
            let sides: Vec<bool> = (0..LOOKS).map(|k| side(angle(k), radius)).collect();
            let changes: Vec<usize> = (0..LOOKS)
                .filter(|&k| sides[k] != sides[(k + 1) % LOOKS])
                .collect();
            if changes.len() != 4 {
                radius *= 0.5;
                continue;
            }
            // Close in on each change, at this radius and at a quarter of
            // it: the two together show the way the arm leaves.
            let close = |from: f64, to: f64, r: f64| -> Option<f64> {
                let (mut lo, mut hi) = (from, to);
                let below = side(lo, r);
                if side(hi, r) == below {
                    return None;
                }
                for _ in 0..60 {
                    let mid = 0.5 * (lo + hi);
                    if side(mid, r) == below {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                Some(0.5 * (lo + hi))
            };
            let mut arms = [(at, at); 4];
            let mut whole = true;
            for (slot, &k) in arms.iter_mut().zip(&changes) {
                let width = TAU / LOOKS as f64;
                let Some(outer) = close(angle(k), angle(k) + width, radius) else {
                    whole = false;
                    break;
                };
                // The arm bends between the two radii by less than the
                // looks are apart, or the looks were too far apart.
                let inner =
                    close(outer - 2.0 * width, outer + 2.0 * width, 0.25 * radius).unwrap_or(outer);
                *slot = (on(outer, radius), v2(inner.cos() / m.x, inner.sin() / m.y));
            }
            if whole {
                return Some(arms);
            }
            radius *= 0.5;
        }
        None
    }

    /// Walk an arm out from its first point to the first place the grid
    /// knows the curve by. `nodes` are all the crossings, `own` the one
    /// this arm leaves.
    fn walk(
        &mut self,
        start: V2,
        heading: V2,
        own: usize,
        nodes: &[V2],
    ) -> Option<(Vec<V2>, Reach)> {
        let mut points = vec![start];
        let mut q = start;
        let m = self.base.scale(q.x, q.y);
        let mut way = v2(heading.x * m.x, heading.y * m.y).unit();
        let stride = 0.2 * (self.step.x * m.x).min(self.step.y * m.y);
        let column = |grid: &Grid, x: f64| ((x - grid.lo.x) / grid.step.x).floor() as i64;
        let row = |grid: &Grid, y: f64| ((y - grid.lo.y) / grid.step.y).floor() as i64;
        for count in 0..40_000 {
            let m = self.base.scale(q.x, q.y);
            if !(m.x > 0.0 && m.y > 0.0) {
                return None;
            }
            let lean = self.lean(q);
            let slope = v2(lean.x / m.x, lean.y / m.y);
            let steep = slope.len();
            if steep < 1.0e-14 {
                return None;
            }
            // Along the curve is square to the slope.
            let mut along = v2(-slope.y, slope.x) * (1.0 / steep);
            if along.dot(way) < 0.0 {
                along = along * -1.0;
            }
            // Another crossing ahead? The curve runs straight into it, and
            // close to it there is no slope to steer by.
            for (k, node) in nodes.iter().enumerate() {
                if k == own && count < 8 {
                    continue;
                }
                let to = v2((node.x - q.x) * m.x, (node.y - q.y) * m.y);
                let ahead = to.dot(along).clamp(0.0, stride);
                if (to - along * ahead).len() < 0.75 * stride {
                    return Some((points, Reach::Node(k, along)));
                }
            }
            // A step along, and back onto the curve down the slope.
            let mut reach = stride;
            let mut next: Option<V2> = None;
            for _ in 0..8 {
                let first = v2(q.x + along.x * reach / m.x, q.y + along.y * reach / m.y);
                let mut p = first;
                let mut on = false;
                for _ in 0..12 {
                    let off = self.distance(p);
                    let lean = self.lean(p);
                    let slope = v2(lean.x / m.x, lean.y / m.y);
                    let steep2 = slope.dot(slope);
                    if steep2 < 1.0e-28 {
                        break;
                    }
                    let fix = slope * (off / steep2);
                    p = v2(p.x - fix.x / m.x, p.y - fix.y / m.y);
                    if fix.len() < 1.0e-11 {
                        on = true;
                        break;
                    }
                }
                if on && ((p.x - first.x) * m.x).hypot((p.y - first.y) * m.y) < 0.5 * reach {
                    next = Some(p);
                    break;
                }
                reach *= 0.5;
            }
            let p = next?;
            // Which grid lines the step went over, in the order it did.
            let mut over: Vec<Over> = Vec::new();
            let (ia, ja, ib, jb) = (
                column(self, q.x),
                row(self, q.y),
                column(self, p.x),
                row(self, p.y),
            );
            if ia != ib {
                let line = ia.max(ib);
                let share = (self.lo.x + line as f64 * self.step.x - q.x) / (p.x - q.x);
                let j = row(self, q.y + (p.y - q.y) * share);
                over.push((share, (line, j, true), (ia, j)));
            }
            if ja != jb {
                let line = ja.max(jb);
                let share = (self.lo.y + line as f64 * self.step.y - q.y) / (p.y - q.y);
                let i = column(self, q.x + (p.x - q.x) * share);
                over.push((share, (i, line, false), (i, ja)));
            }
            over.sort_by(|a, b| a.0.total_cmp(&b.0));
            for (_, (i, j, upright), from) in over {
                // Round a whole turn the last column is the first again.
                // `lines` counts the far edge of the window in.
                let place = |k: i64, n: usize, wraps: bool, lines: bool| -> Option<usize> {
                    if wraps {
                        Some(k.rem_euclid(n as i64) as usize)
                    } else {
                        (k >= 0 && (k as usize) < n + usize::from(lines)).then_some(k as usize)
                    }
                };
                let (n, wrap) = (self.n, self.wrap);
                let edge = place(i, n[0], wrap[0], upright).zip(place(j, n[1], wrap[1], !upright));
                let cell =
                    place(from.0, n[0], wrap[0], false).zip(place(from.1, n[1], wrap[1], false));
                let (Some((i, j)), Some(cell)) = (edge, cell) else {
                    points.push(p);
                    return Some((points, Reach::Open));
                };
                if self.crossed((i, j, upright)) {
                    return Some((points, Reach::Edge(self.key((i, j, upright)), cell)));
                }
            }
            way = v2((p.x - q.x) * m.x, (p.y - q.y) * m.y).unit();
            points.push(p);
            q = p;
        }
        None
    }
}

/// Join contour segments end to end. A closed chain repeats its first edge
/// at the end.
fn chains(links: &[(u64, u64)]) -> Vec<Vec<u64>> {
    let mut at: Map<u64, Vec<u32>> = Map::default();
    for (k, &(a, b)) in links.iter().enumerate() {
        at.entry(a).or_default().push(k as u32);
        at.entry(b).or_default().push(k as u32);
    }
    let mut used = vec![false; links.len()];
    let mut out = Vec::new();
    for first in 0..links.len() {
        if used[first] {
            continue;
        }
        used[first] = true;
        let mut chain = VecDeque::from([links[first].0, links[first].1]);
        for forward in [true, false] {
            if chain.front() == chain.back() {
                break;
            }
            loop {
                let end = if forward {
                    *chain.back().unwrap()
                } else {
                    *chain.front().unwrap()
                };
                let Some(k) = at[&end].iter().copied().find(|&k| !used[k as usize]) else {
                    break;
                };
                used[k as usize] = true;
                let (a, b) = links[k as usize];
                let next = if a == end { b } else { a };
                if forward {
                    chain.push_back(next);
                } else {
                    chain.push_front(next);
                }
                if chain.front() == chain.back() {
                    break;
                }
            }
        }
        out.push(chain.into());
    }
    out
}

/// One piece through the points of `run`, which lie on the curve in order of
/// the driving parameter; halved until it follows them.
///
/// Returns whether every piece does; one that does not is kept as it is.
///
/// `ends` gives the way the curve heads at an end of the run that is a place
/// where it crosses itself: the slope of the distance says nothing there.
fn fit(
    grid: &Grid,
    along_v: bool,
    run: &[V2],
    ends: [Option<V2>; 2],
    depth: u32,
    out: &mut Vec<Curve>,
) -> bool {
    let (base, other) = (grid.base, grid.other);
    let drive = |q: V2| if along_v { q.y } else { q.x };
    let held = |q: V2| if along_v { q.x } else { q.y };
    let slope = |q: V2, heading: Option<V2>| {
        if let Some(heading) = heading {
            let with = drive(heading);
            return if with.abs() > 1.0e-300 {
                held(heading) / with
            } else {
                0.0
            };
        }
        let n = other.normal(base.point(q.x, q.y));
        let (du, dv) = (base.du(q.x, q.y), base.dv(q.x, q.y));
        let (along, across) = if along_v { (dv, du) } else { (du, dv) };
        let d = n.dot(across);
        if d.abs() > 1.0e-300 {
            -n.dot(along) / d
        } else {
            0.0
        }
    };
    let (a, b) = (run[0], run[run.len() - 1]);
    let mut piece = Curve::meet(
        base,
        other,
        along_v,
        [drive(a), drive(b)],
        [held(a), held(b), slope(a, ends[0]), slope(b, ends[1]), 0.0],
    );
    // Half a cell of the held parameter, for runs that hardly move in it.
    let slack = 0.5 * if along_v { grid.step.x } else { grid.step.y };
    // How far the curve gets from the cubic that the piece searches from.
    let mut stray: f64 = 0.0;
    // The piece is a graph over its driver. The curve is one only while the
    // other surface's distance keeps rising, or keeps falling, across it in
    // the held parameter; where that changes the curve has turned back, and
    // points on either side of the turn are on different branches.
    let lean = |q: V2| {
        let n = other.normal(base.point(q.x, q.y));
        n.dot(if along_v {
            base.du(q.x, q.y)
        } else {
            base.dv(q.x, q.y)
        })
    };
    // At a crossing the distance leans neither way: the next point along
    // says which side the branch is on.
    let pinned =
        |k: usize| (k == 0 && ends[0].is_some()) || (k + 1 == run.len() && ends[1].is_some());
    let side = match (0..run.len()).find(|&k| !pinned(k)) {
        Some(k) => lean(run[k]),
        None => lean(piece.meet_uv(0.5 * (drive(a) + drive(b)))),
    };
    let mut follows = run
        .iter()
        .enumerate()
        .all(|(k, q)| pinned(k) || lean(*q) * side > 0.0);
    for q in &run[1..run.len() - 1] {
        let t = drive(*q);
        let got = piece.meet_uv(t);
        stray = stray.max((held(*q) - piece.meet_guess(t)).abs());
        follows &= base.point(got.x, got.y).dist(base.point(q.x, q.y)) < 1.0e-7;
    }
    for w in run.windows(2) {
        // Between two points: on the other surface, between them, and on
        // the same branch.
        let t = 0.5 * (drive(w[0]) + drive(w[1]));
        let got = piece.meet_uv(t);
        stray = stray.max((held(got) - piece.meet_guess(t)).abs());
        let (lo, hi) = (held(w[0]).min(held(w[1])), held(w[0]).max(held(w[1])));
        let room = 0.5 * (hi - lo) + slack;
        follows &= other.sdist(base.point(got.x, got.y)).abs() < 1.0e-9
            && held(got) > lo - room
            && held(got) < hi + room
            && lean(got) * side > 0.0;
    }
    // A search that starts further off than a cell can tell branches apart
    // is not to be trusted, wherever it ended up at the points tried.
    follows &= stray <= 0.5 * slack;
    // Twice the furthest seen, as the piece was only looked at in places.
    // A piece that could not be made to follow says so instead.
    piece.w[4] = if follows || depth < 16 && run.len() > 2 {
        2.0 * stray + 1.0e-9 * (1.0 + held(a).abs() + held(b).abs())
    } else {
        -1.0
    };
    if follows || depth >= 16 {
        out.push(piece);
        follows
    } else if run.len() > 2 {
        let m = run.len() / 2;
        let first = fit(grid, along_v, &run[..=m], [ends[0], None], depth + 1, out);
        let second = fit(grid, along_v, &run[m..], [None, ends[1]], depth + 1, out);
        first && second
    } else {
        // Two points only: find the curve between them and try both halves.
        let t = 0.5 * (drive(a) + drive(b));
        let at = |w: f64| if along_v { v2(w, t) } else { v2(t, w) };
        let (lo, hi) = (held(a).min(held(b)), held(a).max(held(b)));
        let (mut lo, mut hi) = (lo - slack, hi + slack);
        let (glo, ghi) = (grid.distance(at(lo)), grid.distance(at(hi)));
        if (glo > 0.0) == (ghi > 0.0) {
            piece.w[4] = -1.0;
            out.push(piece);
            return false;
        }
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if (grid.distance(at(mid)) > 0.0) == (glo > 0.0) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let middle = at(0.5 * (lo + hi));
        let first = fit(grid, along_v, &[a, middle], [ends[0], None], depth + 1, out);
        let second = fit(grid, along_v, &[middle, b], [None, ends[1]], depth + 1, out);
        first && second
    }
}

/// Cut a chain of curve points into pieces, each driven by the parameter it
/// mostly travels along. Returns whether every piece follows the curve.
///
/// `ends` gives the way the curve heads where the chain starts or stops at a
/// place where the curve crosses itself.
fn pieces(grid: &Grid, points: &[V2], ends: [Option<V2>; 2], out: &mut Vec<Curve>) -> bool {
    let base = grid.base;
    // Travel of a step in model units: along `u`, along `v`.
    let travel = |a: V2, b: V2| {
        let m = base.scale(0.5 * (a.x + b.x), 0.5 * (a.y + b.y));
        v2(((b.x - a.x) * m.x).abs(), ((b.y - a.y) * m.y).abs())
    };
    // Crossings of a row and a column next to the same grid node can be
    // almost one point. Pieces that short would be edges below any useful
    // size, so such points are thinned out; both ends of the chain stay.
    let far = |p: V2, q: V2| {
        let d = travel(p, q);
        d.x + d.y > 1.0e-4
    };
    let mut pts: Vec<V2> = Vec::with_capacity(points.len());
    for (k, &q) in points.iter().enumerate() {
        let last = k + 1 == points.len();
        match pts.last().copied() {
            Some(p) if !far(p, q) => {
                if last && pts.len() > 1 {
                    *pts.last_mut().unwrap() = q;
                }
            }
            _ => pts.push(q),
        }
    }
    if pts.len() == 1 && points.len() > 1 && far(pts[0], points[points.len() - 1]) {
        pts.push(points[points.len() - 1]);
    }
    // A step can hide a turn of the curve: both of its points lie in one
    // cell, and the curve goes round a bend between them. Along a parameter
    // in which it turned back it is no graph, and a piece driven by that
    // parameter would jump from one side of the bend to the other. The
    // other surface's distance tells: across a graph it leans one way.
    let mut leans: Vec<V2> = pts.iter().map(|q| grid.lean(*q)).collect();
    // At a crossing the distance leans neither way, and what rounding
    // leaves of it means nothing: the point next along stands in.
    if pts.len() > 2 || ends[0].is_some() != ends[1].is_some() {
        if ends[0].is_some() {
            leans[0] = leans[1];
        }
        if ends[1].is_some() {
            let last = leans.len() - 1;
            leans[last] = leans[last - 1];
        }
    }
    // A bend that turns back both ways gets a point of its own in between.
    let mut k = 0;
    let mut added = 0;
    while k + 1 < pts.len() {
        let (a, b) = (leans[k], leans[k + 1]);
        if a.x * b.x <= 0.0 && a.y * b.y <= 0.0 && added < 64 {
            if let Some(m) = grid.between(pts[k], pts[k + 1]) {
                pts.insert(k + 1, m);
                leans.insert(k + 1, grid.lean(m));
                added += 1;
                continue;
            }
        }
        k += 1;
    }
    // Whether the step from point `k` can be driven along `v`, or `u`.
    let graph = |k: usize, along_v: bool| {
        let (a, b) = (leans[k], leans[k + 1]);
        if along_v {
            a.x * b.x > 0.0
        } else {
            a.y * b.y > 0.0
        }
    };
    let mut sound = true;
    let mut start = 0;
    while start + 1 < pts.len() {
        let first = travel(pts[start], pts[start + 1]);
        let mut along_v = first.y > first.x;
        let lesser = if along_v { first.x } else { first.y };
        if !graph(start, along_v) && graph(start, !along_v) && lesser > 1.0e-6 {
            along_v = !along_v;
        }
        let drive = |q: V2| if along_v { q.y } else { q.x };
        let rising = drive(pts[start + 1]) > drive(pts[start]);
        let mut end = start + 1;
        if !graph(start, along_v) {
            // Still turning back both ways: not to be followed on cells
            // this size.
            let mut run = vec![pts[start], pts[end]];
            if !rising {
                run.reverse();
            }
            let at = out.len();
            fit(grid, along_v, &run, [None, None], 16, out);
            for piece in &mut out[at..] {
                piece.w[4] = -1.0;
            }
            sound = false;
            start = end;
            continue;
        }
        while end + 1 < pts.len() {
            let d = travel(pts[end], pts[end + 1]);
            let (with, across) = if along_v { (d.y, d.x) } else { (d.x, d.y) };
            // Keep the driver while the curve still mostly follows it.
            if (drive(pts[end + 1]) > drive(pts[end])) != rising
                || across > 2.5 * with
                || !graph(end, along_v)
            {
                break;
            }
            end += 1;
        }
        let mut run = pts[start..=end].to_vec();
        let mut heads = [
            ends[0].filter(|_| start == 0),
            ends[1].filter(|_| end + 1 == pts.len()),
        ];
        if !rising {
            run.reverse();
            heads.swap(0, 1);
        }
        sound &= fit(grid, along_v, &run, heads, 0, out);
        start = end;
    }
    sound
}

/// Whether a piece from [`trace`] could not be made to follow the curve.
/// It is still the best guess at where the curve runs, good enough to see
/// that it stays clear of something, but not to be built on.
pub fn doubtful(piece: &Curve) -> bool {
    piece.kind == crate::geom::CurveKind::Meet && piece.w[4] < 0.0
}

/// What to say when a doubtful piece turns out to matter.
pub fn too_close(a: &Surface, b: &Surface) -> String {
    format!(
        "a {} and a {} come too close to touching for their intersection to be followed",
        name(a.kind),
        name(b.kind)
    )
}

/// Where a plane through the axis of a screw surface cuts it: the rulings
/// of a flank, or the circles a rounded root is swept from, one each half
/// turn. These are exact, and there are as many as the screw has half
/// turns, so they are counted out here, where it is known how far to go.
/// `None` for any other pair.
fn through_the_axis(a: &Surface, b: &Surface, region: &Aabb) -> Option<Vec<Curve>> {
    let (plane, screw) = match (a.kind, b.kind) {
        (SurfKind::Plane, SurfKind::Helicoid | SurfKind::Coil) => (a, b),
        (SurfKind::Helicoid | SurfKind::Coil, SurfKind::Plane) => (b, a),
        _ => return None,
    };
    let (n, axis) = (plane.f.z, screw.f.z);
    if n.dot(axis).abs() > 1.0e-9 || plane.sdist(screw.f.o).abs() > crate::math::TOL {
        return None;
    }
    // The heights the region takes up along the axis, and how far out from
    // it the region goes.
    let (mut low, mut high, mut out): (f64, f64, f64) = (f64::INFINITY, f64::NEG_INFINITY, 0.0);
    for i in 0..8 {
        let local = screw.f.local(v3(
            if i & 1 == 0 { region.lo.x } else { region.hi.x },
            if i & 2 == 0 { region.lo.y } else { region.hi.y },
            if i & 4 == 0 { region.lo.z } else { region.hi.z },
        ));
        low = low.min(local.z);
        high = high.max(local.z);
        out = out.max(local.x.hypot(local.y));
    }
    // What passes those heights starts this much lower or higher.
    let (down, up) = match screw.kind {
        SurfKind::Helicoid => {
            let (sideways, up) = screw.ruling();
            let rise = up * out / sideways;
            (rise.max(0.0), (-rise).max(0.0))
        }
        _ => (screw.s.abs(), screw.s.abs()),
    };
    let ends = [(low - down) / screw.t, (high + up) / screw.t];
    let (first, last) = (ends[0].min(ends[1]), ends[0].max(ends[1]));
    // The plane goes out from the axis twice in a turn.
    let start = (-n.dot(screw.f.x)).atan2(n.dot(screw.f.y));
    let (from, to) = (
        ((first - start) / PI).floor() as i64 - 1,
        ((last - start) / PI).ceil() as i64 + 1,
    );
    if to - from > 8192 {
        return None;
    }
    let radial = |u: f64| screw.f.x * u.cos() + screw.f.y * u.sin();
    Some(
        (from..=to)
            .map(|k| {
                let u = start + PI * k as f64;
                let on_axis = screw.f.o + axis * (screw.t * u);
                match screw.kind {
                    SurfKind::Helicoid => {
                        let (sideways, up) = screw.ruling();
                        Curve::line(on_axis, radial(u) * sideways + axis * up)
                    }
                    _ => Curve::circle(on_axis + radial(u) * screw.r, n, radial(u), screw.s.abs()),
                }
            })
            .collect(),
    )
}

/// Pieces of the intersection of `a` and `b` inside `region`.
pub fn trace(a: &Surface, b: &Surface, region: &Aabb) -> Vec<Curve> {
    if let Some(exact) = through_the_axis(a, b, region) {
        return exact;
    }
    let (base, other) = if rank(a.kind) <= rank(b.kind) {
        (a, b)
    } else {
        (b, a)
    };
    // Finer cells each time a piece will not follow the curve.
    let mut out = Vec::new();
    // As many times as it takes two cones, which have no size of their own
    // to set the first cells by.
    for finer in 0..5 {
        let Some(mut grid) = Grid::new(base, other, region, finer) else {
            return Vec::new();
        };
        let Scan {
            mut links,
            mut owners,
            islands,
        } = grid.scan();
        out.clear();
        let mut sound = true;
        // Where the curve crosses itself, its four arms take the place of
        // whatever the cells there made of it.
        let mut starts: Vec<V2> = Vec::new();
        for chain in chains(&links) {
            let closed = chain.len() > 2 && chain.first() == chain.last();
            let mut points: Vec<V2> = chain.iter().map(|&key| grid.crossing(key)).collect();
            for k in 1..points.len() {
                let before = points[k - 1];
                if grid.wrap[0] {
                    points[k].x = wrap_near(points[k].x, before.x);
                }
                if grid.wrap[1] {
                    points[k].y = wrap_near(points[k].y, before.y);
                }
            }
            grid.dips(&points, closed, &mut starts);
        }
        let (found, mut unclear) = grid.nodes(&starts);
        let centres: Vec<V2> = found.iter().map(|(at, _)| *at).collect();
        let mut arms: Vec<Arm> = Vec::with_capacity(4 * found.len());
        let mut open: Vec<usize> = Vec::new();
        // Arms that are the far end of another crossing's arm, and arms
        // that have been tied to something.
        let mut met = vec![false; 4 * found.len()];
        let mut tied = vec![false; 4 * found.len()];
        for (n, (at, starts)) in found.iter().enumerate() {
            for (k, &(start, heading)) in starts.iter().enumerate() {
                let id = 4 * n + k;
                arms.push(Arm {
                    node: *at,
                    heading,
                    points: Vec::new(),
                });
                if met[id] {
                    continue;
                }
                let Some((points, reach)) = grid.walk(start, heading, n, &centres) else {
                    unclear.push(*at);
                    continue;
                };
                match reach {
                    Reach::Edge(key, cell) => {
                        // What the cell made of this crossing of its edge
                        // goes; the arm comes in its place.
                        if let Some(l) = (0..links.len()).find(|&l| {
                            owners[l] == cell && (links[l].0 == key || links[l].1 == key)
                        }) {
                            links.swap_remove(l);
                            owners.swap_remove(l);
                        }
                        links.push((ARM | id as u64, key));
                        owners.push((usize::MAX, usize::MAX));
                        tied[id] = true;
                        arms[id].points = points;
                    }
                    Reach::Node(m, along) => {
                        // The arm of that crossing that points back the
                        // way this one came in.
                        let scale = base.scale(centres[m].x, centres[m].y);
                        let back = |x: usize| {
                            let h = found[m].1[x].1;
                            -v2(h.x * scale.x, h.y * scale.y).unit().dot(along)
                        };
                        let other_arm = (0..4)
                            .max_by(|&x, &y| back(x).total_cmp(&back(y)))
                            .map_or(id, |x| 4 * m + x);
                        if other_arm == id || tied[other_arm] || met[other_arm] {
                            unclear.push(*at);
                            continue;
                        }
                        met[other_arm] = true;
                        tied[id] = true;
                        tied[other_arm] = true;
                        links.push((ARM | id as u64, ARM | other_arm as u64));
                        owners.push((usize::MAX, usize::MAX));
                        arms[id].points = points;
                    }
                    Reach::Open => {
                        arms[id].points = points;
                        open.push(id);
                    }
                }
            }
        }
        let mut drawn: Vec<(Vec<V2>, [Option<V2>; 2])> = chains(&links)
            .into_iter()
            .map(|chain| {
                let mut points: Vec<V2> = Vec::with_capacity(chain.len());
                let mut ends = [None, None];
                for (k, key) in chain.iter().copied().enumerate() {
                    if key & ARM == 0 {
                        points.push(grid.crossing(key));
                        continue;
                    }
                    // An arm is tied to one thing only, so it is an end.
                    let arm = &arms[(key & !ARM) as usize];
                    if k == 0 {
                        points.push(arm.node);
                        points.extend(arm.points.iter().copied());
                        ends[0] = Some(arm.heading);
                    } else {
                        points.extend(arm.points.iter().rev().copied());
                        points.push(arm.node);
                        ends[1] = Some(arm.heading);
                    }
                }
                (points, ends)
            })
            .collect();
        for id in open {
            let arm = &arms[id];
            let mut points = vec![arm.node];
            points.extend(arm.points.iter().copied());
            drawn.push((points, [Some(arm.heading), None]));
        }
        drawn.extend(islands.into_iter().map(|ring| (ring, [None, None])));
        for (mut points, ends) in drawn {
            // Follow the curve across the ends of a whole turn.
            for k in 1..points.len() {
                let before = points[k - 1];
                if grid.wrap[0] {
                    points[k].x = wrap_near(points[k].x, before.x);
                }
                if grid.wrap[1] {
                    points[k].y = wrap_near(points[k].y, before.y);
                }
            }
            // Where the surfaces only touch, their distance is zero to
            // rounding and changes sign at random. Those are not crossings:
            // the two normals are parallel at every one of them.
            let touching = points.iter().all(|q| {
                let p = base.point(q.x, q.y);
                base.normal(p).cross(other.normal(p)).len() < 1.0e-6
            });
            if touching {
                continue;
            }
            sound &= pieces(&grid, &points, ends, &mut out);
        }
        // A crossing that could not be drawn leaves whatever passes it in
        // doubt.
        if !unclear.is_empty() {
            sound = false;
            for piece in out.iter_mut() {
                let [t0, t1] = piece.span;
                let near = (0..=16).any(|k| {
                    let q = piece.meet_uv(t0 + (t1 - t0) * k as f64 / 16.0);
                    unclear.iter().any(|at| {
                        let m = base.scale(at.x, at.y);
                        let cell = (grid.step.x * m.x).max(grid.step.y * m.y);
                        let mut q = q;
                        if grid.wrap[0] {
                            q.x = wrap_near(q.x, at.x);
                        }
                        if grid.wrap[1] {
                            q.y = wrap_near(q.y, at.y);
                        }
                        grid.apart(*at, q) < 4.0 * cell
                    })
                });
                if near {
                    piece.w[4] = -1.0;
                }
            }
        }
        if sound {
            break;
        }
        if crate::diag::trace() {
            eprintln!(
                "meet: {:?} and {:?} on {:?} cells: a piece does not follow",
                base.kind, other.kind, grid.n
            );
        }
    }
    out
}

fn name(kind: SurfKind) -> &'static str {
    match kind {
        SurfKind::Plane => "plane",
        SurfKind::Cylinder => "cylinder",
        SurfKind::Cone => "cone",
        SurfKind::Sphere => "sphere",
        SurfKind::Torus => "torus",
        SurfKind::Helicoid => "helicoid",
        SurfKind::Coil => "coil",
        SurfKind::Spline => "spline",
        SurfKind::Pipe => "pipe",
    }
}
