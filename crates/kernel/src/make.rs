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

/// One boundary segment of a revolve profile in the half-plane of the axis:
/// `x` is the distance from the axis, `y` the height along it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PSeg {
    Line {
        a: V2,
        b: V2,
    },
    /// Circular arc from `a` to `b` about `c`.
    Arc {
        a: V2,
        b: V2,
        c: V2,
        ccw: bool,
    },
    /// Full circle (a tube section).
    Circle {
        c: V2,
        r: f64,
    },
}

impl PSeg {
    fn start(&self) -> V2 {
        match *self {
            PSeg::Line { a, .. } | PSeg::Arc { a, .. } => a,
            PSeg::Circle { c, r } => v2(c.x + r, c.y),
        }
    }
    fn end(&self) -> V2 {
        match *self {
            PSeg::Line { b, .. } | PSeg::Arc { b, .. } => b,
            PSeg::Circle { c, r } => v2(c.x + r, c.y),
        }
    }
    fn reversed(&self) -> PSeg {
        match *self {
            PSeg::Line { a, b } => PSeg::Line { a: b, b: a },
            PSeg::Arc { a, b, c, ccw } => PSeg::Arc {
                a: b,
                b: a,
                c,
                ccw: !ccw,
            },
            circle => circle,
        }
    }
    /// Sweep angle of an arc, in `(0, 2π]`.
    fn sweep(&self) -> f64 {
        match *self {
            PSeg::Arc { a, b, c, ccw } => {
                let (u, w) = (a - c, b - c);
                let turn = u.cross(w).atan2(u.dot(w));
                let turn = if ccw { turn } else { -turn };
                wrap_from(turn, 1.0e-12)
            }
            _ => 0.0,
        }
    }
}

/// Twice the signed area of a profile loop.
fn profile_area2(lp: &[PSeg]) -> f64 {
    let mut pts: Vec<V2> = Vec::new();
    for seg in lp {
        match *seg {
            PSeg::Line { a, .. } => pts.push(a),
            PSeg::Arc { a, c, ccw, .. } => {
                let sweep = seg.sweep();
                let base = (a - c).y.atan2((a - c).x);
                let r = (a - c).len();
                let k = 12;
                for i in 0..k {
                    let t = base + sweep * i as f64 / k as f64 * if ccw { 1.0 } else { -1.0 };
                    pts.push(v2(c.x + r * t.cos(), c.y + r * t.sin()));
                }
            }
            // A lone circle is counter-clockwise by convention.
            PSeg::Circle { r, .. } => return r * r,
        }
    }
    let mut sum = 0.0;
    for i in 0..pts.len() {
        sum += pts[i].cross(pts[(i + 1) % pts.len()]);
    }
    sum
}

