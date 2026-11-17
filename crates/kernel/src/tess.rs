//! Display and export tessellation.
//!
//! Boundary vertices are the shared edge samples, so meshes of neighbouring
//! faces meet exactly. Planar faces are ear-clipped; cylinders and cones use
//! a strip mesher between their two boundary chains; everything else is
//! ear-clipped in parameter space and refined until every interior edge meets
//! the chord and angle limits. Normals are analytic, so shading is smooth
//! regardless of the triangle layout.

use crate::delaunay::Net;
use crate::geom::SurfKind;
use crate::math::*;
use crate::topo::Body;
use crate::trim::{loop_points, EdgeMesh, FaceTrim, TrimLoop};

#[derive(Clone, Copy, Debug)]
pub struct TessOptions {
    /// Maximum chord deviation (model units).
    pub linear: f64,
    /// Maximum turn of the normal across one segment (radians).
    pub angular: f64,
}

impl Default for TessOptions {
    fn default() -> Self {
        // The viewport policy of the reference adapter (0.15 mm, 0.35 rad)
        // yields 36 segments per full turn there; a segment here may turn
        // the normal by half that angle to match.
        TessOptions {
            linear: 0.15,
            angular: 0.175,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub positions: Vec<V3>,
    pub normals: Vec<V3>,
    pub indices: Vec<u32>,
    /// Per face: first index and index count.
    pub faces: Vec<[u32; 2]>,
}

impl Mesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Enclosed volume by the divergence theorem (exact for planar bodies).
    pub fn volume(&self) -> f64 {
        let mut six = 0.0;
        for t in self.indices.as_chunks::<3>().0 {
            let (a, b, c) = (
                self.positions[t[0] as usize],
                self.positions[t[1] as usize],
                self.positions[t[2] as usize],
            );
            six += a.dot(b.cross(c));
        }
        six / 6.0
    }

    /// Whether this mesh of `body`, made with `opts`, is a clean skin on
    /// it: every triangle within reach of its surface and facing outwards,
    /// and every triangle side shared with exactly one neighbour running
    /// the other way.
    pub fn check(&self, body: &Body, opts: &TessOptions) -> Result<(), String> {
        let key = |p: V3| {
            (
                (p.x * 1.0e6).round() as i64,
                (p.y * 1.0e6).round() as i64,
                (p.z * 1.0e6).round() as i64,
            )
        };
        let mut ids: crate::fast::Map<(i64, i64, i64), u32> = Default::default();
        let mut sides: crate::fast::Map<(u32, u32), i32> = Default::default();
        for (f, &[first, count]) in self.faces.iter().enumerate() {
            let surface = body.f_surface(f as u32);
            for t in self.indices[first as usize..(first + count) as usize]
                .as_chunks::<3>()
                .0
            {
                let p = t.map(|i| self.positions[i as usize]);
                // Inside the triangle and along its sides. A diagonal of
                // two steps at the tolerance is twice as far off as either.
                let probes = [
                    (p[0] + p[1] + p[2]) / 3.0,
                    (p[0] + p[1]) * 0.5,
                    (p[1] + p[2]) * 0.5,
                    (p[2] + p[0]) * 0.5,
                ];
                for q in probes {
                    let off = surface.sdist(q).abs();
                    if off > 2.5 * opts.linear {
                        return Err(format!(
                            "face {f} ({:?}) has a triangle {off:.3} off its surface at {q:?}",
                            surface.kind
                        ));
                    }
                }
                let n = t
                    .iter()
                    .fold(V3::ZERO, |sum, &i| sum + self.normals[i as usize]);
                let twice = (p[1] - p[0]).cross(p[2] - p[0]);
                let longest = (0..3)
                    .map(|k| (p[(k + 1) % 3] - p[k]).len2())
                    .fold(0.0, f64::max);
                // A sliver thinner than the chord tolerance has no direction
                // to speak of: the surface's own bend decides how it tilts.
                let thin = twice.len() < 1.0e-3 * longest
                    || twice.len() / longest.sqrt() < 0.5 * opts.linear;
                let facing = twice.unit().dot(n.unit());
                if facing <= 0.0 && !thin {
                    return Err(format!(
                        "face {f} ({:?}) has a triangle turned {facing:.3} against its surface at {:?}",
                        surface.kind, p[0]
                    ));
                }
                let v = p.map(|q| {
                    let next = ids.len() as u32;
                    *ids.entry(key(q)).or_insert(next)
                });
                for k in 0..3 {
                    let (a, b) = (v[k], v[(k + 1) % 3]);
                    *sides.entry((a.min(b), a.max(b))).or_insert(0) += if a < b { 1 } else { -1 };
                }
            }
        }
        match sides.values().filter(|&&n| n != 0).count() {
            0 => Ok(()),
            open => Err(format!("the mesh has {open} unmatched triangle sides")),
        }
    }

    pub fn area(&self) -> f64 {
        let mut twice = 0.0;
        for t in self.indices.as_chunks::<3>().0 {
            let (a, b, c) = (
                self.positions[t[0] as usize],
                self.positions[t[1] as usize],
                self.positions[t[2] as usize],
            );
            twice += (b - a).cross(c - a).len();
        }
        0.5 * twice
    }
}

pub fn tessellate(body: &Body, opts: &TessOptions) -> Mesh {
    let edges = EdgeMesh::build_for_faces(body, opts.linear, TAU / opts.angular);
    let mut mesh = Mesh::default();
    for f in 0..body.face_count() as u32 {
        let trim = face_trim(body, &edges, f, opts.linear);
        let first = mesh.indices.len() as u32;
        tess_face(&trim, opts, &mut mesh);
        mesh.faces.push([first, mesh.indices.len() as u32 - first]);
    }
    mesh
}

/// Face `f` in the parameters it is meshed in.
///
/// A sphere has no axis of its own, and its two poles are where meshing is
/// hardest: every angle meets there. So each face of a sphere gets the axis
/// that keeps both poles as far from its boundary as can be, off the face
/// altogether where that is possible.
fn face_trim(body: &Body, edges: &EdgeMesh, f: u32, sag: f64) -> FaceTrim {
    let s = body.f_surface(f);
    let loops: Vec<Vec<V3>> = body
        .face_loops(f)
        .map(|l| loop_points(edges, body.loop_hes(l)))
        .collect();
    if s.kind != SurfKind::Sphere || loops.is_empty() {
        return FaceTrim::from_points(*s, !body.f_sense[f as usize], loops, sag);
    }
    // Directions of the boundary from the centre, thinned to a few dozen.
    let total: usize = loops.iter().map(Vec::len).sum();
    let every = total.div_ceil(48).max(1);
    let rim: Vec<V3> = loops
        .iter()
        .flatten()
        .step_by(every)
        .map(|p| (*p - s.f.o).unit())
        .collect();
    let middle = rim.iter().fold(V3::ZERO, |sum, d| sum + *d);
    let mut axes = vec![s.f.z, s.f.x, s.f.y];
    if middle.len() > 1.0e-6 {
        // Through the middle of the boundary, and two ways across it.
        let m = middle.unit();
        let side = m.any_perp();
        axes.extend([m, side, m.cross(side)]);
    }
    for (a, b) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
        axes.push((s.f.x * a + s.f.y * b + s.f.z).unit());
    }
    // How far the boundary keeps from both poles of an axis: nothing when
    // it passes through one, one when it stays on the equator.
    let room = |z: V3| {
        rim.iter()
            .map(|d| 1.0 - d.dot(z).abs())
            .fold(f64::INFINITY, f64::min)
    };
    let mut best = (room(axes[0]), axes[0]);
    for &z in &axes[1..] {
        let r = room(z);
        // The surface's own axis stays unless another is clearly better.
        if r > best.0 + 0.05 {
            best = (r, z);
        }
    }
    let turned = if best.1 == s.f.z {
        *s
    } else {
        crate::geom::Surface::sphere_axis(s.f.o, best.1, s.r)
    };
    FaceTrim::from_points(turned, !body.f_sense[f as usize], loops, sag)
}

/// Polyline of edge `e` for display: two points for a line, otherwise within
/// `deflection` of the curve.
pub fn edge_polyline(body: &Body, e: u32, deflection: f64) -> Vec<V3> {
    let c = body.e_curve_ref(e);
    let [t0, t1] = body.e_t[e as usize];
    let [v0, v1] = body.e_v[e as usize];
    let n = c.segments(t0, t1, deflection, 24.0);
    let mut pts = Vec::with_capacity(n + 1);
    pts.push(body.verts[v0 as usize]);
    for i in 1..n {
        pts.push(c.point(t0 + (t1 - t0) * i as f64 / n as f64));
    }
    pts.push(body.verts[v1 as usize]);
    pts
}

/// A triangle or polygon corner: parameters (possibly shifted by a period)
/// and the face-local vertex it refers to.
#[derive(Clone, Copy, Debug)]
struct Corner {
    uv: V2,
    src: u32,
}

struct FaceVerts {
    p: Vec<V3>,
    n: Vec<V3>,
    /// The vertex this one duplicates (itself unless it is a period image).
    origin: Vec<u32>,
}

