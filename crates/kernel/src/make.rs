//! Direct construction of solids. No Booleans are involved: a profile with
//! holes becomes one solid in a single pass.

use crate::geom::{Curve, Surface};
use crate::math::*;
use crate::topo::Body;

/// One boundary segment of a planar profile, in model space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Line {
        a: V3,
        b: V3,
    },
    /// Circular arc from `a` through `m` to `b`.
    Arc {
        a: V3,
        m: V3,
        b: V3,
    },
    /// Full circle about `c` through `p`, counter-clockwise about `n`.
    Circle {
        c: V3,
        p: V3,
        n: V3,
    },
}

impl Seg {
    fn ends(&self) -> (V3, V3) {
        match *self {
            Seg::Line { a, b } | Seg::Arc { a, b, .. } => (a, b),
            Seg::Circle { p, .. } => (p, p),
        }
    }
}

/// A planar region: `loops[0]` is the outer boundary, the rest are holes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Region {
    pub loops: Vec<Vec<Seg>>,
}

/// Default X axis that Open CASCADE derives from a plane normal (`gp_Ax3`).
/// Reproducing it keeps face-local sketch and hole coordinates of existing
/// projects valid on this kernel.
pub fn reference_x(n: V3) -> V3 {
    let (a, b, c) = (n.x, n.y, n.z);
    let (aa, ba, ca) = (a.abs(), b.abs(), c.abs());
    let d = if ba <= aa && ba <= ca {
        if aa > ca {
            v3(-c, 0.0, a)
        } else {
            v3(c, 0.0, -a)
        }
    } else if aa <= ba && aa <= ca {
        if ba > ca {
            v3(0.0, -c, b)
        } else {
            v3(0.0, c, -b)
        }
    } else if aa > ba {
        v3(-b, a, 0.0)
    } else {
        v3(b, -a, 0.0)
    };
    d.unit()
}

/// Centre, radius and rotation axis (the sense of `a → m → b`) of an arc.
pub fn arc_circle(a: V3, m: V3, b: V3) -> Option<(V3, f64, V3)> {
    let (u, v) = (m - a, b - m);
    let w = u.cross(v);
    let w2 = w.len2();
    if w2 < 1.0e-24 {
        return None;
    }
    // Circumcentre of the triangle (a, m, b).
    let (ab, ac) = (m - a, b - a);
    let n = ab.cross(ac);
    let n2 = n.len2();
    if n2 < 1.0e-24 {
        return None;
    }
    let centre = a + (n.cross(ab) * ac.len2() + ac.cross(n) * ab.len2()) / (2.0 * n2);
    let radius = (centre.dist(a) + centre.dist(m) + centre.dist(b)) / 3.0;
    Some((centre, radius, w.unit()))
}

/// A profile segment placed in its loop, possibly traversed backwards.
#[derive(Clone, Copy)]
struct Placed {
    seg: Seg,
    reversed: bool,
}

impl Placed {
    fn start(&self) -> V3 {
        let (a, b) = self.seg.ends();
        if self.reversed {
            b
        } else {
            a
        }
    }
    fn end(&self) -> V3 {
        let (a, b) = self.seg.ends();
        if self.reversed {
            a
        } else {
            b
        }
    }
}

const CHAIN_TOL: f64 = 1.0e-5;

/// Order and orient the segments head to tail.
fn chain(segs: &[Seg]) -> Result<Vec<Placed>, String> {
    if segs.is_empty() {
        return Err("profile loop has no boundary curves".into());
    }
    if segs.len() == 1 {
        return match segs[0] {
            Seg::Circle { .. } => Ok(vec![Placed {
                seg: segs[0],
                reversed: false,
            }]),
            _ => Err("profile loop is not closed".into()),
        };
    }
    let mut out = Vec::with_capacity(segs.len());
    let mut first = Placed {
        seg: segs[0],
        reversed: false,
    };
    let (sa, sb) = segs[1].ends();
    let touches = |p: V3| p.dist(sa) < CHAIN_TOL || p.dist(sb) < CHAIN_TOL;
    if !touches(first.end()) && touches(first.start()) {
        first.reversed = true;
    }
    out.push(first);
    for seg in &segs[1..] {
        if matches!(seg, Seg::Circle { .. }) {
            return Err("a full circle cannot share a profile loop with other curves".into());
        }
        let at = out.last().unwrap().end();
        let (a, b) = seg.ends();
        let reversed = if at.dist(a) < CHAIN_TOL {
            false
        } else if at.dist(b) < CHAIN_TOL {
            true
        } else {
            return Err("profile loop is not connected end to end".into());
        };
        out.push(Placed {
            seg: *seg,
            reversed,
        });
    }
    if out.last().unwrap().end().dist(out[0].start()) >= CHAIN_TOL {
        return Err("profile loop is not closed".into());
    }
    Ok(out)
}

