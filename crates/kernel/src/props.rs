//! Exact properties from boundary integrals.

use crate::geom::CurveKind;
use crate::math::*;
use crate::topo::Body;

/// Five-point Gauss–Legendre nodes and weights on `[-1, 1]`.
const GAUSS_X: [f64; 5] = [
    0.0,
    0.538_469_310_105_683_1,
    -0.538_469_310_105_683_1,
    0.906_179_845_938_664,
    -0.906_179_845_938_664,
];
const GAUSS_W: [f64; 5] = [
    0.568_888_888_888_888_9,
    0.478_628_670_499_366_5,
    0.478_628_670_499_366_5,
    0.236_926_885_056_189_08,
    0.236_926_885_056_189_08,
];

/// Five-point Gauss–Legendre estimate of the integral of `f` over `[a, b]`.
fn gauss(a: f64, b: f64, f: &mut impl FnMut(f64) -> f64) -> f64 {
    let half = 0.5 * (b - a);
    let mut sum = 0.0;
    for k in 0..5 {
        sum += GAUSS_W[k] * f(a + half * (1.0 + GAUSS_X[k]));
    }
    sum * half
}

/// The integral of `f` over `[a, b]`, halving the range until two halves
/// say the same as the whole. `whole` is the estimate over the range, and
/// `scale` the size of the answer the error is measured against.
fn settle(
    a: f64,
    b: f64,
    whole: f64,
    scale: f64,
    depth: u32,
    f: &mut impl FnMut(f64) -> f64,
) -> f64 {
    let mid = 0.5 * (a + b);
    let (left, right) = (gauss(a, mid, f), gauss(mid, b, f));
    if depth == 0 || (left + right - whole).abs() <= 1.0e-13 * scale {
        return left + right;
    }
    settle(a, mid, left, scale, depth - 1, f) + settle(mid, b, right, scale, depth - 1, f)
}

