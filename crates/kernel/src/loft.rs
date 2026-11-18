//! Lofts that need free-form surfaces.
//!
//! [`crate::make::loft`] joins sections whose sides come out flat, or as
//! cones about one axis. Everything else comes here: sections that are
//! turned against each other, outlines of lines and arcs, a square that
//! becomes a circle, and a smooth skin through more than two sections.
//!
//! Every segment of a section is written as one rational curve of degree
//! two, which a line and an arc of less than half a turn both are exactly.
//! The skin over the same segment of every section is then the spline
//! surface through those curves: of degree one along the loft when the
//! sections are joined by straight lines, of degree three when the skin is
//! to be smooth. The first and last sections stay what they were. Their
//! lines and arcs lie on the skin to the last digit, so the flat ends are
//! bounded by true lines and arcs, not by curves that pass for them.
//!
//! A full circle is cut where the corners of its neighbour fall, so that a
//! square can be joined to a round end side by side; circles among
//! themselves are cut in four.

use crate::form::{self, Hom, SplineCurve, SplineSurface};
use crate::geom::{Curve, Surface};
use crate::make::{arc_circle, chain, reverse_loop, signed_area2, wire_barycentre, Placed, Seg};
use crate::math::*;
use crate::name::Role;
use crate::topo::Body;

/// The widest arc one piece is asked to carry.
const WIDEST: f64 = 100.0 * PI / 180.0;

/// Bands of a loft count in the low part of a side's loop number, pieces
/// of one sketch segment above them.
pub const BANDS: u32 = 4096;

/// A segment of a section, the way the loft travels along it.
#[derive(Clone, Copy, Debug)]
enum Shape {
    Line,
    /// Counter-clockwise about `axis` round `c`.
    Arc {
        c: V3,
        axis: V3,
    },
}

/// One stretch of a section between two of its corners.
#[derive(Clone, Copy, Debug)]
struct Part {
    shape: Shape,
    a: V3,
    b: V3,
    /// How far round an arc goes.
    sweep: f64,
    /// Which segment of the sketch this is, and which piece of it.
    index: u32,
    sub: u32,
}

impl Part {
    /// The point a fraction `f` of the way along.
    fn at(&self, f: f64) -> V3 {
        match self.shape {
            Shape::Line => self.a.lerp(self.b, f),
            Shape::Arc { c, axis } => {
                let frame = Frame::from_zx(c, axis, self.a - c);
                let (sin, cos) = (self.sweep * f).sin_cos();
                c + (frame.x * cos + frame.y * sin) * (self.a - c).len()
            }
        }
    }

    /// Cut into `n` equal pieces.
    fn cut(&self, n: usize) -> Vec<Part> {
        (0..n)
            .map(|i| Part {
                a: if i == 0 {
                    self.a
                } else {
                    self.at(i as f64 / n as f64)
                },
                b: if i + 1 == n {
                    self.b
                } else {
                    self.at((i + 1) as f64 / n as f64)
                },
                sweep: self.sweep / n as f64,
                sub: i as u32,
                ..*self
            })
            .collect()
    }

    /// As a curve of degree two.
    fn hom(&self) -> Result<[Hom; 3], String> {
        match self.shape {
            Shape::Line => Ok(form::line_piece(self.a, self.b)),
            Shape::Arc { c, .. } => form::arc_piece(self.a, self.b, c),
        }
    }
}

/// The parts of a section that has been put in order and turned the right
/// way round.
fn parts(lp: &[Placed]) -> Result<Vec<Part>, String> {
    let mut out = Vec::with_capacity(lp.len());
    for placed in lp {
        let (a, b) = (placed.start(), placed.end());
        let part = match placed.seg {
            Seg::Line { .. } => Part {
                shape: Shape::Line,
                a,
                b,
                sweep: 0.0,
                index: placed.index,
                sub: 0,
            },
            Seg::Arc { a: from, m, b: to } => {
                let (c, _, axis) =
                    arc_circle(from, m, to).ok_or("a profile arc has no centre to turn about")?;
                let frame = Frame::from_zx(c, axis, from - c);
                let l = frame.local(to);
                let sweep = wrap_from(l.y.atan2(l.x), 1.0e-12);
                Part {
                    shape: Shape::Arc {
                        c,
                        axis: if placed.reversed { -axis } else { axis },
                    },
                    a,
                    b,
                    sweep,
                    index: placed.index,
                    sub: 0,
                }
            }
            Seg::Circle { c, p, n } => Part {
                shape: Shape::Arc { c, axis: n.unit() },
                a: p,
                b: p,
                sweep: TAU,
                index: placed.index,
                sub: 0,
            },
        };
        out.push(part);
    }
    Ok(out)
}