/// Twice the signed area of the loop about `n`.
fn signed_area2(lp: &[Placed], n: V3) -> f64 {
    let mut pts: Vec<V3> = Vec::new();
    for placed in lp {
        match placed.seg {
            Seg::Line { .. } => pts.push(placed.start()),
            Seg::Arc { a, m, b } => {
                let Some((c, r, axis)) = arc_circle(a, m, b) else {
                    pts.push(placed.start());
                    continue;
                };
                let frame = Frame::from_zx(c, axis, a - c);
                let lb = frame.local(b);
                let sweep = wrap_from(lb.y.atan2(lb.x), 1.0e-12);
                let k = 12;
                let sample = |i: usize| {
                    let t = sweep * i as f64 / k as f64;
                    c + (frame.x * t.cos() + frame.y * t.sin()) * r
                };
                if placed.reversed {
                    pts.extend((1..=k).rev().map(sample));
                } else {
                    pts.extend((0..k).map(sample));
                }
            }
            Seg::Circle { n: cn, .. } => {
                // A circle runs counter-clockwise about its own normal.
                return cn.dot(n).signum();
            }
        }
    }
    let o = pts[0];
    let mut sum = V3::ZERO;
    for i in 1..pts.len() - 1 {
        sum += (pts[i] - o).cross(pts[i + 1] - o);
    }
    sum.dot(n)
}

fn reverse_loop(lp: &mut [Placed]) {
    lp.reverse();
    for p in lp.iter_mut() {
        match &mut p.seg {
            Seg::Circle { n, .. } => *n = -*n,
            _ => p.reversed = !p.reversed,
        }
    }
}

/// Perimeter-weighted centre that Open CASCADE uses as the origin of a plane
/// fitted through a wire (`BRepLib_FindSurface`).
fn wire_barycentre(lp: &[Placed]) -> V3 {
    let mut sum = V3::ZERO;
    let mut weight = 0.0;
    for placed in lp {
        match placed.seg {
            Seg::Line { a, b } => {
                let l = a.dist(b);
                sum += (a + b) * l;
                weight += 2.0 * l;
            }
            Seg::Arc { a, m, b } => {
                let Some((c, r, axis)) = arc_circle(a, m, b) else {
                    continue;
                };
                let frame = Frame::from_zx(c, axis, a - c);
                let lb = frame.local(b);
                let sweep = wrap_from(lb.y.atan2(lb.x), 1.0e-12);
                let at = |k: f64| {
                    let t = sweep * k / 3.0;
                    c + (frame.x * t.cos() + frame.y * t.sin()) * r
                };
                let pts = [at(0.0), at(1.0), at(2.0), at(3.0)];
                let d = [
                    pts[0].dist(pts[1]),
                    pts[1].dist(pts[2]),
                    pts[2].dist(pts[3]),
                ];
                for (p, w) in pts.iter().zip([d[0], d[0] + d[1], d[1] + d[2], d[2]]) {
                    sum += *p * w;
                    weight += w;
                }
            }
            Seg::Circle { c, .. } => return c,
        }
    }
    if weight > 0.0 {
        sum / weight
    } else {
        lp[0].start()
    }
}

