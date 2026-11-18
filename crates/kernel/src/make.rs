//! Direct construction of solids. No Booleans are involved: a profile with
//! holes becomes one solid in a single pass.

use crate::geom::{Curve, Surface};
use crate::math::*;
use crate::name::Role;
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
pub(crate) struct Placed {
    pub(crate) seg: Seg,
    pub(crate) reversed: bool,
    /// Which segment of the loop this is, as the caller gave them.
    pub(crate) index: u32,
}

impl Placed {
    pub(crate) fn start(&self) -> V3 {
        let (a, b) = self.seg.ends();
        if self.reversed {
            b
        } else {
            a
        }
    }
    pub(crate) fn end(&self) -> V3 {
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
pub(crate) fn chain(segs: &[Seg]) -> Result<Vec<Placed>, String> {
    if segs.is_empty() {
        return Err("profile loop has no boundary curves".into());
    }
    if segs.len() == 1 {
        return match segs[0] {
            Seg::Circle { .. } => Ok(vec![Placed {
                seg: segs[0],
                reversed: false,
                index: 0,
            }]),
            _ => Err("profile loop is not closed".into()),
        };
    }
    let mut out = Vec::with_capacity(segs.len());
    let mut first = Placed {
        seg: segs[0],
        reversed: false,
        index: 0,
    };
    let (sa, sb) = segs[1].ends();
    let touches = |p: V3| p.dist(sa) < CHAIN_TOL || p.dist(sb) < CHAIN_TOL;
    if !touches(first.end()) && touches(first.start()) {
        first.reversed = true;
    }
    out.push(first);
    for (index, seg) in segs.iter().enumerate().skip(1) {
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
            index: index as u32,
        });
    }
    if out.last().unwrap().end().dist(out[0].start()) >= CHAIN_TOL {
        return Err("profile loop is not closed".into());
    }
    Ok(out)
}

/// Twice the signed area of the loop about `n`.
pub(crate) fn signed_area2(lp: &[Placed], n: V3) -> f64 {
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

pub(crate) fn reverse_loop(lp: &mut [Placed]) {
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
pub(crate) fn wire_barycentre(lp: &[Placed]) -> V3 {
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
        tag: u64,
    }
    let mut sides: Vec<Side> = Vec::new();

    for (li, lp) in loops.iter().enumerate() {
        let side = |seg: u32| Role::Side { lp: li as u32, seg }.tag();
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
                // Against the sweep, as the reference kernel has it: features
                // that start from one end of the face start from the same one.
                f: Frame::from_zx(c + base, -sweep_dir, p - c),
                ..Surface::cylinder(c, n, r)
            });
            sides.push(Side {
                surf,
                // Counter-clockwise about `n` bounds material inside.
                sense: axis.dot(n) > 0.0,
                loops: vec![vec![2 * e0], vec![2 * e1 + 1]],
                tag: side(0),
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
                tag: side(placed.index),
            });
            bottom.push(2 * eb + 1);
            top.push(2 * et);
        }
        bottom.reverse();
        bottom_loops.push(bottom);
        top_loops.push(top);
    }

    for side in &sides {
        body.add_tagged_face(side.surf, side.sense, &side.loops, side.tag);
    }
    // The lower cap is where the extrusion starts unless it runs downwards.
    let (low, high) = if start <= end {
        (Role::Start, Role::End)
    } else {
        (Role::End, Role::Start)
    };
    body.add_tagged_face(bottom_surf, !cap_up, &bottom_loops, low.tag());
    body.add_tagged_face(top_surf, cap_up, &top_loops, high.tag());
    body.check()?;
    Ok(body)
}

