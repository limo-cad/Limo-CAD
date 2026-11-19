//! Fillets and chamfers.
//!
//! A blend is modelled as the solid it removes or adds. For every selected
//! edge the kernel builds that solid exactly — a prism for a straight edge,
//! a solid of revolution for a circular one — and applies it with a Boolean.
//! Where the tools of two edges overlap at a corner, the mitre falls out of
//! the Boolean. Where they stop short of each other, at a corner the face
//! they share goes round the outside of, a piece is added for what both
//! would take if they ran on. Where three meet at a square corner a
//! spherical corner piece completes the rolling ball. All of this holds for
//! edges that take material away and for edges that are filled in alike.
//!
//! A tool ends on the face that ends its edge, cut off by that face's own
//! surface where the face is not square to the edge, and is never carried
//! on past the end with nothing to stop it. It may be no wider than the
//! faces beside its edge. An end, a corner or a width that has not been
//! worked out is refused: put together wrongly, any of them still makes a
//! closed solid that looks like the blend asked for.

use std::collections::HashMap;

use crate::boolean::{boolean, Op};
use crate::form::Pipe;
use crate::geom::{Curve, CurveKind, SurfKind, Surface};
use crate::make::{self, PSeg, Region, Seg};
use crate::math::*;
use crate::name::mix;
use crate::ssi::{intersect, Ssi};
use crate::topo::Body;
use crate::trim::edge_closest;

/// Blends smaller than this are below what the Booleans resolve reliably.
const SMALLEST: f64 = 0.01;

#[derive(Clone, Copy, Debug)]
enum Shape {
    Round(f64),
    /// Distance on the first face and, optionally, the angle the bevel makes
    /// with it (radians).
    Bevel(f64, Option<f64>),
}

struct Tool {
    body: Body,
    /// Material is added (a concave edge) rather than removed.
    add: bool,
    /// How far from its edge the blend leaves each of the edge's two faces,
    /// the face of the edge's first half-edge first. Zero where that is not
    /// one distance all along.
    reach: [f64; 2],
}

/// Round the given edges with a constant radius.
pub fn fillet(body: &Body, edges: &[u32], radius: f64) -> Result<Body, String> {
    fillet_as(body, edges, radius, &by_index)
}

/// The tag of an edge when the caller has none to give: its index.
fn by_index(e: u32) -> u64 {
    mix(0xb1e2d, u64::from(e))
}

/// As [`fillet`], with a tag for each edge that the faces of its round are
/// named after (see [`crate::name`]). `tag` is asked about every edge that
/// gets rounded, the ones a picked edge runs on into as well.
pub fn fillet_as(
    body: &Body,
    edges: &[u32],
    radius: f64,
    tag: &dyn Fn(u32) -> u64,
) -> Result<Body, String> {
    if !(radius.is_finite() && radius > TOL) {
        return Err("fillet radius must be greater than zero".into());
    }
    if radius < SMALLEST {
        return Err(format!("fillet radius must be at least {SMALLEST} mm"));
    }
    apply(body, edges, Shape::Round(radius), tag)
}

/// Bevel the given edges. Without an angle both faces lose `distance`; with
/// one, `distance` is measured on the first adjacent face and the bevel makes
/// `angle_deg` with it.
pub fn chamfer(
    body: &Body,
    edges: &[u32],
    distance: f64,
    angle_deg: Option<f64>,
) -> Result<Body, String> {
    chamfer_as(body, edges, distance, angle_deg, &by_index)
}

/// As [`chamfer`], with a tag for each edge that its bevel is named after.
pub fn chamfer_as(
    body: &Body,
    edges: &[u32],
    distance: f64,
    angle_deg: Option<f64>,
    tag: &dyn Fn(u32) -> u64,
) -> Result<Body, String> {
    if !(distance.is_finite() && distance > TOL) {
        return Err("chamfer distance must be greater than zero".into());
    }
    if distance < SMALLEST {
        return Err(format!("chamfer distance must be at least {SMALLEST} mm"));
    }
    if let Some(angle) = angle_deg {
        if !(angle.is_finite() && angle > 0.0 && angle < 90.0) {
            return Err("Chamfer angle must be greater than 0 and less than 90 degrees".into());
        }
    }
    apply(
        body,
        edges,
        Shape::Bevel(distance, angle_deg.map(f64::to_radians)),
        tag,
    )
}

fn apply(
    body: &Body,
    edges: &[u32],
    shape: Shape,
    tag: &dyn Fn(u32) -> u64,
) -> Result<Body, String> {
    if edges.is_empty() {
        return Err("edge refinement needs at least one edge".into());
    }
    let mut picked: Vec<u32> = Vec::with_capacity(edges.len());
    for &e in edges {
        if e as usize >= body.edge_count() {
            return Err(match shape {
                Shape::Round(_) => "referenced fillet edge no longer exists".into(),
                Shape::Bevel(..) => "referenced chamfer edge no longer exists".into(),
            });
        }
        if !picked.contains(&e) {
            picked.push(e);
        }
    }
    // An angled chamfer is measured from one face of each edge; which face
    // that is further along a chain is not tracked yet, so it stays on the
    // picked edges.
    if !matches!(shape, Shape::Bevel(_, Some(_))) {
        chain(body, &mut picked);
    }
    // Every face of a tool is named after the edge the tool is for: what
    // is left of them in the end is the blend on that edge.
    let named = |mut tool: Tool, from: u64| {
        for (f, slot) in tool.body.f_tag.iter_mut().enumerate() {
            *slot = mix(from, f as u64);
        }
        tool
    };
    kinds_agree(body, &picked)?;
    let mut tools: Vec<Tool> = Vec::with_capacity(picked.len());
    let mut reaches: HashMap<u32, [f64; 2]> = HashMap::new();
    for &e in &picked {
        let own = edge_tool_over(body, e, shape, body.e_t[e as usize])?;
        reaches.insert(e, own.reach);
        let (span, ends) = carried(body, e, &picked, own.reach[0].max(own.reach[1]))?;
        let mut tool = if span == body.e_t[e as usize] {
            own
        } else {
            edge_tool_over(body, e, shape, span)?
        };
        // Carried into a leaning or a round face, the tool is cut off by
        // it. A tool that cannot be is not left to end square instead: it
        // would stand proud of that face or stop short of it.
        for end in &ends {
            let op = if end.keep_inside {
                Op::Intersect
            } else {
                Op::Cut
            };
            tool.body = boolean(&tool.body, &end.solid, op).map_err(|_| {
                "the blend could not be ended on the face that ends its edge".to_string()
            })?;
        }
        tools.push(named(tool, tag(e)));
    }
    strips_clear(body, &picked, &reaches)?;
    // The corners where blended edges meet, and which of them a piece of
    // its own finishes.
    let mut finished: Vec<u32> = Vec::new();
    for (tool, around, v) in corner_tools(body, &picked, shape) {
        // A corner belongs to the three edges that meet in it.
        let mut tags = around.map(tag);
        tags.sort_unstable();
        tools.push(named(tool, mix(mix(tags[0], tags[1]), tags[2])));
        finished.push(v);
    }
    for (tool, around, v) in inside_corner_tools(body, &picked, shape) {
        let mut tags = around.map(tag);
        tags.sort_unstable();
        tools.push(named(tool, mix(tags[0], tags[1])));
        finished.push(v);
    }
    corners_finished(body, &picked, &finished)?;
    let mut result = body.clone();
    for tool in &tools {
        let op = if tool.add { Op::Union } else { Op::Cut };
        result = boolean(&result, &tool.body, op)?;
    }
    Ok(result)
}