/// Extrude a planar region along `normal` from signed offset `start` to
/// `end` (both measured from the profile plane).
pub fn extrude(region: &Region, normal: V3, start: f64, end: f64) -> Result<Body, String> {
    let n = normal.unit();
    if !n.is_finite() || n.len2() < 0.5 {
        return Err("extrude direction is degenerate".into());
    }
    if !(start.is_finite() && end.is_finite()) || (end - start).abs() <= TOL {
        return Err("extrude extent must have non-zero depth".into());
    }
    if region.loops.is_empty() {
        return Err("extrude needs at least one profile loop".into());
    }
    let (lo, hi) = (start.min(end), start.max(end));
    let height = hi - lo;

    let mut loops = Vec::with_capacity(region.loops.len());
    for segs in &region.loops {
        loops.push(chain(segs)?);
    }
    // Orientation of the outer wire as given decides the cap plane normal the
    // reference kernel would have chosen.
    let given_area = signed_area2(&loops[0], n);
    if given_area.abs() < 1.0e-18 {
        return Err("profile encloses no area".into());
    }
    let cap_normal = n * given_area.signum();
    let barycentre = wire_barycentre(&loops[0]);
    for (i, lp) in loops.iter_mut().enumerate() {
        let area = signed_area2(lp, n);
        if area.abs() < 1.0e-18 {
            return Err("profile loop encloses no area".into());
        }
        // Material on the left: outer counter-clockwise, holes clockwise.
        if (area > 0.0) != (i == 0) {
            reverse_loop(lp);
        }
    }

    let mut body = Body::new();
    let cap_x = reference_x(cap_normal);
    let cap_frame = |offset: f64| Frame {
        o: barycentre + n * offset,
        x: cap_x,
        y: cap_normal.cross(cap_x),
        z: cap_normal,
    };
    let bottom_surf = body.add_surf(Surface::plane_frame(cap_frame(lo)));
    let top_surf = body.add_surf(Surface::plane_frame(cap_frame(hi)));
    let cap_up = cap_normal.dot(n) > 0.0;

    let mut bottom_loops: Vec<Vec<u32>> = Vec::new();
    let mut top_loops: Vec<Vec<u32>> = Vec::new();
    // Side surfaces follow the reference kernel: frame at the swept edge's
    // start on the `start` plane, axis against the sweep direction.
    let sweep_dir = n * (end - start).signum();
    let base = n * start;

    struct Side {
        surf: u32,
        sense: bool,
        loops: Vec<Vec<u32>>,
    }
    let mut sides: Vec<Side> = Vec::new();

    for lp in &loops {
        if let [Placed {
            seg: Seg::Circle { c, p, n: cn },
            ..
        }] = lp[..]
        {
            let r = c.dist(p);
            if r <= TOL {
                return Err("profile circle has zero radius".into());
            }
            let axis = cn.unit();
            let (v0, v1) = (body.add_vert(p + n * lo), body.add_vert(p + n * hi));
            let c0 = body.add_curve(Curve::circle(c + n * lo, axis, p - c, r));
            let c1 = body.add_curve(Curve::circle(c + n * hi, axis, p - c, r));
            let e0 = body.add_edge(c0, 0.0, TAU, v0, v0);
            let e1 = body.add_edge(c1, 0.0, TAU, v1, v1);
            let surf = body.add_surf(Surface {
                f: Frame::from_zx(c + base, -sweep_dir, p - c),
                ..Surface::cylinder(c, n, r)
            });
            sides.push(Side {
                surf,
                // Counter-clockwise about `n` bounds material inside.
                sense: axis.dot(n) > 0.0,
                loops: vec![vec![2 * e0], vec![2 * e1 + 1]],
            });
            bottom_loops.push(vec![2 * e0 + 1]);
            top_loops.push(vec![2 * e1]);
            continue;
        }

        let count = lp.len();
        let pts: Vec<V3> = lp.iter().map(Placed::start).collect();
        let vb: Vec<u32> = pts.iter().map(|p| body.add_vert(*p + n * lo)).collect();
        let vt: Vec<u32> = pts.iter().map(|p| body.add_vert(*p + n * hi)).collect();
        let vertical: Vec<u32> = (0..count)
            .map(|i| {
                let c = body.add_curve(Curve::line(body.verts[vb[i] as usize], n));
                body.add_edge(c, 0.0, height, vb[i], vt[i])
            })
            .collect();
        let mut bottom = Vec::with_capacity(count);
        let mut top = Vec::with_capacity(count);
        for (i, placed) in lp.iter().enumerate() {
            let j = (i + 1) % count;
            let (p0, p1) = (pts[i], pts[j]);
            let (eb, et, surf, sense) = match placed.seg {
                Seg::Line { a, b } => {
                    let len = p0.dist(p1);
                    if len <= TOL {
                        return Err("profile contains a zero-length line".into());
                    }
                    let dir = (p1 - p0) / len;
                    let cb = body.add_curve(Curve::line(p0 + n * lo, dir));
                    let ct = body.add_curve(Curve::line(p0 + n * hi, dir));
                    let eb = body.add_edge(cb, 0.0, len, vb[i], vb[j]);
                    let et = body.add_edge(ct, 0.0, len, vt[i], vt[j]);
                    // Outward is to the right of travel seen from +n.
                    let outward = dir.cross(n).unit();
                    let x = (b - a).unit();
                    let surf = body.add_surf(Surface::plane_frame(Frame {
                        o: a + base,
                        x,
                        y: outward.cross(x),
                        z: outward,
                    }));
                    (eb, et, surf, true)
                }
                Seg::Arc { a, m, b } => {
                    let (c, r, given_axis) = arc_circle(a, m, b)
                        .ok_or_else(|| "profile arc is degenerate".to_string())?;
                    let axis = if placed.reversed {
                        -given_axis
                    } else {
                        given_axis
                    };
                    let frame = Frame::from_zx(c, axis, p0 - c);
                    let l1 = frame.local(p1);
                    let sweep = wrap_from(l1.y.atan2(l1.x), 1.0e-12);
                    let cb = body.add_curve(Curve::circle(c + n * lo, axis, p0 - c, r));
                    let ct = body.add_curve(Curve::circle(c + n * hi, axis, p0 - c, r));
                    let eb = body.add_edge(cb, 0.0, sweep, vb[i], vb[j]);
                    let et = body.add_edge(ct, 0.0, sweep, vt[i], vt[j]);
                    let surf = body.add_surf(Surface {
                        f: Frame::from_zx(c + base, -sweep_dir, a - c),
                        ..Surface::cylinder(c, n, r)
                    });
                    (eb, et, surf, axis.dot(n) > 0.0)
                }
                Seg::Circle { .. } => unreachable!("circles are single-segment loops"),
            };
            sides.push(Side {
                surf,
                sense,
                loops: vec![vec![
                    2 * eb,
                    2 * vertical[j],
                    2 * et + 1,
                    2 * vertical[i] + 1,
                ]],
            });
            bottom.push(2 * eb + 1);
            top.push(2 * et);
        }
        bottom.reverse();
        bottom_loops.push(bottom);
        top_loops.push(top);
    }

    for side in &sides {
        body.add_face(side.surf, side.sense, &side.loops);
    }
    body.add_face(bottom_surf, !cap_up, &bottom_loops);
    body.add_face(top_surf, cap_up, &top_loops);
    body.check()?;
    Ok(body)
}