impl FaceVerts {
    fn push(&mut self, p: V3, n: V3) -> u32 {
        self.p.push(p);
        self.n.push(n);
        self.origin.push(self.p.len() as u32 - 1);
        (self.p.len() - 1) as u32
    }

    /// A second vertex at the same place, for the same point seen one period
    /// away. Parameter-space edges then stay unique per vertex pair.
    fn alias(&mut self, src: u32) -> u32 {
        self.p.push(self.p[src as usize]);
        self.n.push(self.n[src as usize]);
        self.origin.push(self.origin[src as usize]);
        (self.p.len() - 1) as u32
    }

    /// The corner moved by a period.
    fn shifted(&mut self, c: Corner, by: V2) -> Corner {
        if by.x == 0.0 && by.y == 0.0 {
            return c;
        }
        Corner {
            uv: c.uv + by,
            src: self.alias(c.src),
        }
    }
}

fn singular_normal(trim: &FaceTrim, uv: V2) -> V3 {
    // At a cone apex the normal is that of the ruling through `u`.
    match trim.surf.kind {
        SurfKind::Cone => trim.normal(v2(uv.x, uv.y + 1.0)),
        _ => trim.normal(uv),
    }
}

fn tess_face(trim: &FaceTrim, opts: &TessOptions, mesh: &mut Mesh) {
    let mut fv = FaceVerts {
        p: Vec::new(),
        n: Vec::new(),
        origin: Vec::new(),
    };
    // Face-local vertices: every loop point except the closing duplicate.
    let mut corners: Vec<Vec<Corner>> = Vec::with_capacity(trim.loops.len());
    for lp in &trim.loops {
        let n = lp.uv.len() - 1;
        let mut cs = Vec::with_capacity(n);
        for i in 0..n {
            let src = fv.push(lp.p3[i], singular_normal(trim, lp.uv[i]));
            cs.push(Corner { uv: lp.uv[i], src });
        }
        corners.push(cs);
    }

    let mut tris: Vec<[Corner; 3]> = Vec::new();
    let kind = trim.surf.kind;

    let winding: Vec<usize> = (0..trim.loops.len())
        .filter(|&i| trim.loops[i].winds())
        .collect();
    let plain: Vec<usize> = (0..trim.loops.len())
        .filter(|&i| !trim.loops[i].winds())
        .collect();

    if kind == SurfKind::Cone
        && trim.loops.len() == 1
        && trim.loops[0].winds_u()
        && trim.loops[0].shift.x < 0.0
        && steady(&corners[0], false)
    {
        // Cone tip: a fan from the apex, one apex vertex per ruling so each
        // carries that ruling's normal.
        let cs = &corners[0];
        let apex = trim.surf.f.o;
        let n = cs.len();
        for i in 0..n {
            let (a, b) = (cs[i], cs[(i + 1) % n]);
            let tip_uv = v2(a.uv.x, 0.0);
            let tip = fv.push(apex, fv.n[a.src as usize]);
            let b = if i + 1 == n {
                Corner {
                    uv: b.uv + trim.loops[0].shift,
                    src: b.src,
                }
            } else {
                b
            };
            tris.push([
                a,
                b,
                Corner {
                    uv: tip_uv,
                    src: tip,
                },
            ]);
        }
    } else if let Some((bottom, top)) =
        strip_chains(trim, &corners, &winding, &plain, &mut fv, opts)
    {
        // The face lies between two chains that are monotone in `u`.
        strip(&mut fv, trim, &bottom, &top, opts, &mut tris);
    } else {
        let polys = assemble(trim, &corners, &winding, &plain, &mut fv, opts, &mut tris);
        if crate::diag::trace() {
            eprintln!(
                "tess: {kind:?} face, {} winding and {} plain loops, {} polygons",
                winding.len(),
                plain.len(),
                polys.len()
            );
        }
        for poly in polys {
            if kind == SurfKind::Plane {
                earclip(poly.outer, poly.holes, v2(1.0, 1.0), &mut tris);
            } else {
                fill(trim, opts, &mut fv, poly, &mut tris);
            }
        }
    }

    let base = mesh.positions.len() as u32;
    for t in &tris {
        let (a, b, c) = (
            fv.p[t[0].src as usize],
            fv.p[t[1].src as usize],
            fv.p[t[2].src as usize],
        );
        // A triangle with two corners in one place is nothing: a pole or a
        // tip seen from two angles. One whose corners merely lie in a line
        // stays, flat as it is, because its neighbours meet along its
        // sides and would not meet each other without it.
        let gone = |p: V3, q: V3| (p - q).len2() <= 1.0e-24;
        if gone(a, b) || gone(b, c) || gone(c, a) {
            continue;
        }
        let mut ids = [t[0].src, t[1].src, t[2].src];
        if kind == SurfKind::Cone {
            // A triangle at the apex looks along its own ruling: the apex
            // gets the normal half-way between the other two corners.
            for k in 0..3 {
                let (near, far) = (t[(k + 1) % 3], t[(k + 2) % 3]);
                if t[k].uv.y == 0.0 && near.uv.y != 0.0 && far.uv.y != 0.0 {
                    let u = 0.5 * (near.uv.x + far.uv.x);
                    ids[k] = fv.push(fv.p[t[k].src as usize], singular_normal(trim, v2(u, 0.0)));
                }
            }
        }
        mesh.indices.extend(ids.map(|i| base + i));
    }
    mesh.positions.extend_from_slice(&fv.p);
    mesh.normals.extend_from_slice(&fv.n);
}

