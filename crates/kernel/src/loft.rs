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
    let ready = prepare(sections, None, &|_, _, p| p)?;
    let last = ready.rows.len() - 1;
    let bands: Vec<Band> = if ruled {
        (0..last)
            .map(|k| {
                Ok(Band {
                    first: k,
                    last: k + 1,
                    through: vec![ready.station(k)?, ready.station(k + 1)?],
                })
            })
            .collect::<Result<_, String>>()?
    } else {
        let through = (0..=last)
            .map(|k| ready.station(k))
            .collect::<Result<_, String>>()?;
        vec![Band {
            first: 0,
            last,
            through,
        }]
    };
    build(&ready, &bands, ruled)
}

/// The sections of a loft, put in order: every one counter-clockwise seen
/// against the way the loft runs, cut into the same parts, starting at the
/// corner nearest the start of the one before.
struct Ready {
    rows: Vec<Vec<Part>>,
    /// One point for each corner of each section.
    corners: Vec<Vec<V3>>,
    /// The middle of each section, and the flat of it facing onward.
    middles: Vec<V3>,
    flats: Vec<V3>,
}

/// One row of what a skin is drawn through: the corners of a section, and
/// for each part the middle control point of its curve of degree two.
struct Station {
    corners: Vec<V3>,
    mids: Vec<Hom>,
}

/// A stretch of a loft that is one skin: from one section to another,
/// through these stations. The first and last stations are those two
/// sections.
struct Band {
    first: usize,
    last: usize,
    through: Vec<Station>,
}

impl Ready {
    fn station(&self, k: usize) -> Result<Station, String> {
        Ok(Station {
            corners: self.corners[k].clone(),
            mids: self.rows[k]
                .iter()
                .map(|part| Ok(part.hom()?[1]))
                .collect::<Result<_, String>>()?,
        })
    }
}