/// Axis-aligned box, mainly for tests and tools.
pub fn block(min: V3, max: V3) -> Result<Body, String> {
    let (a, b) = (min.min(max), min.max(max));
    let z = a.z;
    let region = Region {
        loops: vec![vec![
            Seg::Line {
                a: v3(a.x, a.y, z),
                b: v3(b.x, a.y, z),
            },
            Seg::Line {
                a: v3(b.x, a.y, z),
                b: v3(b.x, b.y, z),
            },
            Seg::Line {
                a: v3(b.x, b.y, z),
                b: v3(a.x, b.y, z),
            },
            Seg::Line {
                a: v3(a.x, b.y, z),
                b: v3(a.x, a.y, z),
            },
        ]],
    };
    extrude(&region, V3::Z, 0.0, b.z - a.z)
}

/// Solid cylinder from `base` along `axis` (length `height`).
pub fn cylinder(base: V3, axis: V3, radius: f64, height: f64) -> Result<Body, String> {
    let n = axis.unit();
    let x = n.any_perp();
    let region = Region {
        loops: vec![vec![Seg::Circle {
            c: base,
            p: base + x * radius,
            n,
        }]],
    };
    extrude(&region, n, 0.0, height)
}

/// Solid of revolution of a closed polygon in the half-plane `(ρ, h)` about
/// the axis through `origin` (full turn). Points with `ρ = 0` lie on the
/// axis. No Booleans and no seam edges: every face is bounded by full
/// circles only.
pub fn lathe(origin: V3, axis: V3, profile: &[(f64, f64)]) -> Result<Body, String> {
    let a = axis.unit();
    if a.len2() < 0.5 {
        return Err("revolve axis is degenerate".into());
    }
    // Drop repeated points and normalise to counter-clockwise (material on
    // the left, outward to the right of travel).
    let mut pts: Vec<(f64, f64)> = Vec::with_capacity(profile.len());
    for &p in profile {
        if p.0 < -TOL {
            return Err("revolve profile crosses its axis".into());
        }
        let p = (p.0.max(0.0), p.1);
        if pts
            .last()
            .is_none_or(|q: &(f64, f64)| (q.0 - p.0).hypot(q.1 - p.1) > TOL)
        {
            pts.push(p);
        }
    }
    if pts.len() > 1 {
        let (f, l) = (pts[0], pts[pts.len() - 1]);
        if (f.0 - l.0).hypot(f.1 - l.1) <= TOL {
            pts.pop();
        }
    }
    let n = pts.len();
    if n < 3 {
        return Err("revolve profile needs at least three points".into());
    }
    let mut area2 = 0.0;
    for i in 0..n {
        let (p, q) = (pts[i], pts[(i + 1) % n]);
        area2 += p.0 * q.1 - q.0 * p.1;
    }
    if area2.abs() < 1.0e-18 {
        return Err("revolve profile encloses no area".into());
    }
    if area2 < 0.0 {
        pts.reverse();
    }

    let r_hat = a.any_perp();
    let mut body = Body::new();
    // One closed circular edge per off-axis profile point.
    let rings: Vec<Option<u32>> = pts
        .iter()
        .map(|&(rho, h)| {
            (rho > TOL).then(|| {
                let centre = origin + a * h;
                let v = body.add_vert(centre + r_hat * rho);
                let c = body.add_curve(Curve::circle(centre, a, r_hat, rho));
                body.add_edge(c, 0.0, TAU, v, v)
            })
        })
        .collect();
    for i in 0..n {
        let j = (i + 1) % n;
        let ((r0, h0), (r1, h1)) = (pts[i], pts[j]);
        let (dr, dh) = (r1 - r0, h1 - h0);
        let mut loops: Vec<Vec<u32>> = Vec::new();
        // Travelling counter-clockwise about the axis keeps the face on the
        // left at the start ring; the end ring runs the other way.
        if let Some(e) = rings[i] {
            loops.push(vec![2 * e]);
        }
        if let Some(e) = rings[j] {
            loops.push(vec![2 * e + 1]);
        }
        if loops.is_empty() {
            // The segment runs along the axis: it bounds nothing.
            continue;
        }
        let (surf, sense) = if dh.abs() <= TOL {
            // Flat ring or disc; outward is to the right of travel.
            let normal = a * (-dr.signum());
            (Surface::plane(origin + a * h0, normal), true)
        } else if dr.abs() <= TOL {
            (
                Surface {
                    f: Frame::from_zx(origin + a * h0.min(h1), a, r_hat),
                    ..Surface::cylinder(origin, a, r0)
                },
                dh > 0.0,
            )
        } else {
            let apex_h = h0 - r0 * dh / dr;
            let toward = if r0 > TOL { h0 - apex_h } else { h1 - apex_h };
            let cone_axis = a * toward.signum();
            let half_angle = (dr / dh).abs().atan();
            (
                Surface {
                    f: Frame::from_zx(origin + a * apex_h, cone_axis, r_hat),
                    ..Surface::cone(origin, cone_axis, half_angle)
                },
                dh > 0.0,
            )
        };
        let s = body.add_surf(surf);
        body.add_face(s, sense, &loops);
    }
    body.check()?;
    Ok(body)
}
