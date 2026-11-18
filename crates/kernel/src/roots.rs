//! Intersections of a bounded curve with a surface or with another curve on
//! a shared surface, as one-dimensional root finding on a signed distance.
//!
//! The common pairs have closed forms; everything else goes through one
//! bracketing solver that also recognises tangential contact.

use crate::geom::{Curve, CurveKind, SurfKind, Surface};
use crate::math::*;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Root {
    pub t: f64,
    /// The curve touches without crossing.
    pub tangent: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Hits {
    /// The whole span lies on the other entity.
    Coincident,
    Roots(Vec<Root>),
}

/// Where along a span to look whether it lies in something. Unevenly: a
/// helix passes a plane through its axis every half turn, and looked at
/// only at its quarters, fourteen turns of it seemed to lie in the plane.
const ALONG: [f64; 7] = [0.0, 0.1173, 0.2511, 0.5, 0.6337, 0.8819, 1.0];

/// Roots of a smooth function on `[a, b]`: sign changes, plus local minima of
/// `|g|` below `tol` (tangential contact).
pub fn solve(g: &dyn Fn(f64) -> f64, a: f64, b: f64, samples: usize, tol: f64) -> Vec<Root> {
    let n = samples.max(2);
    let ts: Vec<f64> = (0..=n).map(|i| a + (b - a) * i as f64 / n as f64).collect();
    let gs: Vec<f64> = ts.iter().map(|&t| g(t)).collect();
    let mut roots: Vec<Root> = Vec::new();
    let refine = |mut lo: f64, mut hi: f64, mut glo: f64, mut ghi: f64| -> f64 {
        // Illinois regula falsi: superlinear, never leaves the bracket.
        let mut side = 0;
        for _ in 0..80 {
            if (hi - lo).abs() <= 1.0e-15 * (1.0 + lo.abs().max(hi.abs())) {
                break;
            }
            let mut t = (lo * ghi - hi * glo) / (ghi - glo);
            if !t.is_finite() || t <= lo.min(hi) || t >= lo.max(hi) {
                t = 0.5 * (lo + hi);
            }
            let gt = g(t);
            if gt == 0.0 {
                return t;
            }
            if (gt > 0.0) == (ghi > 0.0) {
                hi = t;
                ghi = gt;
                if side == -1 {
                    glo *= 0.5;
                }
                side = -1;
            } else {
                lo = t;
                glo = gt;
                if side == 1 {
                    ghi *= 0.5;
                }
                side = 1;
            }
        }
        if glo.abs() < ghi.abs() {
            lo
        } else {
            hi
        }
    };
    let minimise = |lo: f64, hi: f64| -> (f64, f64) {
        // Golden-section search for the minimum of |g|.
        let phi = 0.618_033_988_749_894_9;
        let (mut lo, mut hi) = (lo, hi);
        let mut x1 = hi - phi * (hi - lo);
        let mut x2 = lo + phi * (hi - lo);
        let (mut f1, mut f2) = (g(x1).abs(), g(x2).abs());
        for _ in 0..90 {
            if (hi - lo).abs() <= 1.0e-13 * (1.0 + lo.abs().max(hi.abs())) {
                break;
            }
            if f1 < f2 {
                hi = x2;
                x2 = x1;
                f2 = f1;
                x1 = hi - phi * (hi - lo);
                f1 = g(x1).abs();
            } else {
                lo = x1;
                x1 = x2;
                f1 = f2;
                x2 = lo + phi * (hi - lo);
                f2 = g(x2).abs();
            }
        }
        let t = 0.5 * (lo + hi);
        (t, g(t))
    };
    for i in 0..n {
        let (g0, g1) = (gs[i], gs[i + 1]);
        if g0 == 0.0 {
            roots.push(Root {
                t: ts[i],
                tangent: false,
            });
            continue;
        }
        if (g0 > 0.0) != (g1 > 0.0) && g1 != 0.0 {
            roots.push(Root {
                t: refine(ts[i], ts[i + 1], g0, g1),
                tangent: false,
            });
        }
    }
    if gs[n] == 0.0 {
        roots.push(Root {
            t: ts[n],
            tangent: false,
        });
    }
    // Touching or grazing: |g| has an interior local minimum with no sign
    // change at sample resolution.
    for i in 0..=n {
        let here = gs[i].abs();
        let left = if i > 0 {
            gs[i - 1].abs()
        } else {
            f64::INFINITY
        };
        let right = if i < n {
            gs[i + 1].abs()
        } else {
            f64::INFINITY
        };
        if here > left || here > right || gs[i] == 0.0 {
            continue;
        }
        let same_sign = |k: usize| (gs[k] > 0.0) == (gs[i] > 0.0);
        if (i > 0 && !same_sign(i - 1)) || (i < n && !same_sign(i + 1)) {
            continue;
        }
        let lo = ts[i.saturating_sub(1)];
        let hi = ts[(i + 1).min(n)];
        let (t, gt) = minimise(lo, hi);
        if (gt > 0.0) != (gs[i] > 0.0) && gt != 0.0 {
            // The dip crosses zero: two transversal roots.
            roots.push(Root {
                t: refine(lo, t, g(lo), gt),
                tangent: false,
            });
            roots.push(Root {
                t: refine(t, hi, gt, g(hi)),
                tangent: false,
            });
        } else if gt.abs() < tol {
            roots.push(Root { t, tangent: true });
        }
    }
    roots.sort_by(|x, y| x.t.total_cmp(&y.t));
    roots
}

/// Real roots of a polynomial inside `[lo, hi]` at which it changes sign, in
/// increasing order. Coefficients run from the constant term up. Roots are
/// isolated between the roots of the derivative, so none can be missed.
fn poly_roots(c: &[f64], lo: f64, hi: f64) -> Vec<f64> {
    let scale = c.iter().fold(0.0_f64, |m, k| m.max(k.abs()));
    let mut n = c.len();
    while n > 0 && c[n - 1].abs() <= 1.0e-14 * scale {
        n -= 1;
    }
    let c = &c[..n];
    if n < 2 || lo >= hi {
        return Vec::new();
    }
    let eval = |t: f64| c.iter().rev().fold(0.0, |acc, &k| acc * t + k);
    if n == 2 {
        let t = -c[0] / c[1];
        return if t >= lo && t <= hi {
            vec![t]
        } else {
            Vec::new()
        };
    }
    let slope: Vec<f64> = (1..n).map(|i| c[i] * i as f64).collect();
    let mut knots = vec![lo];
    knots.extend(poly_roots(&slope, lo, hi));
    knots.push(hi);
    let mut roots: Vec<f64> = Vec::new();
    for w in knots.windows(2) {
        let (mut a, mut b) = (w[0], w[1]);
        let (mut fa, fb) = (eval(a), eval(b));
        if fa == 0.0 {
            if roots.last() != Some(&a) {
                roots.push(a);
            }
            continue;
        }
        if fb == 0.0 || (fa > 0.0) == (fb > 0.0) {
            continue;
        }
        // The polynomial is monotone here: plain bisection is exact enough
        // and cannot fail.
        for _ in 0..200 {
            let m = 0.5 * (a + b);
            if m <= a || m >= b {
                break;
            }
            let fm = eval(m);
            if fm == 0.0 {
                a = m;
                b = m;
                break;
            }
            if (fm > 0.0) == (fa > 0.0) {
                a = m;
                fa = fm;
            } else {
                b = m;
            }
        }
        roots.push(0.5 * (a + b));
    }
    if eval(hi) == 0.0 && roots.last() != Some(&hi) {
        roots.push(hi);
    }
    roots
}

/// Where a line meets a torus: the roots of a quartic.
fn line_torus(c: &Curve, s: &Surface) -> Vec<(f64, bool)> {
    let (major, minor) = (s.r, s.s);
    let reach = major + minor;
    let d = v3(c.f.x.dot(s.f.x), c.f.x.dot(s.f.y), c.f.x.dot(s.f.z));
    let o = s.f.local(c.f.o);
    // Measure the line from its point nearest the centre, in units of the
    // torus size: the quartic is then well scaled wherever the torus is.
    let t_near = -o.dot(d);
    let p = (o + d * t_near) * (1.0 / reach);
    let free = 1.0 - p.len2();
    if free < -1.0e-9 {
        return Vec::new();
    }
    let (big, small) = (major / reach, minor / reach);
    let b = p.len2() + big * big - small * small;
    let k = 4.0 * big * big;
    let quartic = [
        b * b - k * (p.x * p.x + p.y * p.y),
        -2.0 * k * (p.x * d.x + p.y * d.y),
        2.0 * b - k * (d.x * d.x + d.y * d.y),
        0.0,
        1.0,
    ];
    let half = free.max(0.0).sqrt() + 1.0e-6;
    let point = |tau: f64| c.point(t_near + tau * reach);
    let slope: Vec<f64> = (1..5).map(|i| quartic[i] * i as f64).collect();
    let mut knots = vec![-half];
    knots.extend(poly_roots(&slope, -half, half));
    knots.push(half);
    let crossings = poly_roots(&quartic, -half, half);
    // Where the line only grazes the tube, the crossings on either side of
    // the closest approach merge into one touching point.
    let mut out: Vec<(f64, bool)> = Vec::new();
    let mut grazed: Vec<(f64, f64)> = Vec::new();
    for i in 1..knots.len() - 1 {
        if s.sdist(point(knots[i])).abs() < TOL {
            grazed.push((knots[i - 1], knots[i + 1]));
            out.push((t_near + knots[i] * reach, true));
        }
    }
    for tau in crossings {
        if grazed.iter().any(|&(a, b)| tau > a && tau < b) {
            continue;
        }
        // A spindle torus has an inner sheet that is not part of the surface.
        if s.sdist(point(tau)).abs() < 1.0e-6 * (1.0 + reach) {
            out.push((t_near + tau * reach, false));
        }
    }
    out
}

/// Where a line meets a cone: a quadratic on the double cone.
fn line_cone(c: &Curve, s: &Surface) -> Vec<(f64, bool)> {
    let d = v3(c.f.x.dot(s.f.x), c.f.x.dot(s.f.y), c.f.x.dot(s.f.z));
    let o = s.f.local(c.f.o);
    let k = s.s.tan().powi(2);
    let qa = d.x * d.x + d.y * d.y - k * d.z * d.z;
    let qb = 2.0 * (o.x * d.x + o.y * d.y - k * o.z * d.z);
    let qc = o.x * o.x + o.y * o.y - k * o.z * o.z;
    // The quadratic is the cone and its mirror image behind the tip; only
    // the cone counts.
    let ahead = |t: f64| o.z + t * d.z >= -1.0e-9 * (1.0 + o.z.abs());
    if qa.abs() < 1.0e-12 {
        // Parallel to a ruling: one crossing at most.
        if qb.abs() < 1.0e-12 || !ahead(-qc / qb) {
            return Vec::new();
        }
        return vec![(-qc / qb, false)];
    }
    let t_mid = -qb / (2.0 * qa);
    if s.sdist(c.point(t_mid)).abs() < TOL {
        return vec![(t_mid, true)];
    }
    let disc = qb * qb - 4.0 * qa * qc;
    if disc <= 0.0 {
        return Vec::new();
    }
    let half = disc.sqrt() / (2.0 * qa.abs());
    [t_mid - half, t_mid + half]
        .into_iter()
        .filter(|&t| ahead(t))
        .map(|t| (t, false))
        .collect()
}

/// Keep roots of a periodic closed form that fall inside `[t0, t1]`.
fn in_span(candidates: &[(f64, bool)], t0: f64, t1: f64, periodic: bool) -> Vec<Root> {
    let mut out = Vec::new();
    let slack = 1.0e-12;
    for &(t, tangent) in candidates {
        let t = if periodic {
            let w = wrap_from(t, t0);
            // A root at the very start may wrap to just below t0 + 2π.
            if w > t1 + slack && w - TAU >= t0 - slack {
                w - TAU
            } else {
                w
            }
        } else {
            t
        };
        if t >= t0 - slack && t <= t1 + slack {
            out.push(Root {
                t: t.clamp(t0, t1),
                tangent,
            });
        }
    }
    out.sort_by(|x, y| x.t.total_cmp(&y.t));
    out
}

/// Where a curve passes through the turns of a helicoid.
///
/// The height of a point above the surface is zero on one turn and a whole
/// lead on the next, so the signed distance jumps halfway between them and
/// is no use for bracketing. The height itself, with the angle about the
/// axis carried along the curve without a jump, is continuous: the curve
/// crosses a turn wherever it passes a whole number of leads.
fn helicoid_hits(c: &Curve, t0: f64, t1: f64, s: &Surface) -> Vec<Root> {
    let (out, up) = s.ruling();
    let lead = TAU * s.t;
    // Height above the first turn before the angle is taken off, the angle,
    // and the distance from the axis.
    let at = |t: f64| {
        let l = s.f.local(c.point(t));
        let rho = l.x.hypot(l.y);
        (l.z - up * rho / out, l.y.atan2(l.x), rho)
    };
    let nearest = |a: f64| a - TAU * (a / TAU).round();
    let start = match c.kind {
        CurveKind::Line => 32,
        _ => (((t1 - t0).abs() / TAU * 64.0).ceil() as usize).max(16),
    };
    let mut ts: Vec<f64> = (0..=start)
        .map(|i| t0 + (t1 - t0) * i as f64 / start as f64)
        .collect();
    // Finer wherever the angle or the height moves far between samples.
    for _ in 0..14 {
        let found: Vec<(f64, f64, f64)> = ts.iter().map(|&t| at(t)).collect();
        let mut finer = Vec::with_capacity(ts.len() * 2);
        let mut split = false;
        for i in 0..ts.len() {
            finer.push(ts[i]);
            if i + 1 == ts.len() {
                break;
            }
            let turn = nearest(found[i + 1].1 - found[i].1);
            let climb = found[i + 1].0 - found[i].0 - s.t * turn;
            if (turn.abs() > 0.5 || climb.abs() > 0.25 * lead.abs()) && ts.len() < 200_000 {
                finer.push(0.5 * (ts[i] + ts[i + 1]));
                split = true;
            }
        }
        ts = finer;
        if !split {
            break;
        }
    }
    let found: Vec<(f64, f64, f64)> = ts.iter().map(|&t| at(t)).collect();
    // The height with the angle carried on from sample to sample.
    let mut angle = found[0].1;
    let mut heights = Vec::with_capacity(ts.len());
    let mut angles = Vec::with_capacity(ts.len());
    for (i, sample) in found.iter().enumerate() {
        if i > 0 {
            angle += nearest(sample.1 - found[i - 1].1);
        }
        angles.push(angle);
        heights.push(sample.0 - s.t * angle);
    }
    let mut roots = Vec::new();
    for i in 0..ts.len() - 1 {
        // On the axis the angle means nothing.
        if found[i].2 < 1.0e-9 || found[i + 1].2 < 1.0e-9 {
            continue;
        }
        let (ga, gb) = (heights[i], heights[i + 1]);
        let (lo, hi) = (ga.min(gb), ga.max(gb));
        // Every whole number of leads in between, whichever way the screw
        // is handed.
        let turn = lead.abs();
        let mut k = (lo / turn).ceil();
        while k * turn <= hi {
            let level = k * turn;
            k += 1.0;
            // Each turn once: at its sample, or strictly inside the step.
            if gb == level && i + 2 < ts.len() {
                continue;
            }
            let height = |t: f64| {
                let here = at(t);
                here.0 - s.t * (angles[i] + nearest(here.1 - found[i].1)) - level
            };
            let (mut a, mut b) = (ts[i], ts[i + 1]);
            let (mut fa, mut fb) = (ga - level, gb - level);
            if fa == 0.0 {
                roots.push(Root {
                    t: a,
                    tangent: false,
                });
                continue;
            }
            if fb == 0.0 {
                roots.push(Root {
                    t: b,
                    tangent: false,
                });
                continue;
            }
            for _ in 0..100 {
                if (b - a).abs() <= 1.0e-15 * (1.0 + a.abs().max(b.abs())) {
                    break;
                }
                let mut m = (a * fb - b * fa) / (fb - fa);
                if !m.is_finite() || (m - a) * (m - b) >= 0.0 {
                    m = 0.5 * (a + b);
                }
                let fm = height(m);
                if fm == 0.0 {
                    a = m;
                    b = m;
                    break;
                }
                if (fm > 0.0) == (fa > 0.0) {
                    // Halve the stale end so the bracket keeps closing.
                    a = m;
                    fa = fm;
                    fb *= 0.5;
                } else {
                    b = m;
                    fb = fm;
                    fa *= 0.5;
                }
            }
            roots.push(Root {
                t: 0.5 * (a + b),
                tangent: false,
            });
        }
    }
    roots.sort_by(|x, y| x.t.total_cmp(&y.t));
    roots
}

/// Where the span `[t0, t1]` of `c` meets surface `s`.
pub fn curve_surface(c: &Curve, t0: f64, t1: f64, s: &Surface) -> Hits {
    // Lying in the surface is the common degenerate case (shared faces).
    let on = ALONG
        .iter()
        .all(|k| s.sdist(c.point(t0 + (t1 - t0) * k)).abs() < TOL);
    if on {
        return Hits::Coincident;
    }
    match (c.kind, s.kind) {
        (CurveKind::Line, SurfKind::Plane) => {
            let denom = c.f.x.dot(s.f.z);
            if denom.abs() < ANG_TOL {
                return Hits::Roots(Vec::new());
            }
            let t = -s.sdist(c.f.o) / denom;
            Hits::Roots(in_span(&[(t, false)], t0, t1, false))
        }
        (CurveKind::Line, SurfKind::Cylinder) | (CurveKind::Line, SurfKind::Sphere) => {
            // Reduce to a circle of radius r in the plane (or space) where
            // the line has direction `d` and offset `w`.
            let (d, w) = if s.kind == SurfKind::Cylinder {
                let z = s.f.z;
                let oc = c.f.o - s.f.o;
                (c.f.x - z * c.f.x.dot(z), oc - z * oc.dot(z))
            } else {
                (c.f.x, c.f.o - s.f.o)
            };
            let dd = d.len2();
            if dd < ANG_TOL * ANG_TOL {
                return Hits::Roots(Vec::new());
            }
            let t_mid = -w.dot(d) / dd;
            let closest = (w + d * t_mid).len();
            if (closest - s.r).abs() < TOL {
                return Hits::Roots(in_span(&[(t_mid, true)], t0, t1, false));
            }
            if closest > s.r {
                return Hits::Roots(Vec::new());
            }
            let half = ((s.r * s.r - closest * closest) / dd).sqrt();
            Hits::Roots(in_span(
                &[(t_mid - half, false), (t_mid + half, false)],
                t0,
                t1,
                false,
            ))
        }
        (CurveKind::Circle, SurfKind::Plane) => {
            let n = s.f.z;
            let (a, b) = (c.r * c.f.x.dot(n), c.r * c.f.y.dot(n));
            let c0 = s.sdist(c.f.o);
            let amp = a.hypot(b);
            if amp < TOL {
                return Hits::Roots(Vec::new());
            }
            let phase = b.atan2(a);
            if (c0.abs() - amp).abs() < TOL {
                let t = if c0 > 0.0 { phase + PI } else { phase };
                return Hits::Roots(in_span(&[(t, true)], t0, t1, true));
            }
            if c0.abs() > amp {
                return Hits::Roots(Vec::new());
            }
            let d = (-c0 / amp).clamp(-1.0, 1.0).acos();
            Hits::Roots(in_span(
                &[(phase - d, false), (phase + d, false)],
                t0,
                t1,
                true,
            ))
        }
        (CurveKind::Line, SurfKind::Torus) => {
            Hits::Roots(in_span(&line_torus(c, s), t0, t1, false))
        }
        (CurveKind::Line, SurfKind::Cone) => Hits::Roots(in_span(&line_cone(c, s), t0, t1, false)),
        (_, SurfKind::Helicoid) => Hits::Roots(helicoid_hits(c, t0, t1, s)),
        _ => {
            let samples = match c.kind {
                CurveKind::Line => 24,
                _ => (((t1 - t0) / TAU * 64.0).ceil() as usize).max(8),
            };
            // Sample finely enough to notice the curve passing through the
            // smallest feature of the surface.
            let feature = match s.kind {
                SurfKind::Cylinder | SurfKind::Sphere => s.r,
                SurfKind::Torus => s.s,
                SurfKind::Coil => s.s.abs(),
                SurfKind::Spline => s.spline_form().radius,
                SurfKind::Pipe => s.r,
                SurfKind::Plane | SurfKind::Cone | SurfKind::Helicoid => f64::INFINITY,
            };
            let by_size = if feature.is_finite() && feature > TOL {
                ((c.length(t0, t1) / (0.5 * feature)).ceil() as usize).min(4096)
            } else {
                0
            };
            Hits::Roots(solve(
                &|t| s.sdist(c.point(t)),
                t0,
                t1,
                samples.max(by_size),
                TOL,
            ))
        }
    }
}

/// Where the span `[t0, t1]` of `c` meets curve `other`; both lie on `s`.
/// The distance is signed by the side of `other` within the surface, so a
/// crossing is a sign change.
pub fn curve_curve(c: &Curve, t0: f64, t1: f64, other: &Curve, s: &Surface) -> Hits {
    if c.kind == CurveKind::Line && other.kind == CurveKind::Line {
        // Two lines in one surface: parallel, the same, or meeting once.
        let (d1, d2) = (c.f.x, other.f.x);
        let n = d1.cross(d2);
        let w = other.f.o - c.f.o;
        if n.len2() < ANG_TOL * ANG_TOL {
            let off = (w - d2 * w.dot(d2)).len();
            return if off < TOL {
                Hits::Coincident
            } else {
                Hits::Roots(Vec::new())
            };
        }
        let t = w.cross(d2).dot(n) / n.len2();
        if other.dist(c.point(t)) >= 10.0 * TOL {
            return Hits::Roots(Vec::new());
        }
        return Hits::Roots(in_span(&[(t, false)], t0, t1, false));
    }
    if c.kind == CurveKind::Line && other.kind == CurveKind::Circle {
        // A line in the circle's plane meets it like a chord.
        let w = c.f.o - other.f.o;
        let z = other.f.z;
        if c.f.x.dot(z).abs() < ANG_TOL && w.dot(z).abs() < TOL {
            let t_mid = -w.dot(c.f.x);
            let closest = (w + c.f.x * t_mid).len();
            if (closest - other.r).abs() < TOL {
                return Hits::Roots(in_span(&[(t_mid, true)], t0, t1, false));
            }
            if closest > other.r {
                return Hits::Roots(Vec::new());
            }
            let half = (other.r * other.r - closest * closest).sqrt();
            return Hits::Roots(in_span(
                &[(t_mid - half, false), (t_mid + half, false)],
                t0,
                t1,
                false,
            ));
        }
    }
    let side = |t: f64| {
        let p = c.point(t);
        let u = other.param(p);
        let foot = other.point(u);
        let along = other.deriv(u).unit();
        (p - foot).dot(s.normal(foot).cross(along))
    };
    let dist = |t: f64| other.dist(c.point(t));
    if ALONG.iter().all(|k| dist(t0 + (t1 - t0) * k) < TOL) {
        return Hits::Coincident;
    }
    if c.kind == CurveKind::Circle {
        // A circle against a line or another circle in its own plane.
        let z = c.f.z;
        let w = other.f.o - c.f.o;
        let flat = w.dot(z).abs() < TOL;
        let mut meet: Option<Vec<(V3, bool)>> = None;
        if other.kind == CurveKind::Line && flat && other.f.x.dot(z).abs() < ANG_TOL {
            let d = other.f.x;
            let s_mid = -w.dot(d);
            let foot = w + d * s_mid;
            let closest = foot.len();
            meet = Some(if (closest - c.r).abs() < TOL {
                vec![(c.f.o + foot, true)]
            } else if closest > c.r {
                Vec::new()
            } else {
                let half = (c.r * c.r - closest * closest).sqrt();
                vec![
                    (c.f.o + foot - d * half, false),
                    (c.f.o + foot + d * half, false),
                ]
            });
        } else if other.kind == CurveKind::Circle && flat && other.f.z.cross(z).len() < ANG_TOL {
            let gap = w.len();
            let (r1, r2) = (c.r, other.r);
            meet = Some(
                if gap > r1 + r2 + TOL || gap < (r1 - r2).abs() - TOL || gap < TOL {
                    Vec::new()
                } else {
                    let e = w * (1.0 / gap);
                    if (gap - (r1 + r2)).abs() < TOL || (gap - (r1 - r2)).abs() < TOL {
                        vec![(c.f.o + e * r1, true)]
                    } else if (gap - (r2 - r1)).abs() < TOL {
                        vec![(c.f.o - e * r1, true)]
                    } else {
                        let along = (gap * gap + r1 * r1 - r2 * r2) / (2.0 * gap);
                        let off = (r1 * r1 - along * along).max(0.0).sqrt();
                        let side = z.cross(e);
                        vec![
                            (c.f.o + e * along - side * off, false),
                            (c.f.o + e * along + side * off, false),
                        ]
                    }
                },
            );
        }
        if let Some(points) = meet {
            let params: Vec<(f64, bool)> = points
                .iter()
                .map(|&(p, touch)| (c.param(p), touch))
                .collect();
            return Hits::Roots(in_span(&params, t0, t1, true));
        }
    }
    let samples = match c.kind {
        CurveKind::Line => 24,
        _ => (((t1 - t0) / TAU * 64.0).ceil() as usize).max(8),
    };
    let mut roots = solve(&side, t0, t1, samples, TOL);
    // The side function also vanishes where the foot point jumps to the far
    // side of a closed curve; keep only true meetings.
    roots.retain(|r| dist(r.t) < 10.0 * TOL);
    Hits::Roots(roots)
}