/// Bottom and top boundary chains of a curved face, both ordered by
/// increasing `u`, when the face is exactly the region between them.
fn strip_chains(
    trim: &FaceTrim,
    corners: &[Vec<Corner>],
    winding: &[usize],
    plain: &[usize],
    fv: &mut FaceVerts,
    opts: &TessOptions,
) -> Option<(Vec<Corner>, Vec<Corner>)> {
    let kind = trim.surf.kind;
    if kind == SurfKind::Plane || winding.iter().any(|&i| trim.loops[i].winds_v()) {
        return None;
    }
    let step = u_step(trim, opts, 1.0);
    // A row of period images of one pole vertex.
    let pole_row = |fv: &mut FaceVerts, u0: f64, v: f64| -> Vec<Corner> {
        let n = ((TAU / step).ceil() as usize).max(8);
        let q0 = v2(u0, v);
        let src = fv.push(trim.point(q0), trim.normal(q0));
        (0..=n)
            .map(|i| Corner {
                uv: v2(u0 + TAU * i as f64 / n as f64, v),
                src: if i == 0 { src } else { fv.alias(src) },
            })
            .collect()
    };
    // How far a chain may stray from one height on a doubly curved surface.
    let tilt = 2.0 * v_step(trim, opts);
    match (winding.len(), plain.len()) {
        (2, 0) => {
            let (mut lower, mut upper) = band_order(trim, winding);
            let mean = |i: usize| {
                let l = &trim.loops[i];
                l.uv.iter().map(|q| q.y).sum::<f64>() / l.uv.len() as f64
            };
            // Around a torus tube the band may pass through the seam.
            let mut lift = 0.0;
            if mean(lower) > mean(upper) {
                if kind != SurfKind::Torus {
                    return None;
                }
                lift = TAU;
            }
            if trim.loops[lower].shift.x < 0.0 {
                std::mem::swap(&mut lower, &mut upper);
            }
            // Rows run between chains that keep going the same way round;
            // one that doubles back anywhere is for the general mesher.
            let steadily = |i: usize| steady(&corners[i], trim.loops[i].shift.x > 0.0);
            if !steadily(lower) || !steadily(upper) {
                return None;
            }
            // Where the surface bends both ways the rows must also run
            // level, or the triangles between them lean too far to trust.
            if !level(&corners[lower], tilt) || !level(&corners[upper], tilt) {
                return None;
            }
            let uc: Vec<Corner> = corners[upper]
                .iter()
                .map(|c| fv.shifted(*c, v2(0.0, lift)))
                .collect();
            Some(band_chains(
                fv,
                &trim.loops[lower],
                &corners[lower],
                &trim.loops[upper],
                &uc,
            ))
        }
        (1, 0) if kind == SurfKind::Sphere => {
            // A cap: one loop around a pole. Rows follow its rim, which is
            // only sensible while the rim keeps roughly to one latitude.
            let i = winding[0];
            let lp = &trim.loops[i];
            let cs = &corners[i];
            let (low, high) = cs
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), c| {
                    (lo.min(c.uv.y), hi.max(c.uv.y))
                });
            if high - low > 2.0 * v_step(trim, opts) || !steady(cs, lp.shift.x > 0.0) {
                return None;
            }
            let mut chain: Vec<Corner> = cs.clone();
            chain.push(fv.shifted(cs[0], lp.shift));
            if lp.shift.x > 0.0 {
                let top = pole_row(fv, cs[0].uv.x, 0.5 * PI);
                Some((chain, top))
            } else {
                chain.reverse();
                let bottom = pole_row(fv, chain[0].uv.x, -0.5 * PI);
                Some((bottom, chain))
            }
        }
        // One loop around the face (not a hole in the whole surface). Rows
        // need something to end on: a side of constant `u` or a real corner.
        // Around the smooth end of an oval they would all crowd into one
        // point, so that goes to the general mesher instead.
        (0, 1) if trim.loops[plain[0]].area > 0.0 => {
            let (bottom, top) = monotone_chains(&corners[plain[0]])?;
            let ended = |b: [Corner; 2], t: [Corner; 2]| {
                let m = trim.metric(b[0].uv);
                let dir = |c: [Corner; 2]| {
                    let d = c[1].uv - c[0].uv;
                    v2(d.x * m.x, d.y * m.y)
                };
                let (db, dt) = (dir(b), dir(t));
                let upright = |d: V2| d.x.abs() <= 1.0e-9 * (1.0 + d.y.abs());
                upright(db) || upright(dt) || db.dot(dt) > -0.5 * db.len() * dt.len()
            };
            let (nb, nt) = (bottom.len(), top.len());
            if nb < 2 || nt < 2 {
                return None;
            }
            // Even a real corner is no good if a chain climbs away from it
            // so steeply that several of its vertices share the first
            // column: they would all be joined to the one row vertex there.
            let crowded = |chain: &[Corner], from_start: bool| {
                let tip = if from_start {
                    chain[0]
                } else {
                    chain[chain.len() - 1]
                };
                let upright = chain
                    .windows(2)
                    .take_while(|w| (w[0].uv.x - w[1].uv.x).abs() <= 1.0e-9)
                    .count();
                let upright_end = chain
                    .windows(2)
                    .rev()
                    .take_while(|w| (w[0].uv.x - w[1].uv.x).abs() <= 1.0e-9)
                    .count();
                if (from_start && upright > 0) || (!from_start && upright_end > 0) {
                    return false;
                }
                chain
                    .iter()
                    .filter(|c| {
                        let d = (c.uv.x - tip.uv.x).abs();
                        d > 1.0e-9 && d < 0.9 * step
                    })
                    .count()
                    > 2
            };
            // Rows also need a side to start from: an outline with none is
            // an oval or a lens, however pointed its ends look.
            let sided = |c: [Corner; 2]| (c[0].uv.x - c[1].uv.x).abs() <= 1.0e-9;
            let has_side = sided([bottom[0], bottom[1]])
                || sided([top[0], top[1]])
                || sided([bottom[nb - 1], bottom[nb - 2]])
                || sided([top[nt - 1], top[nt - 2]]);
            (has_side
                && level(&bottom, tilt)
                && level(&top, tilt)
                && ended([bottom[0], bottom[1]], [top[0], top[1]])
                && ended([bottom[nb - 1], bottom[nb - 2]], [top[nt - 1], top[nt - 2]])
                && !crowded(&bottom, true)
                && !crowded(&top, true)
                && !crowded(&bottom, false)
                && !crowded(&top, false))
            .then_some((bottom, top))
        }
        (0, 0) if kind == SurfKind::Sphere => {
            let bottom = pole_row(fv, -PI, -0.5 * PI);
            let top = pole_row(fv, -PI, 0.5 * PI);
            Some((bottom, top))
        }
        (0, 0) if kind == SurfKind::Torus => {
            let n = ((TAU / step).ceil() as usize).max(8);
            let mut bottom: Vec<Corner> = Vec::with_capacity(n + 1);
            for i in 0..n {
                let q = v2(-PI + TAU * i as f64 / n as f64, -PI);
                let src = fv.push(trim.point(q), trim.normal(q));
                bottom.push(Corner { uv: q, src });
            }
            let closing = fv.shifted(bottom[0], v2(TAU, 0.0));
            bottom.push(closing);
            let top: Vec<Corner> = bottom
                .iter()
                .map(|c| fv.shifted(*c, v2(0.0, TAU)))
                .collect();
            Some((bottom, top))
        }
        _ => None,
    }
}

/// Whether a chain keeps to one height, give or take `tilt`. Stretches of
/// constant `u` are the sides of a patch and do not count.
fn level(chain: &[Corner], tilt: f64) -> bool {
    if !tilt.is_finite() {
        return true;
    }
    let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
    for w in chain.windows(2) {
        if (w[0].uv.x - w[1].uv.x).abs() > 1.0e-9 {
            low = low.min(w[0].uv.y).min(w[1].uv.y);
            high = high.max(w[0].uv.y).max(w[1].uv.y);
        }
    }
    high - low <= tilt
}

/// Whether a loop's corners keep travelling the same way in `u`, upwards
/// or downwards, all the way round.
fn steady(cs: &[Corner], upwards: bool) -> bool {
    let n = cs.len();
    (0..n).all(|i| {
        let step = cs[(i + 1) % n].uv.x - cs[i].uv.x;
        // The step that closes the loop jumps back by a whole turn.
        let step = if step.abs() > PI {
            step - TAU * step.signum()
        } else {
            step
        };
        if upwards {
            step >= -1.0e-12
        } else {
            step <= 1.0e-12
        }
    })
}

/// Largest parameter step in `u` (scaled by `scale`) that keeps a chord
/// within tolerance.
fn u_step(trim: &FaceTrim, opts: &TessOptions, scale: f64) -> f64 {
    let s = &trim.surf;
    let radius = match s.kind {
        SurfKind::Torus if trim.turned => s.s,
        SurfKind::Torus => s.r + s.s,
        SurfKind::Cone => trim.hi.y.abs().max(trim.lo.y.abs()) * s.s.tan(),
        _ => s.r,
    }
    .max(1.0e-9)
        * scale;
    let chord = if opts.linear >= radius {
        PI
    } else {
        2.0 * (1.0 - opts.linear / radius).acos()
    };
    chord.min(opts.angular).max(1.0e-3)
}

/// Largest parameter step in `v` that keeps a chord within tolerance, on a
/// surface that bends that way too.
fn v_step(trim: &FaceTrim, opts: &TessOptions) -> f64 {
    let s = &trim.surf;
    let tube = match s.kind {
        SurfKind::Sphere => s.r,
        SurfKind::Torus if trim.turned => s.r + s.s,
        SurfKind::Torus => s.s,
        _ => return f64::INFINITY,
    };
    let chord = if opts.linear >= tube {
        PI
    } else {
        2.0 * (1.0 - opts.linear / tube).acos()
    };
    chord.min(opts.angular).max(1.0e-3)
}

/// Height of a chain (ordered by increasing `u`) at parameter `u`.
fn v_on(chain: &[Corner], u: f64) -> f64 {
    let n = chain.len();
    if u <= chain[0].uv.x {
        return chain[0].uv.y;
    }
    for w in chain.windows(2) {
        let (a, b) = (w[0].uv, w[1].uv);
        if u <= b.x {
            if b.x - a.x <= 1.0e-14 {
                return b.y;
            }
            return a.y + (u - a.x) / (b.x - a.x) * (b.y - a.y);
        }
    }
    chain[n - 1].uv.y
}

