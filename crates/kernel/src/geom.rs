//! Analytic geometry as tagged plain data.
//!
//! A surface or curve is a kind tag, a frame and two scalars. Evaluation is a
//! `match` on the tag, so the hot loops stay branch-predictable and inline
//! without virtual dispatch or heap indirection.

use crate::form::{CurveForm, Pipe, PipeForm, SplineCurve, SplineSurface, SurfForm};
use crate::math::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SurfKind {
    Plane,
    Cylinder,
    Cone,
    Sphere,
    Torus,
    /// A straight line through the axis, turned about it and carried
    /// along it at once: the flank of a screw thread.
    Helicoid,
    /// A circle carried the same way: the rounded root or crest of one.
    Coil,
    /// A free-form surface: a spline, kept in [`crate::form`].
    Spline,
    /// What a ball rolling along a crease leaves behind: see
    /// [`crate::form::Pipe`].
    Pipe,
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
/// | Helicoid | `o` on axis, `z` axis         | –            | ruling angle |
/// | Coil     | `o` on axis, `z` axis         | major radius | minor radius |
/// | Spline   | where its definition was put  | –            | –            |
/// | Pipe     | –                             | –            | –            |
///
/// The two helical kinds also have a lead `t`, the climb along the axis
/// per radian of `u`, which for them is not an angle that comes round
/// again but a count of turns. On a helicoid `v` is the height the line
/// has gained on its way out from the axis and `s` its angle to the axis,
/// negative when the line falls instead. On a coil `v` is the tube angle;
/// a negative minor radius puts `v = 0` on the side of the tube nearest
/// the axis, so that a face there need not straddle `±π`.
///
/// Parametrisations (`u` is always the angle about `z` for curved kinds):
/// plane `o + u·x + v·y`; cylinder `v` is height; cone `v` is axial height
/// above the apex; sphere `v` is latitude; torus `v` is the tube angle.
/// In every case `∂u × ∂v` is the *natural normal*, which points away from the
/// axis / centre / along `z`.
///
/// A spline is defined once, in coordinates of its own, and `f` says where
/// those are: moving the surface moves the frame and leaves the definition
/// alone. `u` and `v` are the spline's own parameters; a direction that
/// closes smoothly is an angle from `-π` to `π`. A reflection leaves the
/// frame left-handed, and `u` then runs backwards, so that `∂u × ∂v` is
/// still the definition's normal carried along with everything else.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surface {
    pub kind: SurfKind,
    pub f: Frame,
    pub r: f64,
    pub s: f64,
    /// Lead of the helical kinds: how far along the axis one radian of
    /// `u` carries a point. Zero for the others.
    pub t: f64,
    /// The definition of a spline; nothing for the others.
    pub form: Option<SurfForm>,
    /// The spine, radius and surfaces of a pipe; nothing for the others.
    pub pipe: Option<PipeForm>,
}

impl Surface {
    pub fn plane(o: V3, normal: V3) -> Surface {
        Surface {
            kind: SurfKind::Plane,
            f: Frame::from_z(o, normal),
            r: 0.0,
            s: 0.0,
            t: 0.0,
            form: None,
            pipe: None,
        }
    }
    pub fn plane_frame(f: Frame) -> Surface {
        Surface {
            kind: SurfKind::Plane,
            f,
            r: 0.0,
            s: 0.0,
            t: 0.0,
            form: None,
            pipe: None,
        }
    }
    pub fn cylinder(o: V3, axis: V3, radius: f64) -> Surface {
        Surface {
            kind: SurfKind::Cylinder,
            f: Frame::from_z(o, axis),
            r: radius,
            s: 0.0,
            t: 0.0,
            form: None,
            pipe: None,
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
            t: 0.0,
            form: None,
            pipe: None,
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
            t: 0.0,
            form: None,
            pipe: None,
        }
    }
    pub fn sphere_axis(centre: V3, axis: V3, radius: f64) -> Surface {
        Surface {
            kind: SurfKind::Sphere,
            f: Frame::from_z(centre, axis),
            r: radius,
            s: 0.0,
            t: 0.0,
            form: None,
            pipe: None,
        }
    }
    pub fn torus(centre: V3, axis: V3, major: f64, minor: f64) -> Surface {
        Surface {
            kind: SurfKind::Torus,
            f: Frame::from_z(centre, axis),
            r: major,
            s: minor,
            t: 0.0,
            form: None,
            pipe: None,
        }
    }

