//! Intersection curves of surface pairs that have no closed form.
//!
//! One surface is the base. In its parameter plane the curve is where the
//! other surface's signed distance is zero. That distance changes no faster
//! than the point moves, so a cell whose centre is further from the other
//! surface than the cell reaches cannot hold any of the curve, and neither
//! can one that far outside the region asked about. The window is halved on
//! that test until what is left is a thin band of small cells.
//! Each is crossed like a contour map, the crossings are joined into chains,
//! and the chains are cut into pieces a [`Curve::meet`] can follow.
//!
//! Tangential contact changes no sign and is not reported, as for the
//! closed-form pairs. Where the surfaces come close to touching, the curve
//! can turn more tightly than the cells are wide, or cross itself. Such a
//! stretch shows up as a piece that will not follow its points. The search
//! is then repeated on finer cells; a piece that still does not follow is
//! handed over marked as doubtful (see [`doubtful`]), for the caller to
//! refuse if anything turns out to depend on it.

use std::collections::VecDeque;

use crate::fast::Map;
use crate::geom::{Curve, SurfKind, Surface};
use crate::math::*;

/// Finest cells across the window, at most.
const FINEST: usize = 512;

/// The better surface to write the curve on comes first: its parameters
/// stay regular for longer.
fn rank(kind: SurfKind) -> u8 {
    match kind {
        SurfKind::Plane => 0,
        SurfKind::Cylinder => 1,
        SurfKind::Torus => 2,
        SurfKind::Cone => 3,
        SurfKind::Sphere => 4,
    }
}

/// The radius a surface bends with, if it has one.
fn bend(s: &Surface) -> f64 {
    match s.kind {
        SurfKind::Plane | SurfKind::Cone => f64::INFINITY,
        SurfKind::Cylinder | SurfKind::Sphere => s.r,
        SurfKind::Torus => s.s,
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
    fn cell(&mut self, i: usize, j: usize, links: &mut Vec<(u64, u64)>) {
        let above = [
            self.value(i, j) > 0.0,
            self.value(i + 1, j) > 0.0,
            self.value(i + 1, j + 1) > 0.0,
            self.value(i, j + 1) > 0.0,
        ];
        let sides = [
            (above[0] != above[1], (i, j, false)),
            (above[1] != above[2], (i + 1, j, true)),
            (above[3] != above[2], (i, j + 1, false)),
            (above[0] != above[3], (i, j, true)),
        ];
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
    }

    /// Contour segments of every cell the curve can be in.
    fn scan(&mut self) -> Vec<(u64, u64)> {
        let mut links = Vec::new();
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
            if self.other.sdist(centre).abs() > reach || self.region.distance(centre) > reach {
                continue;
            }
            if i1 - i0 == 1 && j1 - j0 == 1 {
                self.cell(i0, j0, &mut links);
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
        links
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
fn fit(grid: &Grid, along_v: bool, run: &[V2], depth: u32, out: &mut Vec<Curve>) -> bool {
    let (base, other) = (grid.base, grid.other);
    let drive = |q: V2| if along_v { q.y } else { q.x };
    let held = |q: V2| if along_v { q.x } else { q.y };
    let slope = |q: V2| {
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
        [held(a), held(b), slope(a), slope(b), 0.0],
    );
    // Half a cell of the held parameter, for runs that hardly move in it.
    let slack = 0.5 * if along_v { grid.step.x } else { grid.step.y };
    // How far the curve gets from the cubic that the piece searches from.
    let mut stray: f64 = 0.0;
    let mut follows = true;
    for q in &run[1..run.len() - 1] {
        let t = drive(*q);
        let got = piece.meet_uv(t);
        stray = stray.max((held(*q) - piece.meet_guess(t)).abs());
        follows &= base.point(got.x, got.y).dist(base.point(q.x, q.y)) < 1.0e-7;
    }
    for w in run.windows(2) {
        // Between two points: on the other surface, and between them.
        let t = 0.5 * (drive(w[0]) + drive(w[1]));
        let got = piece.meet_uv(t);
        stray = stray.max((held(got) - piece.meet_guess(t)).abs());
        let (lo, hi) = (held(w[0]).min(held(w[1])), held(w[0]).max(held(w[1])));
        let room = 0.5 * (hi - lo) + slack;
        follows &= other.sdist(base.point(got.x, got.y)).abs() < 1.0e-9
            && held(got) > lo - room
            && held(got) < hi + room;
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
        let first = fit(grid, along_v, &run[..=m], depth + 1, out);
        let second = fit(grid, along_v, &run[m..], depth + 1, out);
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
        let first = fit(grid, along_v, &[a, middle], depth + 1, out);
        let second = fit(grid, along_v, &[middle, b], depth + 1, out);
        first && second
    }
}

/// Cut a chain of curve points into pieces, each driven by the parameter it
/// mostly travels along. Returns whether every piece follows the curve.
fn pieces(grid: &Grid, points: &[V2], out: &mut Vec<Curve>) -> bool {
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
    let mut sound = true;
    let mut start = 0;
    while start + 1 < pts.len() {
        let first = travel(pts[start], pts[start + 1]);
        let along_v = first.y > first.x;
        let drive = |q: V2| if along_v { q.y } else { q.x };
        let rising = drive(pts[start + 1]) > drive(pts[start]);
        let mut end = start + 1;
        while end + 1 < pts.len() {
            let d = travel(pts[end], pts[end + 1]);
            let (with, across) = if along_v { (d.y, d.x) } else { (d.x, d.y) };
            // Keep the driver while the curve still mostly follows it.
            if (drive(pts[end + 1]) > drive(pts[end])) != rising || across > 2.5 * with {
                break;
            }
            end += 1;
        }
        let mut run = pts[start..=end].to_vec();
        if !rising {
            run.reverse();
        }
        sound &= fit(grid, along_v, &run, 0, out);
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

/// Pieces of the intersection of `a` and `b` inside `region`.
pub fn trace(a: &Surface, b: &Surface, region: &Aabb) -> Vec<Curve> {
    let (base, other) = if rank(a.kind) <= rank(b.kind) {
        (a, b)
    } else {
        (b, a)
    };
    // Finer cells each time a piece will not follow the curve.
    let mut out = Vec::new();
    for finer in 0..3 {
        let Some(mut grid) = Grid::new(base, other, region, finer) else {
            return Vec::new();
        };
        let links = grid.scan();
        out.clear();
        let mut sound = true;
        for chain in chains(&links) {
            let mut points: Vec<V2> = Vec::with_capacity(chain.len());
            for key in chain {
                let mut q = grid.crossing(key);
                // Follow the curve across the ends of a whole turn.
                if let Some(&p) = points.last() {
                    if grid.wrap[0] {
                        q.x = wrap_near(q.x, p.x);
                    }
                    if grid.wrap[1] {
                        q.y = wrap_near(q.y, p.y);
                    }
                }
                points.push(q);
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
            sound &= pieces(&grid, &points, &mut out);
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
    }
}