/// Mesh the region between two chains. Singly curved surfaces need one
/// layer; spheres and tori get interior rows so the tube direction is
/// resolved too.
fn strip(
    fv: &mut FaceVerts,
    trim: &FaceTrim,
    bottom: &[Corner],
    top: &[Corner],
    opts: &TessOptions,
    tris: &mut Vec<[Corner; 3]>,
) {
    let tube = v_step(trim, opts);
    // Chains of one loop meet at both ends (the very same vertex, not a
    // period image of it); a band closes on itself instead.
    let mut joined =
        bottom[0].src == top[0].src && bottom[bottom.len() - 1].src == top[top.len() - 1].src;
    // Where such a loop begins or ends with an edge of constant `u`, that
    // edge is a side of the patch in its own right: rows end on it, spread
    // over its vertices, instead of all meeting in one of them.
    let upright = |w: &[Corner]| (w[0].uv.x - w[1].uv.x).abs() <= 1.0e-9;
    let lead = |chain: &[Corner]| chain.windows(2).take_while(|w| upright(w)).count();
    let trail = |chain: &[Corner]| chain.windows(2).rev().take_while(|w| upright(w)).count();
    let (mut left, mut right): (Vec<Corner>, Vec<Corner>) = (Vec::new(), Vec::new());
    let (mut lower, mut upper) = (bottom, top);
    if joined {
        let (pb, pt) = (lead(bottom), lead(top));
        let (qb, qt) = (trail(bottom), trail(top));
        if pb + qb < bottom.len() - 1 && pt + qt < top.len() - 1 {
            // Both sides run upwards, from the lower chain to the upper one.
            left = bottom[..=pb]
                .iter()
                .rev()
                .chain(&top[1..=pt])
                .copied()
                .collect();
            right = bottom[bottom.len() - 1 - qb..]
                .iter()
                .chain(top[top.len() - 1 - qt..top.len() - 1].iter().rev())
                .copied()
                .collect();
            lower = &bottom[pb..bottom.len() - qb];
            upper = &top[pt..top.len() - qt];
        } else {
            joined = false;
        }
    }
    let (ua, ub) = (
        lower[0].uv.x.min(upper[0].uv.x),
        lower[lower.len() - 1].uv.x.max(upper[upper.len() - 1].uv.x),
    );
    let columns = (((ub - ua) / u_step(trim, opts, 1.0)).ceil() as usize).max(1);
    let mut layers = 1;
    if tube.is_finite() {
        let step = v_step(trim, opts);
        let mut gap: f64 = 0.0;
        for i in 0..=columns {
            let u = ua + (ub - ua) * i as f64 / columns as f64;
            gap = gap.max((v_on(upper, u) - v_on(lower, u)).abs());
        }
        layers = ((gap / step).ceil() as usize).max(1);
    }
    if !joined && layers == 1 {
        zipper(bottom, top, tris);
        return;
    }
    let same = |fv: &FaceVerts, a: Corner, b: Corner| {
        fv.origin[a.src as usize] == fv.origin[b.src as usize]
    };
    let closed = !joined && same(fv, bottom[0], bottom[bottom.len() - 1]);
    // Vertex of a side that row `k` ends on.
    let pick = |side: &[Corner], k: usize| {
        ((k * (side.len() - 1)) as f64 / layers as f64).round() as usize
    };
    let mut prev: Vec<Corner> = lower.to_vec();
    let mut prev_ends = (0, 0);
    for k in 1..=layers {
        let ends = if joined {
            (pick(&left, k), pick(&right, k))
        } else {
            (0, 0)
        };
        let row: Vec<Corner> = if k == layers {
            upper.to_vec()
        } else {
            let f = k as f64 / layers as f64;
            let mut row: Vec<Corner> = Vec::with_capacity(columns + 1);
            for i in 0..=columns {
                if joined && i == 0 {
                    row.push(left[ends.0]);
                    continue;
                }
                if joined && i == columns {
                    row.push(right[ends.1]);
                    continue;
                }
                if closed && i == columns {
                    let first = row[0];
                    row.push(fv.shifted(first, v2(ub - ua, 0.0)));
                    continue;
                }
                let u = ua + (ub - ua) * i as f64 / columns as f64;
                let (vb, vt) = (v_on(lower, u), v_on(upper, u));
                let q = v2(u, vb + (vt - vb) * f);
                let src = fv.push(trim.point(q), trim.normal(q));
                row.push(Corner { uv: q, src });
            }
            row
        };
        if joined {
            stitch(
                &prev,
                &row,
                &left[prev_ends.0..=ends.0],
                &right[prev_ends.1..=ends.1],
                tris,
            );
        } else {
            zipper(&prev, &row, tris);
        }
        prev = row;
        prev_ends = ends;
    }
}

/// Triangulate between two rows that end on the sides of a patch. `left`
/// and `right` are the stretches of those sides between the rows' end
/// points, running from the lower row up to the upper one.
fn stitch(
    lower: &[Corner],
    upper: &[Corner],
    left: &[Corner],
    right: &[Corner],
    tris: &mut Vec<[Corner; 3]>,
) {
    let mut emit = |a: Corner, b: Corner, c: Corner| {
        if a.src != b.src && b.src != c.src && a.src != c.src {
            tris.push([a, b, c]);
        }
    };
    // A side stretch is fanned from the nearest vertex off the side, so no
    // triangle has all three corners on it.
    let apex = lower[1];
    for w in left.windows(2) {
        emit(apex, w[1], w[0]);
    }
    let apex = upper[upper.len() - 2];
    for w in right.windows(2) {
        emit(apex, w[0], w[1]);
    }
    zipper(&lower[1..], &upper[..upper.len() - 1], tris);
}

/// Of two u-winding loops, the one that travels in +u has the face above it.
fn band_order(trim: &FaceTrim, winding: &[usize]) -> (usize, usize) {
    let (a, b) = (winding[0], winding[1]);
    if trim.loops[a].shift.x > 0.0 {
        (a, b)
    } else {
        (b, a)
    }
}

/// Bottom and top chains of a band between two u-winding loops, both ordered
/// by increasing `u` and covering one full period.
fn band_chains(
    fv: &mut FaceVerts,
    lower: &TrimLoop,
    lc: &[Corner],
    upper: &TrimLoop,
    uc: &[Corner],
) -> (Vec<Corner>, Vec<Corner>) {
    let mut bottom: Vec<Corner> = lc.to_vec();
    bottom.push(fv.shifted(lc[0], lower.shift));
    // Start the top chain at the vertex nearest in angle to the bottom start.
    let u0 = lc[0].uv.x;
    let j = (0..uc.len())
        .min_by(|&a, &b| {
            let da = (wrap_near(uc[a].uv.x, u0) - u0).abs();
            let db = (wrap_near(uc[b].uv.x, u0) - u0).abs();
            da.total_cmp(&db)
        })
        .unwrap();
    // The upper loop travels in -u; walk it once from `j`, then reverse.
    let delta = wrap_near(uc[j].uv.x, u0 + lower.shift.x) - uc[j].uv.x;
    let m = uc.len();
    let mut top = Vec::with_capacity(m + 1);
    for k in 0..=m {
        let i = (j + k) % m;
        let wrapped = if j + k >= m {
            upper.shift
        } else {
            v2(0.0, 0.0)
        };
        top.push(fv.shifted(uc[i], wrapped + v2(delta, 0.0)));
    }
    top.reverse();
    (bottom, top)
}

/// Split one loop into a bottom and a top chain when both are monotone in
/// `u` (the loop is counter-clockwise).
fn monotone_chains(cs: &[Corner]) -> Option<(Vec<Corner>, Vec<Corner>)> {
    let n = cs.len();
    if n < 3 {
        return None;
    }
    let key_lo = |c: &Corner| (c.uv.x, c.uv.y);
    let imin = (0..n).min_by(|&a, &b| {
        let (ka, kb) = (key_lo(&cs[a]), key_lo(&cs[b]));
        ka.0.total_cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
    })?;
    let imax = (0..n).max_by(|&a, &b| {
        let (ka, kb) = (key_lo(&cs[a]), key_lo(&cs[b]));
        ka.0.total_cmp(&kb.0).then(kb.1.total_cmp(&ka.1))
    })?;
    let walk = |from: usize, to: usize| {
        let mut out = vec![cs[from]];
        let mut i = from;
        while i != to {
            i = (i + 1) % n;
            out.push(cs[i]);
        }
        out
    };
    let bottom = walk(imin, imax);
    let mut top = walk(imax, imin);
    top.reverse();
    let rising = |chain: &[Corner]| chain.windows(2).all(|w| w[1].uv.x >= w[0].uv.x - 1.0e-12);
    (rising(&bottom) && rising(&top)).then_some((bottom, top))
}

/// Triangulate the strip between two chains ordered by increasing `u`.
fn zipper(bottom: &[Corner], top: &[Corner], tris: &mut Vec<[Corner; 3]>) {
    let (n, m) = (bottom.len() - 1, top.len() - 1);
    let (mut i, mut j) = (0, 0);
    let mut emit = |a: Corner, b: Corner, c: Corner| {
        if a.src != b.src && b.src != c.src && a.src != c.src {
            tris.push([a, b, c]);
        }
    };
    while i < n || j < m {
        let advance_bottom = if i == n {
            false
        } else if j == m {
            true
        } else {
            bottom[i + 1].uv.x <= top[j + 1].uv.x
        };
        if advance_bottom {
            emit(bottom[i], bottom[i + 1], top[j]);
            i += 1;
        } else {
            emit(bottom[i], top[j + 1], top[j]);
            j += 1;
        }
    }
}

struct Poly {
    outer: Vec<Corner>,
    holes: Vec<Vec<Corner>>,
}

fn contains(poly: &[Corner], q: V2) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i].uv, poly[(i + 1) % n].uv);
        if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// Turn the face's loops into simple parameter-space polygons with holes.