/// Extend the picked edges along their tangent chains, the way a rolling
/// ball keeps going: through a vertex where one more sharp edge continues the
/// direction of travel and every other edge there is smooth, so the faces on
/// either side run on without a crease.
fn chain(body: &Body, picked: &mut Vec<u32>) {
    let mut at: HashMap<u32, Vec<u32>> = HashMap::new();
    for e in 0..body.edge_count() as u32 {
        let [v0, v1] = body.e_v[e as usize];
        at.entry(v0).or_default().push(e);
        if v1 != v0 {
            at.entry(v1).or_default().push(e);
        }
    }
    let closed = |e: u32| body.e_v[e as usize][0] == body.e_v[e as usize][1];
    // Unit direction in which an open edge leaves one of its vertices.
    let leaving = |e: u32, v: u32| {
        let [t0, t1] = body.e_t[e as usize];
        let c = body.e_curve_ref(e);
        if body.e_v[e as usize][0] == v {
            c.deriv(t0).unit()
        } else {
            -c.deriv(t1).unit()
        }
    };
    // Two edges continue each other when their directions agree to 0.01 rad.
    let aligned = 0.99995;
    let mut i = 0;
    while i < picked.len() {
        let e = picked[i];
        i += 1;
        if closed(e) {
            continue;
        }
        let Some(convex) = convexity(body, e) else {
            continue;
        };
        for v in body.e_v[e as usize] {
            let onward = -leaving(e, v);
            let mut next = None;
            let mut creased = false;
            for &x in &at[&v] {
                if x == e {
                    continue;
                }
                match convexity(body, x) {
                    None => {}
                    Some(turn)
                        if next.is_none()
                            && turn == convex
                            && !closed(x)
                            && leaving(x, v).dot(onward) > aligned =>
                    {
                        next = Some(x);
                    }
                    Some(_) => creased = true,
                }
            }
            if let (Some(x), false) = (next, creased) {
                if !picked.contains(&x) {
                    picked.push(x);
                }
            }
        }
    }
}

/// Is the edge convex (an outside corner)? `None` when the faces are tangent.
pub(crate) fn convexity(body: &Body, e: u32) -> Option<bool> {
    let (f1, f2) = (body.h_face(2 * e), body.h_face(2 * e + 1));
    let [t0, t1] = body.e_t[e as usize];
    let c = body.e_curve_ref(e);
    let t = 0.5 * (t0 + t1);
    let p = c.point(t);
    let d = c.deriv(t).unit();
    let turn = body.f_normal(f1, p).cross(body.f_normal(f2, p)).dot(d);
    (turn.abs() > 1.0e-9).then_some(turn > 0.0)
}

/// How far a blend reaches along each face and, for a round, the centre and
/// radius of its arc.
type Section = (f64, f64, Option<(V2, f64)>);

/// Cross-section of the blend in a plane across the edge: the corner `p`
/// with unit directions `u1`, `u2` running along the two faces away from it.
/// Returns the points where the blend leaves each face and, for a round, the
/// middle of its arc.
fn section(shape: Shape, u1: V2, u2: V2) -> Result<Section, String> {
    let phi = u1.dot(u2).clamp(-1.0, 1.0).acos();
    if phi < 1.0e-6 || phi > PI - 1.0e-6 {
        return Err("the edge is smooth; there is nothing to refine".into());
    }
    match shape {
        Shape::Round(r) => {
            let reach = r / (0.5 * phi).tan();
            let bisector = (u1 + u2) * (1.0 / (u1 + u2).len());
            let centre = bisector * (r / (0.5 * phi).sin());
            Ok((reach, reach, Some((centre, r))))
        }
        Shape::Bevel(d, None) => Ok((d, d, None)),
        Shape::Bevel(d, Some(angle)) => {
            if phi + angle >= PI - 1.0e-9 {
                return Err("chamfer angle does not meet the second face".into());
            }
            Ok((d, d * angle.sin() / (phi + angle).sin(), None))
        }
    }
}

/// What a tool carried past the end of its edge is cut off with: a solid,
/// and whether the tool keeps what is inside it.
struct End {
    solid: Body,
    keep_inside: bool,
}

/// What ends an edge at one of its vertices: the faces there other than
/// the edge's own two.
enum Ending {
    /// None: the edge's own two faces run on past the vertex.
    Nothing,
    /// One face, or several of one surface.
    Face(Surface),
    /// Faces of several surfaces.
    Several,
}

fn ending(body: &Body, e: u32, v: u32) -> Ending {
    let own = [body.h_face(2 * e), body.h_face(2 * e + 1)];
    let mut found: Option<Surface> = None;
    for h in 0..2 * body.edge_count() as u32 {
        if body.h_origin(h) != v || own.contains(&body.h_face(h)) {
            continue;
        }
        let s = *body.f_surface(body.h_face(h));
        match found {
            None => found = Some(s),
            Some(known) if known.same(&s).is_some() => {}
            Some(_) => return Ending::Several,
        }
    }
    found.map_or(Ending::Nothing, Ending::Face)
}

/// The unit direction in which an open edge leaves one of its vertices.
fn leaving(body: &Body, v: u32, e: u32) -> V3 {
    let c = body.e_curve_ref(e);
    let [t0, t1] = body.e_t[e as usize];
    if body.e_v[e as usize][0] == v {
        c.deriv(t0).unit()
    } else {
        -c.deriv(t1).unit()
    }
}

/// Whether two edges that meet at `v` run on into each other without a
/// bend, as the pieces of one chain do.
fn runs_on(body: &Body, v: u32, e1: u32, e2: u32) -> bool {
    let (a, b) = (leaving(body, v, e1), leaving(body, v, e2));
    a.cross(b).len() < 1.0e-6 && a.dot(b) < 0.0
}

const NO_END: &str = "one of these edges ends on a face, or on several, that a blend cannot yet \
                      be ended on; leave that edge sharp";