    /// The surface a straight line through the axis sweeps when it turns
    /// about the axis and climbs it by `lead` with every radian. `angle`
    /// is the angle between the line and the axis, up to a right angle,
    /// where the line goes straight out; negative, the line falls as it
    /// leaves the axis. `v` is the way along the line from the axis:
    /// `o + (radial(u)·sin|angle| ± z·cos|angle|)·v + z·lead·u`.
    pub fn helicoid(o: V3, axis: V3, x_hint: V3, angle: f64, lead: f64) -> Surface {
        Surface {
            kind: SurfKind::Helicoid,
            f: Frame::from_zx(o, axis, x_hint),
            r: 0.0,
            s: angle,
            t: lead,
            form: None,
            pipe: None,
        }
    }
    /// The tube a circle of radius `minor` sweeps when its centre, `major`
    /// from the axis, turns about the axis and climbs it by `lead` with
    /// every radian.
    pub fn coil(o: V3, axis: V3, x_hint: V3, major: f64, minor: f64, lead: f64) -> Surface {
        Surface {
            kind: SurfKind::Coil,
            f: Frame::from_zx(o, axis, x_hint),
            r: major,
            s: minor,
            t: lead,
            form: None,
            pipe: None,
        }
    }
    /// A spline surface where it was defined.
    pub fn spline(form: SurfForm) -> Surface {
        Surface {
            kind: SurfKind::Spline,
            f: Frame::WORLD,
            r: 0.0,
            s: 0.0,
            t: 0.0,
            form: Some(form),
            pipe: None,
        }
    }
    /// The surface a ball of the pipe's radius leaves rolling along its
    /// spine.
    pub fn pipe(form: PipeForm) -> Surface {
        Surface {
            kind: SurfKind::Pipe,
            f: Frame::WORLD,
            r: form.radius,
            s: 0.0,
            t: 0.0,
            form: None,
            pipe: Some(form),
        }
    }
    /// The definition of a pipe.
    #[inline]
    pub fn pipe_form(&self) -> &'static Pipe {
        self.pipe.expect("a pipe has a definition").get()
    }
    /// The definition of a spline surface.
    #[inline]
    pub fn spline_form(&self) -> &'static SplineSurface {
        self.form.expect("a spline surface has a definition").get()
    }
    /// `-1` where a reflection has left a spline's frame left-handed, and
    /// its `u` runs backwards; `+1` otherwise.
    #[inline]
    pub fn hand(&self) -> f64 {
        if self.f.x.cross(self.f.y).dot(self.f.z) < 0.0 {
            -1.0
        } else {
            1.0
        }
    }
    /// A direction of a spline's own coordinates, in the model's.
    #[inline]
    fn carry(&self, v: V3) -> V3 {
        self.f.x * v.x + self.f.y * v.y + self.f.z * v.z
    }
    /// The rectangle of parameters a spline is defined on. A closed
    /// direction runs from `-π` to `π`.
    pub fn spline_box(&self) -> (V2, V2) {
        let form = self.spline_form();
        if self.hand() < 0.0 {
            (v2(-form.hi.x, form.lo.y), v2(-form.lo.x, form.hi.y))
        } else {
            (form.lo, form.hi)
        }
    }
    /// A box that is sure to hold all of a spline: the one round its
    /// control points, wherever the frame has put them.
    pub fn spline_bounds(&self) -> Aabb {
        let b = self.spline_form().bbox;
        let mut out = Aabb::EMPTY;
        for i in 0..8 {
            out.add(self.f.global(v3(
                if i & 1 == 0 { b.lo.x } else { b.hi.x },
                if i & 2 == 0 { b.lo.y } else { b.hi.y },
                if i & 4 == 0 { b.lo.z } else { b.hi.z },
            )));
        }
        out
    }
    #[inline]
    pub fn is_plane(&self) -> bool {
        self.kind == SurfKind::Plane
    }
    /// Whether the surface winds along its axis instead of closing on
    /// itself.
    #[inline]
    pub fn is_helical(&self) -> bool {
        matches!(self.kind, SurfKind::Helicoid | SurfKind::Coil)
    }

    /// Radial unit vector at angle `u` about the axis.
    #[inline]
    fn radial(&self, u: f64) -> V3 {
        let (s, c) = u.sin_cos();
        self.f.x * c + self.f.y * s
    }

    /// Unit vector along the circle of angle `u` about the axis.
    #[inline]
    fn around(&self, u: f64) -> V3 {
        let (s, c) = u.sin_cos();
        self.f.y * c - self.f.x * s
    }

    /// Derivative of [`Surface::point`] with respect to `u`.
    pub fn du(&self, u: f64, v: f64) -> V3 {
        match self.kind {
            SurfKind::Plane => self.f.x,
            SurfKind::Cylinder => self.around(u) * self.r,
            SurfKind::Cone => self.around(u) * (v * self.s.tan()),
            SurfKind::Sphere => self.around(u) * (self.r * v.cos()),
            SurfKind::Torus => self.around(u) * (self.r + self.s * v.cos()),
            SurfKind::Helicoid => self.around(u) * (v * self.s.abs().sin()) + self.f.z * self.t,
            SurfKind::Coil => self.around(u) * (self.r + self.s * v.cos()) + self.f.z * self.t,
            SurfKind::Spline => {
                let hand = self.hand();
                self.carry(self.spline_form().derivs(hand * u, v, 1)[1]) * hand
            }
            // How the frame round the spine turns as the ball rolls on has
            // no formula of its own: two points beside this one tell.
            SurfKind::Pipe => {
                let pipe = self.pipe_form();
                let h = 1.0e-6 * (pipe.span[1] - pipe.span[0]);
                (pipe.point(u + h, v) - pipe.point(u - h, v)) / (2.0 * h)
            }
        }
    }

    /// Derivative of [`Surface::point`] with respect to `v`.
    pub fn dv(&self, u: f64, v: f64) -> V3 {
        let f = &self.f;
        match self.kind {
            SurfKind::Plane => f.y,
            SurfKind::Cylinder => f.z,
            SurfKind::Cone => f.z + self.radial(u) * self.s.tan(),
            SurfKind::Sphere => {
                let (sv, cv) = v.sin_cos();
                (f.z * cv - self.radial(u) * sv) * self.r
            }
            SurfKind::Torus | SurfKind::Coil => {
                let (sv, cv) = v.sin_cos();
                (f.z * cv - self.radial(u) * sv) * self.s
            }
            SurfKind::Helicoid => {
                let (out, up) = self.ruling();
                self.radial(u) * out + f.z * up
            }
            SurfKind::Spline => self.carry(self.spline_form().derivs(self.hand() * u, v, 1)[2]),
            SurfKind::Pipe => {
                let pipe = self.pipe_form();
                let s = pipe.station(u);
                let (sin, cos) = v.sin_cos();
                (s.round * cos + s.off * sin) * pipe.radius
            }
        }
    }

    /// A helicoid's ruling, a unit long: how far it goes out from the axis
    /// and how far up it.
    #[inline]
    pub(crate) fn ruling(&self) -> (f64, f64) {
        let (sin, cos) = self.s.abs().sin_cos();
        (sin, self.s.signum() * cos)
    }

    /// Point and both first derivatives at `(u, v)` in one go.
    pub fn d1(&self, u: f64, v: f64) -> (V3, V3, V3) {
        if self.kind == SurfKind::Spline {
            let hand = self.hand();
            let d = self.spline_form().derivs(hand * u, v, 1);
            return (
                self.f.global(d[0]),
                self.carry(d[1]) * hand,
                self.carry(d[2]),
            );
        }
        (self.point(u, v), self.du(u, v), self.dv(u, v))
    }

    /// Model length of a unit step in each parameter at `(u, v)`.
    pub fn scale(&self, u: f64, v: f64) -> V2 {
        match self.kind {
            SurfKind::Plane => v2(1.0, 1.0),
            SurfKind::Cylinder => v2(self.r, 1.0),
            SurfKind::Cone => v2((v * self.s.tan()).abs(), 1.0 / self.s.cos()),
            SurfKind::Sphere => v2(self.r * v.cos().abs(), self.r),
            SurfKind::Torus => v2((self.r + self.s * v.cos()).abs(), self.s),
            SurfKind::Helicoid => v2((v * self.s.abs().sin()).hypot(self.t), 1.0),
            SurfKind::Coil => v2((self.r + self.s * v.cos()).hypot(self.t), self.s.abs()),
            SurfKind::Spline => {
                let d = self.spline_form().derivs(self.hand() * u, v, 1);
                v2(d[1].len(), d[2].len())
            }
            SurfKind::Pipe => v2(self.du(u, v).len(), self.r),
        }
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
            SurfKind::Helicoid => {
                let (out, up) = self.ruling();
                f.o + self.radial(u) * (v * out) + f.z * (up * v + self.t * u)
            }
            SurfKind::Coil => {
                let (sv, cv) = v.sin_cos();
                f.o + self.radial(u) * (self.r + self.s * cv) + f.z * (self.s * sv + self.t * u)
            }
            SurfKind::Spline => f.global(self.spline_form().point(self.hand() * u, v)),
            SurfKind::Pipe => self.pipe_form().point(u, v),
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
            // A point is on the nearest turn: the one whose height at the
            // point's angle is closest to the point's own.
            SurfKind::Helicoid => {
                let (out, up) = self.ruling();
                let rho = l.x.hypot(l.y);
                let v = rho / out;
                let around = if rho > 0.0 { l.y.atan2(l.x) } else { 0.0 };
                let turns = (((l.z - up * v) / self.t - around) / TAU).round();
                v2(around + TAU * turns, v)
            }
            SurfKind::Coil => {
                let rho = l.x.hypot(l.y);
                let around = if rho > 0.0 { l.y.atan2(l.x) } else { 0.0 };
                let u = around + TAU * ((l.z / self.t - around) / TAU).round();
                let side = self.s.signum();
                v2(u, ((l.z - self.t * u) * side).atan2((rho - self.r) * side))
            }
            SurfKind::Spline => {
                let q = self.spline_form().nearest(l);
                v2(self.hand() * q.x, q.y)
            }
            SurfKind::Pipe => self.pipe_form().uv(p),
        }
    }

    /// Signed distance from `p` to the surface, positive on the natural-normal
    /// side. Exact for every kind.
    ///
    /// A cone is the one nappe that opens along its axis, tip included. The
    /// lines through its tip go on behind it, but that mirror image is no
    /// part of the surface: from back there the nearest point of the cone is
    /// its tip. Counting both made two sheets that all but touch out of any
    /// plane passing close by a tip.
    pub fn sdist(&self, p: V3) -> f64 {
        let l = self.f.local(p);
        match self.kind {
            SurfKind::Plane => l.z,
            SurfKind::Cylinder => l.x.hypot(l.y) - self.r,
            SurfKind::Cone => {
                let (sa, ca) = self.s.sin_cos();
                let rho = l.x.hypot(l.y);
                if rho * sa + l.z * ca >= 0.0 {
                    rho * ca - l.z * sa
                } else {
                    rho.hypot(l.z)
                }
            }
            SurfKind::Sphere => l.len() - self.r,
            SurfKind::Torus => (l.x.hypot(l.y) - self.r).hypot(l.z) - self.s,
            // How far the point is above its turn, turned from a height
            // into a distance: exact on the surface and to first order
            // beside it. Halfway between two turns it changes sign without
            // passing through zero; see `roots::curve_surface`.
            SurfKind::Helicoid => {
                let q = self.uv(p);
                let rho = l.x.hypot(l.y);
                if rho < 1.0e-12 {
                    return 0.0;
                }
                let (out, up) = self.ruling();
                let above = l.z - up * q.y - self.t * q.x;
                -above / (1.0 + (up / out).powi(2) + (self.t / rho).powi(2)).sqrt()
            }
            // In the half-plane through the axis and the point the tube
            // is a circle about the nearest turn of its centre line.
            SurfKind::Coil => {
                let q = self.uv(p);
                let rho = l.x.hypot(l.y);
                let rise = l.z - self.t * q.x;
                let from = (rho - self.r).hypot(rise);
                let steep = if from > 1.0e-12 && rho > 1.0e-12 {
                    (1.0 + (rise * self.t / (from * rho)).powi(2)).sqrt()
                } else {
                    1.0
                };
                (from - self.s.abs()) / steep
            }
            // Along the normal at the nearest point: exact wherever that
            // point is the only one of its kind nearby.
            SurfKind::Spline => {
                let form = self.spline_form();
                let q = form.nearest(l);
                let [s, su, sv, ..] = form.derivs(q.x, q.y, 1);
                (l - s).dot(su.cross(sv).unit())
            }
            // As far from the spine as the ball is big, exactly.
            SurfKind::Pipe => {
                let pipe = self.pipe_form();
                let centre = pipe.spine.point(pipe.along(p));
                pipe.hand * (p.dist(centre) - pipe.radius)
            }
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
                let out = d - f.z * h;
                let (sa, ca) = self.s.sin_cos();
                if out.len() * sa + h * ca >= 0.0 {
                    out.unit() * ca - f.z * sa
                } else {
                    // Behind the tip: straight away from it.
                    d.unit()
                }
            }
            SurfKind::Sphere => (p - f.o).unit(),
            SurfKind::Torus => {
                let d = p - f.o;
                let rad = (d - f.z * d.dot(f.z)).unit();
                (p - (f.o + rad * self.r)).unit()
            }
            SurfKind::Helicoid | SurfKind::Coil => {
                let q = self.uv(p);
                self.du(q.x, q.y).cross(self.dv(q.x, q.y)).unit()
            }
            SurfKind::Spline => {
                let form = self.spline_form();
                let q = form.nearest(f.local(p));
                let [_, su, sv, ..] = form.derivs(q.x, q.y, 1);
                self.carry(su.cross(sv).unit())
            }
            SurfKind::Pipe => {
                let pipe = self.pipe_form();
                let centre = pipe.spine.point(pipe.along(p));
                (p - centre).unit() * pipe.hand
            }
        }
    }

    /// Closest point on the surface.
    #[inline]
    pub fn project(&self, p: V3) -> V3 {
        if self.kind == SurfKind::Spline {
            let form = self.spline_form();
            let q = form.nearest(self.f.local(p));
            return self.f.global(form.point(q.x, q.y));
        }
        if self.kind == SurfKind::Pipe {
            let pipe = self.pipe_form();
            let centre = pipe.spine.point(pipe.along(p));
            return centre + (p - centre).unit() * pipe.radius;
        }
        if !self.is_helical() {
            return p - self.normal(p) * self.sdist(p);
        }
        // The distance is only to first order here: step until it is gone.
        let mut q = p;
        for _ in 0..8 {
            let d = self.sdist(q);
            q = q - self.normal(q) * d;
            if d.abs() < 1.0e-13 {
                break;
            }
        }
        q
    }

    /// Period of the `u` parameter, if any.
    #[inline]
    pub fn u_periodic(&self) -> bool {
        match self.kind {
            SurfKind::Plane | SurfKind::Helicoid | SurfKind::Coil => false,
            SurfKind::Spline => self.spline_form().closed[0],
            SurfKind::Pipe => self.pipe_form().closed,
            _ => true,
        }
    }
    #[inline]
    pub fn v_periodic(&self) -> bool {
        match self.kind {
            SurfKind::Torus => true,
            SurfKind::Spline => self.spline_form().closed[1],
            _ => false,
        }
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
            SurfKind::Cylinder => {
                ((self.r - o.r).abs() < TOL && parallel(self.f.z, o.f.z) && on_axis(o.f.o))
                    .then_some(true)
            }
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
            // The same screw surface can be written from any point of its
            // axis: what counts is that a few points of one lie on the
            // other, facing the same way.
            SurfKind::Helicoid | SurfKind::Coil => {
                if (self.t - o.t).abs() > 1.0e-9
                    || (self.r - o.r).abs() > TOL
                    || !parallel(self.f.z, o.f.z)
                    || !on_axis(o.f.o)
                {
                    return None;
                }
                let (near, far) = if self.kind == SurfKind::Helicoid {
                    (1.0, 2.5)
                } else {
                    (0.3, -0.4)
                };
                let mut facing = 0.0;
                for (u, v) in [(0.3, near), (1.7, far), (4.1, near), (-2.2, far)] {
                    let p = self.point(u, v);
                    if o.sdist(p).abs() > TOL {
                        return None;
                    }
                    let dot = self.normal(p).dot(o.normal(p));
                    if dot.abs() < 1.0 - 1.0e-6 || dot * facing < 0.0 {
                        return None;
                    }
                    facing = dot;
                }
                Some(facing > 0.0)
            }
            // One definition in one place. Two that only happen to describe
            // the same shape are not recognised.
            SurfKind::Spline => (self.form == o.form
                && self.f.o.dist(o.f.o) < TOL
                && self.f.x.dist(o.f.x) < ANG_TOL * 10.0
                && self.f.y.dist(o.f.y) < ANG_TOL * 10.0
                && self.f.z.dist(o.f.z) < ANG_TOL * 10.0)
                .then_some(true),
            // The same ball along the same spine, whichever of its two
            // surfaces the angle round it is counted from.
            SurfKind::Pipe => {
                let (a, b) = (self.pipe_form(), o.pipe_form());
                if std::ptr::eq(a, b) {
                    return Some(true);
                }
                let between = |p: &Surface, q: &Surface| p.kind == q.kind && p.same(q).is_some();
                ((a.radius - b.radius).abs() < TOL
                    && a.spine.same(&b.spine).is_some()
                    && ((between(&a.on, &b.on) && between(&a.other, &b.other))
                        || (between(&a.on, &b.other) && between(&a.other, &b.on))))
                .then_some(a.hand == b.hand)
            }
        }
    }

    pub fn transformed(&self, x: &Xform) -> Surface {
        let mut out = Surface {
            f: x.frame(&self.f),
            ..*self
        };
        // A reflection runs the angle backwards, and a screw with it: a
        // right-hand thread in the mirror is a left-hand one.
        if x.mirror {
            out.t = -self.t;
        }
        if self.kind == SurfKind::Pipe {
            let pipe = self.pipe_form();
            out.pipe = Some(pipe.moved(
                pipe.spine.transformed(x),
                pipe.on.transformed(x),
                pipe.other.transformed(x),
            ));
        }
        // A spline's frame is carried as it is, mirror image and all.
        if self.kind == SurfKind::Spline {
            out.f = Frame {
                o: x.point(self.f.o),
                x: x.vec(self.f.x),
                y: x.vec(self.f.y),
                z: x.vec(self.f.z),
            };
        }
        out
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
            SurfKind::Helicoid | SurfKind::Coil | SurfKind::Spline | SurfKind::Pipe => return None,
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
    /// Where a cylinder meets another quadric: see [`Curve::trace`].
    Trace,
    /// Where any two surfaces meet: see [`Curve::meet`].
    Meet,
    /// A point turning about an axis and climbing it at once.
    Helix,
    /// A free-form curve: a spline, kept in [`crate::form`].
    Spline,
    /// A line of constant `v` on a surface: see [`Curve::iso`].
    Iso,
}