fn assemble(
    trim: &FaceTrim,
    corners: &[Vec<Corner>],
    winding: &[usize],
    plain: &[usize],
    fv: &mut FaceVerts,
    opts: &TessOptions,
    tris: &mut Vec<[Corner; 3]>,
) -> Vec<Poly> {
    let kind = trim.surf.kind;
    let mut polys: Vec<Poly> = Vec::new();
    let mut holes: Vec<Vec<Corner>> = Vec::new();
    for &i in plain {
        if trim.loops[i].area > 0.0 {
            polys.push(Poly {
                outer: corners[i].clone(),
                holes: Vec::new(),
            });
        } else {
            holes.push(corners[i].clone());
        }
    }

    // u-winding loops, sorted by height, pair up into bands.
    let mut wound: Vec<usize> = winding
        .iter()
        .copied()
        .filter(|&i| trim.loops[i].winds_u())
        .collect();
    let mean_v = |i: usize| {
        let l = &trim.loops[i];
        l.uv.iter().map(|q| q.y).sum::<f64>() / l.uv.len() as f64
    };
    wound.sort_by(|&a, &b| mean_v(a).total_cmp(&mean_v(b)));
    // Around a torus tube a band may run through the parameter seam: its
    // upper boundary then sorts first and belongs one period higher.
    let mut lift: Vec<f64> = vec![0.0; wound.len()];
    if kind == SurfKind::Torus {
        let leading = wound
            .iter()
            .take_while(|&&i| trim.loops[i].shift.x < 0.0)
            .count();
        if leading > 0 && leading < wound.len() {
            wound.rotate_left(leading);
            let n = wound.len();
            for slot in &mut lift[n - leading..] {
                *slot = TAU;
            }
        }
    }
    let raised = |fv: &mut FaceVerts, i: usize, dv: f64| -> Vec<Corner> {
        corners[i]
            .iter()
            .map(|c| fv.shifted(*c, v2(0.0, dv)))
            .collect()
    };
    let mut k = 0;
    while k < wound.len() {
        let i = wound[k];
        let up = trim.loops[i].shift.x > 0.0;
        if up && k + 1 < wound.len() && trim.loops[wound[k + 1]].shift.x < 0.0 {
            let j = wound[k + 1];
            let (lc, uc) = (raised(fv, i, lift[k]), raised(fv, j, lift[k + 1]));
            let outer = band(
                trim,
                opts,
                fv,
                (&lc, trim.loops[i].shift),
                (&uc, trim.loops[j].shift),
                &holes,
            );
            polys.push(Poly {
                outer,
                holes: Vec::new(),
            });
            k += 2;
            continue;
        }
        // An unpaired loop closes over a pole (sphere) or apex (cone).
        let limit = match (kind, up) {
            (SurfKind::Sphere, true) => Some(0.5 * PI),
            (SurfKind::Sphere, false) => Some(-0.5 * PI),
            (SurfKind::Cone, false) => Some(0.0),
            _ => None,
        };
        if let Some(v_end) = limit {
            let lp = &trim.loops[i];
            let cs = &corners[i];
            // The tip is one point with every angle, which no polygon in the
            // parameter plane can express. So stop short of it: a ring of
            // vertices between the tip and the nearest boundary stands in
            // for it, and a fan joins the ring to the tip. Up a cone the fan
            // runs along straight rulings and may be as long as it likes;
            // on a sphere it is one chord long at most.
            let nearest = corners
                .iter()
                .flatten()
                .map(|c| (c.uv.y - v_end).abs())
                .fold(f64::INFINITY, f64::min);
            let reach = (0.5 * nearest).min(v_step(trim, opts));
            let v_ring = v_end + reach * (cs[0].uv.y - v_end).signum();
            let steps = ((lp.shift.x.abs() / u_step(trim, opts, 1.0)).ceil() as usize).max(8);
            // The ring goes around the other way from the loop.
            let u_at = |k: usize| cs[0].uv.x + lp.shift.x * (1.0 - k as f64 / steps as f64);
            let ring: Vec<Corner> = (0..steps)
                .map(|k| {
                    let q = v2(u_at(k), v_ring);
                    Corner {
                        uv: q,
                        src: fv.push(trim.point(q), trim.normal(q)),
                    }
                })
                .collect();
            let tip = trim.point(v2(cs[0].uv.x, v_end));
            for k in 0..steps {
                let q = v2(0.5 * (u_at(k) + u_at(k + 1)), v_end);
                let apex = Corner {
                    uv: q,
                    src: fv.push(tip, singular_normal(trim, q)),
                };
                let next = if k + 1 < steps {
                    ring[k + 1]
                } else {
                    // One turn on, the ring is back at its first vertex.
                    Corner {
                        uv: v2(u_at(steps), v_ring),
                        src: fv.alias(ring[0].src),
                    }
                };
                tris.push([ring[k], apex, next]);
            }
            let back = v2(-lp.shift.x, -lp.shift.y);
            let outer = if up {
                band(trim, opts, fv, (cs, lp.shift), (&ring, back), &holes)
            } else {
                band(trim, opts, fv, (&ring, back), (cs, lp.shift), &holes)
            };
            polys.push(Poly {
                outer,
                holes: Vec::new(),
            });
        }
        k += 1;
    }

    // A closed surface with only holes (or no boundary at all) is the whole
    // parameter rectangle, cut along seams that avoid the holes.
    let closed = matches!(kind, SurfKind::Sphere | SurfKind::Torus);
    if closed && polys.is_empty() && winding.is_empty() {
        if let Some(outer) = full_rectangle(trim, &holes, fv, opts) {
            polys.push(Poly {
                outer,
                holes: Vec::new(),
            });
        }
    }

    // Each hole belongs to the polygon that contains it, in whichever period
    // image puts it there.
    let pu = trim.surf.u_periodic();
    let pv = trim.surf.v_periodic();
    for hole in holes {
        let probe = hole[0].uv;
        let mut placed = false;
        'search: for poly in &mut polys {
            for du in if pu { -2..=2 } else { 0..=0 } {
                for dv in if pv { -2..=2 } else { 0..=0 } {
                    let shift = v2(du as f64 * TAU, dv as f64 * TAU);
                    if contains(&poly.outer, probe + shift) {
                        poly.holes
                            .push(hole.iter().map(|c| fv.shifted(*c, shift)).collect());
                        placed = true;
                        break 'search;
                    }
                }
            }
        }
        let _ = placed;
    }
    polys
}

/// Corners along the straight cut from `a` to `b` in the parameter plane,
/// close enough together to follow the surface. The ends are not included.
fn cut_points(
    trim: &FaceTrim,
    opts: &TessOptions,
    fv: &mut FaceVerts,
    a: V2,
    b: V2,
) -> Vec<Corner> {
    let du = (b.x - a.x) / u_step(trim, opts, 1.0);
    let dv = (b.y - a.y) / v_step(trim, opts);
    let pieces = (du.hypot(dv).ceil() as usize).clamp(1, 4096);
    (1..pieces)
        .map(|i| {
            let q = a + (b - a) * (i as f64 / pieces as f64);
            Corner {
                uv: q,
                src: fv.push(trim.point(q), trim.normal(q)),
            }
        })
        .collect()
}

/// Parameter steps that count as one unit of length on a curved face: the
/// longest chords the tolerance allows along `u` and `v`. Along the rulings
/// of a cylinder or cone nothing bends, so there a unit is simply long; the
/// flag says when that is so.
fn units(trim: &FaceTrim, opts: &TessOptions) -> (V2, bool) {
    /// Triangles along a ruling are at least this many times as long as
    /// wide, and a face has about this many rows of them at most.
    const SLENDER: f64 = 12.0;
    const ROWS: f64 = 2.5;
    let u = u_step(trim, opts, 1.0);
    let across = v_step(trim, opts);
    if across.is_finite() {
        return (v2(u, across), false);
    }
    // The width of one step where the face is widest, in model units.
    let far = if trim.hi.y.abs() > trim.lo.y.abs() {
        trim.hi.y
    } else {
        trim.lo.y
    };
    let m = trim.metric(v2(0.5 * (trim.lo.x + trim.hi.x), far));
    let slender = SLENDER * u * m.x / m.y.max(1.0e-12);
    let v = slender.max((trim.hi.y - trim.lo.y) / ROWS).max(1.0e-9);
    (v2(u, v), true)
}

/// Whether the segment from `p` to `q` meets the one from `r` to `s`
/// anywhere but at an end they share.
fn meets(p: V2, q: V2, r: V2, s: V2) -> bool {
    let same = |a: V2, b: V2| (a - b).len() <= 1.0e-12 * (1.0 + a.len());
    if same(p, r) || same(p, s) || same(q, r) || same(q, s) {
        return false;
    }
    let slack = 1.0e-12 * (1.0 + p.len() + q.len() + r.len() + s.len());
    let apart =
        |a: f64, b: f64, c: f64, d: f64| a.max(b) < c.min(d) - slack || c.max(d) < a.min(b) - slack;
    if apart(p.x, q.x, r.x, s.x) || apart(p.y, q.y, r.y, s.y) {
        return false;
    }
    let size = (q - p).len() * (s - r).len();
    let eps = 1.0e-12 * size;
    let one_side = |a: f64, b: f64| (a > eps && b > eps) || (a < -eps && b < -eps);
    !one_side((q - p).cross(r - p), (q - p).cross(s - p))
        && !one_side((s - r).cross(p - r), (s - r).cross(q - r))
}

