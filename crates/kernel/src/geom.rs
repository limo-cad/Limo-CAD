//! Analytic geometry as tagged plain data.
//!
//! A surface or curve is a kind tag, a frame and two scalars. Evaluation is a
//! `match` on the tag, so the hot loops stay branch-predictable and inline
//! without virtual dispatch or heap indirection.

use crate::math::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SurfKind {
    Plane,
    Cylinder,
    Cone,
    Sphere,
    Torus,
}

/// Analytic surface.
///
/// | kind     | frame                         | `r`          | `s`          |
/// |----------|-------------------------------|--------------|--------------|
/// | Plane    | `o` on plane, `z` normal      | –            | –            |
/// | Cylinder | `o` on axis, `z` axis         | radius       | –            |
/// | Cone     | `o` apex, `z` into the nappe  | –            | half-angle   |
/// | Sphere   | `o` centre, `z` pole axis     | radius       | –            |
/// | Torus    | `o` centre, `z` axis          | major radius | minor radius |
///
/// Parametrisations (`u` is always the angle about `z` for curved kinds):
/// plane `o + u·x + v·y`; cylinder `v` is height; cone `v` is axial height
/// above the apex; sphere `v` is latitude; torus `v` is the tube angle.
/// In every case `∂u × ∂v` is the *natural normal*, which points away from the
/// axis / centre / along `z`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surface {
    pub kind: SurfKind,
    pub f: Frame,
    pub r: f64,
    pub s: f64,
}

impl Surface {
    pub fn plane(o: V3, normal: V3) -> Surface {
        Surface {
            kind: SurfKind::Plane,
            f: Frame::from_z(o, normal),
            r: 0.0,
            s: 0.0,
        }
    }
    pub fn plane_frame(f: Frame) -> Surface {
        Surface {
            kind: SurfKind::Plane,
            f,
            r: 0.0,
            s: 0.0,
        }
    }
    pub fn cylinder(o: V3, axis: V3, radius: f64) -> Surface {
        Surface {
            kind: SurfKind::Cylinder,
            f: Frame::from_z(o, axis),
            r: radius,
            s: 0.0,
        }
    }
    /// Cone with its apex at `apex`, opening along `axis` with the given
    /// half-angle (between the axis and a ruling), in radians.
    pub fn cone(apex: V3, axis: V3, half_angle: f64) -> Surface {
        Surface {
            kind: SurfKind::Cone,
            f: Frame::from_z(apex, axis),
            r: 0.0,
            s: half_angle,
        }
    }
    pub fn sphere(centre: V3, radius: f64) -> Surface {
        Surface {
            kind: SurfKind::Sphere,
            f: Frame {
                o: centre,
                ..Frame::WORLD
            },
            r: radius,
            s: 0.0,
        }
    }
    pub fn sphere_axis(centre: V3, axis: V3, radius: f64) -> Surface {
        Surface {
            kind: SurfKind::Sphere,
            f: Frame::from_z(centre, axis),
            r: radius,
            s: 0.0,
        }
    }
    pub fn torus(centre: V3, axis: V3, major: f64, minor: f64) -> Surface {
        Surface {
            kind: SurfKind::Torus,
            f: Frame::from_z(centre, axis),
            r: major,
            s: minor,
        }
    }

    #[inline]
    pub fn is_plane(&self) -> bool {
        self.kind == SurfKind::Plane
    }

    /// Radial unit vector at angle `u` about the axis.
    #[inline]
    fn radial(&self, u: f64) -> V3 {
        let (s, c) = u.sin_cos();
        self.f.x * c + self.f.y * s
    }

    pub fn point(&self, u: f64, v: f64) -> V3 {
        let f = &self.f;
        match self.kind {
            SurfKind::Plane => f.o + f.x * u + f.y * v,
            SurfKind::Cylinder => f.o + self.radial(u) * self.r + f.z * v,
            SurfKind::Cone => f.o + f.z * v + self.radial(u) * (v * self.s.tan()),
            SurfKind::Sphere => {
                let (sv, cv) = v.sin_cos();
                f.o + (self.radial(u) * cv + f.z * sv) * self.r
            }
            SurfKind::Torus => {
                let (sv, cv) = v.sin_cos();
                f.o + self.radial(u) * (self.r + self.s * cv) + f.z * (self.s * sv)
            }
        }
    }