/// The stretch of its curve that the tool for an edge covers, and what the
/// tool is cut off with at its ends. `width` is how far the blend reaches
/// across the edge.
///
/// A blend ends where its edge does, on the face that ends the edge, and
/// nowhere else. Square to the edge, that face ends the tool where the
/// edge ends. Leaning, or round like the shaft a shoulder runs up against,
/// it does not: cut square, the blend would stop short of a face that
/// leans away and stand proud of one that leans in. The tool is then
/// carried on past the end of the edge and cut off by that face's own
/// surface, on the side the edge is on. This holds for a blend that takes
/// material away and for one that fills an inside edge in alike.
///
/// A tool is never carried on with nothing to stop it. Past the end of its
/// edge it is among whatever else stands there, and would be cut into that
/// or stand on its own in the open.
///
/// Where another blended edge meets this one at a corner, each of the two
/// ends on the third face of the corner, the one that ends them both, as
/// it would alone; what is between them is the business of the corner (see
/// [`corners_finished`]). The pieces of a chain meet end to end, and where
/// three blends meet the corner piece is made to fit tools that end square.
///
/// An end that is none of these is refused: several faces at once, a face
/// of a kind no tool is cut off by yet, an arc that ends on anything but a
/// flat face through its own axis.
fn carried(
    body: &Body,
    e: u32,
    picked: &[u32],
    width: f64,
) -> Result<([f64; 2], Vec<End>), String> {
    let [t0, t1] = body.e_t[e as usize];
    let c = body.e_curve_ref(e);
    if body.e_closed(e) || convexity(body, e).is_none() {
        return Ok(([t0, t1], Vec::new()));
    }
    let middle = c.point(0.5 * (t0 + t1));
    let size = body.bbox().diagonal() + 8.0 * width + 1.0;
    let mut span = [t0, t1];
    let mut ends = Vec::new();
    for (k, (t, away)) in [(t0, -1.0), (t1, 1.0)].into_iter().enumerate() {
        let v = body.e_v[e as usize][k];
        let others: Vec<u32> = picked
            .iter()
            .copied()
            .filter(|&other| other != e && body.e_v[other as usize].contains(&v))
            .collect();
        match others.as_slice() {
            [other] if runs_on(body, v, e, *other) => continue,
            [] | [_] => {}
            _ => continue,
        }
        let s = match ending(body, e, v) {
            Ending::Nothing => continue,
            Ending::Face(s) => s,
            Ending::Several => return Err(NO_END.into()),
        };
        // The way the edge runs on past this end.
        let onward = c.deriv(t).unit() * away;
        let p = c.point(t);
        let solid = match (c.kind, s.kind) {
            // A flat face through the axis of an arc ends its tool where
            // the arc ends.
            (CurveKind::Circle, SurfKind::Plane)
                if s.f.z.dot(c.f.z).abs() < 1.0e-9 && s.sdist(c.f.o).abs() < 10.0 * TOL =>
            {
                continue
            }
            // A flat face square to a straight edge likewise.
            (CurveKind::Line, SurfKind::Plane) if s.f.z.cross(onward).len() < 1.0e-9 => continue,
            (CurveKind::Line, SurfKind::Plane | SurfKind::Cylinder) => {
                // Far enough that the whole section is past the face.
                let normal = s.normal(p);
                let square = normal.dot(onward).abs();
                if square < 0.05 {
                    return Err(NO_END.into());
                }
                let more = width * ((1.0 - square * square).max(0.0).sqrt() / square) * 1.5 + width;
                span[k] = t + away * more;
                let outside = s.sdist(middle) > 0.0;
                if s.sdist(middle).abs() < 10.0 * TOL {
                    return Err(NO_END.into());
                }
                if s.kind == SurfKind::Plane {
                    let towards = if outside { s.f.z } else { -s.f.z };
                    let f = Frame::from_z(s.f.o, towards);
                    let corner = |x: f64, y: f64| s.f.o + f.x * (x * size) + f.y * (y * size);
                    let q = [
                        corner(-1.0, -1.0),
                        corner(1.0, -1.0),
                        corner(1.0, 1.0),
                        corner(-1.0, 1.0),
                    ];
                    let segs = (0..4)
                        .map(|i| Seg::Line {
                            a: q[i],
                            b: q[(i + 1) % 4],
                        })
                        .collect();
                    End {
                        solid: make::extrude(&Region { loops: vec![segs] }, towards, 0.0, size)
                            .map_err(|_| NO_END.to_string())?,
                        keep_inside: true,
                    }
                } else {
                    End {
                        solid: make::cylinder(s.f.o - s.f.z * size, s.f.z, s.r, 2.0 * size)
                            .map_err(|_| NO_END.to_string())?,
                        keep_inside: !outside,
                    }
                }
            }
            _ => return Err(NO_END.into()),
        };
        ends.push(solid);
    }
    Ok((span, ends))
}

/// The way into face `f` from a point of its boundary: across half-edge
/// `h` at parameter `t` of its edge, to the left of the way `h` runs.
fn into(body: &Body, f: u32, h: u32, t: f64) -> V3 {
    let c = body.e_curve_ref(h >> 1);
    let mut travel = c.deriv(t).unit();
    if h & 1 == 1 {
        travel = -travel;
    }
    body.f_normal(f, c.point(t)).cross(travel)
}

/// How far out from half-edge `own` of face `f`, across the face, the
/// boundary of the face comes back at half-edge `other`: the least distance
/// of a point of `other` that lies beside `own`, on the face's side of it.
/// A point past either end of `own` is not beside it, and one behind it is
/// across some other stretch of the face.
///
/// `theirs` is the width of a strip along `other` that is taken as well.
/// Where that strip lies towards `own`, the room between the two is less
/// by that much.
fn room(body: &Body, f: u32, own: u32, other: u32, theirs: f64) -> f64 {
    let (e, g) = (own >> 1, other >> 1);
    let (c, ce) = (body.e_curve_ref(g), body.e_curve_ref(e));
    let [t0, t1] = body.e_t[g as usize];
    let from = |t: f64| {
        let p = c.point(t);
        let (at, d, within) = edge_closest(body, e, p);
        let foot = ce.point(at);
        if !within || (p - foot).dot(into(body, f, own, at)) <= 0.0 {
            return f64::MAX;
        }
        if (foot - p).dot(into(body, f, other, t)) > 0.0 {
            d - theirs
        } else {
            d
        }
    };
    const STEPS: usize = 96;
    let step = (t1 - t0) / STEPS as f64;
    let (mut least, mut nearest) = (f64::MAX, 0usize);
    for i in 0..=STEPS {
        let d = from(t0 + step * i as f64);
        if d < least {
            (least, nearest) = (d, i);
        }
    }
    if least == f64::MAX {
        return least;
    }
    // Between the samples either side of the nearest one.
    let mut lo = (t0 + step * (nearest as f64 - 1.0)).max(t0);
    let mut hi = (t0 + step * (nearest as f64 + 1.0)).min(t1);
    for _ in 0..60 {
        let (a, b) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
        if from(a) < from(b) {
            hi = b;
        } else {
            lo = a;
        }
    }
    least.min(from(0.5 * (lo + hi)))
}

/// Whether a loop of a flat face goes round a hole in the face and not
/// round the face itself: it runs the other way about the face's normal.
fn goes_round_a_hole(body: &Body, f: u32, l: u32) -> bool {
    let mut points: Vec<V3> = Vec::new();
    for h in body.loop_hes(l) {
        let c = body.e_curve_ref(h >> 1);
        let (a, b) = body.h_span(h);
        let steps = if c.kind == CurveKind::Line { 1 } else { 16 };
        points.extend((0..steps).map(|i| c.point(a + (b - a) * i as f64 / steps as f64)));
    }
    let Some(&o) = points.first() else {
        return false;
    };
    let normal = body.f_normal(f, o);
    let twice: f64 = (0..points.len())
        .map(|i| {
            (points[i] - o)
                .cross(points[(i + 1) % points.len()] - o)
                .dot(normal)
        })
        .sum();
    twice < 0.0
}