/// The outline of the band between two loops that go around the surface in
/// opposite directions: `lower` up `u` with the face above it, `upper` back
/// down `u` above that. Each comes with the step that closes it, a whole
/// turn. The band is cut open along a straight line from a corner of one to
/// a corner of the other, the shortest such line that crosses neither loop
/// nor any of `holes`.
fn band(
    trim: &FaceTrim,
    opts: &TessOptions,
    fv: &mut FaceVerts,
    lower: (&[Corner], V2),
    upper: (&[Corner], V2),
    holes: &[Vec<Corner>],
) -> Vec<Corner> {
    let (lower, turn) = lower;
    let (upper, back) = upper;
    let (n, m) = (lower.len(), upper.len());
    let (unit, _) = units(trim, opts);
    let apart = |a: V2, b: V2| ((a.x - b.x) / unit.x).hypot((a.y - b.y) / unit.y);
    // Whole turns that bring `q` nearest to `p`.
    let turns = |p: V2, q: V2| ((p - q).dot(turn) / turn.dot(turn)).round();

    // Everything a cut must not cross.
    let mut walls: Vec<[V2; 2]> = Vec::with_capacity(n + m);
    for i in 0..n {
        let next = if i + 1 < n {
            lower[i + 1].uv
        } else {
            lower[0].uv + turn
        };
        walls.push([lower[i].uv, next]);
    }
    for j in 0..m {
        let next = if j + 1 < m {
            upper[j + 1].uv
        } else {
            upper[0].uv + back
        };
        walls.push([upper[j].uv, next]);
    }
    for hole in holes {
        for k in 0..hole.len() {
            walls.push([hole[k].uv, hole[(k + 1) % hole.len()].uv]);
        }
    }
    // A torus repeats the other way as well, and a hole may be written
    // one turn of the tube away from the band it lies in.
    let tube: &[f64] = if trim.surf.kind == SurfKind::Torus {
        &[-TAU, 0.0, TAU]
    } else {
        &[0.0]
    };
    let blocked = |p: V2, q: V2| {
        walls.iter().any(|&[r, s]| {
            (-2..=2).any(|k| {
                tube.iter().any(|&lift| {
                    let by = turn * k as f64 + v2(0.0, lift);
                    meets(p, q, r + by, s + by)
                })
            })
        })
    };
    // For every corner of the lower loop, the upper corner closest to it
    // and the one most nearly straight above it; then the closest of those
    // pairs that can see each other. Where the upper loop dips, the closest
    // corner is the same for a long way round and something may stand in
    // front of it; straight up is a different line from every corner.
    let mut pairs: Vec<(f64, usize, usize)> = Vec::with_capacity(2 * n);
    for (i, corner) in lower.iter().enumerate() {
        let a = corner.uv;
        let image = |j: usize| upper[j].uv + turn * turns(a, upper[j].uv);
        let nearest = |measure: &dyn Fn(V2) -> f64| {
            (0..m)
                .min_by(|&x, &y| measure(image(x)).total_cmp(&measure(image(y))))
                .unwrap()
        };
        let closest = nearest(&|b| apart(a, b));
        let above = nearest(&|b| (b.x - a.x).abs());
        pairs.push((apart(a, image(closest)), i, closest));
        if above != closest {
            pairs.push((apart(a, image(above)), i, above));
        }
    }
    pairs.sort_by(|x, y| x.0.total_cmp(&y.0));
    let (_, i0, j0) = pairs
        .iter()
        .copied()
        .find(|&(_, i, j)| {
            let (a, b) = (lower[i].uv, upper[j].uv);
            !blocked(a, b + turn * turns(a, b))
        })
        .unwrap_or_else(|| {
            if crate::diag::trace() {
                eprintln!("tess: no clear cut across a band of {n} and {m} corners");
            }
            pairs[0]
        });
    let near_turns = turns(lower[i0].uv, upper[j0].uv);

    // Along the lower loop for one turn, up the far bank of the cut, back
    // along the upper loop, and down the near bank.
    let mut outer: Vec<Corner> = Vec::with_capacity(n + m + 8);
    for k in 0..=n {
        let i = (i0 + k) % n;
        let wraps = if i0 + k >= n { 1.0 } else { 0.0 };
        outer.push(fv.shifted(lower[i], turn * wraps));
    }
    let top: Vec<Corner> = (0..=m)
        .map(|k| {
            let j = (j0 + k) % m;
            // The upper loop unwinds what the lower one wound up.
            let wraps = if j0 + k >= m { 1.0 } else { 0.0 };
            fv.shifted(upper[j], turn * (near_turns + 1.0 - wraps))
        })
        .collect();
    let far = cut_points(trim, opts, fv, outer[n].uv, top[0].uv);
    let near: Vec<Corner> = far
        .iter()
        .map(|c| fv.shifted(*c, v2(-turn.x, -turn.y)))
        .collect();
    outer.extend(far);
    outer.extend(top);
    outer.extend(near.into_iter().rev());
    outer
}

/// Boundary of the full parameter rectangle of a sphere or torus, with seam
/// points sampled so both copies of each seam match.
fn full_rectangle(
    trim: &FaceTrim,
    holes: &[Vec<Corner>],
    fv: &mut FaceVerts,
    opts: &TessOptions,
) -> Option<Vec<Corner>> {
    let sphere = trim.surf.kind == SurfKind::Sphere;
    // Find seam positions that no hole straddles.
    let clear = |seam: f64, axis: usize| {
        holes.iter().all(|hole| {
            let vals: Vec<f64> = hole
                .iter()
                .map(|c| if axis == 0 { c.uv.x } else { c.uv.y })
                .collect();
            let lo = vals.iter().cloned().fold(f64::INFINITY, f64::min);
            let hi = vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            let start = wrap_from(lo, seam);
            start > seam + 1.0e-9 && start + (hi - lo) < seam + TAU - 1.0e-9
        })
    };
    let pick = |axis: usize| {
        (0..32)
            .map(|k| -PI + TAU * k as f64 / 32.0)
            .find(|&s| clear(s, axis))
    };
    let u0 = pick(0)?;
    let (v0, v1) = if sphere {
        (-0.5 * PI, 0.5 * PI)
    } else {
        let v0 = pick(1)?;
        (v0, v0 + TAU)
    };
    let nu = ((TAU / u_step(trim, opts, 1.0)).ceil() as usize).max(8);
    let nv = (((v1 - v0) / v_step(trim, opts)).ceil() as usize).max(4);
    let u_at = |i: usize| u0 + TAU * i as f64 / nu as f64;
    let v_at = |j: usize| v0 + (v1 - v0) * j as f64 / nv as f64;
    let fresh = |fv: &mut FaceVerts, q: V2| Corner {
        uv: q,
        src: fv.push(trim.point(q), trim.normal(q)),
    };
    // The same point seen from the other side of a seam: one vertex, so the
    // seam cannot open, and one name for the edge along it.
    let image = |fv: &mut FaceVerts, c: Corner, q: V2| Corner {
        uv: q,
        src: fv.alias(c.src),
    };
    let bottom: Vec<Corner> = (0..nu).map(|i| fresh(fv, v2(u_at(i), v0))).collect();
    let left: Vec<Corner> = (1..nv).map(|j| fresh(fv, v2(u0, v_at(j)))).collect();
    let top: Vec<Corner> = if sphere {
        (0..nu).map(|i| fresh(fv, v2(u_at(i), v1))).collect()
    } else {
        (0..nu)
            .map(|i| image(fv, bottom[i], v2(u_at(i), v1)))
            .collect()
    };
    let bottom_end = image(fv, bottom[0], v2(u0 + TAU, v0));
    let right: Vec<Corner> = (1..nv)
        .map(|j| image(fv, left[j - 1], v2(u0 + TAU, v_at(j))))
        .collect();
    let top_end = image(fv, top[0], v2(u0 + TAU, v1));
    // Counter-clockwise: along the bottom, up the right, back along the
    // top, down the left.
    let mut outer: Vec<Corner> = bottom.clone();
    outer.push(bottom_end);
    outer.extend(right.iter().copied());
    outer.push(top_end);
    outer.extend(top[1..].iter().rev().copied());
    outer.push(top[0]);
    outer.extend(left.iter().rev().copied());
    Some(outer)
}

