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
        let left = if i > 0 { gs[i - 1].abs() } else { f64::INFINITY };
        let right = if i < n { gs[i + 1].abs() } else { f64::INFINITY };
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

/// Where the span `[t0, t1]` of `c` meets surface `s`.
pub fn curve_surface(c: &Curve, t0: f64, t1: f64, s: &Surface) -> Hits {
    // Lying in the surface is the common degenerate case (shared faces).
    let on = [0.0, 0.25, 0.5, 0.75, 1.0]
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
        _ => {
            let samples = match c.kind {
                CurveKind::Line => 24,
                _ => (((t1 - t0) / TAU * 64.0).ceil() as usize).max(8),
            };
            Hits::Roots(solve(
                &|t| s.sdist(c.point(t)),
                t0,
                t1,
                samples,
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
    if [0.0, 0.25, 0.5, 0.75, 1.0]
        .iter()
        .all(|k| dist(t0 + (t1 - t0) * k) < TOL)
    {
        return Hits::Coincident;
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
