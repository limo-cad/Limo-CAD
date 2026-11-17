//! Boundary representation as flat arrays.
//!
//! A [`Body`] is a handful of `Vec`s indexed by `u32`. There are no pointers,
//! reference counts or per-entity allocations: copying a body is a `memcpy`,
//! transforming it is a loop over three geometry pools, and a snapshot for
//! undo or history is a clone of plain data.
//!
//! Topology uses half-edges with implicit pairing: edge `e` owns half-edges
//! `2e` (running `t0 → t1` along its curve) and `2e + 1` (running back), so
//! `twin(h) = h ^ 1` and no pairing table exists. Every half-edge belongs to
//! exactly one loop; a loop runs with its face on the left when seen from
//! outside the solid. A face is a surface, an orientation flag and a
//! contiguous range of loops; there are no seam edges, so a full cylindrical
//! face simply has two circular loops.

use crate::geom::{Curve, Surface};
use crate::math::*;

pub const NONE: u32 = u32::MAX;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Body {
    // Geometry pools.
    pub surfs: Vec<Surface>,
    pub curves: Vec<Curve>,
    pub verts: Vec<V3>,
    // Edges.
    pub e_curve: Vec<u32>,
    /// Parameter range, `t0 < t1`. A closed edge spans exactly one period.
    pub e_t: Vec<[f64; 2]>,
    /// Vertices at `t0` and `t1` (equal for a closed edge).
    pub e_v: Vec<[u32; 2]>,
    // Half-edges, two per edge.
    pub h_next: Vec<u32>,
    pub h_loop: Vec<u32>,
    // Loops.
    pub l_first: Vec<u32>,
    pub l_face: Vec<u32>,
    // Faces.
    pub f_surf: Vec<u32>,
    /// True when the outward face normal is the surface's natural normal.
    pub f_sense: Vec<bool>,
    /// `[start, end)` range of loop indices.
    pub f_loops: Vec<[u32; 2]>,
    /// Where each face came from: a number set by whatever made the face
    /// and handed on by every operation, to every piece when a face is cut
    /// up. Zero when nothing is known. See [`crate::name`].
    pub f_tag: Vec<u64>,
}

impl Body {
    pub fn new() -> Body {
        Body::default()
    }