/// Curve as plain data.
///
/// * Line: `o + t·x` (`t` is arc length).
/// * Circle: `o + r(cos t·x + sin t·y)`.
/// * Ellipse: `o + r cos t·x + s sin t·y`.
/// * Helix: `o + r(cos t·x + sin t·y) + s·t·z`, with `s` the climb per
///   radian. It does not come round again: `t` counts turns.
/// * Trace: the intersection of the cylinder (`f`, `r`) with the quadric
///   `other`, written in closed form over the cylinder's angle `θ`: the
///   height along the axis is a root of a quadratic, and `s = ±1` selects
///   which. A branch that exists for every `θ` is closed and uses `t = θ`.
///   Otherwise the curve is the piece between two turning points
///   `span = [θa, θb]`, parametrised over `t ∈ [0, π]` so that it stays
///   smooth where the two roots merge.
/// * Meet: a piece of the intersection of the surfaces `base` and `other`,
///   written on `base`. One parameter of `base` drives (`r` is `0` for `u`,
///   `1` for `v`) and equals `s·t` with `s = ±1`; the other is the nearby
///   root of `other`'s signed distance. `w` holds that parameter and its
///   slope at both ends of `span`: the cubic through them is where the root
///   search starts. It also holds how far from that cubic the curve can be,
///   which is as far as the search may go, so it cannot wander off to
///   another branch of the intersection. A point costs a few Newton steps
///   and is exact.
/// * Iso: the curve on `base` along which `v` is `s`; `t` is the surface's
///   `u`. Where a rolling ball touches one of its surfaces is such a curve
///   on its pipe.
/// * Spline: a free-form curve, defined in coordinates of its own that `f`
///   places. `t` is the spline's parameter, an angle from `-π` to `π` if
///   it closes smoothly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Curve {
    pub kind: CurveKind,
    pub f: Frame,
    pub r: f64,
    pub s: f64,
    /// Trace and Meet: the surface that cuts.
    pub other: Surface,
    pub span: [f64; 2],
    /// Meet only: the surface the curve is written on.
    pub base: Surface,
    /// Meet only: the dependent parameter at `span[0]` and `span[1]`, its
    /// derivative with respect to `t` at both, and the furthest the curve
    /// strays from the cubic those four define (zero: not known).
    pub w: [f64; 5],
    /// The definition of a spline; nothing for the others.
    pub form: Option<CurveForm>,
}