/// Triangulate a polygon on a curved surface.
///
/// Where the surface bends, the polygon needs vertices inside as well as on
/// its outline. They come from a lattice of equilateral triangles and go one
/// by one into a Delaunay triangulation of the outline. Lengths are counted
/// in the longest chord the tolerance allows along each parameter: a
/// triangle that is round in those units is equally accurate every way, and
/// a lattice of side one is exactly fine enough.
///
/// Along the rulings of a cylinder or cone nothing bends. There a unit is
/// simply long, so the triangles come out as slivers along the rulings with
/// a few rows of lattice to hang on.
fn fill(
    trim: &FaceTrim,
    opts: &TessOptions,
    fv: &mut FaceVerts,
    poly: Poly,
    tris: &mut Vec<[Corner; 3]>,
) {
    /// Height of a row of the lattice.
    const ROW: f64 = 0.866_025_403_784_438_6;
    /// How far lattice points keep from the outline. Closer ones would make
    /// needles against it, flat enough for the surface's bend to tip over.
    const CLEAR: f64 = 0.35;
    /// Where the lattice starts within its first cell: off the lines of
    /// symmetry that outlines like to follow.
    const START: V2 = v2(0.29, 0.37);
    /// The longest side left alone, where the lattice stopped short of the
    /// outline and a triangle has to span the gap.
    const LONGEST: f64 = 1.4;

    let (step, ruled) = units(trim, opts);
    // How much of a side's length counts against the tolerance.
    let bends = if ruled { v2(1.0, 0.0) } else { v2(1.0, 1.0) };
    let scaled = |q: V2| v2(q.x / step.x, q.y / step.y);
    let mut sides: Vec<[V2; 2]> = Vec::new();
    for lp in std::iter::once(&poly.outer).chain(poly.holes.iter()) {
        for k in 0..lp.len() {
            sides.push([scaled(lp[k].uv), scaled(lp[(k + 1) % lp.len()].uv)]);
        }
    }

    let mut start: Vec<[Corner; 3]> = Vec::new();
    earclip(
        poly.outer,
        poly.holes,
        v2(1.0 / step.x, 1.0 / step.y),
        &mut start,
    );
    let mut corners: Vec<Corner> = Vec::new();
    let mut local: crate::fast::Map<u32, u32> = Default::default();
    let mut index = |c: Corner| {
        *local.entry(c.src).or_insert_with(|| {
            corners.push(c);
            corners.len() as u32 - 1
        })
    };
    let indexed: Vec<[u32; 3]> = start.iter().map(|t| t.map(&mut index)).collect();
    let points: Vec<V2> = corners.iter().map(|c| scaled(c.uv)).collect();
    let mut net = Net::new(points, indexed);

    let (mut lo, mut hi) = (sides[0][0], sides[0][0]);
    for side in &sides {
        lo = v2(lo.x.min(side[0].x), lo.y.min(side[0].y));
        hi = v2(hi.x.max(side[0].x), hi.y.max(side[0].y));
    }
    let rows = (((hi.y - lo.y) / ROW).ceil() as usize).min(8192);
    let columns = ((hi.x - lo.x).ceil() as usize + 1).min(8192);
    // The sides that pass near each row.
    let row_at = |y: f64| (y - lo.y) / ROW - START.y;
    let mut passing: Vec<Vec<u32>> = vec![Vec::new(); rows];
    for (s, side) in sides.iter().enumerate() {
        let (below, above) = (side[0].y.min(side[1].y), side[0].y.max(side[1].y));
        let first = row_at(below - CLEAR).ceil().max(0.0) as usize;
        let last = row_at(above + CLEAR).floor();
        if last < 0.0 {
            continue;
        }
        for row in passing.iter_mut().take(last as usize + 1).skip(first) {
            row.push(s as u32);
        }
    }
    let mut near = 0;
    let mut cuts: Vec<f64> = Vec::new();
    let mut taken = vec![false; columns];
    for (j, passing) in passing.iter().enumerate() {
        let y = lo.y + (j as f64 + START.y) * ROW;
        let x0 = lo.x + START.x + if j % 2 == 1 { 0.5 } else { 0.0 };
        // Lattice columns from `from` to `to`, as far as there are any.
        let span = |from: f64, to: f64| {
            let first = (from - x0).ceil().max(0.0) as usize;
            let last = (to - x0).floor();
            let end = if last < 0.0 {
                0
            } else {
                (last as usize + 1).min(columns)
            };
            first..end
        };
        cuts.clear();
        taken.iter_mut().for_each(|t| *t = false);
        for &s in passing {
            let [a, b] = sides[s as usize];
            if (a.y > y) != (b.y > y) {
                cuts.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
            }
            let d = b - a;
            for i in span(a.x.min(b.x) - CLEAR, a.x.max(b.x) + CLEAR) {
                let q = v2(x0 + i as f64, y);
                let along = ((q - a).dot(d) / d.dot(d).max(1.0e-300)).clamp(0.0, 1.0);
                if (q - (a + d * along)).len() < CLEAR {
                    taken[i] = true;
                }
            }
        }
        cuts.sort_by(f64::total_cmp);
        for inside in cuts.as_chunks::<2>().0 {
            for i in span(inside[0], inside[1]) {
                if taken[i] {
                    continue;
                }
                let q = v2(x0 + i as f64, y);
                if let Some(t) = net.insert(q, near) {
                    near = t;
                    let uv = v2(q.x * step.x, q.y * step.y);
                    corners.push(Corner {
                        uv,
                        src: fv.push(trim.point(uv), trim.normal(uv)),
                    });
                }
            }
        }
    }
    // Halve what is still too long, unless that would crowd the outline.
    for _ in 0..8 {
        let mut long: Vec<V2> = Vec::new();
        for (t, tri) in net.tris.iter().enumerate() {
            for k in 0..3 {
                let (a, b) = (tri[k], tri[(k + 1) % 3]);
                if a < b && !net.on_outline(t, k) {
                    let (pa, pb) = (net.points[a as usize], net.points[b as usize]);
                    let d = pb - pa;
                    if d.x * d.x * bends.x + d.y * d.y * bends.y > LONGEST * LONGEST {
                        long.push((pa + pb) * 0.5);
                    }
                }
            }
        }
        let mut added = false;
        for q in long {
            // On a ruled face a side that long cuts a corner off the
            // surface wherever it lies; a needle against the outline is
            // the lesser evil there, and nothing can tip it over.
            let crowded = !ruled
                && sides.iter().any(|&[a, b]| {
                    let d = b - a;
                    let along = ((q - a).dot(d) / d.dot(d).max(1.0e-300)).clamp(0.0, 1.0);
                    (q - (a + d * along)).len() < CLEAR
                });
            if crowded {
                continue;
            }
            if let Some(t) = net.insert(q, near) {
                near = t;
                added = true;
                let uv = v2(q.x * step.x, q.y * step.y);
                corners.push(Corner {
                    uv,
                    src: fv.push(trim.point(uv), trim.normal(uv)),
                });
            }
        }
        if !added {
            break;
        }
    }
    if crate::diag::trace() {
        eprintln!(
            "  fill: unit {step:?}, {} outline sides, {} corners, {} triangles",
            sides.len(),
            corners.len(),
            net.tris.len()
        );
    }
    if let Some(dir) = std::env::var_os("LIMO_CAD_KERNEL_PICTURES") {
        picture(std::path::Path::new(&dir), trim.surf.kind, &net, &sides);
    }
    tris.extend(net.tris.iter().map(|t| t.map(|i| corners[i as usize])));
}

/// `LIMO_CAD_KERNEL_PICTURES`: draw a triangulation and its outline as they
/// lie in the parameter plane, one numbered SVG file per face.
fn picture(dir: &std::path::Path, kind: SurfKind, net: &Net, sides: &[[V2; 2]]) {
    use std::fmt::Write as _;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let (mut lo, mut hi) = (net.points[0], net.points[0]);
    for q in &net.points {
        lo = v2(lo.x.min(q.x), lo.y.min(q.y));
        hi = v2(hi.x.max(q.x), hi.y.max(q.y));
    }
    // Stretched to a square: the shapes matter here, not the proportions.
    let size = 1500.0;
    let scale = v2(
        size / (hi.x - lo.x).max(1.0e-9),
        size / (hi.y - lo.y).max(1.0e-9),
    );
    let at = |q: V2| ((q.x - lo.x) * scale.x, size - (q.y - lo.y) * scale.y);
    let mut svg = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' viewBox='-10 -10 {0} {0}'>\
         <rect x='-10' y='-10' width='{0}' height='{0}' fill='white'/>",
        size + 20.0
    );
    for tri in &net.tris {
        let p = tri.map(|i| at(net.points[i as usize]));
        let _ = write!(
            svg,
            "<polygon points='{:.1},{:.1} {:.1},{:.1} {:.1},{:.1}' \
             fill='#cde' stroke='#369' stroke-width='0.6'/>",
            p[0].0, p[0].1, p[1].0, p[1].1, p[2].0, p[2].1
        );
    }
    for side in sides {
        let (a, b) = (at(side[0]), at(side[1]));
        let _ = write!(
            svg,
            "<line x1='{:.1}' y1='{:.1}' x2='{:.1}' y2='{:.1}' stroke='red' stroke-width='1.5'/>\
             <circle cx='{:.1}' cy='{:.1}' r='2.2' fill='red'/>",
            a.0, a.1, b.0, b.1, a.0, a.1
        );
    }
    svg.push_str("</svg>");
    let name = format!("face-{}-{kind:?}.svg", NEXT.fetch_add(1, Ordering::Relaxed));
    let _ = std::fs::write(dir.join(name), svg);
}

fn in_triangle(q: V2, a: V2, b: V2, c: V2, eps: f64) -> bool {
    let (d0, d1, d2) = (
        (b - a).cross(q - a),
        (c - b).cross(q - b),
        (a - c).cross(q - c),
    );
    (d0 >= -eps && d1 >= -eps && d2 >= -eps) || (d0 <= eps && d1 <= eps && d2 <= eps)
}

