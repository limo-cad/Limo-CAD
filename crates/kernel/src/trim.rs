//! Trimmed-face queries: where a face is, in the parameter space of its
//! surface.
//!
//! Edges are discretised once per body ([`EdgeMesh`]) so the two faces of an
//! edge see identical points. Each face then gets a [`FaceTrim`]: its loops as
//! polylines in a face-oriented parameter space (`u` is mirrored for reversed
//! faces, so the face is always on the left of its loops). The polylines
//! answer far-field questions; anything close to the boundary is decided with
//! the exact edge curves.

use crate::geom::{Curve, SurfKind, Surface};
use crate::math::*;
use crate::topo::Body;

/// Discretisation of every edge of a body.
#[derive(Clone, Debug, Default)]
pub struct EdgeMesh {
    offs: Vec<u32>,
    pub pts: Vec<V3>,
}

impl EdgeMesh {
    pub fn build(body: &Body, sag: f64, min_per_turn: f64) -> EdgeMesh {
        let mut offs = Vec::with_capacity(body.edge_count() + 1);
        let mut pts = Vec::new();
        offs.push(0);
        for e in 0..body.edge_count() as u32 {
            let c = body.e_curve_ref(e);
            let [t0, t1] = body.e_t[e as usize];
            let [v0, v1] = body.e_v[e as usize];
            let n = c.segments(t0, t1, sag, min_per_turn);
            // End points are the vertices themselves so loops close exactly.
            pts.push(body.verts[v0 as usize]);
            for i in 1..n {
                pts.push(c.point(t0 + (t1 - t0) * i as f64 / n as f64));
            }
            pts.push(body.verts[v1 as usize]);
            offs.push(pts.len() as u32);
        }
        EdgeMesh { offs, pts }
    }

    #[inline]
    pub fn edge(&self, e: u32) -> &[V3] {
        &self.pts[self.offs[e as usize] as usize..self.offs[e as usize + 1] as usize]
    }
}

/// Sample points of a loop given by its half-edges, without the closing
/// duplicate.
pub fn loop_points(mesh: &EdgeMesh, hes: impl Iterator<Item = u32>) -> Vec<V3> {
    let mut out = Vec::new();
    for h in hes {
        let pts = mesh.edge(h >> 1);
        let n = pts.len();
        if h & 1 == 0 {
            out.extend_from_slice(&pts[..n - 1]);
        } else {
            out.extend(pts[1..].iter().rev());
        }
    }
    out
}

#[derive(Clone, Debug)]
pub struct TrimLoop {
    /// Closed polyline: `uv[n] = uv[0] + shift`.
    pub uv: Vec<V2>,
    /// Model-space point of each `uv` entry (shared edge samples).
    pub p3: Vec<V3>,
    /// Period jump accumulated around the loop (non-zero when it winds).
    pub shift: V2,
    /// Signed parameter-space area; meaningful when `shift` is zero.
    pub area: f64,
}