/// Integrate `f(t)` over the parameter range of edge `e`.
fn integrate_edge(body: &Body, e: u32, mut f: impl FnMut(f64) -> f64) -> f64 {
    let [t0, t1] = body.e_t[e as usize];
    let kind = body.e_curve_ref(e).kind;
    let pieces = match kind {
        CurveKind::Line => 1,
        _ => (((t1 - t0) / (PI / 8.0)).ceil() as usize).max(1),
    };
    let step = (t1 - t0) / pieces as f64;
    // Lines, circles and ellipses turn evenly with their parameter; any
    // other curve is followed as closely as it takes.
    let even = matches!(
        kind,
        CurveKind::Line | CurveKind::Circle | CurveKind::Ellipse
    );
    let size = if even { 0.0 } else { body.e_length(e).max(1.0) };
    let mut sum = 0.0;
    for i in 0..pieces {
        let (a, b) = (t0 + step * i as f64, t0 + step * (i + 1) as f64);
        let whole = gauss(a, b, &mut f);
        sum += if even {
            whole
        } else {
            settle(a, b, whole, whole.abs().max(size * size), 12, &mut f)
        };
    }
    sum
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlanarProps {
    pub centroid: V3,
    pub area: f64,
    pub perimeter: f64,
    pub loops: u32,
    pub edges: u32,
}

/// Area, centroid and perimeter of a planar face, to quadrature precision.
pub fn planar_props(body: &Body, f: u32) -> Option<PlanarProps> {
    let s = body.f_surface(f);
    if !s.is_plane() {
        return None;
    }
    // Integrate in plane coordinates oriented with the outward normal.
    let frame = s.f;
    let outward = body.f_sense[f as usize];
    let (mut area2, mut mu, mut mv, mut perimeter) = (0.0, 0.0, 0.0, 0.0);
    let mut edges: Vec<u32> = Vec::new();
    let mut loops = 0;
    for l in body.face_loops(f) {
        loops += 1;
        for h in body.loop_hes(l) {
            let e = h >> 1;
            edges.push(e);
            let c = *body.e_curve_ref(e);
            // Travel direction of the half-edge relative to the edge, and of
            // the plane coordinates relative to the outward normal.
            let sign = if (h & 1 == 0) == outward { 1.0 } else { -1.0 };
            let local = |t: f64| {
                let p = frame.local(c.point(t));
                let d = c.deriv(t);
                (p.x, p.y, d.dot(frame.x), d.dot(frame.y))
            };
            area2 += sign
                * integrate_edge(body, e, |t| {
                    let (u, v, du, dv) = local(t);
                    u * dv - v * du
                });
            mu += sign
                * integrate_edge(body, e, |t| {
                    let (u, _, _, dv) = local(t);
                    0.5 * u * u * dv
                });
            mv -= sign
                * integrate_edge(body, e, |t| {
                    let (_, v, du, _) = local(t);
                    0.5 * v * v * du
                });
            perimeter += match c.kind {
                CurveKind::Ellipse => integrate_edge(body, e, |t| c.deriv(t).len()),
                _ => body.e_length(e),
            };
        }
    }
    let area = 0.5 * area2;
    if area.abs() < 1.0e-18 {
        return None;
    }
    edges.sort_unstable();
    edges.dedup();
    let centroid = frame.o + frame.x * (mu / area) + frame.y * (mv / area);
    Some(PlanarProps {
        centroid,
        area: area.abs(),
        perimeter,
        loops,
        edges: edges.len() as u32,
    })
}

// ---------------------------------------------------------------------
// Volume, area and centre of mass.
//
// Everything a solid weighs in on is an integral over its faces, and an
// integral over a face is one around its edges: with `P(u, v)` the
// integral of the quantity along `v` from a fixed line, Green's theorem
// turns the sum over the face into the line integral of `P du` along its
// loops. The inner integral is over a polynomial or a trigonometric
// polynomial and the outer one along an exact curve, so both converge to
// rounding; no face is ever cut into triangles.
// ---------------------------------------------------------------------

use crate::geom::{SurfKind, Surface};

/// Quantities carried through the integrals: the volume under the face,
/// its area, and the three first moments of the volume.
const N: usize = 5;
type Sums = [f64; N];

fn add(a: &mut Sums, b: &Sums, k: f64) {
    for i in 0..N {
        a[i] += k * b[i];
    }
}

/// Point and both parameter derivatives. A cone is read the way
/// [`Surface::uv`] writes it: `u` is the true angle on either side of the
/// tip.
fn at(s: &Surface, u: f64, v: f64) -> (V3, V3, V3) {
    if s.kind != SurfKind::Cone {
        return s.d1(u, v);
    }
    let (sin, cos) = u.sin_cos();
    let radial = s.f.x * cos + s.f.y * sin;
    let around = s.f.y * cos - s.f.x * sin;
    let slope = s.s.tan();
    (
        s.f.o + s.f.z * v + radial * (v.abs() * slope),
        around * (v.abs() * slope),
        s.f.z + radial * (slope * v.signum()),
    )
}

/// What a unit of `(u, v)` at this spot adds to each sum, measured from
/// `origin`.
fn density(s: &Surface, u: f64, v: f64, origin: V3) -> Sums {
    let (p, su, sv) = at(s, u, v);
    let n = su.cross(sv);
    let q = p - origin;
    let half = 0.5 * q.len2();
    [q.dot(n) / 3.0, n.len(), half * n.x, half * n.y, half * n.z]
}

/// Gauss–Legendre over `[a, b]` in pieces no longer than `piece`.
fn span(a: f64, b: f64, piece: f64, mut f: impl FnMut(f64) -> Sums) -> Sums {
    let mut sum = [0.0; N];
    if a == b {
        return sum;
    }
    let pieces = (((b - a).abs() / piece).ceil() as usize).max(1);
    let step = (b - a) / pieces as f64;
    for i in 0..pieces {
        let (lo, half) = (a + step * i as f64, 0.5 * step);
        for k in 0..5 {
            add(
                &mut sum,
                &f(lo + half * (1.0 + GAUSS_X[k])),
                GAUSS_W[k] * half,
            );
        }
    }
    sum
}

/// The inner integral along `v` of a spline, from `from` to `to`: knot span
/// by knot span, since the surface is a different polynomial on each. What
/// a strip of a polynomial patch of degree three weighs is a polynomial
/// that one Gauss rule takes whole; each span is still cut in two for the
/// area, and in four for a surface with weights, neither of which is a
/// polynomial at all.
fn up_a_spline(s: &Surface, from: f64, to: f64, mut f: impl FnMut(f64) -> Sums) -> Sums {
    let form = s.spline_form();
    let knots = &form.knots[1];
    let share = if form.rational || form.degree[1] > 3 {
        0.25
    } else {
        0.5
    };
    let (lo, hi) = (from.min(to), from.max(to));
    let mut sum = [0.0; N];
    let mut a = lo;
    for &k in knots.iter().chain(std::iter::once(&f64::INFINITY)) {
        let b = k.min(hi);
        if b > a {
            let width = b - a;
            add(&mut sum, &span(a, b, share * width, &mut f), 1.0);
            a = b;
        }
        if k >= hi {
            break;
        }
    }
    if to < from {
        for x in &mut sum {
            *x = -*x;
        }
    }
    sum
}

/// How long a stretch of a parameter one Gauss rule can take: the whole
/// of it where the surface is straight that way, a slice of a turn where
/// it bends.
fn stride(s: &Surface, along_v: bool) -> f64 {
    match (s.kind, along_v) {
        (SurfKind::Plane, _) | (SurfKind::Cylinder | SurfKind::Cone, true) => f64::INFINITY,
        // Straight along its rulings, but they fan out from a waist as wide
        // as the lead: what a strip of it weighs is a polynomial along a
        // ruling, its area is not.
        (SurfKind::Helicoid, true) => (2.0 * s.t.abs()).max(1.0e-6),
        _ => PI / 6.0,
    }
}

/// Estimate over `[a, b]` and, where its two halves disagree with it, the
/// halves' halves.
fn refine(
    a: f64,
    b: f64,
    whole: Sums,
    scale: &Sums,
    depth: u32,
    f: &mut impl FnMut(f64) -> Sums,
) -> Sums {
    let mid = 0.5 * (a + b);
    let left = span(a, mid, f64::INFINITY, &mut *f);
    let right = span(mid, b, f64::INFINITY, &mut *f);
    let settled = (0..N).all(|i| (left[i] + right[i] - whole[i]).abs() <= scale[i]);
    let mut sum = [0.0; N];
    if depth == 0 || settled {
        add(&mut sum, &left, 1.0);
        add(&mut sum, &right, 1.0);
        return sum;
    }
    add(&mut sum, &refine(a, mid, left, scale, depth - 1, f), 1.0);
    add(&mut sum, &refine(mid, b, right, scale, depth - 1, f), 1.0);
    sum
}

/// A pole axis for a sphere that keeps both poles as far from the edges of
/// face `f` as any of a handful of directions can.
fn clear_axis(body: &Body, f: u32, s: &Surface) -> V3 {
    let mut rim: Vec<V3> = Vec::new();
    for h in body.face_hes(f) {
        let e = h >> 1;
        let [t0, t1] = body.e_t[e as usize];
        for i in 0..=12 {
            let p = body.e_point(e, t0 + (t1 - t0) * i as f64 / 12.0);
            rim.push((p - s.f.o).unit());
        }
    }
    let candidates = [
        s.f.z,
        s.f.x,
        s.f.y,
        (s.f.x + s.f.y + s.f.z).unit(),
        (s.f.x - s.f.y + s.f.z * 0.5).unit(),
        (s.f.x * 0.3 - s.f.y - s.f.z).unit(),
        (s.f.x * -1.0 + s.f.y * 0.4 + s.f.z * 0.7).unit(),
    ];
    let clearance = |axis: V3| {
        rim.iter()
            .map(|p| 1.0 - p.dot(axis).abs())
            .fold(f64::INFINITY, f64::min)
    };
    candidates
        .into_iter()
        .max_by(|a, b| clearance(*a).total_cmp(&clearance(*b)))
        .unwrap_or(s.f.z)
}

/// Sums over face `f`, measured from `origin`: the flux terms as they
/// count towards the solid (outward), and the area as it is. `size` is
/// the size of the body, for telling an answer from rounding. `None` for a
/// face whose loops wind round a torus both ways.
fn face_sums(body: &Body, f: u32, origin: V3, size: f64) -> Option<Sums> {
    let outward = if body.f_sense[f as usize] { 1.0 } else { -1.0 };
    let mut s = *body.f_surface(f);
    if s.kind == SurfKind::Sphere {
        s = Surface::sphere_axis(s.f.o, clear_axis(body, f, &s), s.r);
    }
    let torus = s.kind == SurfKind::Torus;
    // Where the inner integral starts: the tip of a cone and the upper pole
    // of a sphere, where a loop round the axis closes on nothing. A screw
    // surface never closes on itself, so anywhere will do, and next to the
    // face keeps the stretch short.
    let from = match s.kind {
        SurfKind::Sphere => 0.5 * PI,
        SurfKind::Helicoid => body
            .face_hes(f)
            .next()
            .map_or(0.0, |h| s.uv(body.verts[body.h_origin(h) as usize]).y),
        // The edge of the patch: every face on it lies above.
        SurfKind::Spline => s.spline_box().0.y,
        _ => 0.0,
    };
    let scale = {
        let l = size.max(1.0);
        let small = 1.0e-13;
        [
            small * l * l * l,
            small * l * l,
            small * l * l * l * l,
            small * l * l * l * l,
            small * l * l * l * l,
        ]
    };

    // On a torus both parameters come round again: follow each loop without
    // a jump, and see which way it winds.
    const STEPS: usize = 48;
    struct Walk {
        h: u32,
        u: Vec<f64>,
        v: Vec<f64>,
    }
    let mut walks: Vec<Walk> = Vec::new();
    let mut across = false;
    if torus {
        for l in body.face_loops(f) {
            let mut last: Option<V2> = None;
            let (mut first, mut turned) = (v2(0.0, 0.0), v2(0.0, 0.0));
            for h in body.loop_hes(l) {
                let e = h >> 1;
                let [t0, t1] = body.e_t[e as usize];
                let (a, b) = if h & 1 == 0 { (t0, t1) } else { (t1, t0) };
                let (mut us, mut vs) = (Vec::new(), Vec::new());
                for i in 0..=STEPS {
                    let raw = s.uv(body.e_point(e, a + (b - a) * i as f64 / STEPS as f64));
                    let q = match last {
                        None => {
                            first = raw;
                            raw
                        }
                        Some(prev) => v2(
                            raw.x + TAU * ((prev.x - raw.x) / TAU).round(),
                            raw.y + TAU * ((prev.y - raw.y) / TAU).round(),
                        ),
                    };
                    last = Some(q);
                    us.push(q.x);
                    vs.push(q.y);
                }
                walks.push(Walk { h, u: us, v: vs });
            }
            if let Some(end) = last {
                turned = end - first;
            }
            let (round, over) = (
                (turned.x / TAU).round() as i64,
                (turned.y / TAU).round() as i64,
            );
            if round != 0 && over != 0 {
                return None;
            }
            across |= over != 0;
        }
        // Loops over the tube and loops round the axis always cross, so a
        // face has one kind or the other.
    }

    let mut sums = [0.0; N];
    let mut walk = walks.iter();
    for h in body.face_hes(f) {
        let e = h >> 1;
        let c = *body.e_curve_ref(e);
        let [t0, t1] = body.e_t[e as usize];
        let (a, b) = if h & 1 == 0 { (t0, t1) } else { (t1, t0) };
        let followed = walk.next().filter(|w| w.h == h);
        let mut along = |t: f64| -> Sums {
            let p = c.point(t);
            let d = c.deriv(t);
            let mut q = s.uv(p);
            if let Some(w) = followed {
                // The same spot on the walk, to pick the turn.
                let place = ((t - a) / (b - a) * STEPS as f64).clamp(0.0, STEPS as f64);
                let i = (place.floor() as usize).min(STEPS - 1);
                let k = place - i as f64;
                let near = v2(
                    w.u[i] + (w.u[i + 1] - w.u[i]) * k,
                    w.v[i] + (w.v[i + 1] - w.v[i]) * k,
                );
                q = v2(
                    q.x + TAU * ((near.x - q.x) / TAU).round(),
                    q.y + TAU * ((near.y - q.y) / TAU).round(),
                );
            }
            let (_, su, sv) = at(&s, q.x, q.y);
            let mut out = [0.0; N];
            // How fast each parameter runs along the edge. The two
            // directions of a screw surface are not square to each other.
            let (e, f, g) = (su.len2(), su.dot(sv), sv.len2());
            let det = e * g - f * f;
            if det < 1.0e-24 * (e + g).max(1.0e-300) {
                return out;
            }
            let (du, dv) = (
                (g * d.dot(su) - f * d.dot(sv)) / det,
                (e * d.dot(sv) - f * d.dot(su)) / det,
            );
            if across {
                // ∮ Q dv, with Q the integral along u.
                let inner = span(0.0, q.x, stride(&s, false), |u| density(&s, u, q.y, origin));
                add(&mut out, &inner, dv);
            } else {
                // ∮ P du, with P minus the integral along v.
                let inner = if s.kind == SurfKind::Spline {
                    up_a_spline(&s, from, q.y, |v| density(&s, q.x, v, origin))
                } else {
                    span(from, q.y, stride(&s, true), |v| density(&s, q.x, v, origin))
                };
                add(&mut out, &inner, -du);
            }
            out
        };
        let pieces = match c.kind {
            CurveKind::Line => 1,
            // A spline is a different polynomial on each span: as many
            // pieces, each of which is then smooth.
            CurveKind::Spline => {
                let form = c.spline_form();
                let spans = form.knots.windows(2).filter(|w| w[1] > w[0]).count();
                let share = ((b - a).abs() / (form.hi - form.lo)).min(1.0);
                ((spans as f64 * share).ceil() as usize).max(1)
            }
            _ => (((b - a).abs() / (PI / 8.0)).ceil() as usize).max(1),
        };
        let step = (b - a) / pieces as f64;
        for i in 0..pieces {
            let (lo, hi) = (a + step * i as f64, a + step * (i + 1) as f64);
            let whole = span(lo, hi, f64::INFINITY, &mut along);
            add(
                &mut sums,
                &refine(lo, hi, whole, &scale, 14, &mut along),
                1.0,
            );
        }
    }

    // What the loops leave out: the lower pole of a sphere when the face
    // holds it, whole turns of a torus. The area tells, since it has to
    // come out between nothing and the whole surface.
    let whole = |s: &Surface| -> Sums {
        let (lo, hi) = if s.kind == SurfKind::Sphere {
            (-0.5 * PI, 0.5 * PI)
        } else {
            (0.0, TAU)
        };
        span(0.0, TAU, PI / 6.0, |u| {
            span(lo, hi, PI / 6.0, |v| density(s, u, v, origin))
        })
    };
    let mut area = outward * sums[1];
    let floor = 1.0e-9 * size.max(1.0) * size.max(1.0);
    match s.kind {
        SurfKind::Sphere if area <= floor => {
            let all = whole(&s);
            add(&mut sums, &all, outward);
            area += all[1];
        }
        SurfKind::Torus => {
            let all = whole(&s);
            let turns = if body.face_loops(f).is_empty() {
                1.0
            } else {
                (-area / all[1] - 1.0e-9).ceil()
            };
            if turns != 0.0 {
                add(&mut sums, &all, outward * turns);
                area += turns * all[1];
            }
        }
        _ => {}
    }
    sums[1] = area;
    Some(sums)
}

/// Volume, surface area and centre of mass of a solid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mass {
    pub volume: f64,
    pub area: f64,
    pub centre: V3,
    /// Whether every face was integrated exactly. A face whose loops wind
    /// round a torus both ways is left out, and this is false.
    pub exact: bool,
}

