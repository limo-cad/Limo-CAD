//! Plain-text snapshot of a body, for bug reports and regression fixtures.
//!
//! The format mirrors the flat arrays one line per entity. Floats are written
//! with enough digits to round-trip exactly.
//!
//! Splines come first, each written out in full once: its degrees, knots
//! and weighted control points. A surface or curve made of one names it by
//! its place in that list, counted from one.

use crate::form::{CurveForm, SplineCurve, SplineSurface, SurfForm};
use crate::geom::{Curve, CurveKind, SurfKind, Surface};
use crate::math::*;
use crate::topo::Body;

fn frame_text(f: &Frame) -> String {
    let v = [f.o, f.x, f.y, f.z];
    v.iter()
        .map(|p| format!("{:?} {:?} {:?}", p.x, p.y, p.z))
        .collect::<Vec<_>>()
        .join(" ")
}

fn surf_kind(code: f64) -> Result<SurfKind, String> {
    Ok(match code as u8 {
        0 => SurfKind::Plane,
        1 => SurfKind::Cylinder,
        2 => SurfKind::Cone,
        3 => SurfKind::Sphere,
        4 => SurfKind::Torus,
        5 => SurfKind::Helicoid,
        6 => SurfKind::Coil,
        7 => SurfKind::Spline,
        _ => return Err("unknown surface kind".into()),
    })
}

/// A spline of either sort, as the list at the head of a file holds them.
#[derive(Clone, Copy)]
enum Form {
    Surface(SurfForm),
    Curve(CurveForm),
}