const NO_SURFACE: Surface = Surface {
    kind: SurfKind::Plane,
    f: Frame::WORLD,
    r: 0.0,
    s: 0.0,
    t: 0.0,
    form: None,
    pipe: None,
};

impl Curve {
    pub fn line(o: V3, dir: V3) -> Curve {
        Curve {
            kind: CurveKind::Line,
            f: Frame::from_zx(o, dir.unit().any_perp(), dir),
            r: 0.0,
            s: 0.0,
            other: NO_SURFACE,
            span: [0.0; 2],
            base: NO_SURFACE,
            w: [0.0; 5],
            form: None,
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
            other: NO_SURFACE,
            span: [0.0; 2],
            base: NO_SURFACE,
            w: [0.0; 5],
            form: None,
        }
    }
    pub fn ellipse(centre: V3, normal: V3, major_dir: V3, major: f64, minor: f64) -> Curve {
        Curve {
            kind: CurveKind::Ellipse,
            f: Frame::from_zx(centre, normal, major_dir),
            r: major,
            s: minor,
            other: NO_SURFACE,
            span: [0.0; 2],
            base: NO_SURFACE,
            w: [0.0; 5],
            form: None,
        }
    }
    /// The helix of `radius` about `axis` through the point `radius` from
    /// `o` along `x_hint`, climbing by `lead` with every radian.
    pub fn helix(o: V3, axis: V3, x_hint: V3, radius: f64, lead: f64) -> Curve {
        Curve {
            kind: CurveKind::Helix,
            f: Frame::from_zx(o, axis, x_hint),
            r: radius,
            s: lead,
            other: NO_SURFACE,
            span: [0.0; 2],
            base: NO_SURFACE,
            w: [0.0; 5],
            form: None,
        }
    }
    /// Intersection of `cylinder` with `other` (a cylinder or sphere) on the
    /// branch `sign`, either closed (`span = [0, 2π]`) or between two turning
    /// points.
    pub fn trace(cylinder: &Surface, other: &Surface, sign: f64, span: [f64; 2]) -> Curve {
        Curve {
            kind: CurveKind::Trace,
            f: cylinder.f,
            r: cylinder.r,
            s: sign,
            other: *other,
            span,
            base: NO_SURFACE,
            w: [0.0; 5],
            form: None,
        }
    }