/// Extrude a planar region while scaling it about `centre`, a point of its
/// plane: at signed offset `o` along `normal` the region is `1 + rate·o`
/// times its own size. `start` and `end` are offsets as for [`extrude`].
///
/// Every point of the region then travels a straight line, and all those
/// lines meet in one point, so the result is a slice of a cone over the
/// region. Straight sides stay flat. A circle or arc about `centre` sweeps
/// a right circular cone; one about any other point would sweep a slanted
/// cone, which this kernel has no surface for.
pub fn taper(
    region: &Region,
    normal: V3,
    start: f64,
    end: f64,
    centre: V3,
    rate: f64,
) -> Result<Body, String> {
    if rate.abs() < 1.0e-15 {
        return extrude(region, normal, start, end);
    }
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
    let scale = |offset: f64| 1.0 + rate * offset;
    if !(scale(lo).is_finite() && scale(hi).is_finite())
        || scale(lo) <= 1.0e-6
        || scale(hi) <= 1.0e-6
    {
        return Err("taper collapses or inverts the profile".into());
    }
    // Where a point of the profile plane is at a given offset.
    let place = |p: V3, offset: f64| centre + (p - centre) * scale(offset) + n * offset;
    // The point all the travel lines meet in, and the way from it to the
    // solid.
    let apex = centre - n / rate;
    let opening = n * rate.signum();
    let centred = |c: V3, r: f64| {
        let off = c - centre;
        (off - n * off.dot(n)).len() <= 1.0e-6 * (1.0 + r)
    };
    let slanted = || {
        "a tapered extrusion needs every arc and circle of the profile to be centred on it;          others would sweep a slanted cone, which is not supported yet"
            .to_string()
    };

    let mut loops = Vec::with_capacity(region.loops.len());
    for segs in &region.loops {
        loops.push(chain(segs)?);
    }
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
        if (area > 0.0) != (i == 0) {
            reverse_loop(lp);
        }
    }

    let mut body = Body::new();
    let cap_x = reference_x(cap_normal);
    let cap = |offset: f64| {
        Surface::plane_frame(Frame {
            o: barycentre + n * offset,
            x: cap_x,
            y: cap_normal.cross(cap_x),
            z: cap_normal,
        })
    };
    let bottom_surf = body.add_surf(cap(lo));
    let top_surf = body.add_surf(cap(hi));
    let cap_up = cap_normal.dot(n) > 0.0;
    let cone = |r: f64, reference: V3| Surface {
        f: Frame::from_zx(apex, opening, reference),
        ..Surface::cone(apex, opening, (r * rate.abs()).atan())
    };

    let mut bottom_loops: Vec<Vec<u32>> = Vec::new();
    let mut top_loops: Vec<Vec<u32>> = Vec::new();
    struct Side {
        surf: u32,
        sense: bool,
        loops: Vec<Vec<u32>>,
        tag: u64,
    }
    let mut sides: Vec<Side> = Vec::new();

    for (li, lp) in loops.iter().enumerate() {
        let side = |seg: u32| Role::Side { lp: li as u32, seg }.tag();
        if let [Placed {
            seg: Seg::Circle { c, p, n: cn },
            ..
        }] = lp[..]
        {
            let r = c.dist(p);
            if r <= TOL {
                return Err("profile circle has zero radius".into());
            }
            if !centred(c, r) {
                return Err(slanted());
            }
            let axis = cn.unit();
            let (v0, v1) = (body.add_vert(place(p, lo)), body.add_vert(place(p, hi)));
            let ring = |body: &mut Body, offset: f64| {
                body.add_curve(Curve::circle(
                    place(c, offset),
                    axis,
                    p - c,
                    r * scale(offset),
                ))
            };
            let (c0, c1) = (ring(&mut body, lo), ring(&mut body, hi));
            let e0 = body.add_edge(c0, 0.0, TAU, v0, v0);
            let e1 = body.add_edge(c1, 0.0, TAU, v1, v1);
            let surf = body.add_surf(cone(r, p - c));
            sides.push(Side {
                surf,
                sense: axis.dot(n) > 0.0,
                loops: vec![vec![2 * e0], vec![2 * e1 + 1]],
                tag: side(0),
            });
            bottom_loops.push(vec![2 * e0 + 1]);
            top_loops.push(vec![2 * e1]);
            continue;
        }

        let count = lp.len();
        let pts: Vec<V3> = lp.iter().map(Placed::start).collect();
        let vb: Vec<u32> = pts.iter().map(|p| body.add_vert(place(*p, lo))).collect();
        let vt: Vec<u32> = pts.iter().map(|p| body.add_vert(place(*p, hi))).collect();
        let upright: Vec<u32> = (0..count)
            .map(|i| body.add_line_edge(vb[i], vt[i]))
            .collect();
        let mut bottom = Vec::with_capacity(count);
        let mut top = Vec::with_capacity(count);
        for (i, placed) in lp.iter().enumerate() {
            let j = (i + 1) % count;
            let (eb, et, surf, sense) = match placed.seg {
                Seg::Line { .. } => {
                    let (p0, p1) = (body.verts[vb[i] as usize], body.verts[vb[j] as usize]);
                    if p0.dist(p1) <= TOL {
                        return Err("profile contains a zero-length line".into());
                    }
                    let eb = body.add_line_edge(vb[i], vb[j]);
                    let et = body.add_line_edge(vt[i], vt[j]);
                    // Outward is to the right of travel, seen along the
                    // way the side leans.
                    let along = (p1 - p0).unit();
                    let up = body.verts[vt[i] as usize] - p0;
                    let outward = along.cross(up).unit();
                    let surf = body.add_surf(Surface::plane_frame(Frame {
                        o: p0,
                        x: along,
                        y: outward.cross(along),
                        z: outward,
                    }));
                    (eb, et, surf, true)
                }
                Seg::Arc { a, m, b } => {
                    let (c, r, given_axis) = arc_circle(a, m, b)
                        .ok_or_else(|| "profile arc is degenerate".to_string())?;
                    if !centred(c, r) {
                        return Err(slanted());
                    }
                    let axis = if placed.reversed {
                        -given_axis
                    } else {
                        given_axis
                    };
                    let (p0, p1) = (pts[i], pts[j]);
                    let frame = Frame::from_zx(c, axis, p0 - c);
                    let l1 = frame.local(p1);
                    let sweep = wrap_from(l1.y.atan2(l1.x), 1.0e-12);
                    let ring = |body: &mut Body, offset: f64| {
                        body.add_curve(Curve::circle(
                            place(c, offset),
                            axis,
                            p0 - c,
                            r * scale(offset),
                        ))
                    };
                    let (cb, ct) = (ring(&mut body, lo), ring(&mut body, hi));
                    let eb = body.add_edge(cb, 0.0, sweep, vb[i], vb[j]);
                    let et = body.add_edge(ct, 0.0, sweep, vt[i], vt[j]);
                    let surf = body.add_surf(cone(r, a - c));
                    (eb, et, surf, axis.dot(n) > 0.0)
                }
                Seg::Circle { .. } => unreachable!("circles are single-segment loops"),
            };
            sides.push(Side {
                surf,
                sense,
                loops: vec![vec![2 * eb, 2 * upright[j], 2 * et + 1, 2 * upright[i] + 1]],
                tag: side(placed.index),
            });
            bottom.push(2 * eb + 1);
            top.push(2 * et);
        }
        bottom.reverse();
        bottom_loops.push(bottom);
        top_loops.push(top);
    }

    for side in &sides {
        body.add_tagged_face(side.surf, side.sense, &side.loops, side.tag);
    }
    let (low, high) = if start <= end {
        (Role::Start, Role::End)
    } else {
        (Role::End, Role::Start)
    };
    body.add_tagged_face(bottom_surf, !cap_up, &bottom_loops, low.tag());
    body.add_tagged_face(top_surf, cap_up, &top_loops, high.tag());
    body.check()?;
    Ok(body)
}