impl TrimLoop {
    #[inline]
    pub fn winds_u(&self) -> bool {
        self.shift.x.abs() > 1.0
    }
    #[inline]
    pub fn winds_v(&self) -> bool {
        self.shift.y.abs() > 1.0
    }
    #[inline]
    pub fn winds(&self) -> bool {
        self.winds_u() || self.winds_v()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PtClass {
    In,
    Out,
    On,
}

#[derive(Clone, Debug)]
pub struct FaceTrim {
    pub surf: Surface,
    /// `u` is mirrored because the face is reversed on its surface.
    pub flip: bool,
    pub loops: Vec<TrimLoop>,
    pub lo: V2,
    pub hi: V2,
    /// Sphere only: whether each pole belongs to the face.
    north_in: bool,
    south_in: bool,
    /// Chord tolerance the polylines were built with.
    pub sag: f64,
}

/// Parameter-space point is at a pole or apex, where `u` is undefined.
fn singular(surf: &Surface, p: V3) -> bool {
    match surf.kind {
        SurfKind::Cone => {
            let l = surf.f.local(p);
            l.x.hypot(l.y) < TOL
        }
        SurfKind::Sphere => {
            let l = surf.f.local(p);
            l.x.hypot(l.y) < TOL
        }
        _ => false,
    }
}

impl FaceTrim {
    pub fn build(body: &Body, mesh: &EdgeMesh, f: u32, sag: f64) -> FaceTrim {
        let loops = body
            .face_loops(f)
            .map(|l| loop_points(mesh, body.loop_hes(l)))
            .collect();
        FaceTrim::from_points(*body.f_surface(f), !body.f_sense[f as usize], loops, sag)
    }

    /// Build from loops given as model-space sample points (no closing
    /// duplicate). `flip` is true for a face reversed on its surface.
    pub fn from_points(surf: Surface, flip: bool, loop_pts: Vec<Vec<V3>>, sag: f64) -> FaceTrim {
        let mut trim = FaceTrim {
            surf,
            flip,
            loops: Vec::new(),
            lo: v2(f64::INFINITY, f64::INFINITY),
            hi: v2(f64::NEG_INFINITY, f64::NEG_INFINITY),
            north_in: false,
            south_in: false,
            sag,
        };
        let (pu, pv) = (surf.u_periodic(), surf.v_periodic());
        for pts in &loop_pts {
            let mut uv: Vec<V2> = Vec::new();
            let mut p3: Vec<V3> = Vec::new();
            // Leading singular points wait for the first well-defined `u`.
            let mut pending: Vec<(V3, f64)> = Vec::new();
            let mut push = |trim: &FaceTrim, uv: &mut Vec<V2>, p3: &mut Vec<V3>, p: V3| {
                let mut q = trim.uv(p);
                if singular(&surf, p) {
                    match uv.last() {
                        Some(prev) => {
                            uv.push(v2(prev.x, q.y));
                            p3.push(p);
                        }
                        None => pending.push((p, q.y)),
                    }
                    return;
                }
                if let Some(prev) = uv.last().copied() {
                    if pu {
                        q.x = wrap_near(q.x, prev.x);
                    }
                    if pv {
                        q.y = wrap_near(q.y, prev.y);
                    }
                    // Leaving a singular point: the surface degenerates there,
                    // so the boundary runs along the singular line first.
                    if singular(&surf, *p3.last().unwrap()) && (q.x - prev.x).abs() > 1.0e-9 {
                        uv.push(v2(q.x, prev.y));
                        p3.push(*p3.last().unwrap());
                    }
                } else {
                    for (sp, sv) in pending.drain(..) {
                        uv.push(v2(q.x, sv));
                        p3.push(sp);
                    }
                }
                uv.push(q);
                p3.push(p);
            };
            for p in pts {
                push(&trim, &mut uv, &mut p3, *p);
            }
            if uv.is_empty() {
                continue;
            }
            // Closing point: the first point again, continued from the last.
            let first = uv[0];
            let start_singular = singular(&surf, p3[0]);
            if start_singular && (uv.last().unwrap().x - first.x).abs() > 1.0e-9 {
                // Back at a singular start: run along the singular line.
                uv.push(v2(uv.last().unwrap().x, first.y));
                p3.push(p3[0]);
            }
            let last = *uv.last().unwrap();
            let mut close = first;
            if pu && !start_singular {
                close.x = wrap_near(first.x, last.x);
            }
            if pv {
                close.y = wrap_near(first.y, last.y);
            }
            let shift = close - first;
            let shift = v2(
                if shift.x.abs() > 1.0 { shift.x } else { 0.0 },
                if shift.y.abs() > 1.0 { shift.y } else { 0.0 },
            );
            uv.push(first + shift);
            p3.push(p3[0]);
            let mut area = 0.0;
            for w in uv.windows(2) {
                area += w[0].cross(w[1]);
            }
            for q in &uv {
                trim.lo = v2(trim.lo.x.min(q.x), trim.lo.y.min(q.y));
                trim.hi = v2(trim.hi.x.max(q.x), trim.hi.y.max(q.y));
            }
            trim.loops.push(TrimLoop {
                uv,
                p3,
                shift,
                area: 0.5 * area,
            });
        }
        if surf.kind == SurfKind::Sphere {
            let mean_v = |l: &TrimLoop| l.uv.iter().map(|q| q.y).sum::<f64>() / l.uv.len() as f64;
            let winding: Vec<&TrimLoop> = trim.loops.iter().filter(|l| l.winds_u()).collect();
            if winding.is_empty() {
                let complement =
                    trim.loops.is_empty() || trim.loops.iter().map(|l| l.area).sum::<f64>() < 0.0;
                trim.north_in = complement;
                trim.south_in = complement;
            } else {
                let top = winding
                    .iter()
                    .max_by(|a, b| mean_v(a).total_cmp(&mean_v(b)))
                    .unwrap();
                let bottom = winding
                    .iter()
                    .min_by(|a, b| mean_v(a).total_cmp(&mean_v(b)))
                    .unwrap();
                // Travelling +u keeps the face above the loop.
                trim.north_in = top.shift.x > 0.0;
                trim.south_in = bottom.shift.x < 0.0;
            }
        }
        if trim.loops.is_empty() {
            trim.lo = v2(-PI, -PI);
            trim.hi = v2(PI, PI);
        }
        trim
    }

    /// Face-oriented parameters of (the projection of) `p`.
    #[inline]
    pub fn uv(&self, p: V3) -> V2 {
        let q = self.surf.uv(p);
        if self.flip {
            v2(-q.x, q.y)
        } else {
            q
        }
    }

    /// Model-space point at face-oriented parameters.
    #[inline]
    pub fn point(&self, q: V2) -> V3 {
        self.surf.point(if self.flip { -q.x } else { q.x }, q.y)
    }

    /// Outward face normal at face-oriented parameters.
    #[inline]
    pub fn normal(&self, q: V2) -> V3 {
        let n = self.surf.normal(self.point(q));
        if self.flip {
            -n
        } else {
            n
        }
    }

    /// Visit every crossing of the line `u = x` with the loops: the crossing's
    /// `v` and the sign of the crossed segment's travel in `u`.
    pub fn cross_vertical(&self, x: f64, mut visit: impl FnMut(f64, f64)) {
        let periodic = self.surf.u_periodic();
        for lp in &self.loops {
            for w in lp.uv.windows(2) {
                let (a, b) = (w[0], w[1]);
                let (lo, hi) = if a.x < b.x { (a.x, b.x) } else { (b.x, a.x) };
                if lo == hi {
                    continue;
                }
                let xs = if periodic {
                    x + ((lo - x) / TAU).ceil() * TAU
                } else {
                    x
                };
                // Half-open so a vertex on the line is counted exactly once.
                if xs < lo || xs >= hi {
                    continue;
                }
                let t = (xs - a.x) / (b.x - a.x);
                visit(a.y + t * (b.y - a.y), (b.x - a.x).signum());
            }
        }
    }

    /// Visit every crossing of the line `v = y`: the crossing's `u` and the
    /// sign of the crossed segment's travel in `v`.
    pub fn cross_horizontal(&self, y: f64, mut visit: impl FnMut(f64, f64)) {
        let periodic = self.surf.v_periodic();
        for lp in &self.loops {
            for w in lp.uv.windows(2) {
                let (a, b) = (w[0], w[1]);
                let (lo, hi) = if a.y < b.y { (a.y, b.y) } else { (b.y, a.y) };
                if lo == hi {
                    continue;
                }
                let ys = if periodic {
                    y + ((lo - y) / TAU).ceil() * TAU
                } else {
                    y
                };
                if ys < lo || ys >= hi {
                    continue;
                }
                let t = (ys - a.y) / (b.y - a.y);
                visit(a.x + t * (b.x - a.x), (b.y - a.y).signum());
            }
        }
    }

    /// Far-field membership of face-oriented parameters `q`.
    pub fn contains_uv(&self, q: V2) -> bool {
        match self.surf.kind {
            SurfKind::Plane | SurfKind::Cylinder | SurfKind::Cone => {
                let mut odd = false;
                self.cross_vertical(q.x, |v, _| {
                    if v > q.y {
                        odd = !odd;
                    }
                });
                odd
            }
            SurfKind::Sphere => {
                let mut odd = false;
                self.cross_vertical(q.x, |v, _| {
                    if v > q.y {
                        odd = !odd;
                    }
                });
                odd != self.north_in
            }
            SurfKind::Torus => {
                // No "outside" exists; read the orientation of the nearest
                // boundary instead. The face lies to the left of travel.
                let mut best = (f64::INFINITY, 0.0);
                self.cross_vertical(q.x, |v, dir| {
                    let d = wrap_from(v - q.y, 0.0);
                    if d < best.0 {
                        best = (d, dir);
                    }
                });
                if best.0.is_finite() {
                    return best.1 < 0.0;
                }
                self.cross_horizontal(q.y, |u, dir| {
                    let d = wrap_from(u - q.x, 0.0);
                    if d < best.0 {
                        best = (d, dir);
                    }
                });
                if best.0.is_finite() {
                    return best.1 > 0.0;
                }
                self.loops.is_empty() || self.loops.iter().map(|l| l.area).sum::<f64>() < 0.0
            }
        }
    }

    /// Model-space length of a parameter step at `q`.
    fn metric(&self, q: V2) -> V2 {
        let s = &self.surf;
        match s.kind {
            SurfKind::Plane => v2(1.0, 1.0),
            SurfKind::Cylinder => v2(s.r, 1.0),
            SurfKind::Cone => v2((q.y * s.s.tan()).abs(), 1.0 / s.s.cos()),
            SurfKind::Sphere => v2(s.r * q.y.cos().abs(), s.r),
            SurfKind::Torus => v2((s.r + s.s * q.y.cos()).abs(), s.s),
        }
    }

    /// The best interior point: see [`FaceTrim::interior_points`].
    pub fn interior(&self) -> Option<(V2, f64)> {
        self.interior_points().into_iter().next()
    }

    /// Parameters of points well inside the face, best first, each with its
    /// clearance from the boundary along the two parameter directions (model
    /// units). One candidate per scanline, so the points are spread out.
    pub fn interior_points(&self) -> Vec<(V2, f64)> {
        if self.loops.is_empty() {
            return vec![(v2(0.0, 0.0), f64::INFINITY)];
        }
        let span = self.hi.x - self.lo.x;
        let mut found: Vec<(V2, f64)> = Vec::new();
        let mut xs: Vec<(f64, f64)> = Vec::new();
        for frac in [
            0.5, 0.25, 0.75, 0.375, 0.625, 0.125, 0.875, 0.4375, 0.5625, 0.3125, 0.6875, 0.1875,
            0.8125, 0.0625, 0.9375,
        ] {
            let x = self.lo.x + span * frac;
            xs.clear();
            self.cross_vertical(x, |v, dir| xs.push((v, dir)));
            xs.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut best: Option<(V2, f64)> = None;
            let mut consider = |v0: f64, v1: f64| {
                if v1 - v0 <= 1.0e-12 {
                    return;
                }
                let q = v2(x, 0.5 * (v0 + v1));
                let m = self.metric(q);
                let vertical = 0.5 * (v1 - v0) * m.y;
                // Horizontal clearance on the scanline through the candidate.
                let mut horizontal = f64::INFINITY;
                self.cross_horizontal(q.y, |u, _| {
                    let d = if self.surf.u_periodic() {
                        (wrap_near(u, q.x) - q.x).abs()
                    } else {
                        (u - q.x).abs()
                    };
                    horizontal = horizontal.min(d * m.x);
                });
                let score = vertical.min(horizontal);
                if best.is_none_or(|(_, s)| score > s) {
                    best = Some((q, score));
                }
            };
            let n = xs.len();
            for i in 0..n.saturating_sub(1) {
                // The face is above a boundary that travels in +u.
                if xs[i].1 > 0.0 && xs[i + 1].1 < 0.0 {
                    consider(xs[i].0, xs[i + 1].0);
                }
            }
            match self.surf.kind {
                SurfKind::Torus if n > 0 && xs[n - 1].1 > 0.0 && xs[0].1 < 0.0 => {
                    consider(xs[n - 1].0, xs[0].0 + TAU);
                }
                SurfKind::Sphere if n > 0 => {
                    if xs[0].1 < 0.0 {
                        consider(-0.5 * PI, xs[0].0);
                    }
                    if xs[n - 1].1 > 0.0 {
                        consider(xs[n - 1].0, 0.5 * PI);
                    }
                }
                SurfKind::Cone if n > 0 && xs[0].1 < 0.0 && xs[0].0 > 0.0 => {
                    consider(0.0, xs[0].0);
                }
                _ => {}
            }
            if let Some(candidate) = best {
                found.push(candidate);
            }
        }
        found.sort_by(|a, b| b.1.total_cmp(&a.1));
        found
    }

    /// Rough model-space size of the face.
    fn extent(&self) -> f64 {
        let mut b = Aabb::EMPTY;
        for lp in &self.loops {
            for p in &lp.p3 {
                b.add(*p);
            }
        }
        b.diagonal().max(TOL)
    }

    /// Conservative model-space bounds.
    pub fn bbox(&self) -> Aabb {
        let mut b = Aabb::EMPTY;
        for lp in &self.loops {
            for p in &lp.p3 {
                b.add(*p);
            }
        }
        match self.surf.kind {
            SurfKind::Plane => {}
            _ => {
                // Curved faces bulge beyond their boundary: sample the
                // parameter rectangle and pad for the chord error.
                let (nu, nv) = (16, if self.surf.kind == SurfKind::Cylinder { 1 } else { 8 });
                let (mut lo, mut hi) = (self.lo, self.hi);
                // A face that is "everything except its holes" covers the
                // whole closed surface.
                let closed = matches!(self.surf.kind, SurfKind::Sphere | SurfKind::Torus);
                if closed
                    && self.loops.iter().all(|l| !l.winds())
                    && self.loops.iter().map(|l| l.area).sum::<f64>() <= 0.0
                {
                    lo = v2(-PI, if self.surf.kind == SurfKind::Sphere { -0.5 * PI } else { -PI });
                    hi = v2(PI, if self.surf.kind == SurfKind::Sphere { 0.5 * PI } else { PI });
                }
                if self.surf.kind == SurfKind::Sphere {
                    if self.north_in {
                        hi.y = 0.5 * PI;
                    }
                    if self.south_in {
                        lo.y = -0.5 * PI;
                    }
                    if self.north_in || self.south_in || self.loops.is_empty() {
                        lo.x = -PI;
                        hi.x = PI;
                    }
                }
                if self.surf.kind == SurfKind::Cone
                    && self.loops.iter().all(|l| l.winds_u())
                    && self.loops.len() == 1
                {
                    lo.y = lo.y.min(0.0);
                }
                let mut radius: f64 = 0.0;
                for i in 0..=nu {
                    for j in 0..=nv {
                        let q = v2(
                            lo.x + (hi.x - lo.x) * i as f64 / nu as f64,
                            lo.y + (hi.y - lo.y) * j as f64 / nv as f64,
                        );
                        b.add(self.point(q));
                        radius = radius.max(self.metric(q).x);
                    }
                }
                let step = ((hi.x - lo.x) / nu as f64).min(PI);
                let pad = radius.max(self.surf.r).max(self.surf.s.abs())
                    * (1.0 - (0.5 * step).cos());
                return b.grown(pad + 10.0 * TOL);
            }
        }
        b.grown(10.0 * TOL)
    }
}

/// Closest point of the bounded edge `e` to `p`: parameter, distance and
/// whether it is strictly inside the edge.
pub fn edge_closest(body: &Body, e: u32, p: V3) -> (f64, f64, bool) {
    let c: &Curve = body.e_curve_ref(e);
    let [t0, t1] = body.e_t[e as usize];
    let mut t = c.param(p);
    if c.periodic() {
        t = wrap_from(t, t0);
        if t > t1 {
            // Past the end: the nearer end point wins.
            let (d0, d1) = (c.point(t0).dist(p), c.point(t1).dist(p));
            return if d0 < d1 { (t0, d0, false) } else { (t1, d1, false) };
        }
    } else if t <= t0 {
        return (t0, c.point(t0).dist(p), false);
    } else if t >= t1 {
        return (t1, c.point(t1).dist(p), false);
    }
    let speed = c.deriv(t).len().max(1.0e-12);
    let margin = 10.0 * TOL / speed;
    let interior = t > t0 + margin && t < t1 - margin;
    (t, c.point(t).dist(p), interior)
}

/// Classify model-space point `p` (assumed on the surface of face `f`)
/// against the trimmed face.
pub fn classify(body: &Body, trim: &FaceTrim, f: u32, p: V3) -> PtClass {
    let near = 6.0 * trim.sag + 100.0 * TOL;
    let mut best: Option<(f64, u32, f64, bool)> = None;
    for h in body.face_hes(f) {
        let (t, d, interior) = edge_closest(body, h >> 1, p);
        if d < 2.0 * TOL {
            return PtClass::On;
        }
        if best.is_none_or(|(bd, ..)| d < bd) {
            best = Some((d, h, t, interior));
        }
    }
    if let Some((d, h, t, interior)) = best {
        // Close to a smooth stretch of boundary: the side of the exact curve
        // decides, not the polyline.
        let slit = body.h_face(h ^ 1) == f;
        if d < near && interior && !slit {
            let e = h >> 1;
            let c = body.e_curve_ref(e);
            let foot = c.point(t);
            let mut travel = c.deriv(t).unit();
            if h & 1 == 1 {
                travel = -travel;
            }
            let left = body.f_normal(f, foot).cross(travel);
            return if (p - foot).dot(left) > 0.0 {
                PtClass::In
            } else {
                PtClass::Out
            };
        }
    }
    if trim.contains_uv(trim.uv(p)) {
        PtClass::In
    } else {
        PtClass::Out
    }
}