    /// Parameters of (the projection of) `p`. Angles are in `(-π, π]`.
    pub fn uv(&self, p: V3) -> V2 {
        let l = self.f.local(p);
        match self.kind {
            SurfKind::Plane => v2(l.x, l.y),
            SurfKind::Cylinder | SurfKind::Cone => v2(l.y.atan2(l.x), l.z),
            SurfKind::Sphere => {
                let d = l.len();
                let lat = if d > 0.0 {
                    (l.z / d).clamp(-1.0, 1.0).asin()
                } else {
                    0.0
                };
                v2(l.y.atan2(l.x), lat)
            }
            SurfKind::Torus => {
                let rho = l.x.hypot(l.y);
                v2(l.y.atan2(l.x), l.z.atan2(rho - self.r))
            }
        }
    }

    /// Signed distance from `p` to the surface, positive on the natural-normal
    /// side. Exact for every kind (the cone value is exact for the nappe that
    /// `p` projects onto).
    pub fn sdist(&self, p: V3) -> f64 {
        let l = self.f.local(p);
        match self.kind {
            SurfKind::Plane => l.z,
            SurfKind::Cylinder => l.x.hypot(l.y) - self.r,
            SurfKind::Cone => {
                let (sa, ca) = self.s.sin_cos();
                l.x.hypot(l.y) * ca - l.z.abs() * sa
            }
            SurfKind::Sphere => l.len() - self.r,
            SurfKind::Torus => (l.x.hypot(l.y) - self.r).hypot(l.z) - self.s,
        }
    }

    /// Natural unit normal at (the projection of) `p`.
    pub fn normal(&self, p: V3) -> V3 {
        let f = &self.f;
        match self.kind {
            SurfKind::Plane => f.z,
            SurfKind::Cylinder => {
                let d = p - f.o;
                (d - f.z * d.dot(f.z)).unit()
            }
            SurfKind::Cone => {
                let d = p - f.o;
                let h = d.dot(f.z);
                let rad = (d - f.z * h).unit();
                let (sa, ca) = self.s.sin_cos();
                rad * ca - f.z * (sa * h.signum())
            }
            SurfKind::Sphere => (p - f.o).unit(),
            SurfKind::Torus => {
                let d = p - f.o;
                let rad = (d - f.z * d.dot(f.z)).unit();
                (p - (f.o + rad * self.r)).unit()
            }
        }
    }

    /// Closest point on the surface.
    #[inline]
    pub fn project(&self, p: V3) -> V3 {
        p - self.normal(p) * self.sdist(p)
    }

    /// Period of the `u` parameter, if any.
    #[inline]
    pub fn u_periodic(&self) -> bool {
        self.kind != SurfKind::Plane
    }
    #[inline]
    pub fn v_periodic(&self) -> bool {
        self.kind == SurfKind::Torus
    }

    /// Geometric coincidence. `Some(true)` when the natural normals agree,
    /// `Some(false)` when they are opposite (only possible for planes).
    pub fn same(&self, o: &Surface) -> Option<bool> {
        if self.kind != o.kind {
            return None;
        }
        let parallel = |a: V3, b: V3| a.cross(b).len() < ANG_TOL * 10.0;
        let on_axis = |p: V3| {
            let d = p - self.f.o;
            (d - self.f.z * d.dot(self.f.z)).len() < TOL
        };
        match self.kind {
            SurfKind::Plane => (parallel(self.f.z, o.f.z)
                && (o.f.o - self.f.o).dot(self.f.z).abs() < TOL)
                .then(|| self.f.z.dot(o.f.z) > 0.0),
            SurfKind::Cylinder => ((self.r - o.r).abs() < TOL
                && parallel(self.f.z, o.f.z)
                && on_axis(o.f.o))
            .then_some(true),
            SurfKind::Cone => ((self.s - o.s).abs() < ANG_TOL * 10.0
                && self.f.o.dist(o.f.o) < TOL
                && parallel(self.f.z, o.f.z)
                && self.f.z.dot(o.f.z) > 0.0)
                .then_some(true),
            SurfKind::Sphere => {
                (self.f.o.dist(o.f.o) < TOL && (self.r - o.r).abs() < TOL).then_some(true)
            }
            SurfKind::Torus => (self.f.o.dist(o.f.o) < TOL
                && (self.r - o.r).abs() < TOL
                && (self.s - o.s).abs() < TOL
                && parallel(self.f.z, o.f.z))
            .then_some(true),
        }
    }

