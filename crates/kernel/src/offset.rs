//! Moving the faces of a body along their normals, and hollowing it out.
//!
//! Every surface this kernel knows stays in the family when it is moved
//! along its own normal: a plane is another plane, a cylinder a wider or
//! narrower one, a cone the same cone slid along its axis, a sphere or the
//! tube of a torus grows or shrinks. So a body can be offset face by face
//! without leaving exact geometry. Each face goes onto its moved surface,
//! each edge onto the curve where its two moved surfaces meet, each vertex
//! to the point where its surfaces meet. Nothing else changes: the result
//! has the faces, edges and vertices of the original, in the same order.
//!
//! That works until a face is squeezed to nothing or two faces that met no
//! longer do. Those cases are reported rather than guessed at.
//!
//! A shell is the body minus a copy of itself offset inwards (or, for a
//! wall added on the outside, the copy offset outwards minus the body).
//! Faces chosen as openings stay where they are in the copy, so the two
//! coincide there and the Boolean leaves the wall open. Where the offset
//! pulls two faces apart along an edge, the gap is closed by an arc about
//! the original edge, as a wall of constant thickness needs.

use crate::blend;
use crate::boolean::{boolean, Op};
use crate::geom::{Curve, CurveKind, Surface};
use crate::math::*;
use crate::name::mix;
use crate::ssi::{intersect, Ssi};
use crate::topo::Body;

/// The same curve run the other way: `reversed.point(-t) == self.point(t)`.
fn reversed(c: &Curve) -> Option<Curve> {
    match c.kind {
        CurveKind::Line => Some(Curve::line(c.f.o, -c.f.x)),
        CurveKind::Circle => Some(Curve::circle(c.f.o, -c.f.z, c.f.x, c.r)),
        CurveKind::Ellipse => Some(Curve::ellipse(c.f.o, -c.f.z, c.f.x, c.r, c.s)),
        CurveKind::Trace | CurveKind::Meet => None,
    }
}

/// Solve `rows · x = rhs` for three rows.
fn solve3(rows: [V3; 3], rhs: V3) -> V3 {
    let (c0, c1, c2) = (
        rows[1].cross(rows[2]),
        rows[2].cross(rows[0]),
        rows[0].cross(rows[1]),
    );
    (c0 * rhs.x + c1 * rhs.y + c2 * rhs.z) / rows[0].dot(c0)
}

/// The least step `x` that best gives `normals[i] · x = wanted[i]` for all
/// `i`. Directions that no normal speaks of are left alone.
fn least_step(normals: &[V3], wanted: &[f64]) -> V3 {
    // Normal equations, with a whisper of damping to keep them solvable
    // when the normals do not span space.
    let mut rows = [V3::ZERO; 3];
    let mut rhs = V3::ZERO;
    for (n, w) in normals.iter().zip(wanted) {
        rows[0] += *n * n.x;
        rows[1] += *n * n.y;
        rows[2] += *n * n.z;
        rhs += *n * *w;
    }
    const DAMP: f64 = 1.0e-11;
    rows[0].x += DAMP;
    rows[1].y += DAMP;
    rows[2].z += DAMP;
    solve3(rows, rhs)
}

/// The point nearest `start` on all of `surfaces` at once.
fn meeting_point(start: V3, surfaces: &[Surface]) -> Option<V3> {
    let mut p = start;
    let mut normals = Vec::with_capacity(surfaces.len());
    let mut off = Vec::with_capacity(surfaces.len());
    for _ in 0..60 {
        normals.clear();
        off.clear();
        for s in surfaces {
            normals.push(s.normal(p));
            off.push(-s.sdist(p));
        }
        let step = least_step(&normals, &off);
        if !step.is_finite() {
            return None;
        }
        p += step;
        if step.len() <= 1.0e-14 * (1.0 + p.len()) {
            break;
        }
    }
    surfaces
        .iter()
        .all(|s| s.sdist(p).abs() < 1.0e-8)
        .then_some(p)
}

fn squeezed() -> String {
    "the offset leaves no room for one of the faces; use a smaller distance".to_string()
}

fn parted() -> String {
    "faces that met no longer do once they are offset; use a smaller distance".to_string()
}