/// A solid through a row of flat sections, joined by straight lines from
/// each to the next.
///
/// The sections are either all outlines of straight segments, as many in
/// each, or all circles. Between two outlines every side has to come out
/// flat, which it does when the two segments it joins are parallel or would
/// meet: sections that are scaled, moved or sheared copies of one another.
/// Circles have to share an axis, and give cones and cylinders. Anything
/// else would need a twisted surface, which this kernel does not have.
pub fn loft(sections: &[Vec<Seg>]) -> Result<Body, String> {
    if sections.len() < 2 {
        return Err("Loft needs at least two sections".into());
    }
    let mut loops: Vec<Vec<Placed>> = Vec::with_capacity(sections.len());
    for segs in sections {
        loops.push(chain(segs)?);
    }
    let round = |lp: &[Placed]| {
        matches!(
            lp,
            [Placed {
                seg: Seg::Circle { .. },
                ..
            }]
        )
    };
    let straight = |lp: &[Placed]| lp.iter().all(|p| matches!(p.seg, Seg::Line { .. }));
    let twisted = || {
        "a loft between these sections would need a twisted surface, which is not supported yet"
            .to_string()
    };
    // Which way the loft runs past each section.
    let middles: Vec<V3> = loops.iter().map(|lp| wire_barycentre(lp)).collect();
    let last = loops.len() - 1;
    let onward = |k: usize| {
        if k < last {
            middles[k + 1] - middles[k]
        } else {
            middles[k] - middles[k - 1]
        }
    };
    let mut body = Body::new();
    let side = |band: usize, seg: u32| {
        Role::Side {
            lp: band as u32,
            seg,
        }
        .tag()
    };

    if loops.iter().all(|lp| round(lp)) {
        // Circles about one axis: a cylinder or a cone between each two.
        let axis = (middles[last] - middles[0]).unit();
        if !axis.is_finite() || axis.len2() < 0.5 {
            return Err("Loft sections lie on top of each other".into());
        }
        let mut rings: Vec<(u32, V3, f64)> = Vec::with_capacity(loops.len());
        for lp in &loops {
            let Seg::Circle { c, p, n } = lp[0].seg else {
                unreachable!()
            };
            let off = c - middles[0];
            if n.unit().cross(axis).len() > 1.0e-9 || (off - axis * off.dot(axis)).len() > 1.0e-6 {
                return Err(twisted());
            }
            let r = c.dist(p);
            if r <= TOL {
                return Err("profile circle has zero radius".into());
            }
            let v = body.add_vert(p);
            let curve = body.add_curve(Curve::circle(c, axis, p - c, r));
            rings.push((body.add_edge(curve, 0.0, TAU, v, v), c, r));
        }
        for (k, pair) in rings.windows(2).enumerate() {
            let ((e0, c0, r0), (e1, c1, r1)) = (pair[0], pair[1]);
            let rise = (c1 - c0).dot(axis);
            if rise <= TOL {
                return Err("Loft sections have to follow one another along their axis".into());
            }
            let surf = if (r1 - r0).abs() <= 1.0e-9 * r0.max(r1) {
                Surface::cylinder(c0, axis, r0)
            } else {
                // The tip the cone would reach, and the way from it here.
                let tip = c0 - axis * (r0 * rise / (r1 - r0));
                let opening = axis * (r1 - r0).signum();
                Surface::cone(tip, opening, ((r1 - r0).abs() / rise).atan())
            };
            let s = body.add_surf(surf);
            body.add_tagged_face(s, true, &[vec![2 * e0], vec![2 * e1 + 1]], side(k, 0));
        }
        let s = body.add_surf(Surface::plane(rings[0].1, -axis));
        body.add_tagged_face(s, true, &[vec![2 * rings[0].0 + 1]], Role::Start.tag());
        let s = body.add_surf(Surface::plane(rings[last].1, axis));
        body.add_tagged_face(s, true, &[vec![2 * rings[last].0]], Role::End.tag());
        body.check()?;
        return Ok(body);
    }

    let count = loops[0].len();
    if !loops.iter().all(|lp| straight(lp) && lp.len() == count) {
        return Err(
            "Loft sections must all be circles, or all be outlines of straight lines with the \
             same number of sides"
                .into(),
        );
    }
    // Every outline counter-clockwise seen against the way the loft runs,
    // and started at the corner nearest the start of the one before.
    let mut corners: Vec<Vec<V3>> = Vec::with_capacity(loops.len());
    let mut given: Vec<Vec<u32>> = Vec::with_capacity(loops.len());
    for (k, lp) in loops.iter_mut().enumerate() {
        let pts: Vec<V3> = lp.iter().map(Placed::start).collect();
        let mut around = V3::ZERO;
        for i in 0..count {
            around += (pts[i] - middles[k]).cross(pts[(i + 1) % count] - middles[k]);
        }
        if around.len2() < 1.0e-18 {
            return Err("profile encloses no area".into());
        }
        if around.dot(onward(k)) < 0.0 {
            reverse_loop(lp);
        }
        let mut pts: Vec<V3> = lp.iter().map(Placed::start).collect();
        let mut index: Vec<u32> = lp.iter().map(|p| p.index).collect();
        if let Some(before) = corners.last() {
            let apart = |shift: usize| -> f64 {
                (0..count)
                    .map(|i| (pts[(i + shift) % count] - before[i]).len2())
                    .sum()
            };
            let shift = (0..count)
                .min_by(|&a, &b| apart(a).total_cmp(&apart(b)))
                .unwrap();
            pts.rotate_left(shift);
            index.rotate_left(shift);
        }
        corners.push(pts);
        given.push(index);
    }
    let verts: Vec<Vec<u32>> = corners
        .iter()
        .map(|pts| pts.iter().map(|p| body.add_vert(*p)).collect())
        .collect();
    let rings: Vec<Vec<u32>> = verts
        .iter()
        .map(|v| {
            (0..count)
                .map(|i| body.add_line_edge(v[i], v[(i + 1) % count]))
                .collect()
        })
        .collect();
    for k in 0..last {
        let rails: Vec<u32> = (0..count)
            .map(|i| body.add_line_edge(verts[k][i], verts[k + 1][i]))
            .collect();
        for i in 0..count {
            let j = (i + 1) % count;
            let (a0, a1) = (corners[k][i], corners[k][j]);
            let (b0, b1) = (corners[k + 1][i], corners[k + 1][j]);
            let along = a1 - a0;
            let normal = along.cross(b0 - a0);
            let size = along.len() * (b0 - a0).len();
            if normal.len() <= 1.0e-9 * size {
                return Err("Loft sections lie on top of each other".into());
            }
            let outward = normal.unit();
            if (b1 - a0).dot(outward).abs() > 1.0e-7 * (1.0 + (b1 - a0).len()) {
                return Err(twisted());
            }
            let x = along.unit();
            let s = body.add_surf(Surface::plane_frame(Frame {
                o: a0,
                x,
                y: outward.cross(x),
                z: outward,
            }));
            body.add_tagged_face(
                s,
                true,
                &[vec![
                    2 * rings[k][i],
                    2 * rails[j],
                    2 * rings[k + 1][i] + 1,
                    2 * rails[i] + 1,
                ]],
                side(k, given[k][i]),
            );
        }
    }
    let cap = |pts: &[V3], k: usize, flip: f64| {
        let mut around = V3::ZERO;
        for i in 0..count {
            around += (pts[i] - middles[k]).cross(pts[(i + 1) % count] - middles[k]);
        }
        Surface::plane(middles[k], around.unit() * flip)
    };
    let s = body.add_surf(cap(&corners[0], 0, -1.0));
    let bottom: Vec<u32> = rings[0].iter().rev().map(|e| 2 * e + 1).collect();
    body.add_tagged_face(s, true, &[bottom], Role::Start.tag());
    let s = body.add_surf(cap(&corners[last], last, 1.0));
    let top: Vec<u32> = rings[last].iter().map(|e| 2 * e).collect();
    body.add_tagged_face(s, true, &[top], Role::End.tag());
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
    let mut order: Vec<Vec<u32>> = Vec::with_capacity(loops.len());
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
        // Where each segment was in the loop as given.
        let mut given: Vec<u32> = (0..lp.len() as u32).collect();
        if (area > 0.0) != (i == 0) {
            lp.reverse();
            given.reverse();
            for seg in &mut lp {
                *seg = seg.reversed();
            }
        }
        profile.push(lp);
        order.push(given);
    }

    let place = |q: V2, x: V3| origin + a * q.y + x * q.x.max(0.0);
    let mut body = Body::new();
    let mut start_cap: Vec<Vec<u32>> = Vec::new();
    let mut end_cap: Vec<Vec<u32>> = Vec::new();
    struct Swept {
        surf: Surface,
        sense: bool,
        loops: Vec<Vec<u32>>,
        tag: u64,
    }
    let mut swept: Vec<Swept> = Vec::new();

    for (li, lp) in profile.iter().enumerate() {
        let n = lp.len();
        let outer = li == 0;
        let side = |k: usize| {
            Role::Side {
                lp: li as u32,
                seg: order[li][k],
            }
            .tag()
        };
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
                tag: side(0),
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
                tag: side(i),
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
        body.add_tagged_face(s, face.sense, &face.loops, face.tag);
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
        body.add_tagged_face(s, true, &start_cap, Role::Start.tag());
        let s = body.add_surf(cap(t1));
        body.add_tagged_face(s, true, &end_cap, Role::End.tag());
    }
    body.check()?;
    Ok(body)
}