fn numbers(values: &[f64]) -> String {
    values
        .iter()
        .map(|x| format!("{x:?}"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The place of a surface's spline in the list, added if it is new; zero
/// for a surface that has none.
fn surface_form(forms: &mut Vec<Form>, s: &Surface) -> usize {
    let Some(form) = s.form else { return 0 };
    let found = forms
        .iter()
        .position(|f| matches!(f, Form::Surface(other) if *other == form));
    found.unwrap_or_else(|| {
        forms.push(Form::Surface(form));
        forms.len() - 1
    }) + 1
}

fn curve_form(forms: &mut Vec<Form>, c: &Curve) -> usize {
    let Some(form) = c.form else { return 0 };
    let found = forms
        .iter()
        .position(|f| matches!(f, Form::Curve(other) if *other == form));
    found.unwrap_or_else(|| {
        forms.push(Form::Curve(form));
        forms.len() - 1
    }) + 1
}

impl Body {
    pub fn to_text(&self) -> String {
        let mut forms: Vec<Form> = Vec::new();
        let mut out = String::new();
        out += &format!("surfs {}\n", self.surfs.len());
        for s in &self.surfs {
            out += &format!(
                "{} {} {:?} {:?} {:?}",
                s.kind as u8,
                frame_text(&s.f),
                s.r,
                s.s,
                s.t
            );
            if s.kind == SurfKind::Spline {
                out += &format!(" {}", surface_form(&mut forms, s));
            }
            out.push('\n');
        }
        out += &format!("curves {}\n", self.curves.len());
        for c in &self.curves {
            out += &format!("{} {} {:?} {:?}", c.kind as u8, frame_text(&c.f), c.r, c.s);
            if c.kind == CurveKind::Spline {
                out += &format!(" {}", curve_form(&mut forms, c));
            }
            if matches!(c.kind, CurveKind::Trace | CurveKind::Meet) {
                out += &format!(
                    " {} {} {:?} {:?} {:?} {:?}",
                    c.other.kind as u8,
                    frame_text(&c.other.f),
                    c.other.r,
                    c.other.s,
                    c.span[0],
                    c.span[1]
                );
            }
            if c.kind == CurveKind::Meet {
                out += &format!(
                    " {} {} {:?} {:?} {:?} {:?} {:?} {:?} {:?}",
                    c.base.kind as u8,
                    frame_text(&c.base.f),
                    c.base.r,
                    c.base.s,
                    c.w[0],
                    c.w[1],
                    c.w[2],
                    c.w[3],
                    c.w[4]
                );
            }
            // The leads of the surfaces a curve is made from come last:
            // files written before there were any stop short of them.
            if matches!(c.kind, CurveKind::Trace | CurveKind::Meet) {
                out += &format!(" {:?}", c.other.t);
            }
            if c.kind == CurveKind::Meet {
                out += &format!(" {:?}", c.base.t);
                // And, last of all, the splines of the two surfaces.
                if c.other.form.is_some() || c.base.form.is_some() {
                    out += &format!(
                        " {} {}",
                        surface_form(&mut forms, &c.other),
                        surface_form(&mut forms, &c.base)
                    );
                }
            }
            out.push('\n');
        }
        out += &format!("verts {}\n", self.verts.len());
        for v in &self.verts {
            out += &format!("{:?} {:?} {:?}\n", v.x, v.y, v.z);
        }
        out += &format!("edges {}\n", self.edge_count());
        for e in 0..self.edge_count() {
            out += &format!(
                "{} {:?} {:?} {} {}\n",
                self.e_curve[e], self.e_t[e][0], self.e_t[e][1], self.e_v[e][0], self.e_v[e][1]
            );
        }
        out += &format!("faces {}\n", self.face_count());
        for f in 0..self.face_count() as u32 {
            out += &format!(
                "{} {} {} {:x}\n",
                self.f_surf[f as usize],
                u8::from(self.f_sense[f as usize]),
                self.face_loops(f).len(),
                self.f_tag[f as usize]
            );
            for l in self.face_loops(f) {
                let hes: Vec<String> = self.loop_hes(l).map(|h| h.to_string()).collect();
                out += &format!("{}\n", hes.join(" "));
            }
        }
        // The splines head the file, though which there are is only known
        // once everything that uses one has been written.
        let mut head = String::from("limo-body 1\n");
        if !forms.is_empty() {
            head += &format!("forms {}\n", forms.len());
            for form in &forms {
                match form {
                    Form::Surface(s) => {
                        head += &format!(
                            "surface {} {} {} {}\n{}\n{}\n{}\n",
                            s.degree[0],
                            s.degree[1],
                            s.count[0],
                            s.count[1],
                            numbers(&s.knots[0]),
                            numbers(&s.knots[1]),
                            numbers(s.hom.as_flattened())
                        );
                    }
                    Form::Curve(c) => {
                        head += &format!(
                            "curve {} {}\n{}\n{}\n",
                            c.degree,
                            c.hom.len(),
                            numbers(&c.knots),
                            numbers(c.hom.as_flattened())
                        );
                    }
                }
            }
        }
        head + &out
    }

    pub fn from_text(text: &str) -> Result<Body, String> {
        let mut lines = text.lines().filter(|l| !l.trim().is_empty());
        let mut next = || {
            lines
                .next()
                .ok_or_else(|| "unexpected end of body".to_string())
        };
        if next()?.trim() != "limo-body 1" {
            return Err("not a limo-body file".into());
        }
        let count = |line: &str, name: &str| -> Result<usize, String> {
            line.strip_prefix(name)
                .and_then(|v| v.trim().parse().ok())
                .ok_or_else(|| format!("expected '{name} N'"))
        };
        let floats = |line: &str| -> Result<Vec<f64>, String> {
            line.split_whitespace()
                .map(|v| v.parse::<f64>().map_err(|e| e.to_string()))
                .collect()
        };
        let frame = |v: &[f64]| Frame {
            o: v3(v[0], v[1], v[2]),
            x: v3(v[3], v[4], v[5]),
            y: v3(v[6], v[7], v[8]),
            z: v3(v[9], v[10], v[11]),
        };
        let mut body = Body::new();
        let mut forms: Vec<Form> = Vec::new();
        let mut line = next()?;
        if line.starts_with("forms") {
            let weighted = |flat: Vec<f64>| -> Result<Vec<[f64; 4]>, String> {
                if flat.len() % 4 != 0 {
                    return Err("malformed spline".into());
                }
                Ok(flat.as_chunks::<4>().0.to_vec())
            };
            for _ in 0..count(line, "forms")? {
                let head: Vec<&str> = next()?.split_whitespace().collect();
                let sizes: Vec<usize> = head[1..]
                    .iter()
                    .map(|v| v.parse::<usize>().map_err(|e| e.to_string()))
                    .collect::<Result<_, _>>()?;
                match (head.first().copied(), sizes.as_slice()) {
                    (Some("surface"), &[p, q, nu, nv]) => {
                        let (ku, kv) = (floats(next()?)?, floats(next()?)?);
                        let hom = weighted(floats(next()?)?)?;
                        forms.push(Form::Surface(SplineSurface::from_hom(
                            [p, q],
                            [ku, kv],
                            [nu, nv],
                            hom,
                        )?));
                    }
                    (Some("curve"), &[p, _]) => {
                        let knots = floats(next()?)?;
                        let hom = weighted(floats(next()?)?)?;
                        forms.push(Form::Curve(SplineCurve::from_hom(p, knots, hom)?));
                    }
                    _ => return Err("malformed spline".into()),
                }
            }
            line = next()?;
        }
        let surface_at = |forms: &[Form], place: f64| -> Result<Option<SurfForm>, String> {
            match forms.get((place as usize).wrapping_sub(1)) {
                Some(Form::Surface(s)) => Ok(Some(*s)),
                None if place == 0.0 => Ok(None),
                _ => Err("a surface names a spline that is not there".into()),
            }
        };
        for _ in 0..count(line, "surfs")? {
            let v = floats(next()?)?;
            if ![15, 16, 17].contains(&v.len()) {
                return Err("malformed surface".into());
            }
            let kind = surf_kind(v[0])?;
            let form = surface_at(&forms, v.get(16).copied().unwrap_or(0.0))?;
            if (kind == SurfKind::Spline) != form.is_some() {
                return Err("malformed surface".into());
            }
            body.surfs.push(Surface {
                kind,
                f: frame(&v[1..13]),
                r: v[13],
                s: v[14],
                t: v.get(15).copied().unwrap_or(0.0),
                form,
            });
        }
        for _ in 0..count(next()?, "curves")? {
            let v = floats(next()?)?;
            if ![15, 16, 32, 33, 52, 54, 56].contains(&v.len()) {
                return Err("malformed curve".into());
            }
            let mut curve = match v[0] as u8 {
                6 if v.len() == 16 => match forms.get((v[15] as usize).wrapping_sub(1)) {
                    Some(Form::Curve(c)) => Curve::spline(*c),
                    _ => return Err("a curve names a spline that is not there".into()),
                },
                0 => Curve::line(V3::ZERO, V3::X),
                1 => Curve::circle(V3::ZERO, V3::Z, V3::X, 1.0),
                2 => Curve::ellipse(V3::ZERO, V3::Z, V3::X, 1.0, 1.0),
                3 if v.len() == 32 || v.len() == 33 => {
                    let other = Surface {
                        kind: surf_kind(v[15])?,
                        f: frame(&v[16..28]),
                        r: v[28],
                        s: v[29],
                        t: v.get(32).copied().unwrap_or(0.0),
                        form: None,
                    };
                    Curve::trace(&other, &other, 1.0, [v[30], v[31]])
                }
                4 if v.len() == 52 || v.len() == 54 || v.len() == 56 => {
                    let other = Surface {
                        kind: surf_kind(v[15])?,
                        f: frame(&v[16..28]),
                        r: v[28],
                        s: v[29],
                        t: v.get(52).copied().unwrap_or(0.0),
                        form: surface_at(&forms, v.get(54).copied().unwrap_or(0.0))?,
                    };
                    let base = Surface {
                        kind: surf_kind(v[32])?,
                        f: frame(&v[33..45]),
                        r: v[45],
                        s: v[46],
                        t: v.get(53).copied().unwrap_or(0.0),
                        form: surface_at(&forms, v.get(55).copied().unwrap_or(0.0))?,
                    };
                    let w = [v[47], v[48], v[49], v[50], v[51]];
                    Curve::meet(&base, &other, false, [v[30], v[31]], w)
                }
                5 => Curve::helix(V3::ZERO, V3::Z, V3::X, 1.0, 1.0),
                _ => return Err("unknown curve kind".into()),
            };
            curve.f = frame(&v[1..13]);
            curve.r = v[13];
            curve.s = v[14];
            body.curves.push(curve);
        }
        for _ in 0..count(next()?, "verts")? {
            let v = floats(next()?)?;
            if v.len() != 3 {
                return Err("malformed vertex".into());
            }
            body.verts.push(v3(v[0], v[1], v[2]));
        }
        for _ in 0..count(next()?, "edges")? {
            let v = floats(next()?)?;
            if v.len() != 5 {
                return Err("malformed edge".into());
            }
            body.add_edge(v[0] as u32, v[1], v[2], v[3] as u32, v[4] as u32);
        }
        for _ in 0..count(next()?, "faces")? {
            // Surface, sense and number of loops; then, since faces have
            // them, the tag in hexadecimal.
            let line = next()?;
            let tag = match line.split_whitespace().nth(3) {
                Some(tag) => u64::from_str_radix(tag, 16).map_err(|e| e.to_string())?,
                None => 0,
            };
            let v = floats(
                &line
                    .split_whitespace()
                    .take(3)
                    .collect::<Vec<_>>()
                    .join(" "),
            )?;
            if v.len() != 3 {
                return Err("malformed face".into());
            }
            let mut loops = Vec::with_capacity(v[2] as usize);
            for _ in 0..v[2] as usize {
                let hes: Vec<u32> = next()?
                    .split_whitespace()
                    .map(|h| h.parse::<u32>().map_err(|e| e.to_string()))
                    .collect::<Result<_, _>>()?;
                loops.push(hes);
            }
            body.add_tagged_face(v[0] as u32, v[1] != 0.0, &loops, tag);
        }
        Ok(body)
    }
}