    /// A piece of the intersection of `base` and `other` over `span` of the
    /// driving parameter of `base` (`v` when `along_v`), with the dependent
    /// parameter's end values and slopes in `w`.
    pub fn meet(
        base: &Surface,
        other: &Surface,
        along_v: bool,
        span: [f64; 2],
        w: [f64; 5],
    ) -> Curve {
        Curve {
            kind: CurveKind::Meet,
            f: Frame::WORLD,
            r: if along_v { 1.0 } else { 0.0 },
            s: 1.0,
            other: *other,
            span,
            base: *base,
            w,
            form: None,
        }
    }

    /// The curve on `surface` along which `v` is `at`, with the surface's
    /// `u` for its parameter.
    pub fn iso(surface: &Surface, at: f64, span: [f64; 2]) -> Curve {
        Curve {
            kind: CurveKind::Iso,
            f: Frame::WORLD,
            r: 0.0,
            s: at,
            other: NO_SURFACE,
            span,
            base: *surface,
            w: [0.0; 5],
            form: None,
        }
    }

    /// A spline curve where it was defined.
    pub fn spline(form: CurveForm) -> Curve {
        Curve {
            kind: CurveKind::Spline,
            f: Frame::WORLD,
            r: 0.0,
            s: 0.0,
            other: NO_SURFACE,
            span: [0.0; 2],
            base: NO_SURFACE,
            w: [0.0; 5],
            form: Some(form),
        }
    }
    /// The definition of a spline curve.
    #[inline]
    pub fn spline_form(&self) -> &'static SplineCurve {
        self.form.expect("a spline curve has a definition").get()
    }

    /// Where the cubic through the ends of a meet puts the dependent
    /// parameter at `t`.
    pub fn meet_guess(&self, t: f64) -> f64 {
        let [t0, t1] = self.span;
        let h = t1 - t0;
        let x = if h.abs() > 0.0 { (t - t0) / h } else { 0.0 };
        let [w0, w1, m0, m1, _] = self.w;
        let (x2, x3) = (x * x, x * x * x);
        (2.0 * x3 - 3.0 * x2 + 1.0) * w0
            + (x3 - 2.0 * x2 + x) * h * m0
            + (3.0 * x2 - 2.0 * x3) * w1
            + (x3 - x2) * h * m1
    }

    /// Parameters on `base` of the point of a meet at `t`.
    pub fn meet_uv(&self, t: f64) -> V2 {
        let start = self.meet_guess(t);
        let drive = self.s * t;
        let along_v = self.r != 0.0;
        // Distance from the other surface at dependent parameter `w`, and
        // how fast it changes with `w`.
        let value = |w: f64| {
            let (u, v) = if along_v { (w, drive) } else { (drive, w) };
            let p = self.base.point(u, v);
            let across = if along_v {
                self.base.du(u, v)
            } else {
                self.base.dv(u, v)
            };
            (self.other.sdist(p), self.other.normal(p).dot(across))
        };
        let reach = if self.w[4] > 0.0 {
            self.w[4]
        } else {
            f64::INFINITY
        };
        let (lo, hi) = (start - reach, start + reach);
        let mut w = start;
        let mut strayed = false;
        for _ in 0..12 {
            let (g, slope) = value(w);
            if slope.abs() < 1.0e-14 {
                break;
            }
            let step = g / slope;
            if w - step < lo || w - step > hi {
                strayed = true;
                break;
            }
            w -= step;
            if step.abs() <= 1.0e-14 * (1.0 + w.abs()) {
                break;
            }
        }
        if strayed {
            // Newton was about to leave for another branch. The curve is
            // within reach of the cubic: close in on it from both sides.
            let (mut a, mut b) = (lo, hi);
            let (mut ga, mut gb) = (value(a).0, value(b).0);
            w = start;
            // The reach was measured at a few places along the piece and
            // can fall short between them. Before giving up on a crossing,
            // look a little further out each way.
            let mut wider = reach;
            for _ in 0..6 {
                if (ga > 0.0) != (gb > 0.0) {
                    break;
                }
                wider *= 2.0;
                a = start - wider;
                b = start + wider;
                ga = value(a).0;
                gb = value(b).0;
            }
            if (ga > 0.0) != (gb > 0.0) {
                let mut side = 0;
                for _ in 0..80 {
                    if b - a <= 1.0e-15 * (1.0 + a.abs() + b.abs()) {
                        break;
                    }
                    let mut m = (a * gb - b * ga) / (gb - ga);
                    if !m.is_finite() || m <= a || m >= b {
                        m = 0.5 * (a + b);
                    }
                    let g = value(m).0;
                    w = m;
                    if g == 0.0 {
                        break;
                    }
                    if (g > 0.0) == (gb > 0.0) {
                        b = m;
                        gb = g;
                        if side == -1 {
                            ga *= 0.5;
                        }
                        side = -1;
                    } else {
                        a = m;
                        ga = g;
                        if side == 1 {
                            gb *= 0.5;
                        }
                        side = 1;
                    }
                }
            } else {
                // No crossing to close in on: take what short steps allow.
                for _ in 0..12 {
                    let (g, slope) = value(w);
                    if slope.abs() < 1.0e-14 {
                        break;
                    }
                    let next = (w - g / slope).clamp(lo, hi);
                    if (next - w).abs() <= 1.0e-14 * (1.0 + w.abs()) {
                        break;
                    }
                    w = next;
                }
            }
        }
        if along_v {
            v2(w, drive)
        } else {
            v2(drive, w)
        }
    }

    /// Coefficients of the quadratic in the axial height `h` for a point of
    /// the trace's cylinder at `foot + h·z`.
    pub fn trace_quadratic(other: &Surface, foot: V3, z: V3) -> (f64, f64, f64) {
        let w = foot - other.f.o;
        match other.kind {
            SurfKind::Cylinder => {
                let a = other.f.z;
                let (wp, zp) = (w - a * w.dot(a), z - a * z.dot(a));
                (zp.len2(), 2.0 * wp.dot(zp), wp.len2() - other.r * other.r)
            }
            _ => (1.0, 2.0 * w.dot(z), w.len2() - other.r * other.r),
        }
    }

    #[inline]
    fn trace_closed(&self) -> bool {
        self.span[1] - self.span[0] >= TAU - 1.0e-12
    }

    /// Cylinder angle at parameter `t` of a trace.
    #[inline]
    fn trace_angle(&self, t: f64) -> f64 {
        if self.trace_closed() {
            t
        } else {
            self.span[0] + (self.span[1] - self.span[0]) * 0.5 * (1.0 - t.cos())
        }
    }

    /// Parameter range of a bounded curve.
    #[inline]
    pub fn bounds(&self) -> Option<(f64, f64)> {
        match self.kind {
            CurveKind::Trace if !self.trace_closed() => Some((0.0, PI)),
            CurveKind::Meet => Some((self.span[0], self.span[1])),
            CurveKind::Iso if !self.base.u_periodic() => Some((self.span[0], self.span[1])),
            CurveKind::Spline if !self.spline_form().closed => {
                let form = self.spline_form();
                Some((form.lo, form.hi))
            }
            _ => None,
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
            CurveKind::Trace => {
                let (s, c) = self.trace_angle(t).sin_cos();
                let foot = f.o + (f.x * c + f.y * s) * self.r;
                let (qa, qb, qc) = Curve::trace_quadratic(&self.other, foot, f.z);
                let disc = (qb * qb - 4.0 * qa * qc).max(0.0);
                foot + f.z * ((-qb + self.s * disc.sqrt()) / (2.0 * qa))
            }
            CurveKind::Meet => {
                let q = self.meet_uv(t);
                self.base.point(q.x, q.y)
            }
            CurveKind::Helix => {
                let (s, c) = t.sin_cos();
                f.o + (f.x * c + f.y * s) * self.r + f.z * (self.s * t)
            }
            CurveKind::Spline => f.global(self.spline_form().point(t)),
            CurveKind::Iso => self.base.point(t, self.s),
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
            CurveKind::Trace => {
                let h = 1.0e-6;
                let (lo, hi) = match self.bounds() {
                    Some((a, b)) => ((t - h).max(a), (t + h).min(b)),
                    None => (t - h, t + h),
                };
                (self.point(hi) - self.point(lo)) / (hi - lo)
            }
            CurveKind::Meet => {
                // The dependent parameter follows from staying on `other`.
                let q = self.meet_uv(t);
                let n = self.other.normal(self.base.point(q.x, q.y));
                let (du, dv) = (self.base.du(q.x, q.y), self.base.dv(q.x, q.y));
                let (along, across) = if self.r != 0.0 { (dv, du) } else { (du, dv) };
                let slope = n.dot(across);
                let dw = if slope.abs() > 1.0e-300 {
                    -n.dot(along) / slope
                } else {
                    0.0
                };
                (along + across * dw) * self.s
            }
            CurveKind::Helix => {
                let (s, c) = t.sin_cos();
                (f.y * c - f.x * s) * self.r + f.z * self.s
            }
            CurveKind::Spline => {
                let d = self.spline_form().derivs(t, 1)[1];
                f.x * d.x + f.y * d.y + f.z * d.z
            }
            CurveKind::Iso => self.base.du(t, self.s),
        }
    }

    #[inline]
    pub fn periodic(&self) -> bool {
        match self.kind {
            CurveKind::Line | CurveKind::Meet | CurveKind::Helix => false,
            CurveKind::Trace => self.trace_closed(),
            CurveKind::Spline => self.spline_form().closed,
            CurveKind::Iso => self.base.u_periodic(),
            _ => true,
        }
    }

    /// Parameter of the point on the curve closest to `p` (exact for lines and
    /// circles; refined by a few Newton steps otherwise). Angles are in
    /// `(-π, π]`.
    pub fn param(&self, p: V3) -> f64 {
        let l = self.f.local(p);
        let mut t = match self.kind {
            CurveKind::Line => return l.x,
            CurveKind::Circle => return l.y.atan2(l.x),
            // The turn whose height at the point's angle is nearest.
            CurveKind::Helix => {
                let around = l.y.atan2(l.x);
                around + TAU * ((l.z / self.s - around) / TAU).round()
            }
            CurveKind::Ellipse => (l.y / self.s).atan2(l.x / self.r),
            CurveKind::Spline => return self.spline_form().nearest(l),
            CurveKind::Iso => return self.base.uv(p).x,
            CurveKind::Trace => {
                let theta = l.y.atan2(l.x);
                if self.trace_closed() {
                    theta
                } else {
                    let [a, b] = self.span;
                    let mut u = wrap_from(theta, a);
                    if u > b {
                        // Outside the piece: the nearer turning point.
                        u = if u - b < a + TAU - u { b } else { a };
                    }
                    (1.0 - 2.0 * (u - a) / (b - a)).clamp(-1.0, 1.0).acos()
                }
            }
            CurveKind::Meet => {
                // The driving parameter of the point's own place on `base`.
                let q = self.base.uv(p);
                let (drive, turns) = if self.r != 0.0 {
                    (q.y, self.base.v_periodic())
                } else {
                    (q.x, self.base.u_periodic())
                };
                let mut t = self.s * drive;
                if turns {
                    t = wrap_near(t, 0.5 * (self.span[0] + self.span[1]));
                }
                return t.clamp(self.span[0], self.span[1]);
            }
        };
        for _ in 0..4 {
            let d = self.point(t) - p;
            let d1 = self.deriv(t);
            let h = d1.dot(d1);
            if h < 1.0e-30 {
                break;
            }
            let step = d.dot(d1) / h;
            t -= step;
            if let Some((a, b)) = self.bounds() {
                t = t.clamp(a, b);
            }
            if step.abs() < 1.0e-14 {
                break;
            }
        }
        t
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
                ((d - self.f.x * along).len() < TOL).then(|| (self.f.x.dot(o.f.x).signum(), along))
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
            CurveKind::Trace => {
                // Only the identical construction counts as the same curve.
                let frames = self.f.o.dist(o.f.o) < TOL
                    && self.f.x.dist(o.f.x) < ANG_TOL * 10.0
                    && self.f.z.dist(o.f.z) < ANG_TOL * 10.0;
                (frames
                    && (self.r - o.r).abs() < TOL
                    && self.s == o.s
                    && self.other.same(&o.other).is_some()
                    && (self.span[0] - o.span[0]).abs() < 1.0e-9
                    && (self.span[1] - o.span[1]).abs() < 1.0e-9)
                    .then_some((1.0, 0.0))
            }
            CurveKind::Helix => {
                // One helix written from two points of its axis.
                if (self.r - o.r).abs() >= TOL
                    || (self.s - o.s).abs() >= 1.0e-9
                    || !parallel(self.f.z, o.f.z)
                    || self.f.z.dot(o.f.z) < 0.0
                {
                    return None;
                }
                let d = o.f.o - self.f.o;
                let up = d.dot(self.f.z);
                if (d - self.f.z * up).len() >= TOL {
                    return None;
                }
                let turned = o.f.x.dot(self.f.y).atan2(o.f.x.dot(self.f.x));
                let whole = ((up - self.s * turned) / (TAU * self.s)).round();
                ((up - self.s * turned - TAU * self.s * whole).abs() < TOL)
                    .then_some((1.0, turned + TAU * whole))
            }
            CurveKind::Meet => {
                // Only the identical piece counts as the same curve.
                let close = |a: &Surface, b: &Surface| {
                    a.kind == b.kind
                        && a.form == b.form
                        && a.f.o.dist(b.f.o) < TOL
                        && a.f.x.dist(b.f.x) < ANG_TOL * 10.0
                        && a.f.z.dist(b.f.z) < ANG_TOL * 10.0
                        && (a.r - b.r).abs() < TOL
                        && (a.s - b.s).abs() < TOL
                };
                (close(&self.base, &o.base)
                    && close(&self.other, &o.other)
                    && self.r == o.r
                    && self.s == o.s
                    && (0..2).all(|i| (self.span[i] - o.span[i]).abs() < 1.0e-9)
                    && (0..4).all(|i| (self.w[i] - o.w[i]).abs() < 1.0e-9))
                .then_some((1.0, 0.0))
            }
            CurveKind::Iso => (self.base.kind == o.base.kind
                && self.base.same(&o.base) == Some(true)
                && (self.s - o.s).abs() < 1.0e-12)
                .then_some((1.0, 0.0)),
            // One definition in one place.
            CurveKind::Spline => (self.form == o.form
                && self.f.o.dist(o.f.o) < TOL
                && self.f.x.dist(o.f.x) < ANG_TOL * 10.0
                && self.f.y.dist(o.f.y) < ANG_TOL * 10.0
                && self.f.z.dist(o.f.z) < ANG_TOL * 10.0)
                .then_some((1.0, 0.0)),
        }
    }

    pub fn transformed(&self, x: &Xform) -> Curve {
        let mut out = Curve {
            f: x.frame_xy(&self.f),
            ..*self
        };
        if self.kind == CurveKind::Trace {
            out.other = self.other.transformed(x);
            // Keeping x and y under a reflection reverses the axis, so the
            // two roots of the height swap.
            if x.mirror {
                out.s = -self.s;
            }
        }
        // Keeping x and y under a reflection reverses the axis, and with it
        // the way the helix climbs.
        if self.kind == CurveKind::Helix && x.mirror {
            out.s = -self.s;
        }
        // A spline's frame is carried as it is: nothing about the curve
        // depends on which hand it is.
        if self.kind == CurveKind::Spline {
            out.f = Frame {
                o: x.point(self.f.o),
                x: x.vec(self.f.x),
                y: x.vec(self.f.y),
                z: x.vec(self.f.z),
            };
        }
        if self.kind == CurveKind::Iso {
            out.f = Frame::WORLD;
            out.base = self.base.transformed(x);
        }
        if self.kind == CurveKind::Meet {
            out.f = Frame::WORLD;
            out.base = self.base.transformed(x);
            out.other = self.other.transformed(x);
            if x.mirror {
                // A reflected surface runs one of its parameters backwards:
                // `v` on a plane, `u` on everything else.
                let flips_v = matches!(self.base.kind, SurfKind::Plane | SurfKind::Pipe);
                if flips_v == (self.r != 0.0) {
                    out.s = -self.s;
                } else {
                    for k in 0..4 {
                        out.w[k] = -self.w[k];
                    }
                }
            }
        }
        out
    }

    /// Arc length of the parameter span `[t0, t1]`.
    pub fn length(&self, t0: f64, t1: f64) -> f64 {
        match self.kind {
            CurveKind::Line => (t1 - t0).abs(),
            CurveKind::Circle => (t1 - t0).abs() * self.r,
            CurveKind::Helix => (t1 - t0).abs() * self.r.hypot(self.s),
            CurveKind::Ellipse | CurveKind::Trace | CurveKind::Meet | CurveKind::Iso => {
                let n = 24;
                let mut len = 0.0;
                let mut prev = self.point(t0);
                for i in 1..=n {
                    let p = self.point(t0 + (t1 - t0) * i as f64 / n as f64);
                    len += p.dist(prev);
                    prev = p;
                }
                len
            }
            // The speed, summed over pieces short enough for four points
            // each to be exact to many places.
            CurveKind::Spline => {
                const AT: [f64; 4] = [
                    -0.861_136_311_594_052_6,
                    -0.339_981_043_584_856_3,
                    0.339_981_043_584_856_3,
                    0.861_136_311_594_052_6,
                ];
                const WEIGHT: [f64; 4] = [
                    0.347_854_845_137_453_85,
                    0.652_145_154_862_546_2,
                    0.652_145_154_862_546_2,
                    0.347_854_845_137_453_85,
                ];
                let form = self.spline_form();
                let spans = form.knots.windows(2).filter(|w| w[1] > w[0]).count();
                let share = ((t1 - t0).abs() / (form.hi - form.lo)).min(1.0);
                let n = ((4 * spans) as f64 * share).ceil().max(4.0) as usize;
                let h = (t1 - t0) / n as f64;
                let mut len = 0.0;
                for i in 0..n {
                    let mid = t0 + h * (i as f64 + 0.5);
                    for k in 0..4 {
                        len += WEIGHT[k] * form.derivs(mid + 0.5 * h * AT[k], 1)[1].len();
                    }
                }
                len * 0.5 * h.abs()
            }
        }
    }

    /// Number of straight segments that approximate `[t0, t1]` within the
    /// chord tolerance `sag`, with at least `min_per_turn` segments per turn.
    ///
    /// A curved edge gets two at least. Its middle point then stands off
    /// its chord, so that a sliver between an arc and the line across it,
    /// or between two arcs, keeps an outline with an inside however thin
    /// it is.
    pub fn segments(&self, t0: f64, t1: f64, sag: f64, min_per_turn: f64) -> usize {
        let (r, per_turn) = match self.kind {
            CurveKind::Line => return 1,
            CurveKind::Meet | CurveKind::Spline | CurveKind::Iso => {
                // No radius to go by: double until every chord is close.
                let mut n = 2;
                loop {
                    let at = |i: usize| self.point(t0 + (t1 - t0) * i as f64 / (2 * n) as f64);
                    let worst = (0..n)
                        .map(|i| at(2 * i + 1).dist((at(2 * i) + at(2 * i + 2)) * 0.5))
                        .fold(0.0, f64::max);
                    if worst <= sag || n >= 256 {
                        return n;
                    }
                    n *= 2;
                }
            }
            CurveKind::Circle | CurveKind::Ellipse => (self.r.max(self.s), min_per_turn),
            CurveKind::Helix => (self.r, min_per_turn),
            // The curve bends with the tighter of its two surfaces.
            CurveKind::Trace => (self.r.min(self.other.r), 2.0 * min_per_turn),
        };
        let step = if sag >= r {
            PI
        } else {
            2.0 * (1.0 - sag / r).clamp(-1.0, 1.0).acos()
        };
        let step = step.min(TAU / per_turn).max(1.0e-3);
        (((t1 - t0).abs() / step).ceil() as usize).max(2)
    }
}
