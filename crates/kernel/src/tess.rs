//! Display and export tessellation.
//!
//! Boundary vertices are the shared edge samples, so meshes of neighbouring
//! faces meet exactly. Planar faces are ear-clipped; cylinders and cones use
//! a strip mesher between their two boundary chains; everything else is
//! ear-clipped in parameter space and refined until every interior edge meets
//! the chord and angle limits. Normals are analytic, so shading is smooth
//! regardless of the triangle layout.

use std::collections::{HashMap, HashSet};

use crate::geom::SurfKind;
use crate::math::*;
use crate::topo::Body;
use crate::trim::{EdgeMesh, FaceTrim, TrimLoop};

#[derive(Clone, Copy, Debug)]
pub struct TessOptions {
    /// Maximum chord deviation (model units).
    pub linear: f64,
    /// Maximum turn of the normal across one segment (radians).
    pub angular: f64,
}

impl Default for TessOptions {
    fn default() -> Self {
        // Matches the viewport policy of the reference kernel adapter.
        TessOptions {
            linear: 0.15,
            angular: 0.35,
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
        for t in self.indices.chunks_exact(3) {
            let (a, b, c) = (
                self.positions[t[0] as usize],
                self.positions[t[1] as usize],
                self.positions[t[2] as usize],
            );
            six += a.dot(b.cross(c));
        }
        six / 6.0
    }

    pub fn area(&self) -> f64 {
        let mut twice = 0.0;
        for t in self.indices.chunks_exact(3) {
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
    let edges = EdgeMesh::build(body, opts.linear, TAU / opts.angular);
    let mut mesh = Mesh::default();
    for f in 0..body.face_count() as u32 {
        let trim = FaceTrim::build(body, &edges, f, opts.linear);
        let first = mesh.indices.len() as u32;
        tess_face(&trim, opts, &mut mesh);
        mesh.faces
            .push([first, mesh.indices.len() as u32 - first]);
    }
    mesh
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
}

impl FaceVerts {
    fn push(&mut self, p: V3, n: V3) -> u32 {
        self.p.push(p);
        self.n.push(n);
        (self.p.len() - 1) as u32
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
    };
    // Face-local vertices: every loop point except the closing duplicate.
    let mut corners: Vec<Vec<Corner>> = Vec::with_capacity(trim.loops.len());
    let mut boundary: HashSet<(u32, u32)> = HashSet::new();
    for lp in &trim.loops {
        let n = lp.uv.len() - 1;
        let mut cs = Vec::with_capacity(n);
        for i in 0..n {
            let src = fv.push(lp.p3[i], singular_normal(trim, lp.uv[i]));
            cs.push(Corner { uv: lp.uv[i], src });
        }
        for i in 0..n {
            let (a, b) = (cs[i].src, cs[(i + 1) % n].src);
            boundary.insert((a.min(b), a.max(b)));
        }
        corners.push(cs);
    }

    let mut tris: Vec<[Corner; 3]> = Vec::new();
    let kind = trim.surf.kind;
    let mut curved = kind != SurfKind::Plane;

    let winding: Vec<usize> = (0..trim.loops.len())
        .filter(|&i| trim.loops[i].winds())
        .collect();
    let plain: Vec<usize> = (0..trim.loops.len())
        .filter(|&i| !trim.loops[i].winds())
        .collect();

    let ruled = matches!(kind, SurfKind::Cylinder | SurfKind::Cone);
    if kind == SurfKind::Cone
        && trim.loops.len() == 1
        && trim.loops[0].winds_u()
        && trim.loops[0].shift.x < 0.0
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
            let b_uv = if i + 1 == n {
                b.uv + trim.loops[0].shift
            } else {
                b.uv
            };
            tris.push([
                a,
                Corner { uv: b_uv, src: b.src },
                Corner { uv: tip_uv, src: tip },
            ]);
        }
        curved = false;
    } else if ruled && plain.is_empty() && winding.len() == 2 {
        let (lower, upper) = band_order(trim, &winding);
        let (b, t) = band_chains(&trim.loops[lower], &corners[lower], &trim.loops[upper], &corners[upper]);
        zipper(&b, &t, &mut tris);
        curved = false;
    } else if ruled && winding.is_empty() && plain.len() == 1 && monotone_chains(&corners[plain[0]]).is_some() {
        let (b, t) = monotone_chains(&corners[plain[0]]).unwrap();
        zipper(&b, &t, &mut tris);
        curved = false;
    } else {
        for poly in assemble(trim, &corners, &winding, &plain, &mut fv, opts) {
            earclip(poly.outer, poly.holes, &mut tris);
        }
    }

    if curved {
        refine(&mut tris, &mut fv, trim, opts, &boundary);
    }

    let base = mesh.positions.len() as u32;
    mesh.positions.extend_from_slice(&fv.p);
    mesh.normals.extend_from_slice(&fv.n);
    for t in &tris {
        let (a, b, c) = (
            fv.p[t[0].src as usize],
            fv.p[t[1].src as usize],
            fv.p[t[2].src as usize],
        );
        if (b - a).cross(c - a).len2() <= 1.0e-24 {
            continue;
        }
        mesh.indices
            .extend([base + t[0].src, base + t[1].src, base + t[2].src]);
    }
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
    lower: &TrimLoop,
    lc: &[Corner],
    upper: &TrimLoop,
    uc: &[Corner],
) -> (Vec<Corner>, Vec<Corner>) {
    let mut bottom: Vec<Corner> = lc.to_vec();
    bottom.push(Corner {
        uv: lc[0].uv + lower.shift,
        src: lc[0].src,
    });
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
        let wrapped = if j + k >= m { upper.shift } else { v2(0.0, 0.0) };
        top.push(Corner {
            uv: uc[i].uv + wrapped + v2(delta, 0.0),
            src: uc[i].src,
        });
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
    let mut k = 0;
    while k < wound.len() {
        let i = wound[k];
        let up = trim.loops[i].shift.x > 0.0;
        if up && k + 1 < wound.len() && trim.loops[wound[k + 1]].shift.x < 0.0 {
            let j = wound[k + 1];
            let (bottom, top) =
                band_chains(&trim.loops[i], &corners[i], &trim.loops[j], &corners[j]);
            let mut outer = bottom;
            outer.extend(top.into_iter().rev());
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
            let mut outer: Vec<Corner> = cs.clone();
            outer.push(Corner {
                uv: cs[0].uv + lp.shift,
                src: cs[0].src,
            });
            let tip = trim.point(v2(cs[0].uv.x, v_end));
            let far = fv.push(tip, singular_normal(trim, v2(cs[0].uv.x + lp.shift.x, v_end)));
            let near = fv.push(tip, singular_normal(trim, v2(cs[0].uv.x, v_end)));
            outer.push(Corner {
                uv: v2(cs[0].uv.x + lp.shift.x, v_end),
                src: far,
            });
            outer.push(Corner {
                uv: v2(cs[0].uv.x, v_end),
                src: near,
            });
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
                        poly.holes.push(
                            hole.iter()
                                .map(|c| Corner {
                                    uv: c.uv + shift,
                                    src: c.src,
                                })
                                .collect(),
                        );
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
    let nu = ((TAU / opts.angular).ceil() as usize).max(8);
    let nv = (((v1 - v0) / opts.angular).ceil() as usize).max(4);
    let mut outer = Vec::new();
    let mut add = |fv: &mut FaceVerts, q: V2| {
        let src = fv.push(trim.point(q), trim.normal(q));
        outer.push(Corner { uv: q, src });
    };
    for i in 0..nu {
        add(fv, v2(u0 + TAU * i as f64 / nu as f64, v0));
    }
    for j in 0..nv {
        add(fv, v2(u0 + TAU, v0 + (v1 - v0) * j as f64 / nv as f64));
    }
    for i in 0..nu {
        add(fv, v2(u0 + TAU - TAU * i as f64 / nu as f64, v1));
    }
    for j in 0..nv {
        add(fv, v2(u0, v1 - (v1 - v0) * j as f64 / nv as f64));
    }
    Some(outer)
}

fn in_triangle(q: V2, a: V2, b: V2, c: V2, eps: f64) -> bool {
    let (d0, d1, d2) = ((b - a).cross(q - a), (c - b).cross(q - b), (a - c).cross(q - c));
    (d0 >= -eps && d1 >= -eps && d2 >= -eps) || (d0 <= eps && d1 <= eps && d2 <= eps)
}

/// Ear-clip a counter-clockwise polygon with clockwise holes.
fn earclip(mut poly: Vec<Corner>, mut holes: Vec<Vec<Corner>>, out: &mut Vec<[Corner; 3]>) {
    let mut lo = v2(f64::INFINITY, f64::INFINITY);
    let mut hi = v2(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for c in &poly {
        lo = v2(lo.x.min(c.uv.x), lo.y.min(c.uv.y));
        hi = v2(hi.x.max(c.uv.x), hi.y.max(c.uv.y));
    }
    let scale = (hi - lo).len().max(1.0e-12);
    let eps = 1.0e-13 * scale * scale;

    // Bridge every hole into the outer polygon, rightmost hole first.
    let max_x = |h: &Vec<Corner>| h.iter().map(|c| c.uv.x).fold(f64::NEG_INFINITY, f64::max);
    holes.sort_by(|a, b| max_x(b).total_cmp(&max_x(a)));
    for hole in holes {
        if hole.len() < 3 {
            continue;
        }
        let mi = (0..hole.len())
            .max_by(|&a, &b| hole[a].uv.x.total_cmp(&hole[b].uv.x))
            .unwrap();
        let m = hole[mi].uv;
        let n = poly.len();
        let mut best_x = f64::INFINITY;
        let mut cand: Option<usize> = None;
        for i in 0..n {
            let (a, b) = (poly[i].uv, poly[(i + 1) % n].uv);
            if (a.y > m.y) == (b.y > m.y) {
                continue;
            }
            let x = a.x + (m.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if x >= m.x - 1.0e-12 * scale && x < best_x {
                best_x = x;
                cand = Some(if a.x > b.x { i } else { (i + 1) % n });
            }
        }
        let Some(mut p) = cand else {
            continue;
        };
        // A reflex vertex inside the candidate triangle would block the
        // bridge; the one closest in angle to the ray is visible instead.
        let (ta, tb, tc) = (m, v2(best_x, m.y), poly[p].uv);
        let mut best_tan = f64::INFINITY;
        let mut best_d = f64::INFINITY;
        for i in 0..n {
            let q = poly[i].uv;
            if q.x < m.x || (q - tc).len() < 1.0e-14 * scale {
                continue;
            }
            if in_triangle(q, ta, tb, tc, eps) {
                let d = q - m;
                let tan = d.y.abs() / d.x.max(1.0e-300);
                if tan < best_tan - 1.0e-12 || ((tan - best_tan).abs() <= 1.0e-12 && d.len() < best_d)
                {
                    best_tan = tan;
                    best_d = d.len();
                    p = i;
                }
            }
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
    let area2 = |a: usize, b: usize, c: usize| (poly[b].uv - poly[a].uv).cross(poly[c].uv - poly[a].uv);
    let same = |a: V2, b: V2| (a - b).len() <= 1.0e-14 * scale;
    let mut remaining = n;
    let mut i = 0;
    while remaining > 3 {
        let mut clipped = false;
        let mut cursor = i;
        for _ in 0..remaining {
            let (a, b, c) = (prev[cursor], cursor, next[cursor]);
            if area2(a, b, c) > eps {
                let (pa, pb, pc) = (poly[a].uv, poly[b].uv, poly[c].uv);
                let mut blocked = false;
                let mut j = next[c];
                while j != a {
                    let q = poly[j].uv;
                    if !same(q, pa)
                        && !same(q, pb)
                        && !same(q, pc)
                        && area2(prev[j], j, next[j]) <= eps
                        && in_triangle(q, pa, pb, pc, eps)
                    {
                        blocked = true;
                        break;
                    }
                    j = next[j];
                }
                if !blocked {
                    out.push([poly[a], poly[b], poly[c]]);
                    next[a] = c;
                    prev[c] = a;
                    remaining -= 1;
                    i = c;
                    clipped = true;
                    break;
                }
            }
            cursor = next[cursor];
        }
        if !clipped {
            // Degenerate remainder: drop the flattest corner and carry on.
            let mut worst = i;
            let mut flattest = f64::INFINITY;
            let mut cursor = i;
            for _ in 0..remaining {
                let a = area2(prev[cursor], cursor, next[cursor]).abs();
                if a < flattest {
                    flattest = a;
                    worst = cursor;
                }
                cursor = next[cursor];
            }
            let (a, c) = (prev[worst], next[worst]);
            if flattest > eps {
                out.push([poly[a], poly[worst], poly[c]]);
            }
            next[a] = c;
            prev[c] = a;
            remaining -= 1;
            i = c;
        }
    }
    let (a, b, c) = (prev[i], i, next[i]);
    if area2(a, b, c).abs() > eps {
        out.push([poly[a], poly[b], poly[c]]);
    }
}

/// Split interior edges until each meets the chord and angle limits.
fn refine(
    tris: &mut Vec<[Corner; 3]>,
    fv: &mut FaceVerts,
    trim: &FaceTrim,
    opts: &TessOptions,
    boundary: &HashSet<(u32, u32)>,
) {
    let cos_limit = opts.angular.cos();
    for _ in 0..12 {
        let mut decided: HashMap<(u32, u32), Option<u32>> = HashMap::new();
        let mut any = false;
        for t in tris.iter() {
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                let key = (a.src.min(b.src), a.src.max(b.src));
                if a.src == b.src || boundary.contains(&key) || decided.contains_key(&key) {
                    continue;
                }
                let (pa, pb) = (fv.p[a.src as usize], fv.p[b.src as usize]);
                let mid = (a.uv + b.uv) * 0.5;
                let pm = trim.point(mid);
                let bend = fv.n[a.src as usize].dot(fv.n[b.src as usize]) < cos_limit;
                let chord = pm.dist((pa + pb) * 0.5) > opts.linear;
                let split = (bend || chord) && pa.dist(pb) > 1.0e-6;
                let entry = if split {
                    any = true;
                    Some(fv.push(pm, trim.normal(mid)))
                } else {
                    None
                };
                decided.insert(key, entry);
            }
        }
        if !any {
            break;
        }
        let mut next: Vec<[Corner; 3]> = Vec::with_capacity(tris.len() * 2);
        for t in tris.iter() {
            let mid = |a: Corner, b: Corner| -> Option<Corner> {
                let key = (a.src.min(b.src), a.src.max(b.src));
                decided.get(&key).copied().flatten().map(|src| Corner {
                    uv: (a.uv + b.uv) * 0.5,
                    src,
                })
            };
            let (a, b, c) = (t[0], t[1], t[2]);
            match (mid(a, b), mid(b, c), mid(c, a)) {
                (None, None, None) => next.push(*t),
                (Some(ab), Some(bc), Some(ca)) => {
                    next.extend([[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]]);
                }
                (Some(ab), None, None) => next.extend([[a, ab, c], [ab, b, c]]),
                (None, Some(bc), None) => next.extend([[b, bc, a], [bc, c, a]]),
                (None, None, Some(ca)) => next.extend([[c, ca, b], [ca, a, b]]),
                (Some(ab), Some(bc), None) => next.extend([[ab, b, bc], [a, ab, bc], [a, bc, c]]),
                (None, Some(bc), Some(ca)) => next.extend([[bc, c, ca], [b, bc, ca], [b, ca, a]]),
                (Some(ab), None, Some(ca)) => next.extend([[ca, a, ab], [c, ca, ab], [c, ab, b]]),
            }
        }
        *tris = next;
    }
}