/// The body with every face `f` moved outwards by `by[f]`, inwards if that
/// is negative.
pub fn offset_faces(body: &Body, by: &[f64]) -> Result<Body, String> {
    let (nf, ne, nv) = (body.face_count(), body.edge_count(), body.verts.len());
    if by.len() != nf {
        return Err("an offset is needed for every face".into());
    }
    let moved: Vec<Surface> = (0..nf as u32)
        .map(|f| {
            let d = by[f as usize];
            let along = if body.f_sense[f as usize] { d } else { -d };
            body.f_surface(f).offset(along).ok_or_else(squeezed)
        })
        .collect::<Result<_, _>>()?;

    // Vertices: where the moved surfaces of the faces around each now meet.
    let mut around: Vec<Vec<u32>> = vec![Vec::new(); nv];
    for e in 0..ne as u32 {
        for v in body.e_v[e as usize] {
            for h in [2 * e, 2 * e + 1] {
                let f = body.h_face(h);
                if !around[v as usize].contains(&f) {
                    around[v as usize].push(f);
                }
            }
        }
    }
    let mut verts: Vec<V3> = Vec::with_capacity(nv);
    for (v, around) in around.iter().enumerate() {
        let p = body.verts[v];
        // Faces that meet smoothly here say the same thing twice, as long
        // as they move together; if they do not, they come apart.
        let mut faces: Vec<u32> = Vec::new();
        for &f in around {
            let n = body.f_normal(f, p);
            let twin = faces.iter().find(|&&g| {
                let m = body.f_normal(g, p);
                n.cross(m).len() < 1.0e-6 && n.dot(m) > 0.0
            });
            match twin {
                Some(&g) if (by[g as usize] - by[f as usize]).abs() > 1.0e-12 => {
                    return Err(
                        "faces that run smoothly into each other have to be offset together".into(),
                    )
                }
                Some(_) => {}
                None => faces.push(f),
            }
        }
        let normals: Vec<V3> = faces.iter().map(|&f| body.f_normal(f, p)).collect();
        let wanted: Vec<f64> = faces.iter().map(|&f| by[f as usize]).collect();
        let start = p + least_step(&normals, &wanted);
        let surfaces: Vec<Surface> = faces.iter().map(|&f| moved[f as usize]).collect();
        verts.push(meeting_point(start, &surfaces).ok_or_else(parted)?);
    }

    // Edges: where the two moved surfaces meet, run the way the edge ran.
    let mut curves: Vec<Curve> = Vec::with_capacity(ne);
    let mut spans: Vec<[f64; 2]> = Vec::with_capacity(ne);
    for e in 0..ne as u32 {
        let (fa, fb) = (body.h_face(2 * e), body.h_face(2 * e + 1));
        let old = body.e_curve_ref(e);
        let [t0, t1] = body.e_t[e as usize];
        let [v0, v1] = body.e_v[e as usize];
        let (pa, pb) = (verts[v0 as usize], verts[v1 as usize]);
        let closed = body.e_closed(e);
        let tm = 0.5 * (t0 + t1);
        let mut curve = if fa == fb || !body.e_sharp(e) {
            if (by[fa as usize] - by[fb as usize]).abs() > 1.0e-12 {
                return Err(
                    "faces that run smoothly into each other have to be offset together".into(),
                );
            }
            carried(body, fa, old, [t0, tm, t1], by[fa as usize])?
        } else {
            let Ssi::Curves(found) = intersect(&moved[fa as usize], &moved[fb as usize]) else {
                return Err(match intersect(&moved[fa as usize], &moved[fb as usize]) {
                    Ssi::General => {
                        "offsetting an edge of this kind is not supported yet".to_string()
                    }
                    _ => parted(),
                });
            };
            // The one that runs through where the edge's ends have gone.
            let miss = |c: &Curve, p: V3| c.point(c.param(p)).dist(p);
            found
                .into_iter()
                .map(|c| (miss(&c, pa).max(miss(&c, pb)), c))
                .filter(|(far, _)| *far < 1.0e-6)
                .min_by(|x, y| x.0.total_cmp(&y.0))
                .map(|(_, c)| c)
                .ok_or_else(parted)?
        };
        // Where the middle of the edge has gone, near enough to tell which
        // way the new curve runs past it.
        let lean = (verts[v0 as usize] - body.verts[v0 as usize] + verts[v1 as usize]
            - body.verts[v1 as usize])
            * 0.5;
        let near = curve.param(old.point(tm) + lean);
        if curve.deriv(near).dot(old.deriv(tm)) < 0.0 {
            curve =
                reversed(&curve).ok_or("offsetting an edge of this kind is not supported yet")?;
        }
        let from = curve.param(pa);
        let to = if closed {
            from + TAU
        } else if curve.periodic() {
            let to = wrap_from(curve.param(pb), from);
            // A whole turn where the edge only went part of the way means
            // its two ends have met.
            if to - from < 1.0e-9 {
                return Err(squeezed());
            }
            to
        } else {
            curve.param(pb)
        };
        if to - from <= 1.0e-9 {
            return Err(squeezed());
        }
        curves.push(curve);
        spans.push([from, to]);
    }

    let mut out = Body::new();
    out.verts = verts;
    for (curve, span) in curves.into_iter().zip(&spans) {
        let c = out.add_curve(curve);
        let e = out.e_curve.len();
        let [v0, v1] = body.e_v[e];
        out.add_edge(c, span[0], span[1], v0, v1);
    }
    // One surface for all the faces that shared one and moved alike.
    let mut placed: Vec<(u32, f64, u32)> = Vec::new();
    for f in 0..nf as u32 {
        let (was, d) = (body.f_surf[f as usize], by[f as usize]);
        let surf = match placed.iter().find(|(s, by, _)| *s == was && *by == d) {
            Some(&(_, _, surf)) => surf,
            None => {
                let surf = out.add_surf(moved[f as usize]);
                placed.push((was, d, surf));
                surf
            }
        };
        let loops: Vec<Vec<u32>> = body
            .face_loops(f)
            .map(|l| body.loop_hes(l).collect())
            .collect();
        out.add_tagged_face(
            surf,
            body.f_sense[f as usize],
            &loops,
            body.f_tag[f as usize],
        );
    }
    out.check().map_err(|_| squeezed())?;
    Ok(out)
}

