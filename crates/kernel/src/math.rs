//! Small fixed-size linear algebra. Everything is `Copy` plain data.

use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

/// Two points closer than this (millimetres) are the same point.
pub const TOL: f64 = 1.0e-6;
/// Two unit directions whose cross product is shorter than this are parallel.
pub const ANG_TOL: f64 = 1.0e-9;
pub const PI: f64 = std::f64::consts::PI;
pub const TAU: f64 = std::f64::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct V3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[inline]
pub const fn v3(x: f64, y: f64, z: f64) -> V3 {
    V3 { x, y, z }
}

impl V3 {
    pub const ZERO: V3 = v3(0.0, 0.0, 0.0);
    pub const X: V3 = v3(1.0, 0.0, 0.0);
    pub const Y: V3 = v3(0.0, 1.0, 0.0);
    pub const Z: V3 = v3(0.0, 0.0, 1.0);

    #[inline]
    pub fn dot(self, o: V3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    #[inline]
    pub fn cross(self, o: V3) -> V3 {
        v3(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }
    #[inline]
    pub fn len2(self) -> f64 {
        self.dot(self)
    }
    #[inline]
    pub fn len(self) -> f64 {
        self.len2().sqrt()
    }
    #[inline]
    pub fn dist(self, o: V3) -> f64 {
        (self - o).len()
    }
    /// Unit vector; the zero vector stays zero.
    #[inline]
    pub fn unit(self) -> V3 {
        let l = self.len();
        if l > 0.0 {
            self * (1.0 / l)
        } else {
            self
        }
    }
    #[inline]
    pub fn lerp(self, o: V3, t: f64) -> V3 {
        self + (o - self) * t
    }
    /// A deterministic unit vector perpendicular to `self` (assumed unit).
    pub fn any_perp(self) -> V3 {
        let a = if self.x.abs() <= self.y.abs() && self.x.abs() <= self.z.abs() {
            V3::X
        } else if self.y.abs() <= self.z.abs() {
            V3::Y
        } else {
            V3::Z
        };
        self.cross(a).unit()
    }
    #[inline]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
    #[inline]
    pub fn arr(self) -> [f64; 3] {
        [self.x, self.y, self.z]
    }
    #[inline]
    pub fn min(self, o: V3) -> V3 {
        v3(self.x.min(o.x), self.y.min(o.y), self.z.min(o.z))
    }
    #[inline]
    pub fn max(self, o: V3) -> V3 {
        v3(self.x.max(o.x), self.y.max(o.y), self.z.max(o.z))
    }
}

impl From<[f64; 3]> for V3 {
    #[inline]
    fn from(a: [f64; 3]) -> V3 {
        v3(a[0], a[1], a[2])
    }
}

impl Add for V3 {
    type Output = V3;
    #[inline]
    fn add(self, o: V3) -> V3 {
        v3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl Sub for V3 {
    type Output = V3;
    #[inline]
    fn sub(self, o: V3) -> V3 {
        v3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f64> for V3 {
    type Output = V3;
    #[inline]
    fn mul(self, s: f64) -> V3 {
        v3(self.x * s, self.y * s, self.z * s)
    }
}
impl Div<f64> for V3 {
    type Output = V3;
    #[inline]
    fn div(self, s: f64) -> V3 {
        v3(self.x / s, self.y / s, self.z / s)
    }
}
impl Neg for V3 {
    type Output = V3;
    #[inline]
    fn neg(self) -> V3 {
        v3(-self.x, -self.y, -self.z)
    }
}
impl AddAssign for V3 {
    #[inline]
    fn add_assign(&mut self, o: V3) {
        *self = *self + o;
    }
}
impl SubAssign for V3 {
    #[inline]
    fn sub_assign(&mut self, o: V3) {
        *self = *self - o;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct V2 {
    pub x: f64,
    pub y: f64,
}

#[inline]
pub const fn v2(x: f64, y: f64) -> V2 {
    V2 { x, y }
}

impl V2 {
    #[inline]
    pub fn dot(self, o: V2) -> f64 {
        self.x * o.x + self.y * o.y
    }
    /// z component of the 3D cross product.
    #[inline]
    pub fn cross(self, o: V2) -> f64 {
        self.x * o.y - self.y * o.x
    }
    #[inline]
    pub fn len(self) -> f64 {
        self.dot(self).sqrt()
    }
}
impl Add for V2 {
    type Output = V2;
    #[inline]
    fn add(self, o: V2) -> V2 {
        v2(self.x + o.x, self.y + o.y)
    }
}
impl Sub for V2 {
    type Output = V2;
    #[inline]
    fn sub(self, o: V2) -> V2 {
        v2(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f64> for V2 {
    type Output = V2;
    #[inline]
    fn mul(self, s: f64) -> V2 {
        v2(self.x * s, self.y * s)
    }
}

/// Right-handed orthonormal frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub o: V3,
    pub x: V3,
    pub y: V3,
    pub z: V3,
}

impl Frame {
    pub const WORLD: Frame = Frame {
        o: V3::ZERO,
        x: V3::X,
        y: V3::Y,
        z: V3::Z,
    };

    /// Frame with the given `z`; `x` is chosen deterministically.
    pub fn from_z(o: V3, z: V3) -> Frame {
        let z = z.unit();
        let x = z.any_perp();
        Frame {
            o,
            x,
            y: z.cross(x),
            z,
        }
    }

    /// Frame with the given `z` and `x` as close as possible to `x_hint`.
    pub fn from_zx(o: V3, z: V3, x_hint: V3) -> Frame {
        let z = z.unit();
        let x = x_hint - z * x_hint.dot(z);
        if x.len2() < 1.0e-24 {
            return Frame::from_z(o, z);
        }
        let x = x.unit();
        Frame {
            o,
            x,
            y: z.cross(x),
            z,
        }
    }

    /// Coordinates of world point `p` in this frame.
    #[inline]
    pub fn local(&self, p: V3) -> V3 {
        let d = p - self.o;
        v3(d.dot(self.x), d.dot(self.y), d.dot(self.z))
    }
    /// World point with frame coordinates `l`.
    #[inline]
    pub fn global(&self, l: V3) -> V3 {
        self.o + self.x * l.x + self.y * l.y + self.z * l.z
    }
}

/// Isometry (rotation, optional reflection, translation): `p' = m·p + t`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xform {
    /// Images of the world axes (matrix columns).
    pub cx: V3,
    pub cy: V3,
    pub cz: V3,
    pub t: V3,
    /// True when the linear part is a reflection (determinant −1).
    pub mirror: bool,
}

impl Xform {
    pub const IDENTITY: Xform = Xform {
        cx: V3::X,
        cy: V3::Y,
        cz: V3::Z,
        t: V3::ZERO,
        mirror: false,
    };

    pub fn translation(t: V3) -> Xform {
        Xform {
            t,
            ..Xform::IDENTITY
        }
    }

    /// Rotation by `angle` about the axis through `origin` along `axis`.
    pub fn rotation(origin: V3, axis: V3, angle: f64) -> Xform {
        let a = axis.unit();
        let (s, c) = angle.sin_cos();
        let rot = |v: V3| v * c + a.cross(v) * s + a * (a.dot(v) * (1.0 - c));
        let (cx, cy, cz) = (rot(V3::X), rot(V3::Y), rot(V3::Z));
        let m = Xform {
            cx,
            cy,
            cz,
            t: V3::ZERO,
            mirror: false,
        };
        Xform {
            t: origin - m.vec(origin),
            ..m
        }
    }

    /// Reflection in the plane through `origin` with the given normal.
    pub fn mirror(origin: V3, normal: V3) -> Xform {
        let n = normal.unit();
        let refl = |v: V3| v - n * (2.0 * v.dot(n));
        let m = Xform {
            cx: refl(V3::X),
            cy: refl(V3::Y),
            cz: refl(V3::Z),
            t: V3::ZERO,
            mirror: true,
        };
        Xform {
            t: origin - m.vec(origin),
            ..m
        }
    }

    /// Unit quaternion (x, y, z, w) about `pivot`, followed by `translation`.
    pub fn rigid(translation: V3, q: [f64; 4], pivot: V3) -> Option<Xform> {
        let mag = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
        if !mag.is_finite() || mag <= 1.0e-12 {
            return None;
        }
        let (x, y, z, w) = (q[0] / mag, q[1] / mag, q[2] / mag, q[3] / mag);
        let m = Xform {
            cx: v3(
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y + z * w),
                2.0 * (x * z - y * w),
            ),
            cy: v3(
                2.0 * (x * y - z * w),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z + x * w),
            ),
            cz: v3(
                2.0 * (x * z + y * w),
                2.0 * (y * z - x * w),
                1.0 - 2.0 * (x * x + y * y),
            ),
            t: V3::ZERO,
            mirror: false,
        };
        Some(Xform {
            t: pivot + translation - m.vec(pivot),
            ..m
        })
    }

    /// Transform a direction.
    #[inline]
    pub fn vec(&self, v: V3) -> V3 {
        self.cx * v.x + self.cy * v.y + self.cz * v.z
    }
    /// Transform a point.
    #[inline]
    pub fn point(&self, p: V3) -> V3 {
        self.vec(p) + self.t
    }
    /// Transform a frame, keeping it right-handed under reflection by
    /// recomputing `y`. `x` and `z` map directly.
    pub fn frame(&self, f: &Frame) -> Frame {
        let x = self.vec(f.x);
        let z = self.vec(f.z);
        Frame {
            o: self.point(f.o),
            x,
            y: z.cross(x),
            z,
        }
    }
    /// Transform a frame keeping `x` and `y` (curve parametrisations survive
    /// reflection); `z` is recomputed.
    pub fn frame_xy(&self, f: &Frame) -> Frame {
        let x = self.vec(f.x);
        let y = self.vec(f.y);
        Frame {
            o: self.point(f.o),
            x,
            y,
            z: x.cross(y),
        }
    }
}

/// Axis-aligned bounding box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub lo: V3,
    pub hi: V3,
}

impl Aabb {
    pub const EMPTY: Aabb = Aabb {
        lo: v3(f64::INFINITY, f64::INFINITY, f64::INFINITY),
        hi: v3(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
    };
    #[inline]
    pub fn add(&mut self, p: V3) {
        self.lo = self.lo.min(p);
        self.hi = self.hi.max(p);
    }
    #[inline]
    pub fn union(&mut self, o: &Aabb) {
        self.lo = self.lo.min(o.lo);
        self.hi = self.hi.max(o.hi);
    }
    #[inline]
    pub fn grown(&self, d: f64) -> Aabb {
        Aabb {
            lo: self.lo - v3(d, d, d),
            hi: self.hi + v3(d, d, d),
        }
    }
    #[inline]
    pub fn overlaps(&self, o: &Aabb) -> bool {
        self.lo.x <= o.hi.x
            && o.lo.x <= self.hi.x
            && self.lo.y <= o.hi.y
            && o.lo.y <= self.hi.y
            && self.lo.z <= o.hi.z
            && o.lo.z <= self.hi.z
    }
    #[inline]
    pub fn contains(&self, p: V3) -> bool {
        p.x >= self.lo.x
            && p.x <= self.hi.x
            && p.y >= self.lo.y
            && p.y <= self.hi.y
            && p.z >= self.lo.z
            && p.z <= self.hi.z
    }
    /// Distance from `p` to the box; zero inside.
    pub fn distance(&self, p: V3) -> f64 {
        let gap = |lo: f64, hi: f64, x: f64| (lo - x).max(x - hi).max(0.0);
        v3(
            gap(self.lo.x, self.hi.x, p.x),
            gap(self.lo.y, self.hi.y, p.y),
            gap(self.lo.z, self.hi.z, p.z),
        )
        .len()
    }

    pub fn is_empty(&self) -> bool {
        self.lo.x > self.hi.x
    }
    pub fn diagonal(&self) -> f64 {
        if self.is_empty() {
            0.0
        } else {
            (self.hi - self.lo).len()
        }
    }
    /// Slab test for the ray `o + s·d`, `s ≥ 0`.
    pub fn hit_by_ray(&self, o: V3, d: V3) -> bool {
        let mut s0 = 0.0_f64;
        let mut s1 = f64::INFINITY;
        for (oo, dd, lo, hi) in [
            (o.x, d.x, self.lo.x, self.hi.x),
            (o.y, d.y, self.lo.y, self.hi.y),
            (o.z, d.z, self.lo.z, self.hi.z),
        ] {
            if dd.abs() < 1.0e-300 {
                if oo < lo || oo > hi {
                    return false;
                }
            } else {
                let (a, b) = ((lo - oo) / dd, (hi - oo) / dd);
                let (a, b) = if a < b { (a, b) } else { (b, a) };
                s0 = s0.max(a);
                s1 = s1.min(b);
                if s0 > s1 {
                    return false;
                }
            }
        }
        true
    }
}

/// Wrap an angle into `[base, base + 2π)`.
#[inline]
pub fn wrap_from(angle: f64, base: f64) -> f64 {
    let mut a = (angle - base) % TAU;
    if a < 0.0 {
        a += TAU;
    }
    base + a
}

/// The representative of `angle` (mod 2π) nearest to `near`.
#[inline]
pub fn wrap_near(angle: f64, near: f64) -> f64 {
    angle + ((near - angle) / TAU).round() * TAU
}
