//! How far apart two things are, where they come closest, and how much
//! of one is inside the other.
//!
//! The closest approach of two bodies is found where one of three things
//! happens: a point of an edge is nearest to a face, a vertex is, or two
//! faces face each other squarely across the gap. An edge is walked
//! against one face at a time, since against one face the distance along
//! it has few dips; boxes rule out every pair that cannot beat the best
//! found so far, so a search between two large bodies looks only at where
//! they are close.

use crate::boolean::{boolean, point_in_solid, Op, Side, NO_OVERLAP};
use crate::geom::{CurveKind, SurfKind};
use crate::math::*;
use crate::props;
use crate::topo::Body;
use crate::trim::{classify, edge_closest, PtClass};

/// The closest approach of two things: how far, and the point of each.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gap {
    pub distance: f64,
    pub a: V3,
    pub b: V3,
}

/// A body, or one face, edge or point of it, ready to be measured to.
pub struct Part<'a> {
    side: Side<'a>,
    faces: Vec<u32>,
    edges: Vec<u32>,
    points: Vec<V3>,
}

impl<'a> Part<'a> {
    pub fn body(body: &'a Body) -> Part<'a> {
        Part {
            side: Side::new(body),
            faces: (0..body.face_count() as u32).collect(),
            edges: (0..body.edge_count() as u32).collect(),
            points: Vec::new(),
        }
    }

    pub fn face(body: &'a Body, f: u32) -> Part<'a> {
        let mut edges: Vec<u32> = body.face_hes(f).map(|h| h >> 1).collect();
        edges.sort_unstable();
        edges.dedup();
        Part {
            side: Side::new(body),
            faces: vec![f],
            edges,
            points: Vec::new(),
        }
    }

    pub fn edge(body: &'a Body, e: u32) -> Part<'a> {
        Part {
            side: Side::new(body),
            faces: Vec::new(),
            edges: vec![e],
            points: Vec::new(),
        }
    }

    pub fn point(body: &'a Body, p: V3) -> Part<'a> {
        Part {
            side: Side::new(body),
            faces: Vec::new(),
            edges: Vec::new(),
            points: vec![p],
        }
    }

    fn body_ref(&self) -> &Body {
        self.side.body
    }

    /// Whether `p` is inside the solid, when the part is a whole body.
    pub fn holds(&self, p: V3) -> bool {
        self.faces.len() == self.body_ref().face_count()
            && !self.faces.is_empty()
            && point_in_solid(&self.side, p) == Some(true)
    }

    /// Bounds of everything in the part.
    fn bounds(&self) -> Aabb {
        let mut b = Aabb::EMPTY;
        for &f in &self.faces {
            b.union(&self.side.boxes[f as usize]);
        }
        for &e in &self.edges {
            b.union(&self.side.eboxes[e as usize]);
        }
        for &p in &self.points {
            b.add(p);
        }
        b
    }

    /// Closest point of the part to `p`, when it is closer than `below`.
    fn closest(&self, p: V3, mut below: f64) -> Option<(f64, V3)> {
        let mut found = None;
        for &f in &self.faces {
            if self.side.boxes[f as usize].distance(p) >= below {
                continue;
            }
            let (d, q) = on_face(&self.side, f, p);
            if d < below {
                below = d;
                found = Some((d, q));
            }
        }
        // The edges of a face are part of it.
        if self.faces.is_empty() {
            for &e in &self.edges {
                if self.side.eboxes[e as usize].distance(p) >= below {
                    continue;
                }
                let (d, q) = on_edge(self.body_ref(), e, p);
                if d < below {
                    below = d;
                    found = Some((d, q));
                }
            }
        }
        for &q in &self.points {
            let d = q.dist(p);
            if d < below {
                below = d;
                found = Some((d, q));
            }
        }
        found
    }
}

/// How far apart two boxes are; zero when they overlap.
fn apart(a: &Aabb, b: &Aabb) -> f64 {
    let gap = |alo: f64, ahi: f64, blo: f64, bhi: f64| (blo - ahi).max(alo - bhi).max(0.0);
    v3(
        gap(a.lo.x, a.hi.x, b.lo.x, b.hi.x),
        gap(a.lo.y, a.hi.y, b.lo.y, b.hi.y),
        gap(a.lo.z, a.hi.z, b.lo.z, b.hi.z),
    )
    .len()
}

/// The lowest value of `f` over `[a, b]`: every dip among `samples` even
/// steps that could go below `below` is followed to its bottom. `reach` is
/// how far the value can fall between two samples.
fn lowest(
    a: f64,
    b: f64,
    samples: usize,
    reach: f64,
    below: f64,
    mut f: impl FnMut(f64) -> f64,
) -> Option<(f64, f64)> {
    let n = samples.max(2);
    let at = |i: usize| a + (b - a) * i as f64 / n as f64;
    let values: Vec<f64> = (0..=n).map(|i| f(at(i))).collect();
    let mut best: Option<(f64, f64)> = None;
    let mut bound = below;
    for i in 0..=n {
        let v = values[i];
        let left = if i > 0 { values[i - 1] } else { f64::INFINITY };
        let right = if i < n { values[i + 1] } else { f64::INFINITY };
        if v > left || v > right || v - reach >= bound {
            continue;
        }
        // Golden section between the neighbours.
        let (mut lo, mut hi) = (at(i.saturating_sub(1)), at((i + 1).min(n)));
        const SHRINK: f64 = 0.618_033_988_749_894_9;
        let mut x1 = hi - SHRINK * (hi - lo);
        let mut x2 = lo + SHRINK * (hi - lo);
        let (mut f1, mut f2) = (f(x1), f(x2));
        for _ in 0..48 {
            if f1 <= f2 {
                hi = x2;
                x2 = x1;
                f2 = f1;
                x1 = hi - SHRINK * (hi - lo);
                f1 = f(x1);
            } else {
                lo = x1;
                x1 = x2;
                f1 = f2;
                x2 = lo + SHRINK * (hi - lo);
                f2 = f(x2);
            }
        }
        let (t, value) = [(x1, f1), (x2, f2), (at(i), v)]
            .into_iter()
            .min_by(|p, q| p.1.total_cmp(&q.1))
            .unwrap();
        if value < bound {
            bound = value;
            best = Some((t, value));
        }
    }
    best
}

/// Closest point of the bounded edge `e` to `p`.
fn on_edge(body: &Body, e: u32, p: V3) -> (f64, V3) {
    let c = body.e_curve_ref(e);
    let [t0, t1] = body.e_t[e as usize];
    if matches!(c.kind, CurveKind::Line | CurveKind::Circle) {
        let (t, d, _) = edge_closest(body, e, p);
        return (d, c.point(t));
    }
    let length = body.e_length(e);
    let steps = steps(body, e).max(24);
    match lowest(t0, t1, steps, length / steps as f64, f64::INFINITY, |t| {
        c.point(t).dist(p)
    }) {
        Some((t, d)) => (d, c.point(t)),
        None => (c.point(t0).dist(p), c.point(t0)),
    }
}

/// Closest point of the trimmed face `f` to `p`: the foot of `p` on the
/// surface when that is on the face, else the nearest point of its edges.
/// (On each of the five surfaces the nearest foot is the only dip of the
/// distance, so when it falls outside the face the answer is on the rim.)
fn on_face(side: &Side, f: u32, p: V3) -> (f64, V3) {
    let body = side.body;
    let foot = body.f_surface(f).project(p);
    if foot.is_finite() && classify(body, side.trim(f), f, foot) != PtClass::Out {
        return (foot.dist(p), foot);
    }
    let mut best = (f64::INFINITY, foot);
    for h in body.face_hes(f) {
        let found = on_edge(body, h >> 1, p);
        if found.0 < best.0 {
            best = found;
        }
    }
    best
}

/// How many steps an edge is walked in.
fn steps(body: &Body, e: u32) -> usize {
    let c = body.e_curve_ref(e);
    let [t0, t1] = body.e_t[e as usize];
    match c.kind {
        CurveKind::Line => 8,
        CurveKind::Circle | CurveKind::Ellipse => {
            (((t1 - t0).abs() / (PI / 12.0)).ceil() as usize).clamp(6, 48)
        }
        CurveKind::Helix => (((t1 - t0).abs() / (PI / 12.0)).ceil() as usize).max(6),
        _ => 32,
    }
}

/// Where the things in `a` and `b` come closest. Zero when they touch or
/// cross.
pub fn gap(a: &Part, b: &Part) -> Option<Gap> {
    /// Nearer than this is touching.
    const TOUCH: f64 = 1.0e-9;
    let mut best: Option<Gap> = None;
    let below = |best: &Option<Gap>| best.map_or(f64::INFINITY, |g| g.distance);

    // The points of each against the other. A few vertices first give a
    // bound that lets the boxes start ruling pairs out.
    let seeds = |part: &Part| -> Vec<V3> {
        let mut points = part.points.clone();
        let body = part.body_ref();
        let ends: Vec<V3> = part
            .edges
            .iter()
            .map(|&e| body.verts[body.e_v[e as usize][0] as usize])
            .collect();
        let stride = (ends.len() / 24).max(1);
        points.extend(ends.into_iter().step_by(stride));
        points
    };
    for (flip, from, to) in [(false, a, b), (true, b, a)] {
        for p in seeds(from) {
            if let Some((d, q)) = to.closest(p, below(&best)) {
                best = Some(if flip {
                    Gap {
                        distance: d,
                        a: q,
                        b: p,
                    }
                } else {
                    Gap {
                        distance: d,
                        a: p,
                        b: q,
                    }
                });
            }
        }
    }
    if below(&best) <= TOUCH {
        return best;
    }

    // Every edge of one against every face (or lone edge) of the other,
    // nearest boxes first.
    struct Pair {
        apart: f64,
        flip: bool,
        edge: u32,
        /// Face of the other part, or its edge when it has no faces.
        target: u32,
    }
    let mut pairs: Vec<Pair> = Vec::new();
    for (flip, from, to) in [(false, a, b), (true, b, a)] {
        let reach = to.bounds();
        for &e in &from.edges {
            let ebox = &from.side.eboxes[e as usize];
            if apart(ebox, &reach) >= below(&best) {
                continue;
            }
            let (targets, boxes) = if to.faces.is_empty() {
                (&to.edges, &to.side.eboxes)
            } else {
                (&to.faces, &to.side.boxes)
            };
            for &target in targets {
                let d = apart(ebox, &boxes[target as usize]);
                if d < below(&best) {
                    pairs.push(Pair {
                        apart: d,
                        flip,
                        edge: e,
                        target,
                    });
                }
            }
        }
    }
    pairs.sort_by(|p, q| p.apart.total_cmp(&q.apart));
    for pair in &pairs {
        if pair.apart >= below(&best) {
            break;
        }
        let (from, to) = if pair.flip { (b, a) } else { (a, b) };
        let body = from.body_ref();
        let c = body.e_curve_ref(pair.edge);
        let [t0, t1] = body.e_t[pair.edge as usize];
        let n = steps(body, pair.edge);
        let reach = body.e_length(pair.edge) / n as f64;
        let to_target = |p: V3| {
            if to.faces.is_empty() {
                on_edge(to.body_ref(), pair.target, p)
            } else {
                on_face(&to.side, pair.target, p)
            }
        };
        if let Some((t, d)) = lowest(t0, t1, n, reach, below(&best), |t| to_target(c.point(t)).0) {
            let p = c.point(t);
            let q = to_target(p).1;
            best = Some(if pair.flip {
                Gap {
                    distance: d,
                    a: q,
                    b: p,
                }
            } else {
                Gap {
                    distance: d,
                    a: p,
                    b: q,
                }
            });
            if d <= TOUCH {
                return best;
            }
        }
    }

    // Two faces that face each other squarely, away from their edges. Two
    // flat faces never do so anywhere their edges do not.
    for &fa in &a.faces {
        let abox = &a.side.boxes[fa as usize];
        let sa = a.body_ref().f_surface(fa);
        for &fb in &b.faces {
            let sb = b.body_ref().f_surface(fb);
            if sa.kind == SurfKind::Plane && sb.kind == SurfKind::Plane {
                continue;
            }
            if apart(abox, &b.side.boxes[fb as usize]) >= below(&best) {
                continue;
            }
            // Start from a few points of the first face and step from one
            // surface to the other and back until the steps stop moving.
            let trim = a.side.trim(fa);
            let mut starts: Vec<V3> = trim
                .interior_points()
                .into_iter()
                .take(4)
                .map(|(q, _)| trim.point(q))
                .collect();
            starts.extend(
                a.body_ref()
                    .face_hes(fa)
                    .take(4)
                    .map(|h| a.body_ref().e_mid(h >> 1)),
            );
            for start in starts {
                let mut x = sa.project(start);
                let mut y = sb.project(x);
                let mut settled = false;
                for _ in 0..200 {
                    let next = sa.project(y);
                    if !next.is_finite() {
                        break;
                    }
                    let moved = next.dist(x);
                    x = next;
                    y = sb.project(x);
                    if !y.is_finite() {
                        break;
                    }
                    if moved <= 1.0e-12 * (1.0 + x.len()) {
                        settled = true;
                        break;
                    }
                }
                if !settled || x.dist(y) >= below(&best) {
                    continue;
                }
                if classify(a.body_ref(), trim, fa, x) == PtClass::Out
                    || classify(b.body_ref(), b.side.trim(fb), fb, y) == PtClass::Out
                {
                    continue;
                }
                best = Some(Gap {
                    distance: x.dist(y),
                    a: x,
                    b: y,
                });
                if x.dist(y) <= TOUCH {
                    return best;
                }
            }
        }
    }
    best
}

/// How two bodies stand to each other: the clearance between them, and
/// the volume they both fill.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Clash {
    /// Zero when the bodies touch, cross, or one holds the other.
    pub clearance: f64,
    pub overlap: f64,
    pub a: V3,
    pub b: V3,
}

/// A point of the body's skin: a vertex, or for a body with no edge at
/// all (a ball, a ring) a point of its one surface.
fn skin_point(body: &Body) -> V3 {
    body.verts
        .first()
        .copied()
        .or_else(|| body.surfs.first().map(|s| s.point(0.0, 0.0)))
        .unwrap_or(V3::ZERO)
}

/// Clearance and shared volume of two bodies.
pub fn clash(a: &Body, b: &Body) -> Result<Clash, String> {
    let shared = |common: &Body, at: V3| Clash {
        clearance: 0.0,
        overlap: props::volume(common).abs(),
        a: at,
        b: at,
    };
    // What they share, when their boxes say they might.
    let mut unsure = None;
    if a.bbox().grown(TOL).overlaps(&b.bbox().grown(TOL)) {
        match boolean(a, b, Op::Intersect) {
            Ok(common) if !common.is_empty() => {
                return Ok(shared(&common, skin_point(&common)));
            }
            Ok(_) => {}
            Err(error) if error == NO_OVERLAP => {}
            Err(error) => unsure = Some(error),
        }
    }
    let (pa, pb) = (Part::body(a), Part::body(b));
    let found = gap(&pa, &pb).ok_or("there is nothing to measure between")?;
    if let Some(error) = unsure {
        // The overlap could not be worked out. Skins that keep apart
        // settle it all the same: nothing is shared, unless one body is
        // wholly inside the other.
        if found.distance <= 1.0e-7 {
            return Err(format!("the overlap could not be worked out: {error}"));
        }
        for (inner, outer) in [(a, &pb), (b, &pa)] {
            let probe = skin_point(inner);
            if point_in_solid(&outer.side, probe) == Some(true) {
                return Ok(shared(inner, probe));
            }
        }
    }
    Ok(Clash {
        clearance: if found.distance <= 1.0e-9 {
            0.0
        } else {
            found.distance
        },
        overlap: 0.0,
        a: found.a,
        b: found.b,
    })
}