/// `ways` is the way the loft heads at each section, where that is not
/// simply towards the next one, and `carry(from, to, p)` is where a point
/// of section `from` would be by the time the loft has reached section
/// `to`: the same place, unless something carries the sections along.
fn prepare(
    sections: &[Vec<Seg>],
    ways: Option<&[V3]>,
    carry: &dyn Fn(usize, usize, V3) -> V3,
) -> Result<Ready, String> {
    if sections.len() < 2 {
        return Err("Loft needs at least two sections".into());
    }
    let mut loops: Vec<Vec<Placed>> = Vec::with_capacity(sections.len());
    for segs in sections {
        loops.push(chain(segs)?);
    }
    let middles: Vec<V3> = loops.iter().map(|lp| wire_barycentre(lp)).collect();
    let last = loops.len() - 1;
    let onward = |k: usize| match ways {
        Some(ways) => ways[k],
        None if k < last => middles[k + 1] - middles[k],
        None => middles[k] - middles[k - 1],
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
            (Some(before), _) => rows[before].iter().map(|p| carry(before, k, p.a)).collect(),
            (None, Some(first)) => rows[first].iter().map(|p| carry(first, k, p.a)).collect(),
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
                .map(|i| {
                    (rows[k][(i + shift) % sides].a - carry(k - 1, k, rows[k - 1][i].a)).len2()
                })
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
    Ok(Ready {
        rows,
        corners,
        middles,
        flats,
    })
}

/// The solid whose sides are the skins of `bands` and whose ends are the
/// first and last sections. `ruled` joins the stations of a band by
/// straight lines.
fn build(ready: &Ready, bands: &[Band], ruled: bool) -> Result<Body, String> {
    let Ready {
        rows,
        corners,
        middles,
        flats,
    } = ready;
    let last = rows.len() - 1;
    let count = rows[0].len();
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

    for (band, stretch) in bands.iter().enumerate() {
        let through = &stretch.through;
        let deep = through.len();
        let degree = if ruled { 1 } else { (deep - 1).min(3) };
        // The middle of a part: where its curve of degree two is halfway.
        let halfway = |row: &Station, i: usize| {
            let (a, b, m) = (row.corners[i], row.corners[(i + 1) % count], row.mids[i]);
            (a + v3(m[0], m[1], m[2]) * 2.0 + b) / (2.0 + 2.0 * m[3])
        };
        // How far along the loft each station is: by how far its corners
        // and the middles of its parts are from those of the one before.
        let mut at = vec![0.0; deep];
        for j in 1..deep {
            let (here, before) = (&through[j], &through[j - 1]);
            let moved: f64 = (0..count)
                .map(|i| {
                    here.corners[i].dist(before.corners[i])
                        + halfway(here, i).dist(halfway(before, i))
                })
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
        // The curve up the loft through one control point of each station.
        let up = |pick: &dyn Fn(&Station) -> Hom| -> Result<Vec<Hom>, String> {
            let data: Vec<Hom> = through.iter().map(pick).collect();
            if degree == 1 {
                Ok(data)
            } else {
                form::through(&knots, degree, &at, &data)
            }
        };
        let (first, final_) = (stretch.first, stretch.last);
        let ((low_verts, low_edges), (high_verts, high_edges)) =
            (ring(&mut body, first), ring(&mut body, final_));
        // The rails up the corners. All of their weights are one, apart
        // from what rounding left on them.
        let mut columns: Vec<Vec<Hom>> = Vec::with_capacity(count);
        let mut rails: Vec<u32> = Vec::with_capacity(count);
        for i in 0..count {
            let mut column = up(&|row| {
                let p = row.corners[i];
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
                    let middle = up(&|row| row.mids[i])?;
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

impl Part {
    /// The same part moved by `by`.
    fn moved(&self, by: V3) -> Part {
        Part {
            shape: match self.shape {
                Shape::Line => Shape::Line,
                Shape::Arc { c, axis } => Shape::Arc { c: c + by, axis },
            },
            a: self.a + by,
            b: self.b + by,
            ..*self
        }
    }

    /// The way it is heading at its start, a unit long.
    fn heading(&self) -> V3 {
        self.heading_at(0.0)
    }

    /// The way it is heading a fraction `f` of the way along.
    fn heading_at(&self, f: f64) -> V3 {
        match self.shape {
            Shape::Line => (self.b - self.a).unit(),
            Shape::Arc { c, axis } => axis.cross(self.at(f) - c).unit(),
        }
    }

    /// The edge of `body` along it, between two vertices already there.
    fn edge(&self, body: &mut Body, v0: u32, v1: u32) -> u32 {
        match self.shape {
            Shape::Line => body.add_line_edge(v0, v1),
            Shape::Arc { c, axis } => {
                let frame = Frame::from_zx(c, axis, self.a - c);
                let l = frame.local(self.b);
                let sweep = wrap_from(l.y.atan2(l.x), 1.0e-12);
                let radius = 0.5 * ((self.a - c).len() + (self.b - c).len());
                let curve = body.add_curve(Curve::circle(c, axis, self.a - c, radius));
                body.add_edge(curve, 0.0, sweep, v0, v1)
            }
        }
    }
}

/// The pieces of a path of lines and arcs, each running on from the end of
/// the one before, with no arc wider than one curve can carry.
fn path_parts(path: &[Seg]) -> Result<Vec<Part>, String> {
    let mut out: Vec<Part> = Vec::new();
    for (index, seg) in path.iter().enumerate() {
        let placed = Placed {
            seg: *seg,
            reversed: false,
            index: index as u32,
        };
        if matches!(seg, Seg::Circle { .. }) {
            return Err(
                "a profile kept facing one way cannot be carried round a full circle".into(),
            );
        }
        let part = parts(&[placed])?.remove(0);
        if let Some(before) = out.last() {
            if before.b.dist(part.a) > 1.0e-5 {
                return Err("the sweep path is not connected end to end".into());
            }
        }
        let n = match part.shape {
            Shape::Line => 1,
            Shape::Arc { .. } => (part.sweep / WIDEST).ceil().max(1.0) as usize,
        };
        out.extend(part.cut(n));
    }
    Ok(out)
}

/// A profile carried along a path without turning: every point of it
/// follows a copy of the path. `profile` is one closed outline of lines and
/// arcs, or a circle; `path` is lines and arcs in order, each starting
/// where the one before ends.
///
/// A line carried along a line sweeps a plane, and a line or an arc carried
/// along its own axis a cylinder. Anything else is the surface every point
/// of one curve carried along the other makes, which as a spline is exact:
/// its control points are the sums of theirs, its weights the products.
/// Every edge is a line or an arc: a copy of a piece of the profile, or of
/// a piece of the path.
///
/// Sides are named as [`skin`] names them, with the piece of the path for
/// the band.
pub fn carried(profile: &[Seg], path: &[Seg]) -> Result<Body, String> {
    let legs = path_parts(path)?;
    if legs.is_empty() {
        return Err("Sweep needs a path".into());
    }
    let mut lp = chain(profile)?;
    let middle = wire_barycentre(&lp);
    // The flat of the profile, facing the way the path sets out.
    let setting_out = legs[0].heading();
    let area = signed_area2(&lp, setting_out);
    if area == 0.0 || !area.is_finite() {
        return Err("a profile kept facing one way has to be crossed by its path".into());
    }
    if area < 0.0 {
        reverse_loop(&mut lp);
    }
    let mut row = parts(&lp)?;
    if row.len() == 1 && row[0].sweep == TAU {
        let Shape::Arc { c, axis } = row[0].shape else {
            unreachable!()
        };
        let x = row[0].a - c;
        let y = axis.cross(x);
        row = cut_circle(&row[0], &[c + x, c + y, c - x, c - y]);
    }
    let row: Vec<Part> = row
        .iter()
        .flat_map(|part| {
            let n = match part.shape {
                Shape::Line => 1,
                Shape::Arc { .. } => (part.sweep / WIDEST).ceil().max(1.0) as usize,
            };
            let first = part.sub;
            let mut cut = part.cut(n);
            for piece in &mut cut {
                piece.sub += first * 64;
            }
            cut
        })
        .collect();
    let count = row.len();
    // One point for each corner, whichever of its two parts it is asked of.
    let row: Vec<Part> = (0..count)
        .map(|i| Part {
            b: row[(i + 1) % count].a,
            ..row[i]
        })
        .collect();
    let around: Vec<V3> = row.iter().flat_map(|p| [p.a, p.at(0.5)]).collect();
    let mut flat = V3::ZERO;
    for i in 0..around.len() {
        flat += (around[i] - middle).cross(around[(i + 1) % around.len()] - middle);
    }
    let flat = flat.unit();
    if !flat.is_finite() || flat.len2() < 0.5 {
        return Err("profile encloses no area".into());
    }
    // The path must keep leaving the profile's plane on the same side, or
    // the solid would fold back through itself.
    for leg in &legs {
        for f in [0.0, 0.25, 0.5, 0.75, 1.0] {
            if leg.heading_at(f).dot(flat) < 0.02 {
                return Err(
                    "a profile kept facing one way cannot follow a path that turns to run along \
                     it, or back through it"
                        .into(),
                );
            }
        }
    }
    let origin = legs[0].a;
    let mut body = Body::new();
    let station = |body: &mut Body, at: V3| -> (Vec<u32>, Vec<u32>) {
        let by = at - origin;
        let verts: Vec<u32> = row.iter().map(|p| body.add_vert(p.a + by)).collect();
        let edges: Vec<u32> = (0..count)
            .map(|i| {
                row[i]
                    .moved(by)
                    .edge(body, verts[i], verts[(i + 1) % count])
            })
            .collect();
        (verts, edges)
    };
    let (mut low_verts, mut low_edges) = station(&mut body, origin);
    let first_edges = low_edges.clone();
    let unit_knots = || vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    for (band, leg) in legs.iter().enumerate() {
        let (high_verts, high_edges) = station(&mut body, leg.b);
        let rails: Vec<u32> = (0..count)
            .map(|i| {
                leg.moved(row[i].a - origin)
                    .edge(&mut body, low_verts[i], high_verts[i])
            })
            .collect();
        let along = leg.hom()?;
        for i in 0..count {
            let j = (i + 1) % count;
            let part = row[i].moved(leg.a - origin);
            // Which way the face looks, at its first corner.
            let outward = part.heading().cross(leg.heading());
            if outward.len() < 1.0e-9 {
                return Err("a sweep path runs along a side of its own profile".into());
            }
            let outward = outward.unit();
            let (surface, sense) = match (part.shape, leg.shape) {
                (Shape::Line, Shape::Line) => {
                    let x = part.heading();
                    (
                        Surface::plane_frame(Frame {
                            o: part.a,
                            x,
                            y: outward.cross(x),
                            z: outward,
                        }),
                        true,
                    )
                }
                // A line carried round an arc about its own direction, or
                // an arc carried along its own axis.
                (Shape::Line, Shape::Arc { c, axis })
                    if part.heading().cross(axis).len() < 1.0e-9 =>
                {
                    let centre = c + (part.a - leg.a);
                    let radius = (leg.a - c).len();
                    (
                        Surface::cylinder(centre, axis, radius),
                        (part.a - centre).dot(outward) > 0.0,
                    )
                }
                (Shape::Arc { c, axis }, Shape::Line)
                    if leg.heading().cross(axis).len() < 1.0e-9 =>
                {
                    let radius = (part.a - c).len();
                    (
                        Surface::cylinder(c, axis, radius),
                        (part.a - c).dot(outward) > 0.0,
                    )
                }
                _ => {
                    let across = part.hom()?;
                    let mut net = Vec::with_capacity(9);
                    for q in &along {
                        for p in &across {
                            let w = p[3] * q[3];
                            let at =
                                v3(p[0], p[1], p[2]) / p[3] + v3(q[0], q[1], q[2]) / q[3] - leg.a;
                            net.push([at.x * w, at.y * w, at.z * w, w]);
                        }
                    }
                    (
                        Surface::spline(SplineSurface::from_hom(
                            [2, 2],
                            [unit_knots(), unit_knots()],
                            [3, 3],
                            net,
                        )?),
                        true,
                    )
                }
            };
            let s = body.add_surf(surface);
            body.add_tagged_face(
                s,
                sense,
                &[vec![
                    2 * low_edges[i],
                    2 * rails[j],
                    2 * high_edges[i] + 1,
                    2 * rails[i] + 1,
                ]],
                Role::Side {
                    lp: band as u32 + BANDS * row[i].sub,
                    seg: row[i].index,
                }
                .tag(),
            );
        }
        (low_verts, low_edges) = (high_verts, high_edges);
    }
    let s = body.add_surf(Surface::plane(middle, -flat));
    let bottom: Vec<u32> = first_edges.iter().rev().map(|e| 2 * e + 1).collect();
    body.add_tagged_face(s, true, &[bottom], Role::Start.tag());
    let end = legs[legs.len() - 1].b - origin;
    let s = body.add_surf(Surface::plane(middle + end, flat));
    let top: Vec<u32> = low_edges.iter().map(|e| 2 * e).collect();
    body.add_tagged_face(s, true, &[top], Role::End.tag());
    body.check()?;
    Ok(body)
}

/// A path or a rail: parts in a row, and how long each is.
struct Trail {
    parts: Vec<Part>,
    lengths: Vec<f64>,
    total: f64,
}

impl Trail {
    fn of(path: &[Seg]) -> Result<Trail, String> {
        let parts = path_parts(path)?;
        let lengths: Vec<f64> = parts
            .iter()
            .map(|part| match part.shape {
                Shape::Line => part.a.dist(part.b),
                Shape::Arc { c, .. } => part.sweep * (part.a - c).len(),
            })
            .collect();
        let total: f64 = lengths.iter().sum();
        if parts.is_empty() || total <= 1.0e-9 {
            return Err("a sweep path has no length".into());
        }
        Ok(Trail {
            parts,
            lengths,
            total,
        })
    }

    /// The point a fraction `f` of the way along, by length, and the way
    /// the trail is heading there.
    fn at(&self, f: f64) -> (V3, V3) {
        let mut left = f.clamp(0.0, 1.0) * self.total;
        for (part, &length) in self.parts.iter().zip(&self.lengths) {
            if left <= length {
                let g = if length > 0.0 { left / length } else { 0.0 };
                return (part.at(g), part.heading_at(g));
            }
            left -= length;
        }
        let last = &self.parts[self.parts.len() - 1];
        (last.b, last.heading_at(1.0))
    }
}

/// A profile carried along a path while a rail beside the path steers it:
/// the profile turns about the path so that it keeps facing the rail, and
/// grows or shrinks about the path so that what touched the rail at the
/// start still does.
///
/// The path and the rail are walked together, each by the same share of
/// its own length. The solid is the smooth skin through copies of the
/// profile set down along the way, close enough together that the skin
/// between two of them stays near where a copy would be; unlike every
/// other maker here it is not exact between its stations.
pub fn guided(profile: &[Seg], path: &[Seg], rail: &[Seg]) -> Result<Body, String> {
    let (path, rail) = (Trail::of(path)?, Trail::of(rail)?);
    let stations = (8 * (path.parts.len() + rail.parts.len()) + 1).clamp(9, 49);
    let (start, heading) = path.at(0.0);
    // The rail as seen from the path, square to the way the path heads.
    let across = |at: V3, heading: V3, to: V3| {
        let d = to - at;
        d - heading * d.dot(heading)
    };
    let reach = across(start, heading, rail.at(0.0).0);
    if reach.len() < 1.0e-6 {
        return Err("the guide rail of a sweep has to start beside the path, not on it".into());
    }
    let (e0, r0) = (reach.unit(), reach.len());
    let n0 = heading.cross(e0);
    let mut sections: Vec<Vec<Seg>> = Vec::with_capacity(stations);
    for k in 0..stations {
        let f = k as f64 / (stations - 1) as f64;
        let (at, way) = path.at(f);
        let reach = across(at, way, rail.at(f).0);
        if reach.len() < 1.0e-6 {
            return Err("the guide rail of a sweep crosses its path".into());
        }
        let (e, scale) = (reach.unit(), reach.len() / r0);
        let n = way.cross(e);
        let turned = |v: V3| way * v.dot(heading) + e * v.dot(e0) + n * v.dot(n0);
        let placed = |p: V3| at + turned(p - start) * scale;
        sections.push(
            profile
                .iter()
                .map(|seg| match *seg {
                    Seg::Line { a, b } => Seg::Line {
                        a: placed(a),
                        b: placed(b),
                    },
                    Seg::Arc { a, m, b } => Seg::Arc {
                        a: placed(a),
                        m: placed(m),
                        b: placed(b),
                    },
                    Seg::Circle { c, p, n } => Seg::Circle {
                        c: placed(c),
                        p: placed(p),
                        n: turned(n),
                    },
                })
                .collect(),
        );
    }
    skin(&sections, false)
}

impl Trail {
    /// The same trail walked from its other end.
    fn reversed(&self) -> Trail {
        let parts = self
            .parts
            .iter()
            .rev()
            .map(|part| Part {
                shape: match part.shape {
                    Shape::Line => Shape::Line,
                    Shape::Arc { c, axis } => Shape::Arc { c, axis: -axis },
                },
                a: part.b,
                b: part.a,
                ..*part
            })
            .collect();
        Trail {
            parts,
            lengths: self.lengths.iter().rev().copied().collect(),
            total: self.total,
        }
    }

    /// How far along, as a share of its length, the trail comes nearest to
    /// `p`.
    fn nearest(&self, p: V3) -> f64 {
        const LOOKS: usize = 256;
        let away = |f: f64| self.at(f).0.dist(p);
        let best = (0..=LOOKS)
            .min_by(|&a, &b| {
                away(a as f64 / LOOKS as f64).total_cmp(&away(b as f64 / LOOKS as f64))
            })
            .unwrap_or(0);
        let (mut lo, mut hi) = (
            best.saturating_sub(1) as f64 / LOOKS as f64,
            (best + 1).min(LOOKS) as f64 / LOOKS as f64,
        );
        const G: f64 = 0.618_033_988_749_894_9;
        let (mut a, mut b) = (hi - G * (hi - lo), lo + G * (hi - lo));
        let (mut da, mut db) = (away(a), away(b));
        for _ in 0..60 {
            if da < db {
                (hi, b, db) = (b, a, da);
                a = hi - G * (hi - lo);
                da = away(a);
            } else {
                (lo, a, da) = (a, b, db);
                b = lo + G * (hi - lo);
                db = away(b);
            }
        }
        0.5 * (lo + hi)
    }
}

/// A solid through a row of sections that follows a centerline from one to
/// the next, with a rail beside the centerline to steer it if there is one.
///
/// Each section is taken up by a frame that travels along the centerline
/// without twisting (or, with a rail, keeps facing the rail), and between
/// two sections the outline is what the two are in that frame, changing
/// smoothly from one into the other. With a rail the outline also grows and
/// shrinks about the centerline as the rail comes nearer or goes further
/// off than it is at the sections. With no centerline the straight way
/// from the middle of each section to the next stands in for one.
///
/// The solid is the smooth skin through the sections and through outlines
/// set down along the way between them, close enough together that the
/// skin between two stays near where one more would be. It passes through
/// every section exactly, and its two ends are the first and last as drawn.
pub fn along(sections: &[Vec<Seg>], centerline: &[Seg], rail: &[Seg]) -> Result<Body, String> {
    if sections.len() < 2 {
        return Err("Loft needs at least two sections".into());
    }
    let middles: Vec<V3> = sections
        .iter()
        .map(|segs| Ok(wire_barycentre(&chain(segs)?)))
        .collect::<Result<_, String>>()?;
    let last = middles.len() - 1;
    let mut spine = if centerline.is_empty() {
        let straight: Vec<Seg> = middles
            .windows(2)
            .map(|w| Seg::Line { a: w[0], b: w[1] })
            .collect();
        Trail::of(&straight)?
    } else {
        Trail::of(centerline)?
    };
    let mut rail = if rail.is_empty() {
        None
    } else {
        Some(Trail::of(rail)?)
    };
    // Where along the centerline each section sits. It may have been drawn
    // from the far end; so may the rail.
    if spine.nearest(middles[0]) > spine.nearest(middles[last]) {
        spine = spine.reversed();
    }
    if let Some(beside) = &mut rail {
        let start = spine.at(0.0).0;
        if beside.at(1.0).0.dist(start) < beside.at(0.0).0.dist(start) {
            *beside = beside.reversed();
        }
    }
    let sits: Vec<f64> = middles.iter().map(|m| spine.nearest(*m)).collect();
    if sits.windows(2).any(|w| w[1] - w[0] < 1.0e-6) {
        return Err(
            "the sections of a loft have to follow one another along its centerline".into(),
        );
    }
    // The frame that travels along: the way the centerline heads, and two
    // directions across. Without a rail the first of those is carried along
    // turning only as much as the centerline does.
    const STEPS: usize = 512;
    let mut carried: Vec<V3> = Vec::with_capacity(STEPS + 1);
    let mut across = spine.at(0.0).1.any_perp();
    for k in 0..=STEPS {
        let heading = spine.at(k as f64 / STEPS as f64).1;
        across = (across - heading * across.dot(heading)).unit();
        if !across.is_finite() {
            return Err("the centerline of a loft turns back on itself".into());
        }
        carried.push(across);
    }
    // At a share `f` of the way: where, heading, the two directions across,
    // and how far off the rail is.
    let frame = |f: f64| -> Result<(V3, V3, V3, V3, f64), String> {
        let (at, heading) = spine.at(f);
        let (side, reach) = match &rail {
            Some(beside) => {
                let d = beside.at(f).0 - at;
                let off = d - heading * d.dot(heading);
                if off.len() < 1.0e-6 {
                    return Err("the guide rail of a loft crosses its centerline".into());
                }
                (off.unit(), off.len())
            }
            None => {
                let place = (f.clamp(0.0, 1.0) * STEPS as f64).min(STEPS as f64 - 1.0e-9);
                let (k, share) = (place.floor() as usize, place.fract());
                let side = carried[k].lerp(carried[k + 1], share);
                ((side - heading * side.dot(heading)).unit(), 1.0)
            }
        };
        Ok((at, heading, side, heading.cross(side), reach))
    };
    // The sections are matched corner to corner as the frame carries them,
    // not as they happen to lie in space: round a bend the nearest corner
    // of the next section is not the one this corner turns into.
    let stood: Vec<(V3, V3, V3, V3, f64)> = sits
        .iter()
        .map(|&f| frame(f))
        .collect::<Result<_, String>>()?;
    let ways: Vec<V3> = stood.iter().map(|frame| frame.1).collect();
    let ready = prepare(sections, Some(&ways), &|from, to, p| {
        let (at, heading, side, up, _) = stood[from];
        let d = p - at;
        let (x, y, z) = (d.dot(heading), d.dot(side), d.dot(up));
        let (at, heading, side, up, _) = stood[to];
        at + heading * x + side * y + up * z
    })?;
    let count = ready.rows[0].len();
    // Each section as the travelling frame sees it.
    struct Seen {
        corners: Vec<V3>,
        mids: Vec<(V3, f64)>,
        reach: f64,
    }
    let mut seen: Vec<Seen> = Vec::with_capacity(last + 1);
    for (k, &f) in sits.iter().enumerate() {
        let (at, heading, side, up, reach) = frame(f)?;
        let local = |p: V3| {
            let d = p - at;
            v3(d.dot(heading), d.dot(side), d.dot(up))
        };
        let station = ready.station(k)?;
        seen.push(Seen {
            corners: station.corners.iter().map(|p| local(*p)).collect(),
            mids: station
                .mids
                .iter()
                .map(|m| (local(v3(m[0], m[1], m[2]) / m[3]), m[3]))
                .collect(),
            reach,
        });
    }
    // A value between two sections: on the smooth curve through what it is
    // at each of them, with the slope at a section the one from the section
    // before to the section after.
    let between = |k: usize, share: f64, pick: &dyn Fn(&Seen) -> V3| -> V3 {
        let (y0, y1) = (pick(&seen[k]), pick(&seen[k + 1]));
        let span = sits[k + 1] - sits[k];
        let slope = |j: usize| {
            let (before, after) = (j.saturating_sub(1), (j + 1).min(last));
            (pick(&seen[after]) - pick(&seen[before])) / (sits[after] - sits[before])
        };
        let (m0, m1) = (slope(k) * span, slope(k + 1) * span);
        let (t2, t3) = (share * share, share * share * share);
        y0 * (2.0 * t3 - 3.0 * t2 + 1.0)
            + m0 * (t3 - 2.0 * t2 + share)
            + y1 * (3.0 * t2 - 2.0 * t3)
            + m1 * (t3 - t2)
    };
    // As many stations as the centerline has bends to follow.
    let wanted = (12 * spine.parts.len() + 1).clamp(9, 65) as f64;
    let whole = sits[last] - sits[0];
    let mut through: Vec<Station> = Vec::new();
    for k in 0..last {
        through.push(ready.station(k)?);
        let more = (wanted * (sits[k + 1] - sits[k]) / whole).ceil().max(2.0) as usize;
        for step in 1..more {
            let share = step as f64 / more as f64;
            let f = sits[k] + (sits[k + 1] - sits[k]) * share;
            let (at, heading, side, up, reach) = frame(f)?;
            // The rail further off than the sections on either side would
            // have it, and the outline is that much larger.
            let usual = seen[k].reach + (seen[k + 1].reach - seen[k].reach) * share;
            let grown = reach / usual;
            let placed = |l: V3| at + heading * l.x + (side * l.y + up * l.z) * grown;
            let corners: Vec<V3> = (0..count)
                .map(|i| placed(between(k, share, &|s| s.corners[i])))
                .collect();
            let mids: Vec<Hom> = (0..count)
                .map(|i| {
                    let p = placed(between(k, share, &|s| s.mids[i].0));
                    let w = between(k, share, &|s| v3(s.mids[i].1, 0.0, 0.0))
                        .x
                        .max(0.05);
                    [p.x * w, p.y * w, p.z * w, w]
                })
                .collect();
            through.push(Station { corners, mids });
        }
    }
    through.push(ready.station(last)?);
    build(
        &ready,
        &[Band {
            first: 0,
            last,
            through,
        }],
        false,
    )
}