/// A blend covers a strip of each of its two faces, from the edge out to
/// where it leaves the face. Along the whole edge that strip has to be the
/// face's own and nothing else's. A blend wider than its face would run on
/// past the far side of it, into whatever is there or into the open, and
/// what it left would not be a blend between these two faces. A boss or
/// another blend standing in the strip would be cut into or built over.
/// Either is refused.
///
/// One thing may lie in the strip of a blend that takes material away: a
/// hole in a flat face, which has nothing in it to take. The blend runs
/// over it and is cut through by it.
///
/// The two edges that the face's boundary runs on into at the ends of this
/// one are left out: those are the ends of the blend, which [`carried`]
/// sees to.
fn strips_clear(
    body: &Body,
    picked: &[u32],
    reaches: &HashMap<u32, [f64; 2]>,
) -> Result<(), String> {
    for &e in picked {
        let takes = convexity(body, e) == Some(true);
        for k in 0..2u32 {
            let h = 2 * e + k;
            let width = reaches[&e][k as usize];
            if width <= 0.0 {
                continue;
            }
            let f = body.h_face(h);
            let own = body.h_loop[h as usize];
            let ring: Vec<u32> = body.loop_hes(own).collect();
            let at = ring.iter().position(|&x| x == h).unwrap_or(0);
            let before = ring[(at + ring.len() - 1) % ring.len()] >> 1;
            let after = ring[(at + 1) % ring.len()] >> 1;
            for l in body.face_loops(f) {
                let hole = takes
                    && l != own
                    && body.f_surface(f).is_plane()
                    && goes_round_a_hole(body, f, l);
                for other in body.loop_hes(l) {
                    let g = other >> 1;
                    if g == e || g == before || g == after {
                        continue;
                    }
                    let theirs = reaches.get(&g).map(|r| r[(other & 1) as usize]);
                    if hole && theirs.is_none() && convexity(body, g) == Some(true) {
                        continue;
                    }
                    if room(body, f, h, other, theirs.unwrap_or(0.0)) < width - 1.0e-7 {
                        return Err(
                            "the blend is too large for one of the faces beside its edge: it \
                             would run off that face or into something else on it"
                                .into(),
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

/// The tool for edge `e` over a stretch of its curve other than the
/// edge's own: the same section, carried on past an end.
fn edge_tool_over(body: &Body, e: u32, shape: Shape, span: [f64; 2]) -> Result<Tool, String> {
    let convex = convexity(body, e)
        .ok_or_else(|| "the edge is smooth; there is nothing to refine".to_string())?;
    let (f1, f2) = (body.h_face(2 * e), body.h_face(2 * e + 1));
    let (s1, s2) = (body.f_surface(f1), body.f_surface(f2));
    let c = *body.e_curve_ref(e);
    let [t0, t1] = span;
    let (tool, reach) = match (c.kind, s1.kind, s2.kind) {
        (CurveKind::Line, SurfKind::Plane, SurfKind::Plane) => {
            let d = c.f.x;
            let p = c.point(t0);
            // Into each face, away from the edge.
            let u1 = body.f_normal(f1, p).cross(d).unit();
            let u2 = d.cross(body.f_normal(f2, p)).unit();
            // Plane coordinates across the edge.
            let ex = u1;
            let ey = d.cross(u1).unit();
            let flat = |v: V3| v2(v.dot(ex), v.dot(ey));
            let lift = |q: V2| p + ex * q.x + ey * q.y;
            let (q1, q2) = (flat(u1), flat(u2));
            let (reach1, reach2, arc) = section(shape, q1, q2)?;
            let (a1, a2) = (lift(q1 * reach1), lift(q2 * reach2));
            let mut segs = vec![Seg::Line { a: p, b: a1 }];
            match arc {
                Some((centre, r)) => {
                    let mid = centre - centre * (r / centre.len());
                    segs.push(Seg::Arc {
                        a: a1,
                        m: lift(mid),
                        b: a2,
                    });
                }
                None => segs.push(Seg::Line { a: a1, b: a2 }),
            }
            segs.push(Seg::Line { a: a2, b: p });
            (
                make::extrude(&Region { loops: vec![segs] }, d, 0.0, t1 - t0)?,
                [reach1, reach2],
            )
        }
        (
            CurveKind::Line,
            SurfKind::Plane | SurfKind::Cylinder,
            SurfKind::Plane | SurfKind::Cylinder,
        ) => {
            // A straight edge along a cylinder, against a plane or another
            // cylinder: across the edge the faces are a circle and a line,
            // or two circles.
            let d = c.f.x;
            let p = c.point(t0);
            let u1 = body.f_normal(f1, p).cross(d).unit();
            let u2 = d.cross(body.f_normal(f2, p)).unit();
            let ex = u1;
            let ey = d.cross(u1).unit();
            let flat = |v: V3| v2(v.dot(ex), v.dot(ey));
            let lift = |q: V2| p + ex * q.x + ey * q.y;
            let across = |f: u32, into: V3| -> Across {
                let s = body.f_surface(f);
                let normal = flat(body.f_normal(f, p));
                match s.kind {
                    SurfKind::Plane => Across::Line {
                        normal: normal.unit(),
                        at: 0.0,
                        into: flat(into),
                    },
                    _ => {
                        let centre = flat(s.f.o - p);
                        Across::Circle {
                            centre,
                            radius: s.r,
                            // Whether the face looks away from the axis.
                            outward: normal.dot(centre) < 0.0,
                            into: flat(into),
                        }
                    }
                }
            };
            let (a1, a2) = (across(f1, u1), across(f2, u2));
            // Blends sit in the material of a convex edge, in the air of a
            // concave one.
            let side = if convex { -1.0 } else { 1.0 };
            let (q1, q2, join) = match shape {
                Shape::Round(r) => {
                    let centre = a1
                        .offset(side * r)
                        .meet(&a2.offset(side * r))
                        .into_iter()
                        .filter(|o| a1.ahead(a1.foot(*o)) && a2.ahead(a2.foot(*o)))
                        .min_by(|x, y| x.len().total_cmp(&y.len()))
                        .ok_or("the blend is larger than the faces it joins")?;
                    let (q1, q2) = (a1.foot(centre), a2.foot(centre));
                    let mid = centre + ((q1 - centre) + (q2 - centre)).unit() * r;
                    (
                        q1,
                        q2,
                        Seg::Arc {
                            a: lift(q1),
                            m: lift(mid),
                            b: lift(q2),
                        },
                    )
                }
                Shape::Bevel(distance, None) => {
                    let (q1, q2) = (a1.along(distance), a2.along(distance));
                    (
                        q1,
                        q2,
                        Seg::Line {
                            a: lift(q1),
                            b: lift(q2),
                        },
                    )
                }
                Shape::Bevel(_, Some(_)) => return Err(unsupported_edge()),
            };
            let segs = vec![
                a1.run(v2(0.0, 0.0), q1, &lift),
                join,
                a2.run(q2, v2(0.0, 0.0), &lift),
            ];
            (
                make::extrude(&Region { loops: vec![segs] }, d, 0.0, t1 - t0)?,
                [q1.len(), q2.len()],
            )
        }
        (
            CurveKind::Circle,
            SurfKind::Plane | SurfKind::Cylinder | SurfKind::Cone,
            SurfKind::Plane | SurfKind::Cylinder | SurfKind::Cone,
        ) => {
            // Both faces are turned about the axis of the edge: a flat face
            // square to it, a cylinder or a cone round it. In the half-plane
            // through the axis each is a straight line from the corner.
            let axis = c.f.z;
            let turned = |s: &Surface| {
                s.f.z.cross(axis).len() < 1.0e-9
                    && (s.kind == SurfKind::Plane || {
                        let w = c.f.o - s.f.o;
                        (w - s.f.z * w.dot(s.f.z)).len() < 10.0 * TOL
                    })
            };
            if !(turned(s1) && turned(s2)) {
                return Err(unsupported_edge());
            }
            let p0 = c.point(t0);
            let radial = (p0 - c.f.o).unit();
            // Which way each face leaves the edge, in that half-plane.
            let leaves = |f: u32| {
                let along = c.deriv(t0).unit();
                let travel = if body.h_face(2 * e) == f {
                    along
                } else {
                    -along
                };
                let into = body.f_normal(f, p0).cross(travel);
                v2(into.dot(radial), into.dot(axis)).unit()
            };
            let (u1, u2) = (leaves(f1), leaves(f2));
            let (reach1, reach2, arc) = section(shape, u1, u2)?;
            let corner = v2(c.r, 0.0);
            let (a1, a2) = (corner + u1 * reach1, corner + u2 * reach2);
            if a1.x <= TOL || a2.x <= TOL {
                return Err(
                    "the blend is larger than the radius of the circular edge it follows".into(),
                );
            }
            let mut lp = vec![PSeg::Line { a: corner, b: a1 }];
            match arc {
                Some((centre, _)) => {
                    let centre = corner + centre;
                    let ccw = (a1 - centre).cross(a2 - centre) > 0.0;
                    lp.push(PSeg::Arc {
                        a: a1,
                        b: a2,
                        c: centre,
                        ccw,
                    });
                }
                None => lp.push(PSeg::Line { a: a1, b: a2 }),
            }
            lp.push(PSeg::Line { a: a2, b: corner });
            (
                make::revolve(&[lp], c.f.o, axis, radial, t1 - t0)?,
                [reach1, reach2],
            )
        }
        // Any other edge that closes on itself is rounded by rolling the
        // ball along it.
        _ if body.e_closed(e) && matches!(shape, Shape::Round(_)) => {
            let Shape::Round(radius) = shape else {
                unreachable!()
            };
            return rim_tool(body, e, radius, convex);
        }
        _ => {
            if crate::diag::trace() {
                eprintln!(
                    "blend: no tool for a {:?} edge between {:?} (r {}) and {:?} (r {}), convex {convex}, from {:?} to {:?}",
                    c.kind,
                    s1.kind,
                    s1.r,
                    s2.kind,
                    s2.r,
                    c.point(t0),
                    c.point(t1)
                );
            }
            return Err(unsupported_edge());
        }
    };
    Ok(Tool {
        body: tool,
        add: !convex,
        reach,
    })
}

/// A face as it looks in the plane across a straight edge, with the edge
/// at the origin: a line or a circle through it, or either of those moved
/// off along its normal. `into` points along the face away from the edge.
#[derive(Clone, Copy)]
enum Across {
    Line {
        /// Unit normal, and how far along it the line lies.
        normal: V2,
        at: f64,
        into: V2,
    },
    Circle {
        centre: V2,
        radius: f64,
        outward: bool,
        into: V2,
    },
}

impl Across {
    /// The same face moved by `by` along its normal.
    fn offset(&self, by: f64) -> Across {
        match *self {
            Across::Line { normal, at, into } => Across::Line {
                normal,
                at: at + by,
                into,
            },
            Across::Circle {
                centre,
                radius,
                outward,
                into,
            } => Across::Circle {
                centre,
                radius: radius + if outward { by } else { -by },
                outward,
                into,
            },
        }
    }

    /// A point of the line and its direction; only for lines.
    fn line(&self) -> (V2, V2) {
        let Across::Line { normal, at, .. } = *self else {
            unreachable!()
        };
        (normal * at, v2(-normal.y, normal.x))
    }

    /// Where two faces cross.
    fn meet(&self, other: &Across) -> Vec<V2> {
        match (*self, *other) {
            (Across::Line { .. }, Across::Line { .. }) => {
                let ((p, d), (q, e)) = (self.line(), other.line());
                let det = d.cross(e);
                if det.abs() < 1.0e-12 {
                    return Vec::new();
                }
                vec![p + d * ((q - p).cross(e) / det)]
            }
            (Across::Line { .. }, Across::Circle { centre, radius, .. }) => {
                let (p, d) = self.line();
                // Foot of the centre on the line, and half the chord.
                let along = (centre - p).dot(d);
                let foot = p + d * along;
                let half2 = radius * radius - (centre - foot).dot(centre - foot);
                if radius <= 0.0 || half2 < 0.0 {
                    return Vec::new();
                }
                let half = half2.sqrt();
                vec![foot + d * half, foot - d * half]
            }
            (Across::Circle { .. }, Across::Line { .. }) => other.meet(self),
            (
                Across::Circle {
                    centre: c1,
                    radius: r1,
                    ..
                },
                Across::Circle {
                    centre: c2,
                    radius: r2,
                    ..
                },
            ) => {
                let between = c2 - c1;
                let apart = between.len();
                if r1 <= 0.0 || r2 <= 0.0 || apart < 1.0e-12 {
                    return Vec::new();
                }
                let along = (apart * apart + r1 * r1 - r2 * r2) / (2.0 * apart);
                let half2 = r1 * r1 - along * along;
                if half2 < 0.0 {
                    return Vec::new();
                }
                let unit = between * (1.0 / apart);
                let (foot, side) = (c1 + unit * along, v2(-unit.y, unit.x) * half2.sqrt());
                vec![foot + side, foot - side]
            }
        }
    }

    /// The point of the face nearest `q`.
    fn foot(&self, q: V2) -> V2 {
        match *self {
            Across::Line { normal, at, .. } => q - normal * (q.dot(normal) - at),
            Across::Circle { centre, radius, .. } => {
                centre + (q - centre) * (radius / (q - centre).len())
            }
        }
    }

    /// Whether a point of the face lies on the side the face runs to.
    fn ahead(&self, q: V2) -> bool {
        let (Across::Line { into, .. } | Across::Circle { into, .. }) = *self;
        q.dot(into) > 1.0e-12
    }

    /// The point `distance` along the face from the edge.
    fn along(&self, distance: f64) -> V2 {
        match *self {
            Across::Line { into, .. } => into * distance,
            Across::Circle {
                centre,
                radius,
                into,
                ..
            } => {
                // Round the circle, the way `into` leaves the edge.
                let from = v2(0.0, 0.0) - centre;
                let turn = from.cross(into).signum() * distance / radius;
                let (sin, cos) = turn.sin_cos();
                centre + v2(from.x * cos - from.y * sin, from.x * sin + from.y * cos)
            }
        }
    }

    /// The stretch of the face from `a` to `b`, as a profile segment.
    fn run(&self, a: V2, b: V2, lift: &dyn Fn(V2) -> V3) -> Seg {
        match *self {
            Across::Line { .. } => Seg::Line {
                a: lift(a),
                b: lift(b),
            },
            Across::Circle { centre, radius, .. } => Seg::Arc {
                a: lift(a),
                m: lift(centre + ((a - centre) + (b - centre)).unit() * radius),
                b: lift(b),
            },
        }
    }
}

fn unsupported_edge() -> String {
    "this kind of edge cannot be refined by the Limo kernel yet (supported: straight edges \
     between flat faces and cylinders, circular edges, and rounds on edges of any shape \
     that close on themselves)"
        .into()
}

/// The tool for a round on an edge that closes on itself, whatever curve
/// it is: the rim where a rod is cut off on a slant, where one bore runs
/// into another.
///
/// The ball's centre runs along the spine, where the two faces moved by
/// the radius meet. What the round takes away (or, in an inside edge,
/// fills) is a ring with three faces: a strip of each of the two faces,
/// from the edge to where the ball touches, and between those two curves
/// the ball's own surface. The strips lie in the very surfaces of the
/// body, so the Boolean that applies the tool finds them coincident, and
/// the curves the ball touches along are edges of the tool: nothing has to
/// find where two surfaces that only touch would meet.
fn rim_tool(body: &Body, e: u32, radius: f64, convex: bool) -> Result<Tool, String> {
    let (f1, f2) = (body.h_face(2 * e), body.h_face(2 * e + 1));
    let (s1, s2) = (*body.f_surface(f1), *body.f_surface(f2));
    let c = *body.e_curve_ref(e);
    let [t0, t1] = body.e_t[e as usize];
    let p0 = c.point(t0);
    // Each surface moved by the radius towards where the centre runs: into
    // the material at an outside edge, out of it at an inside one.
    let moved = |f: u32, s: &Surface| -> Result<Surface, String> {
        let out = if body.f_sense[f as usize] { 1.0 } else { -1.0 };
        let by = if convex { -radius } else { radius } * out;
        s.offset(by).ok_or_else(|| {
            "the round is too large for one of the faces it rolls on, or that face is of a \
             kind a ball cannot yet be rolled on"
                .to_string()
        })
    };
    let (o1, o2) = (moved(f1, &s1)?, moved(f2, &s2)?);
    let Ssi::Curves(found) = intersect(&o1, &o2) else {
        return Err(unsupported_edge());
    };
    // Of the curves the two moved surfaces meet in, the closed one that
    // passes the edge closest.
    let spine = found
        .into_iter()
        .filter(Curve::periodic)
        .min_by(|a, b| a.dist(p0).total_cmp(&b.dist(p0)))
        .ok_or_else(unsupported_edge)?;
    if spine.dist(p0) > 20.0 * radius {
        return Err("the round is too large for the edge it is asked on".into());
    }
    let from = spine.param(p0);
    let span = [from, from + TAU];
    let pipe = Surface::pipe(Pipe::about(spine, span, true, radius, s1, s2)?);
    let twin = Surface::pipe(Pipe::about(spine, span, true, radius, s2, s1)?);
    let (touch1, touch2) = (Curve::iso(&pipe, 0.0, span), Curve::iso(&twin, 0.0, span));
    // Whether the spine runs the way the edge does.
    let along = spine.deriv(from).dot(c.deriv(t0)) > 0.0;
    // The strips are on the left of the edge as each face of the body has
    // it, and of the touching curve run the other way; the ball's face
    // takes the two touching curves the other way again. A tool that adds
    // material faces the opposite way throughout.
    let build = |senses: [bool; 3]| -> Body {
        let mut tool = Body::new();
        let rim = {
            let v = tool.add_vert(p0);
            let k = tool.add_curve(c);
            tool.add_edge(k, t0, t1, v, v)
        };
        let closed = |tool: &mut Body, curve: Curve| {
            let v = tool.add_vert(curve.point(from));
            let k = tool.add_curve(curve);
            tool.add_edge(k, from, from + TAU, v, v)
        };
        let (e1, e2) = (closed(&mut tool, touch1), closed(&mut tool, touch2));
        let flip = u32::from(!convex);
        let back = u32::from(along);
        let surfaces = [s1, s2, pipe].map(|s| tool.add_surf(s));
        tool.add_face(
            surfaces[0],
            senses[0],
            &[vec![(2 * rim) ^ flip], vec![(2 * e1 + back) ^ flip]],
        );
        tool.add_face(
            surfaces[1],
            senses[1],
            &[vec![(2 * rim + 1) ^ flip], vec![(2 * e2 + 1 - back) ^ flip]],
        );
        tool.add_face(
            surfaces[2],
            senses[2],
            &[
                vec![(2 * e1 + 1 - back) ^ flip],
                vec![(2 * e2 + back) ^ flip],
            ],
        );
        tool
    };
    // The strips face the way the faces they lie in do (or the opposite
    // way, on a tool that adds). The ball's face looks towards the spine:
    // the tool is what lies outside the ball.
    let senses = [
        body.f_sense[f1 as usize] == convex,
        body.f_sense[f2 as usize] == convex,
        pipe.pipe_form().hand < 0.0,
    ];
    let tool = build(senses);
    tool.check()?;
    // Every face must lie to the left of its first edge, seen from
    // outside: towards the other curve that bounds it. If one does not,
    // the ball does not fit the edge the way this was put together.
    for f in 0..3u32 {
        let loops: Vec<u32> = tool.face_loops(f).collect();
        let (h, across) = (
            tool.l_first[loops[0] as usize],
            tool.l_first[loops[1] as usize],
        );
        let (edge, other) = (tool.e_curve_ref(h >> 1), tool.e_curve_ref(across >> 1));
        let [a, b] = tool.e_t[(h >> 1) as usize];
        let t = a + 0.37 * (b - a);
        let p = edge.point(t);
        let travel = edge.deriv(t).unit() * if h & 1 == 0 { 1.0 } else { -1.0 };
        let left = tool.f_normal(f, p).cross(travel);
        let towards = other.point(other.param(p)) - p;
        if left.dot(towards) <= 0.0 {
            return Err("the round does not fit the edge it is asked on".into());
        }
    }
    Ok(Tool {
        body: tool,
        add: !convex,
        reach: [0.0; 2],
    })
}

/// Where two blended edges of one flat face meet at a corner that turns
/// inwards (the inside corner of an L, seen from above), each blend has to
/// run on past the corner until it meets the other. Each edge's own tool
/// stops at the corner; what is missing is the part both tools would take
/// if they ran on: the piece of the corner that lies in both.
///
/// The two edges are both outside edges, as on top of an L-shaped block, or
/// both inside edges, as round the foot of a boss that stands on a plate.
/// In the second case each tool fills its edge in, and the piece fills the
/// corner between them: without it the two rounds end square and leave a
/// notch the size of the radius where they should run into each other.
fn inside_corner_tools(body: &Body, edges: &[u32], shape: Shape) -> Vec<(Tool, [u32; 2], u32)> {
    // How far a blend can reach across the face it shares with the other.
    let reach = match shape {
        Shape::Round(r) => 2.0 * r,
        Shape::Bevel(d, _) => 4.0 * d,
    };
    // A straight edge between flat faces, and which way it bends.
    let bend = |e: u32| {
        (body.e_curve_ref(e).kind == CurveKind::Line
            && body.f_surface(body.h_face(2 * e)).is_plane()
            && body.f_surface(body.h_face(2 * e + 1)).is_plane())
        .then(|| convexity(body, e))
        .flatten()
    };
    let mut at: HashMap<Corner, Vec<u32>> = HashMap::new();
    // Every blended edge at a vertex, whatever its kind.
    let mut all: HashMap<u32, usize> = HashMap::new();
    for &e in edges {
        let [v0, v1] = body.e_v[e as usize];
        if v0 == v1 {
            continue;
        }
        for v in [v0, v1] {
            *all.entry(v).or_default() += 1;
        }
        if let Some(convex) = bend(e) {
            at.entry((v0, convex)).or_default().push(e);
            at.entry((v1, convex)).or_default().push(e);
        }
    }
    let mut corners: Vec<(Corner, Vec<u32>)> = at
        .into_iter()
        // Two inside edges with a third blended edge between them are not
        // this corner: what the third makes of it is not known here.
        .filter(|((v, convex), met)| met.len() == 2 && (*convex || all[v] == 2))
        .collect();
    corners.sort_by_key(|(corner, _)| *corner);
    let mut tools = Vec::new();
    for ((v, convex), met) in corners {
        let (e1, e2) = (met[0], met[1]);
        if turns_inwards(body, v, e1, e2) != Some(true) {
            continue;
        }
        // Each tool about the corner, carried on past it. Where the faces
        // lean, what both take begins before the corner.
        let past = |e: u32| -> Option<Tool> {
            let [t0, t1] = body.e_t[e as usize];
            let span = if body.e_v[e as usize][1] == v {
                [t1 - reach, t1 + reach]
            } else {
                [t0 - reach, t0 + reach]
            };
            edge_tool_over(body, e, shape, span).ok()
        };
        let (Some(a), Some(b)) = (past(e1), past(e2)) else {
            continue;
        };
        if let Ok(piece) = boolean(&a.body, &b.body, Op::Intersect) {
            tools.push((
                Tool {
                    body: piece,
                    add: !convex,
                    reach: [0.0; 2],
                },
                [e1, e2],
                v,
            ));
        }
    }
    tools
}

/// Two edges that meet at `v` and bound one flat face between them: whether
/// the boundary of that face turns inwards there, as at the inside corner
/// of an L seen from above. `None` when they bound no flat face together.
fn turns_inwards(body: &Body, v: u32, e1: u32, e2: u32) -> Option<bool> {
    // The face both edges bound, and the way its boundary runs through
    // the corner: in along one edge, out along the other.
    let faces = |e: u32| [body.h_face(2 * e), body.h_face(2 * e + 1)];
    let shared = faces(e1)
        .into_iter()
        .find(|f| faces(e2).contains(f) && body.f_surface(*f).is_plane())?;
    let on = |e: u32| {
        if body.h_face(2 * e) == shared {
            2 * e
        } else {
            2 * e + 1
        }
    };
    let (h1, h2) = (on(e1), on(e2));
    let (arriving, leaving) = if body.h_dest(h1) == v && body.h_origin(h2) == v {
        (h1, h2)
    } else if body.h_dest(h2) == v && body.h_origin(h1) == v {
        (h2, h1)
    } else {
        return None;
    };
    let normal = body.f_normal(shared, body.verts[v as usize]);
    // Walking with the face on the left, a turn to the right is a corner
    // that turns inwards.
    Some(body.h_dir(arriving).cross(body.h_dir(leaving)).dot(normal) < -1.0e-9)
}

const MIXED: &str = "an inside edge and an outside edge that meet at a corner cannot be blended \
                     together yet; blend them one at a time, or leave one of them sharp";

/// An inside edge and an outside edge that meet at a corner are not blended
/// together: said before any tool is made, so that it is what is said.
fn kinds_agree(body: &Body, picked: &[u32]) -> Result<(), String> {
    let mut at: HashMap<u32, Option<bool>> = HashMap::new();
    for &e in picked {
        let [v0, v1] = body.e_v[e as usize];
        if v0 == v1 {
            continue;
        }
        let kind = convexity(body, e);
        for v in [v0, v1] {
            if at.insert(v, kind).is_some_and(|known| known != kind) {
                return Err(MIXED.into());
            }
        }
    }
    Ok(())
}

/// Every corner where blended edges meet has to be one this module knows
/// how to finish. The tools of the edges that meet there are exact each on
/// its own; what they make of the corner between them is only right where
/// it has been worked out, and anywhere else the result would be a solid
/// that looks like the blend asked for and is not. Such a corner is
/// refused.
///
/// Known are: two edges that run on into each other smoothly; two edges of
/// one kind, outside or inside, that bound a flat face between them, whose
/// tools either overlap at the corner or have a piece to fill it; and three
/// edges of one kind at a corner of three flat faces, with its piece.
fn corners_finished(body: &Body, picked: &[u32], finished: &[u32]) -> Result<(), String> {
    let mut at: HashMap<u32, Vec<u32>> = HashMap::new();
    for &e in picked {
        let [v0, v1] = body.e_v[e as usize];
        if v0 != v1 {
            at.entry(v0).or_default().push(e);
            at.entry(v1).or_default().push(e);
        }
    }
    let mut corners: Vec<(u32, Vec<u32>)> =
        at.into_iter().filter(|(_, met)| met.len() > 1).collect();
    corners.sort_by_key(|(v, _)| *v);
    let straight = |e: u32| {
        body.e_curve_ref(e).kind == CurveKind::Line
            && body.f_surface(body.h_face(2 * e)).is_plane()
            && body.f_surface(body.h_face(2 * e + 1)).is_plane()
    };
    for (v, met) in corners {
        let kinds: Vec<Option<bool>> = met.iter().map(|&e| convexity(body, e)).collect();
        if kinds.iter().any(|kind| *kind != kinds[0]) {
            return Err(MIXED.into());
        }
        let known = match met.as_slice() {
            &[e1, e2] => {
                runs_on(body, v, e1, e2)
                    || match turns_inwards(body, v, e1, e2) {
                        // The tools stop short of each other: only the
                        // piece between them makes the corner whole.
                        Some(true) => straight(e1) && straight(e2) && finished.contains(&v),
                        // The tools overlap, and what both take or both
                        // fill is the corner.
                        Some(false) => true,
                        None => false,
                    }
            }
            [_, _, _] => finished.contains(&v),
            _ => false,
        };
        if !known {
            return Err(
                "blended edges meet at a corner of a kind that cannot be finished yet; leave one of the edges there sharp"
                    .into(),
            );
        }
    }
    Ok(())
}

/// A vertex, and whether the blended edges gathered at it are outside edges.
type Corner = (u32, bool);

/// Corner pieces: where three rounded edges meet at a corner of three flat
/// faces, the rolling ball comes to rest against all three and leaves a
/// sphere. The piece is the corner out to where the ball touches, minus
/// that ball: a cube at a square corner, a slanted box at any other.
///
/// At an outside corner, the corner of a box, the piece is of the body and
/// is taken away. At an inside corner, the corner of a pocket, it is of the
/// room the three faces enclose and is filled in. Without it the three
/// rounds cross each other as three cylinders, which leaves more of an
/// outside corner than the ball does, and less in an inside one.
fn corner_tools(body: &Body, edges: &[u32], shape: Shape) -> Vec<(Tool, [u32; 3], u32)> {
    let mut at: HashMap<Corner, Vec<(V3, u32)>> = HashMap::new();
    for &e in edges {
        let c = body.e_curve_ref(e);
        let straight = c.kind == CurveKind::Line
            && body.f_surface(body.h_face(2 * e)).is_plane()
            && body.f_surface(body.h_face(2 * e + 1)).is_plane();
        let Some(convex) = convexity(body, e).filter(|_| straight) else {
            continue;
        };
        let [v0, v1] = body.e_v[e as usize];
        at.entry((v0, convex)).or_default().push((c.f.x, e));
        at.entry((v1, convex)).or_default().push((-c.f.x, e));
    }
    // A corner of three faces has three edges, and all of them are these.
    let mut edges_at: HashMap<u32, usize> = HashMap::new();
    for e in 0..body.edge_count() as u32 {
        for v in body.e_v[e as usize] {
            *edges_at.entry(v).or_default() += 1;
        }
    }
    let mut corners: Vec<(Corner, Vec<(V3, u32)>)> = at
        .into_iter()
        .filter(|((v, _), d)| d.len() == 3 && edges_at[v] == 3)
        .collect();
    corners.sort_by_key(|(corner, _)| *corner);
    let mut tools = Vec::new();
    for ((v, convex), met) in corners {
        let d = [met[0].0, met[1].0, met[2].0];
        // The edges run along the piece either way: at an outside corner
        // it is the body between them, at an inside corner the room.
        let piece = match shape {
            Shape::Round(radius) => corner_piece(body.verts[v as usize], d[0], d[1], d[2], radius),
            Shape::Bevel(distance, None) => {
                facet_piece(body.verts[v as usize], d[0], d[1], d[2], distance)
            }
            Shape::Bevel(_, Some(_)) => continue,
        };
        if let Ok(piece) = piece {
            tools.push((
                Tool {
                    body: piece,
                    add: !convex,
                    reach: [0.0; 2],
                },
                [met[0].1, met[1].1, met[2].1],
                v,
            ));
        }
    }
    tools
}

/// Where three even bevels meet at a square corner, their three planes
/// run together in a point half the bevel's distance in from each face.
/// The corner is not left as that point: it is cut flat, through the three
/// places where two of the bevels meet on a face. The piece is what lies
/// between the point and that flat, a pyramid on a triangle.
fn facet_piece(v: V3, a: V3, b: V3, c: V3, d: f64) -> Result<Body, String> {
    let square = a.dot(b).abs() < 1.0e-9 && b.dot(c).abs() < 1.0e-9 && a.dot(c).abs() < 1.0e-9;
    if !square {
        return Err("bevels that meet at a corner that is not square are not supported yet".into());
    }
    let pts = [
        v + (a + b + c) * (0.5 * d),
        v + (a + b) * d,
        v + (b + c) * d,
        v + (c + a) * d,
    ];
    let mut body = Body::new();
    for p in pts {
        body.add_vert(p);
    }
    let mut edge_of: HashMap<(u32, u32), u32> = HashMap::new();
    for p in 0..4u32 {
        for q in p + 1..4 {
            edge_of.insert((p, q), body.add_line_edge(p, q));
        }
    }
    let centre = (pts[0] + pts[1] + pts[2] + pts[3]) * 0.25;
    for cycle in [[0u32, 1, 2], [0, 2, 3], [0, 3, 1], [1, 2, 3]] {
        let [p, q, r] = cycle.map(|i| pts[i as usize]);
        let mut normal = (q - p).cross(r - p).unit();
        let mut order = cycle;
        // Seen from outside the piece, its corners go round to the left.
        if normal.dot(p - centre) < 0.0 {
            normal = -normal;
            order.reverse();
        }
        let hes: Vec<u32> = (0..3)
            .map(|i| {
                let (p, q) = (order[i], order[(i + 1) % 3]);
                let e = edge_of[&(p.min(q), p.max(q))];
                2 * e + u32::from(body.e_v[e as usize][0] != p)
            })
            .collect();
        let surf = body.add_surf(Surface::plane(p, normal));
        body.add_face(surf, true, &[hes]);
    }
    body.check()?;
    Ok(body)
}

/// The corner at `v` between the unit directions `a`, `b` and `c` of its
/// three edges, out to where a ball of radius `r` resting in it touches,
/// minus that ball.
///
/// The ball's centre is `r` from each of the three faces. Each face is
/// touched at one point; across each edge, the plane through the centre
/// cuts the piece off where the round along that edge takes over, and the
/// ball shows in it as an arc between two of the touching points. At a
/// square corner all of this is the cube of side `r` minus the ball about
/// its far corner.
fn corner_piece(v: V3, a: V3, b: V3, c: V3, r: f64) -> Result<Body, String> {
    let odd = || "rounds that meet at a corner of this shape are not supported yet".to_string();
    // The face that holds two of the edges looks away from the third.
    let away = |p: V3, q: V3, third: V3| -> Result<V3, String> {
        let n = p.cross(q);
        if n.len() < 1.0e-9 {
            return Err(odd());
        }
        let n = n.unit();
        Ok(if n.dot(third) > 0.0 { -n } else { n })
    };
    let (na, nb, nc) = (away(b, c, a)?, away(c, a, b)?, away(a, b, c)?);
    // The point `r` inside all three faces.
    let span = na.dot(nb.cross(nc));
    if span.abs() < 1.0e-9 {
        return Err(odd());
    }
    let x = (nb.cross(nc) + nc.cross(na) + na.cross(nb)) * (-r / span);
    let s = v + x;
    let (sa, sb, sc) = (x.dot(a), x.dot(b), x.dot(c));
    if !(sa > TOL && sb > TOL && sc > TOL) {
        return Err(odd());
    }
    let mut body = Body::new();
    let pts = [
        v,
        v + a * sa,
        v + b * sb,
        v + c * sc,
        s + nc * r,
        s + nb * r,
        s + na * r,
    ];
    let (iv, ia, ib, ic, iab, iac, ibc) = (0u32, 1u32, 2u32, 3u32, 4u32, 5u32, 6u32);
    for p in pts {
        body.add_vert(p);
    }
    let mut edge_of: HashMap<(u32, u32), u32> = HashMap::new();
    let mut line = |body: &mut Body, p: u32, q: u32| {
        let e = body.add_line_edge(p, q);
        edge_of.insert((p.min(q), p.max(q)), e);
    };
    for (p, q) in [
        (iv, ia),
        (iv, ib),
        (iv, ic),
        (ia, iab),
        (ia, iac),
        (ib, iab),
        (ib, ibc),
        (ic, iac),
        (ic, ibc),
    ] {
        line(&mut body, p, q);
    }
    // Arcs of great circles in the three inner faces, from one touching
    // point to the next.
    for (p, q, from, to) in [(iab, iac, nc, nb), (ibc, iab, na, nc), (iac, ibc, nb, na)] {
        let turn = from.dot(to).clamp(-1.0, 1.0).acos();
        let curve = body.add_curve(Curve::circle(s, from.cross(to).unit(), from * r, r));
        let e = body.add_edge(curve, 0.0, turn, p, q);
        edge_of.insert((p.min(q), p.max(q)), e);
    }
    let face = |body: &mut Body, cycle: &[u32], surf: Surface, sense: bool, outward: V3| {
        // Newell normal of the vertex polygon fixes the travel direction.
        let mut normal = V3::ZERO;
        for i in 0..cycle.len() {
            let (p, q) = (
                pts[cycle[i] as usize],
                pts[cycle[(i + 1) % cycle.len()] as usize],
            );
            normal += p.cross(q);
        }
        let mut order: Vec<u32> = cycle.to_vec();
        if normal.dot(outward) < 0.0 {
            order.reverse();
        }
        let hes: Vec<u32> = (0..order.len())
            .map(|i| {
                let (p, q) = (order[i], order[(i + 1) % order.len()]);
                let e = edge_of[&(p.min(q), p.max(q))];
                2 * e + u32::from(body.e_v[e as usize][0] != p)
            })
            .collect();
        let s = body.add_surf(surf);
        body.add_face(s, sense, &[hes]);
    };
    face(
        &mut body,
        &[iv, ib, ibc, ic],
        Surface::plane(v, na),
        true,
        na,
    );
    face(
        &mut body,
        &[iv, ic, iac, ia],
        Surface::plane(v, nb),
        true,
        nb,
    );
    face(
        &mut body,
        &[iv, ia, iab, ib],
        Surface::plane(v, nc),
        true,
        nc,
    );
    face(
        &mut body,
        &[ia, iab, iac],
        Surface::plane(pts[1], a),
        true,
        a,
    );
    face(
        &mut body,
        &[ib, ibc, iab],
        Surface::plane(pts[2], b),
        true,
        b,
    );
    face(
        &mut body,
        &[ic, iac, ibc],
        Surface::plane(pts[3], c),
        true,
        c,
    );
    // The ball's surface, seen from outside the piece: its normal points at
    // the centre. Poles are kept away from the patch.
    let ball = Surface {
        f: Frame::from_zx(s, nb - na, na + nb + nc),
        ..Surface::sphere(s, r)
    };
    let centroid = (pts[4] + pts[5] + pts[6]) / 3.0;
    face(&mut body, &[iab, iac, ibc], ball, false, s - centroid);
    body.check()?;
    Ok(body)
}