/// Revolve closed profile loops (`loops[0]` outer, the rest holes) about the
/// axis through `origin`. `zero` gives the direction of the profile's
/// half-plane; the sweep runs counter-clockwise about `axis` through
/// `angle` radians (a full turn when `angle ≥ 2π`).
///
/// A full turn has no seam edges: every face is bounded by complete circles.
pub fn revolve(
    loops: &[Vec<PSeg>],
    origin: V3,
    axis: V3,
    zero: V3,
    angle: f64,
) -> Result<Body, String> {
    let a = axis.unit();
    let x0 = zero - a * zero.dot(a);
    if a.len2() < 0.5 || x0.len2() < 1.0e-18 {
        return Err("revolve axis is degenerate".into());
    }
    if !(angle.is_finite() && angle > 1.0e-9) {
        return Err("revolve angle must be greater than zero".into());
    }
    let x0 = x0.unit();
    let full = angle >= TAU - 1.0e-9;
    let angle = angle.min(TAU);
    let t0 = a.cross(x0);
    let x1 = x0 * angle.cos() + t0 * angle.sin();
    let t1 = a.cross(x1);
    if loops.is_empty() {
        return Err("revolve needs at least one profile loop".into());
    }

    // Material on the left: outer counter-clockwise, holes clockwise.
    let mut profile: Vec<Vec<PSeg>> = Vec::with_capacity(loops.len());
    for (i, lp) in loops.iter().enumerate() {
        if lp.is_empty() {
            return Err("revolve profile loop has no boundary curves".into());
        }
        for seg in lp {
            let low = match *seg {
                PSeg::Line { a, b } => a.x.min(b.x),
                PSeg::Arc { a, b, .. } => a.x.min(b.x),
                PSeg::Circle { c, r } => c.x - r - TOL,
            };
            if low < -TOL {
                return Err("revolve profile crosses its axis".into());
            }
        }
        let area = profile_area2(lp);
        if area.abs() < 1.0e-18 {
            return Err("revolve profile encloses no area".into());
        }
        let mut lp = lp.clone();
        if (area > 0.0) != (i == 0) {
            lp.reverse();
            for seg in &mut lp {
                *seg = seg.reversed();
            }
        }
        profile.push(lp);
    }

    let place = |q: V2, x: V3| origin + a * q.y + x * q.x.max(0.0);
    let mut body = Body::new();
    let mut start_cap: Vec<Vec<u32>> = Vec::new();
    let mut end_cap: Vec<Vec<u32>> = Vec::new();
    struct Swept {
        surf: Surface,
        sense: bool,
        loops: Vec<Vec<u32>>,
    }
    let mut swept: Vec<Swept> = Vec::new();

    for (li, lp) in profile.iter().enumerate() {
        let n = lp.len();
        let outer = li == 0;
        if let [PSeg::Circle { c, r }] = lp[..] {
            if c.x - r <= TOL {
                return Err("a revolved circle must not touch the axis".into());
            }
            // After normalisation an outer circle bounds material inside.
            let surf = Surface {
                f: Frame::from_zx(origin + a * c.y, a, x0),
                ..Surface::torus(origin, a, c.x, r)
            };
            let mut face_loops = Vec::new();
            if !full {
                let section = |body: &mut Body, x: V3, t: V3| {
                    let centre = place(c, x);
                    let normal = if outer { -t } else { t };
                    let v = body.add_vert(centre + x * r);
                    let curve = body.add_curve(Curve::circle(centre, normal, x, r));
                    body.add_edge(curve, 0.0, TAU, v, v)
                };
                let s = section(&mut body, x0, t0);
                let e = section(&mut body, x1, t1);
                face_loops = vec![vec![2 * e], vec![2 * s + 1]];
                start_cap.push(vec![2 * s]);
                end_cap.push(vec![2 * e + 1]);
            }
            swept.push(Swept {
                surf,
                sense: outer,
                loops: face_loops,
            });
            continue;
        }
        if lp.iter().any(|s| matches!(s, PSeg::Circle { .. })) {
            return Err("a full circle cannot share a profile loop with other curves".into());
        }
        let pts: Vec<V2> = lp.iter().map(PSeg::start).collect();
        for i in 0..n {
            if (lp[i].end() - pts[(i + 1) % n]).len() > CHAIN_TOL {
                return Err("revolve profile loop is not connected end to end".into());
            }
        }
        let on_axis: Vec<bool> = pts.iter().map(|q| q.x <= TOL).collect();
        let vs: Vec<u32> = pts.iter().map(|q| body.add_vert(place(*q, x0))).collect();
        let ve: Vec<u32> = (0..n)
            .map(|i| {
                if full || on_axis[i] {
                    vs[i]
                } else {
                    body.add_vert(place(pts[i], x1))
                }
            })
            .collect();
        // The path each profile point sweeps.
        let rails: Vec<Option<u32>> = (0..n)
            .map(|i| {
                (!on_axis[i]).then(|| {
                    let curve =
                        body.add_curve(Curve::circle(origin + a * pts[i].y, a, x0, pts[i].x));
                    body.add_edge(curve, 0.0, angle, vs[i], ve[i])
                })
            })
            .collect();
        let mut starts: Vec<u32> = Vec::new();
        let mut ends: Vec<u32> = Vec::new();
        for (i, seg) in lp.iter().enumerate() {
            let j = (i + 1) % n;
            // Copies of the segment in the start and end half-planes.
            let mut copies: Option<(u32, u32)> = None;
            if !full {
                let copy = |body: &mut Body, x: V3, t: V3, v: &[u32]| -> u32 {
                    match *seg {
                        PSeg::Line { .. } => body.add_line_edge(v[i], v[j]),
                        PSeg::Arc { c, ccw, .. } => {
                            let centre = place(c, x);
                            let normal = if ccw { -t } else { t };
                            let from = body.verts[v[i] as usize] - centre;
                            let curve =
                                body.add_curve(Curve::circle(centre, normal, from, from.len()));
                            body.add_edge(curve, 0.0, seg.sweep(), v[i], v[j])
                        }
                        PSeg::Circle { .. } => unreachable!(),
                    }
                };
                let s = copy(&mut body, x0, t0, &vs);
                // A straight segment on the axis is its own copy.
                let e = if on_axis[i] && on_axis[j] && matches!(seg, PSeg::Line { .. }) {
                    s
                } else {
                    copy(&mut body, x1, t1, &ve)
                };
                starts.push(2 * s);
                ends.push(2 * e + 1);
                copies = Some((s, e));
            }
            if on_axis[i] && on_axis[j] && matches!(seg, PSeg::Line { .. }) {
                continue;
            }
            let (pa, pb) = (pts[i], pts[j]);
            let (dr, dh) = (pb.x - pa.x, pb.y - pa.y);
            let (surf, sense) = match *seg {
                PSeg::Line { .. } => {
                    let len = dr.hypot(dh);
                    if len <= TOL {
                        return Err("revolve profile contains a zero-length line".into());
                    }
                    if dh.abs() <= 1.0e-9 * len {
                        (Surface::plane(origin + a * pa.y, a * (-dr.signum())), true)
                    } else if dr.abs() <= 1.0e-9 * len {
                        (
                            Surface {
                                f: Frame::from_zx(origin + a * pa.y.min(pb.y), a, x0),
                                ..Surface::cylinder(origin, a, pa.x)
                            },
                            dh > 0.0,
                        )
                    } else {
                        let apex = pa.y - pa.x * dh / dr;
                        let toward = if pa.x > TOL { pa.y - apex } else { pb.y - apex };
                        let cone_axis = a * toward.signum();
                        (
                            Surface {
                                f: Frame::from_zx(origin + a * apex, cone_axis, x0),
                                ..Surface::cone(origin, cone_axis, (dr / dh).abs().atan())
                            },
                            dh > 0.0,
                        )
                    }
                }
                PSeg::Arc { c, ccw, .. } => {
                    let radius = (pa - c).len();
                    let surf = if c.x <= TOL {
                        Surface {
                            f: Frame::from_zx(origin + a * c.y, a, x0),
                            ..Surface::sphere(origin, radius)
                        }
                    } else {
                        Surface {
                            f: Frame::from_zx(origin + a * c.y, a, x0),
                            ..Surface::torus(origin, a, c.x, radius)
                        }
                    };
                    (surf, ccw)
                }
                PSeg::Circle { .. } => unreachable!(),
            };
            // Travelling with the sweep keeps the face on the left at the
            // segment's start; its end rail runs the other way.
            let face_loops = match copies {
                None => {
                    let mut out = Vec::new();
                    if let Some(e) = rails[i] {
                        out.push(vec![2 * e]);
                    }
                    if let Some(e) = rails[j] {
                        out.push(vec![2 * e + 1]);
                    }
                    out
                }
                Some((s, e)) => {
                    let mut cycle = Vec::with_capacity(4);
                    if let Some(rail) = rails[i] {
                        cycle.push(2 * rail);
                    }
                    cycle.push(2 * e);
                    if let Some(rail) = rails[j] {
                        cycle.push(2 * rail + 1);
                    }
                    cycle.push(2 * s + 1);
                    vec![cycle]
                }
            };
            swept.push(Swept {
                surf,
                sense,
                loops: face_loops,
            });
        }
        if !full {
            ends.reverse();
            start_cap.push(starts);
            end_cap.push(ends);
        }
    }

    for face in &swept {
        let s = body.add_surf(face.surf);
        body.add_face(s, face.sense, &face.loops);
    }
    if !full {
        let cap = |normal: V3| {
            let x = reference_x(normal);
            Surface::plane_frame(Frame {
                o: origin,
                x,
                y: normal.cross(x),
                z: normal,
            })
        };
        let s = body.add_surf(cap(-t0));
        body.add_face(s, true, &start_cap);
        let s = body.add_surf(cap(t1));
        body.add_face(s, true, &end_cap);
    }
    body.check()?;
    Ok(body)
}