/// A full circle cut into arcs where the directions `towards` fall, seen
/// from its centre.
fn cut_circle(circle: &Part, towards: &[V3]) -> Vec<Part> {
    let Shape::Arc { c, axis } = circle.shape else {
        unreachable!("only circles are cut by direction");
    };
    let radius = (circle.a - c).len();
    let frame = Frame::from_zx(c, axis, circle.a - c);
    let n = towards.len();
    let mut angles: Vec<f64> = towards
        .iter()
        .map(|p| {
            let l = frame.local(*p);
            l.y.atan2(l.x)
        })
        .collect();
    // Round once, each corner after the one before.
    let mut sound = angles.iter().all(|a| a.is_finite());
    for i in 1..n {
        angles[i] = wrap_from(angles[i], angles[i - 1]);
        sound &= angles[i] - angles[i - 1] > 1.0e-3;
    }
    sound &= angles[n - 1] - angles[0] < TAU - 1.0e-3;
    if !sound {
        // The neighbour's corners do not go round this circle in order:
        // equal shares, from the first of them.
        let first = if angles[0].is_finite() {
            angles[0]
        } else {
            0.0
        };
        angles = (0..n).map(|i| first + TAU * i as f64 / n as f64).collect();
    }
    let at = |angle: f64| c + (frame.x * angle.cos() + frame.y * angle.sin()) * radius;
    (0..n)
        .map(|i| {
            let next = if i + 1 < n {
                angles[i + 1]
            } else {
                angles[0] + TAU
            };
            Part {
                shape: circle.shape,
                a: at(angles[i]),
                b: at(next),
                sweep: next - angles[i],
                index: circle.index,
                sub: i as u32,
            }
        })
        .collect()
}

