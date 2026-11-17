//! Surface–surface intersection in closed form.
//!
//! Every supported pair yields lines, circles or ellipses. Tangential
//! contact is deliberately not reported: it never separates inside from
//! outside, and shared edges are found through coincident faces instead.

use crate::geom::{Curve, SurfKind, Surface};
use crate::math::*;

#[derive(Clone, Debug, PartialEq)]
pub enum Ssi {
    /// No transversal intersection.
    None,
    /// The same geometric surface; `true` when the natural normals agree.
    Same(bool),
    /// Transversal intersection curves (unbounded lines, full conics).
    Curves(Vec<Curve>),
    /// A general intersection this kernel does not represent yet.
    Unsupported(&'static str),
}

pub fn intersect(a: &Surface, b: &Surface) -> Ssi {
    if let Some(same) = a.same(b) {
        return Ssi::Same(same);
    }
    use SurfKind::*;
    match (a.kind, b.kind) {
        (Plane, Plane) => plane_plane(a, b),
        (Plane, _) => plane_other(a, b),
        (_, Plane) => plane_other(b, a),
        (Cylinder, Cylinder) => cylinder_cylinder(a, b),
        _ => match coaxial(a, b) {
            Some(result) => result,
            None => Ssi::Unsupported(match (a.kind, b.kind) {
                (Cylinder, Sphere) => return trace(a, b),
                (Sphere, Cylinder) => return trace(b, a),
                (Cylinder, Cone) | (Cone, Cylinder) => "a cylinder and a cone on different axes",
                (Cone, Cone) => "two cones on different axes",
                (Torus, _) | (_, Torus) => "a torus and a surface on a different axis",
                _ => "two curved surfaces on different axes",
            }),
        },
    }
}

fn plane_plane(a: &Surface, b: &Surface) -> Ssi {
    let (n1, n2) = (a.f.z, b.f.z);
    let d = n1.cross(n2);
    let dd = d.len2();
    if dd < ANG_TOL * ANG_TOL {
        return Ssi::None;
    }
    let (c1, c2) = (n1.dot(a.f.o), n2.dot(b.f.o));
    let p = (n2.cross(d) * c1 + d.cross(n1) * c2) / dd;
    Ssi::Curves(vec![Curve::line(p, d)])
}

fn plane_other(p: &Surface, s: &Surface) -> Ssi {
    let n = p.f.z;
    let axis = s.f.z;
    let perpendicular = n.cross(axis).len() < ANG_TOL * 10.0;
    match s.kind {
        SurfKind::Cylinder => {
            let c = n.dot(axis);
            if c.abs() < ANG_TOL * 10.0 {
                // Axis parallel to the plane: zero or two rulings.
                let delta = p.sdist(s.f.o);
                if delta.abs() >= s.r - TOL {
                    return Ssi::None;
                }
                let foot = s.f.o - n * delta;
                let side = axis.cross(n).unit();
                let half = (s.r * s.r - delta * delta).sqrt();
                return Ssi::Curves(vec![
                    Curve::line(foot + side * half, axis),
                    Curve::line(foot - side * half, axis),
                ]);
            }
            let centre = s.f.o + axis * ((p.f.o - s.f.o).dot(n) / c);
            if perpendicular {
                return Ssi::Curves(vec![Curve::circle(centre, axis, s.f.x, s.r)]);
            }
            let minor = n.cross(axis).unit();
            let major = n.cross(minor);
            Ssi::Curves(vec![Curve::ellipse(centre, n, major, s.r / c.abs(), s.r)])
        }
        SurfKind::Sphere => {
            let delta = p.sdist(s.f.o);
            if delta.abs() >= s.r - TOL {
                return Ssi::None;
            }
            let radius = (s.r * s.r - delta * delta).sqrt();
            Ssi::Curves(vec![Curve::circle(
                s.f.o - n * delta,
                n,
                n.any_perp(),
                radius,
            )])
        }
        SurfKind::Cone => plane_cone(p, s, perpendicular),
        SurfKind::Torus => {
            if perpendicular {
                let h = (p.f.o - s.f.o).dot(axis);
                if h.abs() >= s.s - TOL {
                    return Ssi::None;
                }
                let w = (s.s * s.s - h * h).sqrt();
                let centre = s.f.o + axis * h;
                let mut curves = vec![Curve::circle(centre, axis, s.f.x, s.r + w)];
                if s.r - w > TOL {
                    curves.push(Curve::circle(centre, axis, s.f.x, s.r - w));
                }
                return Ssi::Curves(curves);
            }
            if n.dot(axis).abs() < ANG_TOL * 10.0 && p.sdist(s.f.o).abs() < TOL {
                // The plane contains the axis: two tube sections.
                let w = axis.cross(n).unit();
                return Ssi::Curves(vec![
                    Curve::circle(s.f.o + w * s.r, n, w, s.s),
                    Curve::circle(s.f.o - w * s.r, n, -w, s.s),
                ]);
            }
            Ssi::Unsupported("a plane cutting a torus obliquely")
        }
        SurfKind::Plane => unreachable!(),
    }
}

fn plane_cone(p: &Surface, s: &Surface, perpendicular: bool) -> Ssi {
    let n = p.f.z;
    let (apex, axis, alpha) = (s.f.o, s.f.z, s.s);
    if perpendicular {
        let h = (p.f.o - apex).dot(axis);
        if h <= TOL {
            return Ssi::None;
        }
        return Ssi::Curves(vec![Curve::circle(
            apex + axis * h,
            axis,
            s.f.x,
            h * alpha.tan(),
        )]);
    }
    let na = n.dot(axis);
    if p.sdist(apex).abs() < TOL {
        // Through the apex: zero or two rulings.
        let (nx, ny) = (n.dot(s.f.x), n.dot(s.f.y));
        let amp = nx.hypot(ny) * alpha.sin();
        let rhs = -na * alpha.cos();
        if amp < 1.0e-14 || (rhs / amp).abs() >= 1.0 - 1.0e-12 {
            return Ssi::None;
        }
        let phase = ny.atan2(nx);
        let d = (rhs / amp).acos();
        let ruling =
            |u: f64| axis * alpha.cos() + (s.f.x * u.cos() + s.f.y * u.sin()) * alpha.sin();
        return Ssi::Curves(vec![
            Curve::line(apex, ruling(phase + d)),
            Curve::line(apex, ruling(phase - d)),
        ]);
    }
    if na.abs() <= alpha.sin() + 1.0e-12 {
        return Ssi::Unsupported("a plane cutting a cone in a parabola or hyperbola");
    }
    // Ellipse: work in the plane spanned by the axis and the plane normal.
    let tq = (p.f.o - apex).dot(n) / na;
    if tq <= TOL {
        return Ssi::None;
    }
    let w = (n - axis * na).unit();
    let e = (axis - n * na).unit();
    let (ea, ew) = (e.dot(axis), e.dot(w));
    let cot = 1.0 / alpha.tan();
    let s1 = tq / (ew * cot - ea);
    let s2 = tq / (-ew * cot - ea);
    if !s1.is_finite() || !s2.is_finite() {
        return Ssi::Unsupported("a plane cutting a cone in a parabola or hyperbola");
    }
    let q = apex + axis * tq;
    let (p1, p2) = (q + e * s1, q + e * s2);
    let centre = (p1 + p2) * 0.5;
    let major = 0.5 * p1.dist(p2);
    let hc = (centre - apex).dot(axis);
    let off = ((centre - apex) - axis * hc).len();
    let minor2 = (hc * alpha.tan()).powi(2) - off * off;
    if hc <= TOL || minor2 <= TOL * TOL {
        return Ssi::None;
    }
    Ssi::Curves(vec![Curve::ellipse(centre, n, e, major, minor2.sqrt())])
}

fn cylinder_cylinder(a: &Surface, b: &Surface) -> Ssi {
    let (za, zb) = (a.f.z, b.f.z);
    let between = b.f.o - a.f.o;
    if za.cross(zb).len() < ANG_TOL * 10.0 {
        // Parallel axes: intersect the two circles in a normal plane.
        let e = between - za * between.dot(za);
        let d = e.len();
        if d < TOL {
            return Ssi::None;
        }
        if d >= a.r + b.r - TOL || d <= (a.r - b.r).abs() + TOL {
            return Ssi::None;
        }
        let e = e / d;
        let x = (d * d + a.r * a.r - b.r * b.r) / (2.0 * d);
        let y2 = a.r * a.r - x * x;
        if y2 <= TOL * TOL {
            return Ssi::None;
        }
        let y = y2.sqrt();
        let side = za.cross(e);
        return Ssi::Curves(vec![
            Curve::line(a.f.o + e * x + side * y, za),
            Curve::line(a.f.o + e * x - side * y, za),
        ]);
    }
    // Crossing axes of equal radius meet in two planar ellipses.
    let normal = za.cross(zb).unit();
    let gap = between.dot(normal).abs();
    if gap < TOL && (a.r - b.r).abs() < TOL {
        // Point where the axes meet.
        let c = za.dot(zb);
        let denom = 1.0 - c * c;
        let ta = (between.dot(za) - between.dot(zb) * c) / denom;
        let meet = a.f.o + za * ta;
        let mut curves = Vec::new();
        for plane_normal in [(za - zb).unit(), (za + zb).unit()] {
            let cut = Surface::plane(meet, plane_normal);
            if let Ssi::Curves(mut found) = plane_other(&cut, a) {
                curves.append(&mut found);
            }
        }
        return Ssi::Curves(curves);
    }
    if gap >= a.r + b.r - TOL {
        return Ssi::None;
    }
    // The general case in closed form over the angle of the thinner
    // cylinder, which the thicker one is more likely to pass right through.
    if a.r <= b.r {
        trace(a, b)
    } else {
        trace(b, a)
    }
}

/// Intersection of cylinder `base` with the cylinder or sphere `other`,
/// written over the angle `θ` of `base`. At each `θ` the height along the
/// axis solves a quadratic; where its discriminant is positive there are two
/// points, and where it changes sign the two branches meet in a turning
/// point.
fn trace(base: &Surface, other: &Surface) -> Ssi {
    // Squared half-distance between the two roots, in model units.
    let reach = |theta: f64| {
        let (s, c) = theta.sin_cos();
        let foot = base.f.o + (base.f.x * c + base.f.y * s) * base.r;
        let (qa, qb, qc) = Curve::trace_quadratic(other, foot, base.f.z);
        (qb * qb - 4.0 * qa * qc) / (4.0 * qa * qa)
    };
    let mut turns: Vec<f64> = crate::roots::solve(&reach, 0.0, TAU, 720, 0.0)
        .into_iter()
        .filter(|r| !r.tangent)
        .map(|r| r.t)
        .collect();
    if turns.len() > 1 && turns[0] < 1.0e-9 && turns[turns.len() - 1] > TAU - 1.0e-9 {
        turns.pop();
    }
    let mut curves = Vec::new();
    if turns.is_empty() {
        if reach(0.0) > TOL * TOL {
            for sign in [1.0, -1.0] {
                curves.push(Curve::trace(base, other, sign, [0.0, TAU]));
            }
        }
    } else {
        for i in 0..turns.len() {
            let from = turns[i];
            let to = if i + 1 < turns.len() {
                turns[i + 1]
            } else {
                turns[0] + TAU
            };
            if (to - from) * base.r < 10.0 * TOL || reach(0.5 * (from + to)) <= 0.0 {
                continue;
            }
            for sign in [1.0, -1.0] {
                curves.push(Curve::trace(base, other, sign, [from, to]));
            }
        }
    }
    if curves.is_empty() {
        Ssi::None
    } else {
        Ssi::Curves(curves)
    }
}

/// Profile of a surface of revolution in the `(ρ, h)` half-plane of a shared
/// axis.
enum Profile {
    /// `ρ = k·(h − h0)`, valid where the result is positive; `k = ∞` is a
    /// cylinder stored as `Vertical`.
    Slanted {
        h0: f64,
        k: f64,
    },
    Vertical {
        rho: f64,
    },
    Circle {
        rho: f64,
        h: f64,
        r: f64,
    },
}

fn profile(s: &Surface, origin: V3, axis: V3) -> Option<Profile> {
    let on_axis = |p: V3| {
        let d = p - origin;
        (d - axis * d.dot(axis)).len() < TOL
    };
    let parallel = s.f.z.cross(axis).len() < ANG_TOL * 10.0;
    let height = |p: V3| (p - origin).dot(axis);
    match s.kind {
        SurfKind::Cylinder => {
            (parallel && on_axis(s.f.o)).then_some(Profile::Vertical { rho: s.r })
        }
        SurfKind::Cone => (parallel && on_axis(s.f.o)).then(|| Profile::Slanted {
            h0: height(s.f.o),
            k: s.s.tan() * s.f.z.dot(axis).signum(),
        }),
        SurfKind::Sphere => on_axis(s.f.o).then(|| Profile::Circle {
            rho: 0.0,
            h: height(s.f.o),
            r: s.r,
        }),
        SurfKind::Torus => (parallel && on_axis(s.f.o)).then(|| Profile::Circle {
            rho: s.r,
            h: height(s.f.o),
            r: s.s,
        }),
        SurfKind::Plane => None,
    }
}

/// Surfaces of revolution about one axis meet in circles: intersect their
/// profiles in the half-plane.
fn coaxial(a: &Surface, b: &Surface) -> Option<Ssi> {
    // The shared axis: that of a non-spherical member, or the line of centres.
    let (origin, axis) = match (a.kind, b.kind) {
        (SurfKind::Sphere, SurfKind::Sphere) => {
            let d = b.f.o - a.f.o;
            if d.len() < TOL {
                return Some(Ssi::None);
            }
            (a.f.o, d.unit())
        }
        (SurfKind::Sphere, _) => (b.f.o, b.f.z),
        _ => (a.f.o, a.f.z),
    };
    let (pa, pb) = (profile(a, origin, axis)?, profile(b, origin, axis)?);
    let mut points: Vec<(f64, f64)> = Vec::new();
    let line_circle =
        |x0: f64, h0: f64, dx: f64, dh: f64, c: (f64, f64, f64), out: &mut Vec<(f64, f64)>| {
            // Line (x0, h0) + t(dx, dh), unit direction, against a circle.
            let (wx, wh) = (x0 - c.0, h0 - c.1);
            let tm = -(wx * dx + wh * dh);
            let closest = ((wx + dx * tm).powi(2) + (wh + dh * tm).powi(2)).sqrt();
            if closest >= c.2 - TOL {
                return;
            }
            let half = (c.2 * c.2 - closest * closest).sqrt();
            for t in [tm - half, tm + half] {
                out.push((x0 + dx * t, h0 + dh * t));
            }
        };
    use Profile::*;
    match (&pa, &pb) {
        (Vertical { .. }, Vertical { .. }) => {}
        (Vertical { rho }, Slanted { h0, k }) | (Slanted { h0, k }, Vertical { rho }) => {
            points.push((*rho, h0 + rho / k));
        }
        (Slanted { h0: h1, k: k1 }, Slanted { h0: h2, k: k2 }) => {
            if (k1 - k2).abs() > 1.0e-12 {
                let h = (k1 * h1 - k2 * h2) / (k1 - k2);
                points.push((k1 * (h - h1), h));
            }
        }
        (Vertical { rho }, Circle { rho: cx, h, r })
        | (Circle { rho: cx, h, r }, Vertical { rho }) => {
            line_circle(*rho, 0.0, 0.0, 1.0, (*cx, *h, *r), &mut points);
        }
        (Slanted { h0, k }, Circle { rho: cx, h, r })
        | (Circle { rho: cx, h, r }, Slanted { h0, k }) => {
            let norm = (1.0 + k * k).sqrt();
            line_circle(0.0, *h0, k / norm, 1.0 / norm, (*cx, *h, *r), &mut points);
        }
        (
            Circle {
                rho: x1,
                h: h1,
                r: r1,
            },
            Circle {
                rho: x2,
                h: h2,
                r: r2,
            },
        ) => {
            let (dx, dh) = (x2 - x1, h2 - h1);
            let d = dx.hypot(dh);
            if d > TOL && d < r1 + r2 - TOL && d > (r1 - r2).abs() + TOL {
                let along = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
                let off2 = r1 * r1 - along * along;
                if off2 > TOL * TOL {
                    let off = off2.sqrt();
                    let (ex, eh) = (dx / d, dh / d);
                    points.push((x1 + ex * along - eh * off, h1 + eh * along + ex * off));
                    points.push((x1 + ex * along + eh * off, h1 + eh * along - ex * off));
                }
            }
        }
    }
    let x = axis.any_perp();
    let on_cone = |p: &Profile, rho: f64, h: f64| match p {
        // A cone profile only exists on its own nappe.
        Slanted { h0, k } => k * (h - h0) > 0.0 && (k * (h - h0) - rho).abs() < 1.0e-6,
        _ => true,
    };
    let mut curves: Vec<Curve> = Vec::new();
    for (rho, h) in points {
        if rho <= TOL || !on_cone(&pa, rho, h) || !on_cone(&pb, rho, h) {
            continue;
        }
        let duplicate = curves
            .iter()
            .any(|c| (c.r - rho).abs() < TOL && ((c.f.o - origin).dot(axis) - h).abs() < TOL);
        if duplicate {
            continue;
        }
        curves.push(Curve::circle(origin + axis * h, axis, x, rho));
    }
    Some(if curves.is_empty() {
        Ssi::None
    } else {
        Ssi::Curves(curves)
    })
}