/// The curve of an edge that both its faces carry along with them: one
/// where they meet smoothly, which no intersection can find again.
fn carried(body: &Body, f: u32, old: &Curve, at: [f64; 3], by: f64) -> Result<Curve, String> {
    let moved = |t: f64| {
        let p = old.point(t);
        p + body.f_normal(f, p) * by
    };
    match old.kind {
        // The normal is the same all along a straight smooth edge.
        CurveKind::Line => Ok(Curve::line(
            old.f.o + body.f_normal(f, old.point(at[1])) * by,
            old.f.x,
        )),
        CurveKind::Circle => {
            // Still a circle about the same axis: further along it, and
            // wider or narrower.
            let (centre, axis) = (old.f.o, old.f.z);
            let place = |t: f64| {
                let d = moved(t) - centre;
                let along = d.dot(axis);
                (along, (d - axis * along).len())
            };
            let (along, radius) = place(at[0]);
            let same = at[1..].iter().all(|&t| {
                let (a, r) = place(t);
                (a - along).abs() < 1.0e-7 && (r - radius).abs() < 1.0e-7
            });
            if !same {
                return Err("offsetting an edge of this kind is not supported yet".into());
            }
            if radius <= TOL {
                return Err(squeezed());
            }
            Ok(Curve::circle(centre + axis * along, axis, old.f.x, radius))
        }
        _ => Err("offsetting an edge of this kind is not supported yet".into()),
    }
}

/// Hollow a body out, leaving a wall of the given thickness. The faces in
/// `open` are removed, so the hollow opens through them. The wall is taken
/// from inside the body, or added around it when `inward` is false.
pub fn shell(body: &Body, open: &[u32], thickness: f64, inward: bool) -> Result<Body, String> {
    shell_as(body, open, thickness, inward, 0x5e11)
}

/// As [`shell`]. The new side of the wall is made of copies of the body's
/// faces; each copy's tag is the original's mixed with `tag`, so the two
/// sides of a wall have names that go together (see [`crate::name`]).
pub fn shell_as(
    body: &Body,
    open: &[u32],
    thickness: f64,
    inward: bool,
    tag: u64,
) -> Result<Body, String> {
    if !(thickness.is_finite() && thickness > TOL) {
        return Err("Shell needs a wall thickness greater than zero".into());
    }
    let nf = body.face_count();
    if open.is_empty() || open.iter().any(|&f| f as usize >= nf) {
        return Err("Shell needs at least one face of the body to remove".into());
    }
    let by: Vec<f64> = (0..nf as u32)
        .map(|f| match (open.contains(&f), inward) {
            (true, _) => 0.0,
            (false, true) => -thickness,
            (false, false) => thickness,
        })
        .collect();
    let mut copy = offset_faces(body, &by)?;
    // Edges where the offset pulls its two faces apart: bends the wall goes
    // round on the outside. The gap is an arc about the original edge.
    let gaps: Vec<u32> = (0..body.edge_count() as u32)
        .filter(|&e| {
            let (fa, fb) = (body.h_face(2 * e), body.h_face(2 * e + 1));
            !open.contains(&fa)
                && !open.contains(&fb)
                && body.e_sharp(e)
                && blend::convexity(body, e) == Some(!inward)
        })
        .collect();
    if !gaps.is_empty() {
        copy = blend::fillet(&copy, &gaps, thickness)
            .map_err(|e| format!("the wall cannot be carried round a bend of the body: {e}"))?;
    }
    copy.retag(|from| mix(tag, from));
    let (result, room) = if inward {
        (boolean(body, &copy, Op::Cut)?, &copy)
    } else {
        (boolean(&copy, body, Op::Cut)?, body)
    };
    // The hollow has to be something, and smaller than what holds it.
    let volume = |b: &Body| crate::tess::tessellate(b, &Default::default()).volume();
    let (whole, hollow) = (volume(if inward { body } else { &copy }), volume(room));
    if !(hollow > 1.0e-9 && hollow < whole) {
        return Err("Shell wall thickness leaves no valid hollow body".into());
    }
    Ok(result)
}