    pub fn transformed(&self, x: &Xform) -> Surface {
        Surface {
            f: x.frame(&self.f),
            ..*self
        }
    }

    /// The surface offset by `d` along its natural normal, when the result is
    /// still in this family.
    pub fn offset(&self, d: f64) -> Option<Surface> {
        let mut out = *self;
        match self.kind {
            SurfKind::Plane => out.f.o = self.f.o + self.f.z * d,
            SurfKind::Cylinder | SurfKind::Sphere => {
                out.r = self.r + d;
                if out.r <= TOL {
                    return None;
                }
            }
            SurfKind::Cone => {
                // Offsetting moves the apex along the axis.
                out.f.o = self.f.o - self.f.z * (d / self.s.sin());
            }
            SurfKind::Torus => {
                out.s = self.s + d;
                if out.s <= TOL || out.s >= out.r {
                    return None;
                }
            }
        }
        Some(out)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CurveKind {
    Line,
    Circle,
    Ellipse,
}

/// Analytic curve.
///
/// * Line: `o + t·x` (`t` is arc length).
/// * Circle: `o + r(cos t·x + sin t·y)`.
/// * Ellipse: `o + r cos t·x + s sin t·y`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Curve {
    pub kind: CurveKind,
    pub f: Frame,
    pub r: f64,
    pub s: f64,
}

impl Curve {
    pub fn line(o: V3, dir: V3) -> Curve {
        Curve {
            kind: CurveKind::Line,
            f: Frame::from_zx(o, dir.unit().any_perp(), dir),
            r: 0.0,
            s: 0.0,
        }
    }
    /// Circle whose parameter runs counter-clockwise about `normal`, with
    /// `t = 0` in the direction of `x_hint`.
    pub fn circle(centre: V3, normal: V3, x_hint: V3, radius: f64) -> Curve {
        Curve {
            kind: CurveKind::Circle,
            f: Frame::from_zx(centre, normal, x_hint),
            r: radius,
            s: radius,
        }
    }
    pub fn ellipse(centre: V3, normal: V3, major_dir: V3, major: f64, minor: f64) -> Curve {
        Curve {
            kind: CurveKind::Ellipse,
            f: Frame::from_zx(centre, normal, major_dir),
            r: major,
            s: minor,
        }
    }

    #[inline]
    pub fn point(&self, t: f64) -> V3 {
        let f = &self.f;
        match self.kind {
            CurveKind::Line => f.o + f.x * t,
            CurveKind::Circle => {
                let (s, c) = t.sin_cos();
                f.o + (f.x * c + f.y * s) * self.r
            }
            CurveKind::Ellipse => {
                let (s, c) = t.sin_cos();
                f.o + f.x * (self.r * c) + f.y * (self.s * s)
            }
        }
    }

    /// Derivative with respect to the parameter.
    #[inline]
    pub fn deriv(&self, t: f64) -> V3 {
        let f = &self.f;
        match self.kind {
            CurveKind::Line => f.x,
            CurveKind::Circle => {
                let (s, c) = t.sin_cos();
                (f.y * c - f.x * s) * self.r
            }
            CurveKind::Ellipse => {
                let (s, c) = t.sin_cos();
                f.y * (self.s * c) - f.x * (self.r * s)
            }
        }
    }

    #[inline]
    pub fn periodic(&self) -> bool {
        self.kind != CurveKind::Line
    }