/// Ear-clip a counter-clockwise polygon with clockwise holes.
///
/// Ears are taken shortest diagonal first, measured with `metric` (model
/// length per unit of `u` and `v`). On a cylinder that makes `u` expensive
/// and `v` free, so the result is strips along the rulings rather than fans
/// across the curvature.
fn earclip(
    mut poly: Vec<Corner>,
    mut holes: Vec<Vec<Corner>>,
    metric: V2,
    out: &mut Vec<[Corner; 3]>,
) {
    let mut lo = v2(f64::INFINITY, f64::INFINITY);
    let mut hi = v2(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for c in &poly {
        lo = v2(lo.x.min(c.uv.x), lo.y.min(c.uv.y));
        hi = v2(hi.x.max(c.uv.x), hi.y.max(c.uv.y));
    }
    let scale = (hi - lo).len().max(1.0e-12);
    let eps = 1.0e-13 * scale * scale;

    // Bridge every hole into the outer polygon, topmost hole first, with a
    // bridge that runs along `v`.
    let max_y = |h: &Vec<Corner>| h.iter().map(|c| c.uv.y).fold(f64::NEG_INFINITY, f64::max);
    holes.sort_by(|a, b| max_y(b).total_cmp(&max_y(a)));
    for hole in holes {
        if hole.len() < 3 {
            continue;
        }
        let mi = (0..hole.len())
            .max_by(|&a, &b| hole[a].uv.y.total_cmp(&hole[b].uv.y))
            .unwrap();
        let m = hole[mi].uv;
        let n = poly.len();
        let mut best_y = f64::INFINITY;
        let mut cand: Option<usize> = None;
        for i in 0..n {
            let (a, b) = (poly[i].uv, poly[(i + 1) % n].uv);
            if (a.x > m.x) == (b.x > m.x) {
                continue;
            }
            let y = a.y + (m.x - a.x) / (b.x - a.x) * (b.y - a.y);
            if y >= m.y - 1.0e-12 * scale && y < best_y {
                best_y = y;
                cand = Some(if a.y > b.y { i } else { (i + 1) % n });
            }
        }
        let Some(mut p) = cand else {
            if crate::diag::trace() {
                eprintln!(
                    "earclip: no bridge for a hole of {} points at {m:?}",
                    hole.len()
                );
            }
            continue;
        };
        // Earlier bridges leave several corners at one point, each owning a
        // different sector of the interior; the bridge must leave through
        // the sector that faces the hole.
        let faces_hole = |poly: &[Corner], i: usize| -> bool {
            let n = poly.len();
            let (a, v, b) = (poly[(i + n - 1) % n].uv, poly[i].uv, poly[(i + 1) % n].uv);
            let left_of_in = (v - a).cross(m - v) >= 0.0;
            let left_of_out = (b - v).cross(m - v) >= 0.0;
            if (v - a).cross(b - v) > 0.0 {
                left_of_in && left_of_out
            } else {
                left_of_in || left_of_out
            }
        };
        if !faces_hole(&poly, p) {
            let at = poly[p].uv;
            if let Some(twin) =
                (0..n).find(|&i| (poly[i].uv - at).len() < 1.0e-14 * scale && faces_hole(&poly, i))
            {
                p = twin;
            }
        }
        // A reflex vertex inside the candidate triangle would block the
        // bridge; the one closest in angle to the ray is visible instead.
        let (ta, tb, tc) = (m, v2(m.x, best_y), poly[p].uv);
        let mut best_tan = f64::INFINITY;
        let mut best_d = f64::INFINITY;
        // A ray that lands on the corner itself sees it directly; the
        // triangle would be a sliver that everything on the ray falls into.
        let direct = (tb - tc).len() < 1.0e-9 * scale;
        for (i, corner) in poly.iter().enumerate().take(n) {
            let q = corner.uv;
            if direct || q.y < m.y || (q - tc).len() < 1.0e-14 * scale {
                continue;
            }
            if in_triangle(q, ta, tb, tc, eps) {
                let d = q - m;
                let tan = d.x.abs() / d.y.max(1.0e-300);
                if tan < best_tan - 1.0e-12
                    || ((tan - best_tan).abs() <= 1.0e-12 && d.len() < best_d)
                {
                    best_tan = tan;
                    best_d = d.len();
                    p = i;
                }
            }
        }
        // Whatever vertex the bridge ends at, use the copy of it whose
        // sector faces the hole.
        if !faces_hole(&poly, p) {
            let at = poly[p].uv;
            if let Some(twin) =
                (0..n).find(|&i| (poly[i].uv - at).len() < 1.0e-14 * scale && faces_hole(&poly, i))
            {
                p = twin;
            }
        }
        if crate::diag::trace() {
            let us: Vec<f64> = poly.iter().map(|c| c.uv.x).collect();
            let (lo, hi) = (
                us.iter().cloned().fold(f64::INFINITY, f64::min),
                us.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            );
            eprintln!(
                "earclip: bridge hole top {m:?} to {:?} (#{}), polygon u {lo:.3}..{hi:.3}, {} corners",
                poly[p].uv,
                poly[p].src,
                poly.len()
            );
        }
        let mut merged = Vec::with_capacity(poly.len() + hole.len() + 2);
        merged.extend_from_slice(&poly[..=p]);
        merged.extend_from_slice(&hole[mi..]);
        merged.extend_from_slice(&hole[..=mi]);
        merged.extend_from_slice(&poly[p..]);
        poly = merged;
    }

    let n = poly.len();
    if n < 3 {
        return;
    }
    let mut prev: Vec<usize> = (0..n).map(|i| (i + n - 1) % n).collect();
    let mut next: Vec<usize> = (0..n).map(|i| (i + 1) % n).collect();
    let mut alive = vec![true; n];
    let area2 =
        |a: usize, b: usize, c: usize| (poly[b].uv - poly[a].uv).cross(poly[c].uv - poly[a].uv);
    let same = |a: V2, b: V2| (a - b).len() <= 1.0e-14 * scale;
    // Length of the diagonal an ear at `i` would add.
    let cost = |prev: &[usize], next: &[usize], i: usize| -> f64 {
        let d = poly[next[i]].uv - poly[prev[i]].uv;
        (d.x * metric.x).hypot(d.y * metric.y)
    };
    let mut costs: Vec<f64> = (0..n).map(|i| cost(&prev, &next, i)).collect();
    // Ears found blocked stay skipped until the polygon changes near them.
    let mut blocked = vec![false; n];
    let mut remaining = n;
    while remaining > 3 {
        let mut pick: Option<usize> = None;
        for i in 0..n {
            if !alive[i] || blocked[i] || area2(prev[i], i, next[i]) <= eps {
                continue;
            }
            if pick.is_none_or(|p| costs[i] < costs[p]) {
                pick = Some(i);
            }
        }
        let Some(b) = pick else {
            if blocked.iter().any(|x| *x) {
                // Nothing convex is free: the remainder is degenerate. Drop
                // its flattest corner and carry on.
                blocked.iter_mut().for_each(|x| *x = false);
            }
            // Prefer a corner that covers nothing; failing that the cheapest
            // convex one, never a reflex corner (its triangle lies outside).
            let mut worst = usize::MAX;
            let mut flattest = f64::INFINITY;
            for i in 0..n {
                if alive[i] {
                    let signed = area2(prev[i], i, next[i]);
                    let rank = if signed.abs() <= eps {
                        0.0
                    } else if signed > 0.0 {
                        1.0 + costs[i]
                    } else {
                        f64::MAX
                    };
                    if rank < flattest {
                        flattest = rank;
                        worst = i;
                    }
                }
            }
            let flattest = area2(prev[worst], worst, next[worst]);
            let (a, c) = (prev[worst], next[worst]);
            if crate::diag::trace() {
                eprintln!(
                    "earclip: stuck with {remaining} corners left, dropping one of area {}",
                    area2(a, worst, c)
                );
                let mut i = worst;
                let mut text = Vec::new();
                for _ in 0..remaining.min(40) {
                    text.push(format!(
                        "({:.3},{:.3})#{}",
                        poly[i].uv.x, poly[i].uv.y, poly[i].src
                    ));
                    i = next[i];
                }
                eprintln!("   ring: {}", text.join(" "));
            }
            if flattest > eps {
                out.push([poly[a], poly[worst], poly[c]]);
            }
            alive[worst] = false;
            next[a] = c;
            prev[c] = a;
            remaining -= 1;
            costs[a] = cost(&prev, &next, a);
            costs[c] = cost(&prev, &next, c);
            continue;
        };
        let (a, c) = (prev[b], next[b]);
        let (pa, pb, pc) = (poly[a].uv, poly[b].uv, poly[c].uv);
        let mut clear = true;
        let mut j = next[c];
        while j != a {
            let q = poly[j].uv;
            if !same(q, pa)
                && !same(q, pb)
                && !same(q, pc)
                && area2(prev[j], j, next[j]) <= eps
                && in_triangle(q, pa, pb, pc, eps)
            {
                clear = false;
                break;
            }
            j = next[j];
        }
        if !clear {
            blocked[b] = true;
            continue;
        }
        out.push([poly[a], poly[b], poly[c]]);
        alive[b] = false;
        next[a] = c;
        prev[c] = a;
        remaining -= 1;
        costs[a] = cost(&prev, &next, a);
        costs[c] = cost(&prev, &next, c);
        // Removing a corner can unblock any ear it was inside.
        blocked.iter_mut().for_each(|x| *x = false);
    }
    if let Some(i) = (0..n).find(|&i| alive[i]) {
        let (a, b, c) = (prev[i], i, next[i]);
        if area2(a, b, c).abs() > eps {
            out.push([poly[a], poly[b], poly[c]]);
        }
    }
}