/// Volume, area and centre of mass, to rounding: every face is integrated
/// along its exact edges.
pub fn mass(body: &Body) -> Mass {
    let (origin, size) = anchor(body);
    let mut total = [0.0; N];
    let mut exact = true;
    for f in 0..body.face_count() as u32 {
        match face_sums(body, f, origin, size) {
            Some(sums) => add(&mut total, &sums, 1.0),
            None => exact = false,
        }
    }
    let volume = total[0];
    let centre = if volume.abs() > 1.0e-12 {
        origin + v3(total[2], total[3], total[4]) / volume
    } else {
        origin
    };
    Mass {
        volume,
        area: total[1],
        centre,
        exact,
    }
}

/// Volume of a solid, to rounding.
pub fn volume(body: &Body) -> f64 {
    mass(body).volume
}

/// Volume inside some of the faces of a body: a closed piece of its skin.
/// Negative for the skin of a hollow.
pub fn volume_within(body: &Body, faces: &[u32]) -> f64 {
    let (origin, size) = anchor(body);
    faces
        .iter()
        .filter_map(|&f| face_sums(body, f, origin, size))
        .map(|sums| sums[0])
        .sum()
}

/// Area of one face, to rounding.
pub fn face_area(body: &Body, f: u32) -> Option<f64> {
    let (origin, size) = anchor(body);
    face_sums(body, f, origin, size).map(|sums| sums[1])
}