    /// Parameter of the point on the curve closest to `p` (exact for lines and
    /// circles; for ellipses exact when `p` lies on the curve, refined by a
    /// few Newton steps otherwise). Angles are in `(-π, π]`.
    pub fn param(&self, p: V3) -> f64 {
        let l = self.f.local(p);
        match self.kind {
            CurveKind::Line => l.x,
            CurveKind::Circle => l.y.atan2(l.x),
            CurveKind::Ellipse => {
                let mut t = (l.y / self.s).atan2(l.x / self.r);
                for _ in 0..4 {
                    let d = self.point(t) - p;
                    let d1 = self.deriv(t);
                    let (s, c) = t.sin_cos();
                    let d2 = -(self.f.x * (self.r * c) + self.f.y * (self.s * s));
                    let g = d.dot(d1);
                    let h = d1.dot(d1) + d.dot(d2);
                    if h.abs() < 1.0e-30 {
                        break;
                    }
                    let step = g / h;
                    t -= step;
                    if step.abs() < 1.0e-14 {
                        break;
                    }
                }
                t
            }
        }
    }

    /// Distance from `p` to the (unbounded) curve.
    #[inline]
    pub fn dist(&self, p: V3) -> f64 {
        self.point(self.param(p)).dist(p)
    }

    /// If `o` is the same geometric curve, the affine map between the
    /// parametrisations: `t_self = sign · t_other + offset`.
    pub fn same(&self, o: &Curve) -> Option<(f64, f64)> {
        if self.kind != o.kind {
            return None;
        }
        let parallel = |a: V3, b: V3| a.cross(b).len() < ANG_TOL * 10.0;
        match self.kind {
            CurveKind::Line => {
                if !parallel(self.f.x, o.f.x) {
                    return None;
                }
                let d = o.f.o - self.f.o;
                let along = d.dot(self.f.x);
                ((d - self.f.x * along).len() < TOL)
                    .then(|| (self.f.x.dot(o.f.x).signum(), along))
            }
            CurveKind::Circle => {
                if self.f.o.dist(o.f.o) >= TOL
                    || (self.r - o.r).abs() >= TOL
                    || !parallel(self.f.z, o.f.z)
                {
                    return None;
                }
                Some((
                    self.f.z.dot(o.f.z).signum(),
                    o.f.x.dot(self.f.y).atan2(o.f.x.dot(self.f.x)),
                ))
            }
            CurveKind::Ellipse => {
                if self.f.o.dist(o.f.o) >= TOL
                    || (self.r - o.r).abs() >= TOL
                    || (self.s - o.s).abs() >= TOL
                    || !parallel(self.f.z, o.f.z)
                    || !parallel(self.f.x, o.f.x)
                {
                    return None;
                }
                let sign = self.f.z.dot(o.f.z).signum();
                let flipped_x = self.f.x.dot(o.f.x) < 0.0;
                Some((sign, if flipped_x { PI } else { 0.0 }))
            }
        }
    }

    pub fn transformed(&self, x: &Xform) -> Curve {
        Curve {
            f: x.frame_xy(&self.f),
            ..*self
        }
    }

    /// Arc length of the parameter span `[t0, t1]`.
    pub fn length(&self, t0: f64, t1: f64) -> f64 {
        match self.kind {
            CurveKind::Line => (t1 - t0).abs(),
            CurveKind::Circle => (t1 - t0).abs() * self.r,
            CurveKind::Ellipse => {
                let n = 16;
                let mut len = 0.0;
                let mut prev = self.point(t0);
                for i in 1..=n {
                    let p = self.point(t0 + (t1 - t0) * i as f64 / n as f64);
                    len += p.dist(prev);
                    prev = p;
                }
                len
            }
        }
    }

    /// Number of straight segments that approximate `[t0, t1]` within the
    /// chord tolerance `sag`, with at least `min_per_turn` segments per turn.
    pub fn segments(&self, t0: f64, t1: f64, sag: f64, min_per_turn: f64) -> usize {
        match self.kind {
            CurveKind::Line => 1,
            CurveKind::Circle | CurveKind::Ellipse => {
                let r = self.r.max(self.s);
                let step = if sag >= r {
                    PI
                } else {
                    2.0 * (1.0 - sag / r).clamp(-1.0, 1.0).acos()
                };
                let step = step.min(TAU / min_per_turn).max(1.0e-3);
                (((t1 - t0).abs() / step).ceil() as usize).max(1)
            }
        }
    }
}