/// The solid a profile sweeps when it turns about an axis and climbs it
/// at once, from the angle `from` to the angle `to` (radians; the profile
/// is given as it stands at angle zero). `lead` is the climb per radian:
/// negative for a left-hand screw.
///
/// The profile is one loop in the half-plane `(ρ, h)`, clear of the axis,
/// and no taller than one turn climbs, so that the turns do not run into
/// each other. A side of it along the axis sweeps a cylinder, any other
/// straight side a helicoid, square to the axis or slanted, and an arc a
/// coil. However many turns there are, each side is one face.
pub fn screw(
    profile: &[PSeg],
    origin: V3,
    axis: V3,
    zero: V3,
    lead: f64,
    from: f64,
    to: f64,
) -> Result<Body, String> {
    let a = axis.unit();
    let x0 = zero - a * zero.dot(a);
    if a.len2() < 0.5 || x0.len2() < 1.0e-18 {
        return Err("the screw axis is degenerate".into());
    }
    if !(lead.is_finite() && lead.abs() > 1.0e-12 && to - from > 1.0e-9) {
        return Err("a screw needs a lead and at least part of a turn".into());
    }
    let x0 = x0.unit();
    let t0 = a.cross(x0);
    let area = profile_area2(profile);
    if profile.len() < 2 || area.abs() < 1.0e-18 {
        return Err("the screw profile encloses no area".into());
    }
    // Material on the left.
    let mut lp = profile.to_vec();
    let mut given: Vec<u32> = (0..lp.len() as u32).collect();
    if area < 0.0 {
        lp.reverse();
        given.reverse();
        for seg in &mut lp {
            *seg = seg.reversed();
        }
    }
    let n = lp.len();
    let pts: Vec<V2> = lp.iter().map(PSeg::start).collect();
    let (mut low, mut high) = (f64::INFINITY, f64::NEG_INFINITY);
    for (i, seg) in lp.iter().enumerate() {
        if matches!(seg, PSeg::Circle { .. }) {
            return Err("a screw profile is made of lines and arcs".into());
        }
        if (seg.end() - pts[(i + 1) % n]).len() > CHAIN_TOL {
            return Err("the screw profile is not connected end to end".into());
        }
        if pts[i].x <= TOL {
            return Err("the screw profile must stay clear of its axis".into());
        }
        low = low.min(pts[i].y);
        high = high.max(pts[i].y);
    }
    if high - low >= TAU * lead.abs() - TOL {
        return Err("the screw profile is taller than one turn climbs".into());
    }
    let out = |angle: f64| x0 * angle.cos() + t0 * angle.sin();
    let place = |q: V2, angle: f64| origin + a * (q.y + lead * angle) + out(angle) * q.x;
    let mut body = Body::new();
    let vs: Vec<u32> = pts.iter().map(|q| body.add_vert(place(*q, from))).collect();
    let ve: Vec<u32> = pts.iter().map(|q| body.add_vert(place(*q, to))).collect();
    // The helix each corner of the profile runs along.
    let rails: Vec<u32> = (0..n)
        .map(|i| {
            let curve = body.add_curve(Curve::helix(origin + a * pts[i].y, a, x0, pts[i].x, lead));
            body.add_edge(curve, from, to, vs[i], ve[i])
        })
        .collect();
    let mut starts: Vec<u32> = Vec::with_capacity(n);
    let mut ends: Vec<u32> = Vec::with_capacity(n);
    struct Swept {
        surf: Surface,
        sense: bool,
        cycle: Vec<u32>,
        tag: u64,
    }
    let mut swept: Vec<Swept> = Vec::with_capacity(n);
    for (i, seg) in lp.iter().enumerate() {
        let j = (i + 1) % n;
        // The segment as it stands at either end of the sweep.
        let copy = |body: &mut Body, angle: f64, v: &[u32]| -> u32 {
            match *seg {
                PSeg::Arc { c, ccw, .. } => {
                    let centre = place(c, angle);
                    let across = a.cross(out(angle));
                    let normal = if ccw { -across } else { across };
                    let reach = body.verts[v[i] as usize] - centre;
                    let curve = body.add_curve(Curve::circle(centre, normal, reach, reach.len()));
                    body.add_edge(curve, 0.0, seg.sweep(), v[i], v[j])
                }
                _ => body.add_line_edge(v[i], v[j]),
            }
        };
        let s = copy(&mut body, from, &vs);
        let e = copy(&mut body, to, &ve);
        starts.push(2 * s);
        ends.push(2 * e + 1);
        let (pa, pb) = (pts[i], pts[j]);
        let (dr, dh) = (pb.x - pa.x, pb.y - pa.y);
        let (surf, sense) = match *seg {
            PSeg::Arc { c, ccw, .. } => {
                let radius = (pa - c).len();
                // Which side of the tube the arc is on: the tube angle is
                // counted from the far side or the near one, whichever
                // keeps the arc away from where it starts over.
                let middle = {
                    let (u, w) = (pa - c, pb - c);
                    let half = 0.5 * seg.sweep() * if ccw { 1.0 } else { -1.0 };
                    let _ = w;
                    u.y.atan2(u.x) + half
                };
                let minor = if middle.cos() < 0.0 { -radius } else { radius };
                (
                    Surface::coil(origin + a * c.y, a, x0, c.x, minor, lead),
                    ccw,
                )
            }
            _ => {
                let len = dr.hypot(dh);
                if len <= TOL {
                    return Err("the screw profile contains a zero-length line".into());
                }
                if dr.abs() <= 1.0e-9 * len {
                    (
                        Surface {
                            f: Frame::from_zx(origin + a * pa.y.min(pb.y), a, x0),
                            ..Surface::cylinder(origin, a, pa.x)
                        },
                        dh > 0.0,
                    )
                } else {
                    // The line runs on to the axis at one height; from
                    // there it rises or falls on its way out, or goes
                    // straight out. Going outwards, the solid is above it.
                    let apex = pa.y - pa.x * dh / dr;
                    let angle = if dh.abs() <= 1.0e-9 * len {
                        std::f64::consts::FRAC_PI_2
                    } else {
                        (dh / dr).signum() * (dr / dh).abs().atan()
                    };
                    (
                        Surface::helicoid(origin + a * apex, a, x0, angle, lead),
                        dr > 0.0,
                    )
                }
            }
        };
        swept.push(Swept {
            surf,
            sense,
            cycle: vec![2 * rails[i], 2 * e, 2 * rails[j] + 1, 2 * s + 1],
            tag: Role::Side {
                lp: 0,
                seg: given[i],
            }
            .tag(),
        });
    }
    ends.reverse();
    for face in &swept {
        let s = body.add_surf(face.surf);
        body.add_tagged_face(s, face.sense, std::slice::from_ref(&face.cycle), face.tag);
    }
    let cap = |normal: V3| {
        let x = reference_x(normal);
        Surface::plane_frame(Frame {
            o: origin,
            x,
            y: normal.cross(x),
            z: normal,
        })
    };
    let s = body.add_surf(cap(-a.cross(out(from))));
    body.add_tagged_face(s, true, &[starts], Role::Start.tag());
    let s = body.add_surf(cap(a.cross(out(to))));
    body.add_tagged_face(s, true, &[ends], Role::End.tag());
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