/// A point to measure from, in the middle of the body, and the body's
/// size. A body with no edge at all (a ball, a ring) has only its surfaces
/// to go by.
fn anchor(body: &Body) -> (V3, f64) {
    let bounds = body.bbox();
    let mut size = bounds.diagonal();
    let mut origin = (bounds.lo + bounds.hi) * 0.5;
    if bounds.is_empty() || !origin.is_finite() {
        origin = body.surfs.first().map_or(V3::ZERO, |s| s.f.o);
        size = 0.0;
    }
    for s in &body.surfs {
        if matches!(s.kind, SurfKind::Sphere | SurfKind::Torus) {
            size = size.max(2.0 * (s.r.abs() + s.s.abs()));
        }
    }
    (origin, size)
}

/// The box a body just fits in. Edges reach furthest where they turn back
/// along an axis, and a ball or a ring where its face faces along one;
/// both are found exactly, so a rod of radius 5 is 10 across, not the
/// width of its facets.
pub fn bounds(body: &Body) -> Aabb {
    use crate::boolean::Side;
    use crate::trim::{classify, PtClass};
    let mut b = Aabb::EMPTY;
    for &p in &body.verts {
        b.add(p);
    }
    let axes = [V3::X, V3::Y, V3::Z];
    for e in 0..body.edge_count() as u32 {
        let c = body.e_curve_ref(e);
        let [t0, t1] = body.e_t[e as usize];
        match c.kind {
            CurveKind::Line => {}
            CurveKind::Circle | CurveKind::Ellipse => {
                // Along `d` the point is furthest where the tangent is
                // square to it.
                let (major, minor) = if c.kind == CurveKind::Circle {
                    (c.r, c.r)
                } else {
                    (c.r, c.s)
                };
                for d in axes {
                    let turn = (minor * c.f.y.dot(d)).atan2(major * c.f.x.dot(d));
                    for t in [turn, turn + PI] {
                        let t = wrap_from(t, t0);
                        if t <= t1 {
                            b.add(c.point(t));
                        }
                    }
                }
            }
            _ => {
                // Walk it, then close in on each place it turns back. A
                // helix turns back twice in every turn.
                let steps = if c.kind == CurveKind::Helix {
                    64 * ((t1 - t0) / TAU).ceil().max(1.0) as usize
                } else {
                    64
                };
                let at = |i: usize| t0 + (t1 - t0) * i as f64 / steps as f64;
                for d in axes {
                    for sign in [1.0, -1.0] {
                        let height = |t: f64| sign * c.point(t).dot(d);
                        let values: Vec<f64> = (0..=steps).map(|i| height(at(i))).collect();
                        for i in 1..steps {
                            if values[i] < values[i - 1] || values[i] < values[i + 1] {
                                continue;
                            }
                            let (mut lo, mut hi) = (at(i - 1), at(i + 1));
                            for _ in 0..60 {
                                let (x1, x2) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
                                if height(x1) < height(x2) {
                                    lo = x1;
                                } else {
                                    hi = x2;
                                }
                            }
                            b.add(c.point(0.5 * (lo + hi)));
                        }
                    }
                }
            }
        }
    }
    // Faces that face along an axis somewhere away from their edges.
    let round = (0..body.face_count() as u32)
        .any(|f| matches!(body.f_surface(f).kind, SurfKind::Sphere | SurfKind::Torus));
    if !round {
        return b;
    }
    let side = Side::new(body);
    for f in 0..body.face_count() as u32 {
        let s = body.f_surface(f);
        let mut tops: Vec<V3> = Vec::new();
        for d in axes {
            for d in [d, -d] {
                match s.kind {
                    SurfKind::Sphere => tops.push(s.f.o + d * s.r),
                    SurfKind::Torus => {
                        let up = d.dot(s.f.z);
                        let flat = d - s.f.z * up;
                        let m = flat.len();
                        if m > 1.0e-9 {
                            tops.push(s.f.o + flat * (s.r / m) + d * s.s);
                        } else {
                            // Along the axis: the whole of a circle.
                            for i in 0..16 {
                                let u = TAU * i as f64 / 16.0;
                                tops.push(s.point(u, 0.5 * PI * up.signum()));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        for p in tops {
            if classify(body, side.trim(f), f, p) != PtClass::Out {
                b.add(p);
            }
        }
    }
    b
}