    #[inline]
    pub fn face_count(&self) -> usize {
        self.f_surf.len()
    }
    #[inline]
    pub fn edge_count(&self) -> usize {
        self.e_curve.len()
    }
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.f_surf.is_empty()
    }

    pub fn add_surf(&mut self, s: Surface) -> u32 {
        self.surfs.push(s);
        (self.surfs.len() - 1) as u32
    }

    /// Reuse a geometrically identical surface when one exists. Returns the
    /// index and whether the natural normal agrees with `s`.
    pub fn intern_surf(&mut self, s: Surface) -> (u32, bool) {
        for (i, existing) in self.surfs.iter().enumerate() {
            if let Some(same) = existing.same(&s) {
                return (i as u32, same);
            }
        }
        (self.add_surf(s), true)
    }

    pub fn add_curve(&mut self, c: Curve) -> u32 {
        self.curves.push(c);
        (self.curves.len() - 1) as u32
    }

    pub fn add_vert(&mut self, p: V3) -> u32 {
        self.verts.push(p);
        (self.verts.len() - 1) as u32
    }

    pub fn add_edge(&mut self, curve: u32, t0: f64, t1: f64, v0: u32, v1: u32) -> u32 {
        debug_assert!(t0 < t1);
        self.e_curve.push(curve);
        self.e_t.push([t0, t1]);
        self.e_v.push([v0, v1]);
        self.h_next.extend([NONE, NONE]);
        self.h_loop.extend([NONE, NONE]);
        (self.e_curve.len() - 1) as u32
    }

    /// Straight edge between two existing vertices.
    pub fn add_line_edge(&mut self, v0: u32, v1: u32) -> u32 {
        let (a, b) = (self.verts[v0 as usize], self.verts[v1 as usize]);
        let c = self.add_curve(Curve::line(a, b - a));
        self.add_edge(c, 0.0, a.dist(b), v0, v1)
    }

    /// Add a face whose loops are given as half-edge cycles.
    pub fn add_face(&mut self, surf: u32, sense: bool, loops: &[Vec<u32>]) -> u32 {
        let face = self.f_surf.len() as u32;
        let start = self.l_first.len() as u32;
        for cycle in loops {
            debug_assert!(!cycle.is_empty());
            let l = self.l_first.len() as u32;
            self.l_first.push(cycle[0]);
            self.l_face.push(face);
            for (i, &h) in cycle.iter().enumerate() {
                debug_assert_eq!(self.h_loop[h as usize], NONE, "half-edge used twice");
                self.h_loop[h as usize] = l;
                self.h_next[h as usize] = cycle[(i + 1) % cycle.len()];
            }
        }
        self.f_surf.push(surf);
        self.f_sense.push(sense);
        self.f_loops.push([start, self.l_first.len() as u32]);
        self.f_tag.push(0);
        face
    }

    /// Add a face as [`Body::add_face`] does, and say where it comes from.
    pub fn add_tagged_face(&mut self, surf: u32, sense: bool, loops: &[Vec<u32>], tag: u64) -> u32 {
        let face = self.add_face(surf, sense, loops);
        self.f_tag[face as usize] = tag;
        face
    }

    /// Replace every face's tag by `to(tag)`.
    pub fn retag(&mut self, to: impl Fn(u64) -> u64) {
        for tag in &mut self.f_tag {
            *tag = to(*tag);
        }
    }

    #[inline]
    pub fn h_origin(&self, h: u32) -> u32 {
        self.e_v[(h >> 1) as usize][(h & 1) as usize]
    }
    #[inline]
    pub fn h_dest(&self, h: u32) -> u32 {
        self.e_v[(h >> 1) as usize][(1 - (h & 1)) as usize]
    }
    #[inline]
    pub fn h_face(&self, h: u32) -> u32 {
        self.l_face[self.h_loop[h as usize] as usize]
    }
    /// Parameter at the start and end of the half-edge (in travel order).
    #[inline]
    pub fn h_span(&self, h: u32) -> (f64, f64) {
        let [t0, t1] = self.e_t[(h >> 1) as usize];
        if h & 1 == 0 {
            (t0, t1)
        } else {
            (t1, t0)
        }
    }
    #[inline]
    pub fn e_curve_ref(&self, e: u32) -> &Curve {
        &self.curves[self.e_curve[e as usize] as usize]
    }
    #[inline]
    pub fn e_point(&self, e: u32, t: f64) -> V3 {
        self.e_curve_ref(e).point(t)
    }
    #[inline]
    pub fn e_mid(&self, e: u32) -> V3 {
        let [t0, t1] = self.e_t[e as usize];
        self.e_point(e, 0.5 * (t0 + t1))
    }
    #[inline]
    pub fn e_closed(&self, e: u32) -> bool {
        let [a, b] = self.e_v[e as usize];
        a == b
    }
    /// A real crease between two faces: their normals differ along the edge.
    /// Tangent joins, such as the border of a fillet, are not sharp.
    pub fn e_sharp(&self, e: u32) -> bool {
        let (f1, f2) = (self.h_face(2 * e), self.h_face(2 * e + 1));
        let mid = self.e_mid(e);
        f1 != f2 && self.f_normal(f1, mid).cross(self.f_normal(f2, mid)).len() > 1.0e-6
    }
    pub fn e_length(&self, e: u32) -> f64 {
        let [t0, t1] = self.e_t[e as usize];
        self.e_curve_ref(e).length(t0, t1)
    }
    #[inline]
    pub fn f_surface(&self, f: u32) -> &Surface {
        &self.surfs[self.f_surf[f as usize] as usize]
    }
    /// Outward unit normal of face `f` at (the projection of) `p`.
    #[inline]
    pub fn f_normal(&self, f: u32, p: V3) -> V3 {
        let n = self.f_surface(f).normal(p);
        if self.f_sense[f as usize] {
            n
        } else {
            -n
        }
    }
    #[inline]
    pub fn face_loops(&self, f: u32) -> std::ops::Range<u32> {
        let [a, b] = self.f_loops[f as usize];
        a..b
    }

    /// Half-edges of loop `l` in order.
    pub fn loop_hes(&self, l: u32) -> LoopIter<'_> {
        let first = self.l_first[l as usize];
        LoopIter {
            body: self,
            first,
            next: first,
            done: false,
        }
    }

    /// Half-edges of every loop of face `f`.
    pub fn face_hes(&self, f: u32) -> impl Iterator<Item = u32> + '_ {
        self.face_loops(f).flat_map(move |l| self.loop_hes(l))
    }

    /// Unit travel direction of half-edge `h` at its origin.
    pub fn h_dir(&self, h: u32) -> V3 {
        let e = h >> 1;
        let [t0, t1] = self.e_t[e as usize];
        let c = self.e_curve_ref(e);
        if h & 1 == 0 {
            c.deriv(t0).unit()
        } else {
            -c.deriv(t1).unit()
        }
    }

    /// The face on the other side of half-edge `h`.
    #[inline]
    pub fn h_other_face(&self, h: u32) -> u32 {
        self.h_face(h ^ 1)
    }

    pub fn bbox(&self) -> Aabb {
        let mut b = Aabb::EMPTY;
        for &p in &self.verts {
            b.add(p);
        }
        // Curved edges and faces bulge beyond their vertices.
        for e in 0..self.edge_count() as u32 {
            let c = self.e_curve_ref(e);
            if c.kind != crate::geom::CurveKind::Line {
                let [t0, t1] = self.e_t[e as usize];
                let n = 16;
                for i in 0..=n {
                    b.add(c.point(t0 + (t1 - t0) * i as f64 / n as f64));
                }
            }
        }
        b
    }

    /// Rigid or mirrored copy.
    pub fn transformed(&self, x: &Xform) -> Body {
        let mut out = self.clone();
        for s in &mut out.surfs {
            *s = s.transformed(x);
        }
        for c in &mut out.curves {
            *c = c.transformed(x);
        }
        for v in &mut out.verts {
            *v = x.point(*v);
        }
        if x.mirror {
            // A reflection reverses the rotational sense of every loop about
            // its outward normal: traverse each loop backwards along the
            // opposite half-edges.
            for h in 0..self.h_next.len() as u32 {
                let n = self.h_next[h as usize];
                out.h_next[(n ^ 1) as usize] = h ^ 1;
                out.h_loop[(h ^ 1) as usize] = self.h_loop[h as usize];
            }
            for l in &mut out.l_first {
                *l ^= 1;
            }
        }
        out
    }

    /// Append every entity of `other` (disjoint lumps of one body).
    pub fn append(&mut self, other: &Body) {
        let (ds, dc, dv) = (
            self.surfs.len() as u32,
            self.curves.len() as u32,
            self.verts.len() as u32,
        );
        let (de, dl, df) = (
            self.e_curve.len() as u32,
            self.l_first.len() as u32,
            self.f_surf.len() as u32,
        );
        let dh = de * 2;
        self.surfs.extend_from_slice(&other.surfs);
        self.curves.extend_from_slice(&other.curves);
        self.verts.extend_from_slice(&other.verts);
        self.e_curve.extend(other.e_curve.iter().map(|c| c + dc));
        self.e_t.extend_from_slice(&other.e_t);
        self.e_v
            .extend(other.e_v.iter().map(|v| [v[0] + dv, v[1] + dv]));
        self.h_next.extend(other.h_next.iter().map(|h| h + dh));
        self.h_loop.extend(other.h_loop.iter().map(|l| l + dl));
        self.l_first.extend(other.l_first.iter().map(|h| h + dh));
        self.l_face.extend(other.l_face.iter().map(|f| f + df));
        self.f_surf.extend(other.f_surf.iter().map(|s| s + ds));
        self.f_sense.extend_from_slice(&other.f_sense);
        self.f_loops
            .extend(other.f_loops.iter().map(|l| [l[0] + dl, l[1] + dl]));
        self.f_tag.extend_from_slice(&other.f_tag);
    }

    /// Structural and geometric consistency. A body that passes is a closed,
    /// consistently oriented 2-manifold whose edges lie on both of their
    /// faces.
    pub fn check(&self) -> Result<(), String> {
        let nh = self.h_next.len();
        if nh != self.e_curve.len() * 2 || self.h_loop.len() != nh {
            return Err("half-edge arrays are inconsistent".into());
        }
        for h in 0..nh as u32 {
            let (l, n) = (self.h_loop[h as usize], self.h_next[h as usize]);
            if l == NONE || n == NONE {
                return Err(format!(
                    "edge {} is not used in both directions (open shell)",
                    h >> 1
                ));
            }
            if self.h_dest(h) != self.h_origin(n) {
                return Err(format!("loop {l} is not connected at half-edge {h}"));
            }
            if self.h_loop[n as usize] != l {
                return Err(format!("half-edge {h} links into another loop"));
            }
        }
        let mut seen = vec![false; nh];
        for l in 0..self.l_first.len() as u32 {
            let mut count = 0;
            for h in self.loop_hes(l) {
                if seen[h as usize] {
                    return Err(format!("loop {l} revisits half-edge {h}"));
                }
                seen[h as usize] = true;
                count += 1;
                if count > nh {
                    return Err(format!("loop {l} does not close"));
                }
            }
        }
        if seen.iter().any(|s| !s) {
            return Err("a half-edge is not reachable from its loop".into());
        }
        for e in 0..self.edge_count() as u32 {
            let [t0, t1] = self.e_t[e as usize];
            let [v0, v1] = self.e_v[e as usize];
            let c = self.e_curve_ref(e);
            if c.point(t0).dist(self.verts[v0 as usize]) > 10.0 * TOL
                || c.point(t1).dist(self.verts[v1 as usize]) > 10.0 * TOL
            {
                return Err(format!("edge {e} does not end at its vertices"));
            }
            for h in [2 * e, 2 * e + 1] {
                let s = self.f_surface(self.h_face(h));
                for k in [0.25, 0.5, 0.75] {
                    let p = c.point(t0 + (t1 - t0) * k);
                    if s.sdist(p).abs() > 100.0 * TOL {
                        return Err(format!(
                            "edge {e} leaves the surface of face {} by {:.3e}",
                            self.h_face(h),
                            s.sdist(p).abs()
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

pub struct LoopIter<'a> {
    body: &'a Body,
    first: u32,
    next: u32,
    done: bool,
}

impl Iterator for LoopIter<'_> {
    type Item = u32;
    #[inline]
    fn next(&mut self) -> Option<u32> {
        if self.done {
            return None;
        }
        let h = self.next;
        self.next = self.body.h_next[h as usize];
        if self.next == self.first {
            self.done = true;
        }
        Some(h)
    }
}