/// A solid through a row of flat sections, each an outline of lines and
/// arcs or a circle. `ruled` joins every section to the next by straight
/// lines; otherwise one smooth skin runs through them all.
///
/// Outlines must have the same number of segments. Sides are named after
/// the segment of the first section of their band: `Role::Side` with the
/// band in the low part of `lp` (see [`BANDS`]) and, above it, which piece
/// of the segment this is where an arc had to be cut.
pub fn skin(sections: &[Vec<Seg>], ruled: bool) -> Result<Body, String> {
    if sections.len() < 2 {
        return Err("Loft needs at least two sections".into());
    }
    let mut loops: Vec<Vec<Placed>> = Vec::with_capacity(sections.len());
    for segs in sections {
        loops.push(chain(segs)?);
    }
    let middles: Vec<V3> = loops.iter().map(|lp| wire_barycentre(lp)).collect();
    let last = loops.len() - 1;
    let onward = |k: usize| {
        if k < last {
            middles[k + 1] - middles[k]
        } else {
            middles[k] - middles[k - 1]
        }
    };
    // Every section counter-clockwise seen against the way the loft runs.
    let mut rows: Vec<Vec<Part>> = Vec::with_capacity(loops.len());
    for (k, lp) in loops.iter_mut().enumerate() {
        let way = onward(k);
        if way.len2() < 1.0e-18 {
            return Err("Loft sections lie on top of each other".into());
        }
        let area = signed_area2(lp, way);
        if area == 0.0 || !area.is_finite() {
            return Err("profile encloses no area".into());
        }
        if area < 0.0 {
            reverse_loop(lp);
        }
        rows.push(parts(lp)?);
    }
    let round = |row: &[Part]| row.len() == 1 && row[0].sweep == TAU;
    let sides = rows
        .iter()
        .filter(|row| !round(row))
        .map(Vec::len)
        .max()
        .unwrap_or(4);
    if rows.iter().any(|row| !round(row) && row.len() != sides) {
        return Err(
            "Loft sections must have the same number of segments, unless they are circles".into(),
        );
    }
    // Circles are cut where the corners of the nearest outline fall, or in
    // quarters starting beside the section before.
    let outline = rows.iter().position(|row| !round(row));
    for k in 0..rows.len() {
        if !round(&rows[k]) {
            continue;
        }
        let guide: Vec<V3> = match (k.checked_sub(1), outline) {
            (Some(before), _) => rows[before].iter().map(|p| p.a).collect(),
            (None, Some(first)) => rows[first].iter().map(|p| p.a).collect(),
            (None, None) => {
                let Shape::Arc { c, axis } = rows[k][0].shape else {
                    unreachable!()
                };
                let x = rows[k][0].a - c;
                let y = axis.cross(x);
                vec![c + x, c + y, c - x, c - y]
            }
        };
        rows[k] = cut_circle(&rows[k][0], &guide);
    }
    // Outlines start at the corner nearest the start of the one before.
    for k in 1..rows.len() {
        let apart = |shift: usize| -> f64 {
            (0..sides)
                .map(|i| (rows[k][(i + shift) % sides].a - rows[k - 1][i].a).len2())
                .sum()
        };
        let shift = (0..sides)
            .min_by(|&a, &b| apart(a).total_cmp(&apart(b)))
            .unwrap();
        rows[k].rotate_left(shift);
    }
    // No piece wider than one curve of degree two can carry, and as many
    // pieces of a segment in every section.
    let pieces: Vec<usize> = (0..sides)
        .map(|i| {
            rows.iter()
                .map(|row| match row[i].shape {
                    Shape::Line => 1,
                    Shape::Arc { .. } => (row[i].sweep / WIDEST).ceil().max(1.0) as usize,
                })
                .max()
                .unwrap()
        })
        .collect();
    let rows: Vec<Vec<Part>> = rows
        .iter()
        .map(|row| {
            row.iter()
                .zip(&pieces)
                .flat_map(|(part, &n)| {
                    let first = part.sub;
                    let mut cut = part.cut(n);
                    for piece in &mut cut {
                        piece.sub += first * 64;
                    }
                    cut
                })
                .collect()
        })
        .collect();
    let count = rows[0].len();
    // One point for each corner, whichever of its two parts it is asked of.
    let corners: Vec<Vec<V3>> = rows
        .iter()
        .map(|row| row.iter().map(|p| p.a).collect())
        .collect();
    let rows: Vec<Vec<Part>> = rows
        .iter()
        .enumerate()
        .map(|(k, row)| {
            row.iter()
                .enumerate()
                .map(|(i, part)| Part {
                    b: corners[k][(i + 1) % count],
                    ..*part
                })
                .collect()
        })
        .collect();
    // The flat of each section, facing onward.
    let mut flats: Vec<V3> = Vec::with_capacity(rows.len());
    for (k, row) in rows.iter().enumerate() {
        let around: Vec<V3> = row.iter().flat_map(|p| [p.a, p.at(0.5)]).collect();
        let mut normal = V3::ZERO;
        for i in 0..around.len() {
            normal += (around[i] - middles[k]).cross(around[(i + 1) % around.len()] - middles[k]);
        }
        let normal = normal.unit();
        if !normal.is_finite() || normal.len2() < 0.5 {
            return Err("profile encloses no area".into());
        }
        let size = around
            .iter()
            .map(|p| p.dist(middles[k]))
            .fold(0.0, f64::max);
        if around
            .iter()
            .any(|p| (*p - middles[k]).dot(normal).abs() > 1.0e-6 * (1.0 + size))
        {
            return Err("Loft sections must be flat".into());
        }
        flats.push(normal);
    }

    let mut body = Body::new();
    // The corners and outline of a section, made when a band first needs
    // them.
    let mut rings: Vec<Option<(Vec<u32>, Vec<u32>)>> = vec![None; rows.len()];
    let mut ring = |body: &mut Body, k: usize| -> (Vec<u32>, Vec<u32>) {
        if let Some(made) = &rings[k] {
            return made.clone();
        }
        let verts: Vec<u32> = corners[k].iter().map(|p| body.add_vert(*p)).collect();
        let edges: Vec<u32> = rows[k]
            .iter()
            .enumerate()
            .map(|(i, part)| {
                let (v0, v1) = (verts[i], verts[(i + 1) % count]);
                match part.shape {
                    Shape::Line => body.add_line_edge(v0, v1),
                    Shape::Arc { c, axis } => {
                        let frame = Frame::from_zx(c, axis, part.a - c);
                        let l = frame.local(part.b);
                        let sweep = wrap_from(l.y.atan2(l.x), 1.0e-12);
                        let radius = 0.5 * ((part.a - c).len() + (part.b - c).len());
                        let curve = body.add_curve(Curve::circle(c, axis, part.a - c, radius));
                        body.add_edge(curve, 0.0, sweep, v0, v1)
                    }
                }
            })
            .collect();
        rings[k] = Some((verts.clone(), edges.clone()));
        (verts, edges)
    };

    let bands: Vec<Vec<usize>> = if ruled {
        (0..last).map(|k| vec![k, k + 1]).collect()
    } else {
        vec![(0..=last).collect()]
    };
    for (band, through) in bands.iter().enumerate() {
        let deep = through.len();
        let degree = if ruled { 1 } else { (deep - 1).min(3) };
        // How far along the loft each section is: by how far its corners
        // and the middles of its parts are from those of the one before.
        let mut at = vec![0.0; deep];
        for j in 1..deep {
            let (here, before) = (&rows[through[j]], &rows[through[j - 1]]);
            let moved: f64 = (0..count)
                .map(|i| here[i].a.dist(before[i].a) + here[i].at(0.5).dist(before[i].at(0.5)))
                .sum();
            if moved < 1.0e-9 {
                return Err("Loft sections lie on top of each other".into());
            }
            at[j] = at[j - 1] + moved;
        }
        let total = at[deep - 1];
        for t in &mut at {
            *t /= total;
        }
        at[deep - 1] = 1.0;
        let knots = form::knots_through(&at, degree);
        // The curve up the loft through one control point of each section.
        let up = |pick: &dyn Fn(usize) -> Hom| -> Result<Vec<Hom>, String> {
            let data: Vec<Hom> = through.iter().map(|&k| pick(k)).collect();
            if degree == 1 {
                Ok(data)
            } else {
                form::through(&knots, degree, &at, &data)
            }
        };
        let (first, final_) = (through[0], through[deep - 1]);
        let ((low_verts, low_edges), (high_verts, high_edges)) =
            (ring(&mut body, first), ring(&mut body, final_));
        // The rails up the corners. All of their weights are one, apart
        // from what rounding left on them.
        let mut columns: Vec<Vec<Hom>> = Vec::with_capacity(count);
        let mut rails: Vec<u32> = Vec::with_capacity(count);
        for i in 0..count {
            let mut column = up(&|k| {
                let p = corners[k][i];
                [p.x, p.y, p.z, 1.0]
            })?;
            for h in &mut column {
                *h = [h[0] / h[3], h[1] / h[3], h[2] / h[3], 1.0];
            }
            let rail = if degree == 1 {
                body.add_line_edge(low_verts[i], high_verts[i])
            } else {
                let form = SplineCurve::from_hom(degree, knots.clone(), column.clone())?;
                let curve = body.add_curve(Curve::spline(form));
                body.add_edge(curve, 0.0, 1.0, low_verts[i], high_verts[i])
            };
            columns.push(column);
            rails.push(rail);
        }
        for i in 0..count {
            let j = (i + 1) % count;
            let (low, high) = (&rows[first][i], &rows[final_][i]);
            // Flat, if two lines are joined by lines that lie in one plane.
            let flat = (degree == 1
                && matches!(low.shape, Shape::Line)
                && matches!(high.shape, Shape::Line))
            .then(|| {
                let along = low.b - low.a;
                let normal = along.cross(high.a - low.a);
                let size = along.len() * (high.a - low.a).len();
                (normal.len() > 1.0e-9 * size
                    && (high.b - low.a).dot(normal.unit()).abs()
                        <= 1.0e-9 * (1.0 + (high.b - low.a).len()))
                .then(|| {
                    let (x, outward) = (along.unit(), normal.unit());
                    Surface::plane_frame(Frame {
                        o: low.a,
                        x,
                        y: outward.cross(x),
                        z: outward,
                    })
                })
            })
            .flatten();
            let surface = match flat {
                Some(plane) => plane,
                None => {
                    let middle = {
                        let mut homs = Vec::with_capacity(deep);
                        for &k in through {
                            homs.push(rows[k][i].hom()?[1]);
                        }
                        up(&|k| homs[through.iter().position(|&t| t == k).unwrap()])?
                    };
                    let mut net = Vec::with_capacity(3 * deep);
                    for row in 0..deep {
                        net.extend([columns[i][row], middle[row], columns[j][row]]);
                    }
                    if net.iter().any(|h| h[3].is_nan() || h[3] <= 1.0e-6) {
                        return Err("the sections of a loft are too unlike to join smoothly".into());
                    }
                    Surface::spline(SplineSurface::from_hom(
                        [2, degree],
                        [vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], knots.clone()],
                        [3, deep],
                        net,
                    )?)
                }
            };
            let s = body.add_surf(surface);
            body.add_tagged_face(
                s,
                true,
                &[vec![
                    2 * low_edges[i],
                    2 * rails[j],
                    2 * high_edges[i] + 1,
                    2 * rails[i] + 1,
                ]],
                Role::Side {
                    lp: band as u32 + BANDS * low.sub,
                    seg: low.index,
                }
                .tag(),
            );
        }
    }
    let (_, low_edges) = ring(&mut body, 0);
    let s = body.add_surf(Surface::plane(middles[0], -flats[0]));
    let bottom: Vec<u32> = low_edges.iter().rev().map(|e| 2 * e + 1).collect();
    body.add_tagged_face(s, true, &[bottom], Role::Start.tag());
    let (_, high_edges) = ring(&mut body, last);
    let s = body.add_surf(Surface::plane(middles[last], flats[last]));
    let top: Vec<u32> = high_edges.iter().map(|e| 2 * e).collect();
    body.add_tagged_face(s, true, &[top], Role::End.tag());
    body.check()?;
    Ok(body)
}