/// Revolve a planar region given in model space. The profile plane must
/// contain the axis.
pub fn revolve_region(region: &Region, origin: V3, axis: V3, angle: f64) -> Result<Body, String> {
    let mut a = axis.unit();
    if a.len2() < 0.5 {
        return Err("revolve axis is degenerate".into());
    }
    let mut angle = angle;
    if angle < 0.0 {
        // Sweeping backwards is sweeping forwards about the opposite axis.
        a = -a;
        angle = -angle;
    }
    // The half-plane direction: towards the profile point farthest out.
    let radial = |p: V3| {
        let d = p - origin;
        d - a * d.dot(a)
    };
    let mut zero = V3::ZERO;
    for segs in &region.loops {
        for seg in segs {
            let candidates = match *seg {
                Seg::Line { a, b } => [a, b, b],
                Seg::Arc { a, m, b } => [a, m, b],
                Seg::Circle { c, p, .. } => [c, p, p],
            };
            for p in candidates {
                let w = radial(p);
                if w.len2() > zero.len2() {
                    zero = w;
                }
            }
        }
    }
    if zero.len2() < 1.0e-18 {
        return Err("revolve profile lies on its axis".into());
    }
    let x0 = zero.unit();
    let flat = |p: V3| -> Result<V2, String> {
        let d = p - origin;
        let w = radial(p);
        if (w - x0 * w.dot(x0)).len() > 1.0e-6 * (1.0 + w.len()) {
            return Err("revolve profile is not in a plane through its axis".into());
        }
        Ok(v2(w.dot(x0), d.dot(a)))
    };
    let mut loops: Vec<Vec<PSeg>> = Vec::with_capacity(region.loops.len());
    for segs in &region.loops {
        let mut out = Vec::with_capacity(segs.len());
        for placed in chain(segs)? {
            let seg = match placed.seg {
                Seg::Line { .. } => PSeg::Line {
                    a: flat(placed.start())?,
                    b: flat(placed.end())?,
                },
                Seg::Arc { a: sa, m, b: sb } => {
                    let (c, _, _) = arc_circle(sa, m, sb)
                        .ok_or_else(|| "profile arc is degenerate".to_string())?;
                    let (qa, qm, qb) = (flat(sa)?, flat(m)?, flat(sb)?);
                    let ccw = (qm - qa).cross(qb - qm) > 0.0;
                    let arc = PSeg::Arc {
                        a: qa,
                        b: qb,
                        c: flat(c)?,
                        ccw,
                    };
                    if placed.reversed {
                        arc.reversed()
                    } else {
                        arc
                    }
                }
                Seg::Circle { c, p, .. } => PSeg::Circle {
                    c: flat(c)?,
                    r: c.dist(p),
                },
            };
            out.push(seg);
        }
        loops.push(out);
    }
    revolve(&loops, origin, a, x0, angle)
}

/// Solid of revolution of a closed polygon in the half-plane `(ρ, h)` about
/// the axis through `origin` (full turn). Points with `ρ = 0` lie on the
/// axis.
pub fn lathe(origin: V3, axis: V3, profile: &[(f64, f64)]) -> Result<Body, String> {
    let mut pts: Vec<V2> = Vec::with_capacity(profile.len());
    for &(rho, h) in profile {
        let q = v2(rho, h);
        if pts.last().is_none_or(|p: &V2| (*p - q).len() > TOL) {
            pts.push(q);
        }
    }
    if pts.len() > 1 && (pts[0] - pts[pts.len() - 1]).len() <= TOL {
        pts.pop();
    }
    if pts.len() < 3 {
        return Err("revolve profile needs at least three points".into());
    }
    let n = pts.len();
    let lp: Vec<PSeg> = (0..n)
        .map(|i| PSeg::Line {
            a: pts[i],
            b: pts[(i + 1) % n],
        })
        .collect();
    revolve(&[lp], origin, axis, axis.unit().any_perp(), TAU)
}
