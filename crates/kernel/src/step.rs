//! STEP (ISO 10303-21) in and out.
//!
//! A body goes out as what it is: planes, cylinders, cones, spheres and
//! tori bounded by lines, circles and ellipses, one `ADVANCED_FACE` per
//! face and no seams. Only the curves in which two curved surfaces meet
//! have no name in STEP; those are written as cubic splines that stay
//! within a ten-millionth of a millimetre of the exact curve.
//!
//! Coming in, a file is read for the same analytic geometry. Other systems
//! cut their round faces open along a seam and their circles in two; both
//! merge away once the faces are sewn, since an edge with the same face on
//! either side bounds nothing. Free-form surfaces are refused by name.

use std::collections::HashMap;

use crate::form::{self, Hom, SplineCurve, SplineSurface};
use crate::geom::{Curve, CurveKind, SurfKind, Surface};
use crate::math::*;
use crate::props;
use crate::simplify::unify;
use crate::topo::Body;

// ---------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------

/// A STEP real: always with a decimal point, exponent in capitals.
fn real(x: f64) -> String {
    let x = if x.abs() < 1.0e-15 { 0.0 } else { x };
    let text = format!("{x:?}");
    match text.split_once('e') {
        None => text,
        Some((mantissa, exponent)) => {
            let dot = if mantissa.contains('.') { "" } else { "." };
            format!("{mantissa}{dot}E{exponent}")
        }
    }
}

/// A STEP string: quotes doubled, anything outside plain ASCII dropped.
fn string(text: &str) -> String {
    let clean: String = text
        .chars()
        .filter(|c| c.is_ascii() && !c.is_ascii_control() && *c != '\\')
        .collect();
    format!("'{}'", clean.replace('\'', "''"))
}

struct Out {
    lines: Vec<String>,
}

impl Out {
    /// Add an entity; its number.
    fn add(&mut self, text: String) -> usize {
        self.lines.push(text);
        self.lines.len()
    }

    fn point(&mut self, p: V3) -> usize {
        self.add(format!(
            "CARTESIAN_POINT('',({},{},{}))",
            real(p.x),
            real(p.y),
            real(p.z)
        ))
    }

    fn direction(&mut self, d: V3) -> usize {
        let d = d.unit();
        self.add(format!(
            "DIRECTION('',({},{},{}))",
            real(d.x),
            real(d.y),
            real(d.z)
        ))
    }

    fn placement(&mut self, o: V3, z: V3, x: V3) -> usize {
        let (o, z, x) = (self.point(o), self.direction(z), self.direction(x));
        self.add(format!("AXIS2_PLACEMENT_3D('',#{o},#{z},#{x})"))
    }

    fn surface(&mut self, body: &Body, f: u32) -> usize {
        let s = body.f_surface(f);
        match s.kind {
            SurfKind::Helicoid | SurfKind::Coil => {
                let (lo, hi) = spread(body, f);
                self.patch(s, lo, hi)
            }
            SurfKind::Plane => {
                let at = self.placement(s.f.o, s.f.z, s.f.x);
                self.add(format!("PLANE('',#{at})"))
            }
            SurfKind::Cylinder => {
                let at = self.placement(s.f.o, s.f.z, s.f.x);
                self.add(format!("CYLINDRICAL_SURFACE('',#{at},{})", real(s.r)))
            }
            SurfKind::Cone => {
                // STEP gives a cone by the circle it has where it is placed,
                // and has it widen along the axis from there. Place it in
                // the middle of the face, on the side of the tip the face
                // is on.
                let (lo, hi) = heights(body, f);
                let mid = if lo.is_finite() && hi.is_finite() && (lo + hi).abs() > 1.0e-9 {
                    0.5 * (lo + hi)
                } else {
                    1.0
                };
                let axis = if mid < 0.0 { -s.f.z } else { s.f.z };
                let at = self.placement(s.f.o + s.f.z * mid, axis, s.f.x);
                self.add(format!(
                    "CONICAL_SURFACE('',#{at},{},{})",
                    real(mid.abs() * s.s.tan()),
                    real(s.s)
                ))
            }
            SurfKind::Sphere => {
                let at = self.placement(s.f.o, s.f.z, s.f.x);
                self.add(format!("SPHERICAL_SURFACE('',#{at},{})", real(s.r)))
            }
            SurfKind::Torus => {
                let at = self.placement(s.f.o, s.f.z, s.f.x);
                self.add(format!(
                    "TOROIDAL_SURFACE('',#{at},{},{})",
                    real(s.r),
                    real(s.s)
                ))
            }
            SurfKind::Spline => self.spline_surface(s),
        }
    }

    /// The curve of an edge, over the span the edge uses.
    fn curve(&mut self, c: &Curve, t0: f64, t1: f64) -> usize {
        match c.kind {
            CurveKind::Line => {
                let (o, d) = (self.point(c.f.o), self.direction(c.f.x));
                let v = self.add(format!("VECTOR('',#{d},1.)"));
                self.add(format!("LINE('',#{o},#{v})"))
            }
            CurveKind::Circle => {
                let at = self.placement(c.f.o, c.f.z, c.f.x);
                self.add(format!("CIRCLE('',#{at},{})", real(c.r)))
            }
            CurveKind::Ellipse => {
                let at = self.placement(c.f.o, c.f.z, c.f.x);
                self.add(format!("ELLIPSE('',#{at},{},{})", real(c.r), real(c.s)))
            }
            CurveKind::Spline => self.spline_curve(c),
            _ => self.spline(c, t0, t1),
        }
    }

    /// Knots as STEP wants them: each value once, and how often it occurs.
    fn knots(knots: &[f64]) -> (String, String) {
        let (mut values, mut counts): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
        let mut i = 0;
        while i < knots.len() {
            let same = knots[i..].iter().take_while(|&&k| k == knots[i]).count();
            values.push(real(knots[i]));
            counts.push(same.to_string());
            i += same;
        }
        (counts.join(","), values.join(","))
    }

    /// A spline curve as it is defined: the same control points, weights
    /// and knots, put where its frame puts them.
    fn spline_curve(&mut self, c: &Curve) -> usize {
        let form = c.spline_form();
        let points: Vec<String> = form
            .points()
            .into_iter()
            .map(|p| format!("#{}", self.point(c.f.global(p))))
            .collect();
        let (counts, values) = Self::knots(&form.knots);
        let closed = if form.closed { ".T." } else { ".F." };
        if !form.rational {
            return self.add(format!(
                "B_SPLINE_CURVE_WITH_KNOTS('',{},({}),.UNSPECIFIED.,{closed},.F.,({counts}),({values}),.UNSPECIFIED.)",
                form.degree,
                points.join(",")
            ));
        }
        let weights: Vec<String> = form.weights().into_iter().map(real).collect();
        self.add(format!(
            "(BOUNDED_CURVE() B_SPLINE_CURVE({},({}),.UNSPECIFIED.,{closed},.F.) B_SPLINE_CURVE_WITH_KNOTS(({counts}),({values}),.UNSPECIFIED.) CURVE() GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_CURVE(({})) REPRESENTATION_ITEM(''))",
            form.degree,
            points.join(","),
            weights.join(",")
        ))
    }

    /// A spline surface as it is defined. Where a reflection has turned
    /// its `u` round, the net is written from the other end, so that the
    /// surface in the file faces the way this one does.
    fn spline_surface(&mut self, s: &Surface) -> usize {
        let form = s.spline_form();
        let turned = s.hand() < 0.0;
        let [nu, nv] = form.count;
        let (points, weights) = (form.points(), form.weights());
        let (mut rows, mut weight_rows) = (Vec::new(), Vec::new());
        for i in 0..nu {
            let i = if turned { nu - 1 - i } else { i };
            let (mut row, mut weight_row) = (Vec::new(), Vec::new());
            for j in 0..nv {
                let at = i + nu * j;
                row.push(format!("#{}", self.point(s.f.global(points[at]))));
                weight_row.push(real(weights[at]));
            }
            rows.push(format!("({})", row.join(",")));
            weight_rows.push(format!("({})", weight_row.join(",")));
        }
        let ku: Vec<f64> = if turned {
            let ends = form.lo.x + form.hi.x;
            form.knots[0].iter().rev().map(|k| ends - k).collect()
        } else {
            form.knots[0].clone()
        };
        let ((mu, ku), (mv, kv)) = (Self::knots(&ku), Self::knots(&form.knots[1]));
        let closed = |yes: bool| if yes { ".T." } else { ".F." };
        let (cu, cv) = (closed(form.closed[0]), closed(form.closed[1]));
        let [du, dv] = form.degree;
        if !form.rational {
            return self.add(format!(
                "B_SPLINE_SURFACE_WITH_KNOTS('',{du},{dv},({}),.UNSPECIFIED.,{cu},{cv},.F.,({mu}),({mv}),({ku}),({kv}),.UNSPECIFIED.)",
                rows.join(",")
            ));
        }
        self.add(format!(
            "(BOUNDED_SURFACE() B_SPLINE_SURFACE({du},{dv},({}),.UNSPECIFIED.,{cu},{cv},.F.) B_SPLINE_SURFACE_WITH_KNOTS(({mu}),({mv}),({ku}),({kv}),.UNSPECIFIED.) GEOMETRIC_REPRESENTATION_ITEM() RATIONAL_B_SPLINE_SURFACE(({})) REPRESENTATION_ITEM('') SURFACE())",
            rows.join(","),
            weight_rows.join(",")
        ))
    }

    /// A screw surface over the stretch of its parameters a face uses, as
    /// a bicubic spline surface through points, slopes and twists of the
    /// exact one. STEP has no name for a surface that turns and climbs at
    /// once, and no rational spline is one; sixty-four spans to the turn
    /// keep this within a few millionths of a millimetre.
    fn patch(&mut self, s: &Surface, lo: V2, hi: V2) -> usize {
        let fine = PI / 32.0;
        let nu = (((hi.x - lo.x) / fine).ceil() as usize).max(1);
        let nv = if s.kind == SurfKind::Helicoid {
            // Straight that way: one span is exact.
            1
        } else {
            (((hi.y - lo.y) / fine).ceil() as usize).max(1)
        };
        let (hu, hv) = ((hi.x - lo.x) / nu as f64, (hi.y - lo.y) / nv as f64);
        // Which of the points before, at and after a node there are.
        let around = |i: usize, n: usize| {
            let mut steps = Vec::with_capacity(2);
            if i > 0 {
                steps.push(-1.0);
            }
            if i == 0 || i == n {
                steps.push(0.0);
            }
            if i < n {
                steps.push(1.0);
            }
            steps
        };
        let mut rows: Vec<String> = Vec::new();
        for i in 0..=nu {
            let u = lo.x + hu * i as f64;
            for a in around(i, nu) {
                let mut row: Vec<String> = Vec::new();
                for j in 0..=nv {
                    let v = lo.y + hv * j as f64;
                    let p = s.point(u, v);
                    let across = s.du(u, v) * (hu / 3.0);
                    let along = s.dv(u, v) * (hv / 3.0);
                    let e = 1.0e-5;
                    let twist = (s.du(u, v + e) - s.du(u, v - e)) * (hu * hv / (18.0 * e));
                    for b in around(j, nv) {
                        let at = self.point(p + across * a + along * b + twist * (a * b));
                        row.push(format!("#{at}"));
                    }
                }
                rows.push(format!("({})", row.join(",")));
            }
        }
        let knots = |n: usize| {
            let mut multiplicities = vec!["4".to_string()];
            multiplicities.extend(std::iter::repeat_n("2".to_string(), n - 1));
            multiplicities.push("4".to_string());
            let values: Vec<String> = (0..=n).map(|i| real(i as f64)).collect();
            (multiplicities.join(","), values.join(","))
        };
        let ((mu, ku), (mv, kv)) = (knots(nu), knots(nv));
        self.add(format!(
            "B_SPLINE_SURFACE_WITH_KNOTS('',3,3,({}),.UNSPECIFIED.,.F.,.F.,.F.,({mu}),({mv}),({ku}),({kv}),.UNSPECIFIED.)",
            rows.join(",")
        ))
    }

    /// A cubic spline through points and tangents of the curve, with as
    /// many spans as it takes to stay on it.
    fn spline(&mut self, c: &Curve, t0: f64, t1: f64) -> usize {
        const CLOSE: f64 = 1.0e-7;
        let mut spans = 8;
        loop {
            let at = |i: usize| t0 + (t1 - t0) * i as f64 / spans as f64;
            let h = (t1 - t0) / spans as f64;
            // Halfway along each span the cubic through its ends is
            // furthest from the curve.
            let worst = (0..spans)
                .map(|i| {
                    let (a, b) = (at(i), at(i + 1));
                    let (pa, pb, da, db) = (c.point(a), c.point(b), c.deriv(a), c.deriv(b));
                    let middle = (pa + pb) * 0.5 + (da - db) * (h / 8.0);
                    middle.dist(c.point(0.5 * (a + b)))
                })
                .fold(0.0, f64::max);
            if worst <= CLOSE || spans >= 65_536 {
                break;
            }
            spans *= 2;
        }
        let at = |i: usize| t0 + (t1 - t0) * i as f64 / spans as f64;
        let third = (t1 - t0) / spans as f64 / 3.0;
        let mut poles: Vec<usize> = Vec::with_capacity(2 * spans + 2);
        for i in 0..=spans {
            let (p, d) = (c.point(at(i)), c.deriv(at(i)));
            if i > 0 {
                poles.push(self.point(p - d * third));
            }
            if i == 0 || i == spans {
                poles.push(self.point(p));
            }
            if i < spans {
                poles.push(self.point(p + d * third));
            }
        }
        let list = |ids: &[usize]| {
            ids.iter()
                .map(|id| format!("#{id}"))
                .collect::<Vec<_>>()
                .join(",")
        };
        let mut multiplicities = vec!["4".to_string()];
        multiplicities.extend(std::iter::repeat_n("2".to_string(), spans - 1));
        multiplicities.push("4".to_string());
        let knots: Vec<String> = (0..=spans).map(|i| real(i as f64)).collect();
        self.add(format!(
            "B_SPLINE_CURVE_WITH_KNOTS('',3,({}),.UNSPECIFIED.,.F.,.F.,({}),({}),.UNSPECIFIED.)",
            list(&poles),
            multiplicities.join(","),
            knots.join(",")
        ))
    }
}

/// The stretch of its surface's parameters that a face takes up.
fn spread(body: &Body, f: u32) -> (V2, V2) {
    let s = body.f_surface(f);
    let (mut lo, mut hi) = (
        v2(f64::INFINITY, f64::INFINITY),
        v2(f64::NEG_INFINITY, f64::NEG_INFINITY),
    );
    for h in body.face_hes(f) {
        let e = h >> 1;
        let c = body.e_curve_ref(e);
        let [t0, t1] = body.e_t[e as usize];
        let steps = c.segments(t0, t1, 0.05, 32.0).max(4);
        for k in 0..=steps {
            let q = s.uv(c.point(t0 + (t1 - t0) * k as f64 / steps as f64));
            lo = v2(lo.x.min(q.x), lo.y.min(q.y));
            hi = v2(hi.x.max(q.x), hi.y.max(q.y));
        }
    }
    // A little beyond the face, so that its edges lie inside the patch.
    let grow = v2(
        1.0e-3 * (hi.x - lo.x).max(1.0),
        1.0e-3 * (hi.y - lo.y).max(1.0e-3),
    );
    (lo - grow, hi + grow)
}

/// The lowest and highest a face reaches along the axis of its surface.
fn heights(body: &Body, f: u32) -> (f64, f64) {
    let s = body.f_surface(f);
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for h in body.face_hes(f) {
        let e = h >> 1;
        let [t0, t1] = body.e_t[e as usize];
        for k in 0..=4 {
            let z =
                s.f.local(body.e_point(e, t0 + (t1 - t0) * k as f64 / 4.0))
                    .z;
            lo = lo.min(z);
            hi = hi.max(z);
        }
    }
    (lo, hi)
}

/// Write bodies as one STEP file, each a product of its own, in
/// millimetres. `description` goes into the header.
pub fn write(parts: &[(String, &Body)], description: &str) -> String {
    let mut out = Out { lines: Vec::new() };
    let application = out.add("APPLICATION_CONTEXT('managed model based 3d engineering')".into());
    out.add(format!(
        "APPLICATION_PROTOCOL_DEFINITION('international standard',\
         'ap242_managed_model_based_3d_engineering',2014,#{application})"
    ));
    let product_context = out.add(format!("PRODUCT_CONTEXT('',#{application},'mechanical')"));
    let definition_context = out.add(format!(
        "PRODUCT_DEFINITION_CONTEXT('part definition',#{application},'design')"
    ));
    let length = out.add("(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))".into());
    let angle = out.add("(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))".into());
    let solid_angle = out.add("(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT())".into());
    let accuracy = out.add(format!(
        "UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-07),#{length},\
         'distance_accuracy_value','confusion accuracy')"
    ));
    let context = out.add(format!(
        "(GEOMETRIC_REPRESENTATION_CONTEXT(3)\
         GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#{accuracy}))\
         GLOBAL_UNIT_ASSIGNED_CONTEXT((#{length},#{angle},#{solid_angle}))\
         REPRESENTATION_CONTEXT('Context #1','3D Context with UNIT and UNCERTAINTY'))"
    ));
    let world = out.placement(V3::ZERO, V3::Z, V3::X);
    let mut products: Vec<usize> = Vec::new();
    for (name, body) in parts {
        let name = string(name);
        let vertices: Vec<usize> = body
            .verts
            .iter()
            .map(|&p| {
                let at = out.point(p);
                out.add(format!("VERTEX_POINT('',#{at})"))
            })
            .collect();
        let edges: Vec<usize> = (0..body.edge_count() as u32)
            .map(|e| {
                let [t0, t1] = body.e_t[e as usize];
                let curve = out.curve(body.e_curve_ref(e), t0, t1);
                let [v0, v1] = body.e_v[e as usize];
                out.add(format!(
                    "EDGE_CURVE('',#{},#{},#{curve},.T.)",
                    vertices[v0 as usize], vertices[v1 as usize]
                ))
            })
            .collect();
        // One shell per piece of skin that hangs together. The skin of a
        // hollow is written facing the other way, as STEP has it, and
        // turned back where the solid lists it.
        let groups = connected(body);
        let hollow: Vec<bool> = groups
            .iter()
            .map(|group| props::volume_within(body, group) < 0.0)
            .collect();
        let mut shells: Vec<Vec<usize>> = Vec::new();
        for (group, &inside_out) in groups.iter().zip(&hollow) {
            let mut faces = Vec::with_capacity(group.len());
            for &f in group {
                let surface = out.surface(body, f);
                let mut bounds: Vec<usize> = Vec::new();
                for l in body.face_loops(f) {
                    let uses: Vec<String> = body
                        .loop_hes(l)
                        .map(|h| {
                            let along = if h & 1 == 0 { ".T." } else { ".F." };
                            let id = out.add(format!(
                                "ORIENTED_EDGE('',*,*,#{},{along})",
                                edges[(h >> 1) as usize]
                            ));
                            format!("#{id}")
                        })
                        .collect();
                    let ring = out.add(format!("EDGE_LOOP('',({}))", uses.join(",")));
                    let as_listed = if inside_out { ".F." } else { ".T." };
                    bounds.push(out.add(format!("FACE_BOUND('',#{ring},{as_listed})")));
                }
                if bounds.is_empty() {
                    // A whole ball or ring has no edge; STEP wants a bound
                    // all the same, and takes a single point for one.
                    let s = body.f_surface(f);
                    let at = out.point(s.point(0.0, 0.5 * PI));
                    let vertex = out.add(format!("VERTEX_POINT('',#{at})"));
                    let ring = out.add(format!("VERTEX_LOOP('',#{vertex})"));
                    bounds.push(out.add(format!("FACE_BOUND('',#{ring},.T.)")));
                }
                let list: Vec<String> = bounds.iter().map(|b| format!("#{b}")).collect();
                let sense = if body.f_sense[f as usize] != inside_out {
                    ".T."
                } else {
                    ".F."
                };
                faces.push(out.add(format!(
                    "ADVANCED_FACE('',({}),#{surface},{sense})",
                    list.join(",")
                )));
            }
            shells.push(faces);
        }
        let mut items = vec![format!("#{world}")];
        let closed: Vec<usize> = shells
            .iter()
            .map(|faces| {
                let list: Vec<String> = faces.iter().map(|f| format!("#{f}")).collect();
                out.add(format!("CLOSED_SHELL('',({}))", list.join(",")))
            })
            .collect();
        // A hollow belongs to the smallest solid whose box holds it.
        let boxes: Vec<Aabb> = groups
            .iter()
            .map(|group| {
                let mut b = Aabb::EMPTY;
                for &f in group {
                    for h in body.face_hes(f) {
                        b.add(body.e_mid(h >> 1));
                    }
                    if body.face_loops(f).is_empty() {
                        let s = body.f_surface(f);
                        let reach = s.r.abs() + s.s.abs();
                        b.add(s.f.o - v3(reach, reach, reach));
                        b.add(s.f.o + v3(reach, reach, reach));
                    }
                }
                b
            })
            .collect();
        let mut voids: Vec<Vec<usize>> = vec![Vec::new(); groups.len()];
        for (k, _) in hollow.iter().enumerate().filter(|(_, h)| **h) {
            let middle = (boxes[k].lo + boxes[k].hi) * 0.5;
            let owner = (0..groups.len())
                .filter(|&o| !hollow[o] && boxes[o].contains(middle))
                .min_by(|a, b| boxes[*a].diagonal().total_cmp(&boxes[*b].diagonal()))
                .or_else(|| (0..groups.len()).find(|&o| !hollow[o]));
            if let Some(owner) = owner {
                let turned = out.add(format!("ORIENTED_CLOSED_SHELL('',*,#{},.F.)", closed[k]));
                voids[owner].push(turned);
            }
        }
        for (k, &shell) in closed.iter().enumerate() {
            if hollow[k] {
                continue;
            }
            let solid = if voids[k].is_empty() {
                out.add(format!("MANIFOLD_SOLID_BREP({name},#{shell})"))
            } else {
                let list: Vec<String> = voids[k].iter().map(|v| format!("#{v}")).collect();
                out.add(format!(
                    "BREP_WITH_VOIDS({name},#{shell},({}))",
                    list.join(",")
                ))
            };
            items.push(format!("#{solid}"));
        }
        let shape = out.add(format!(
            "ADVANCED_BREP_SHAPE_REPRESENTATION({name},({}),#{context})",
            items.join(",")
        ));
        let product = out.add(format!("PRODUCT({name},{name},'',(#{product_context}))"));
        let formation = out.add(format!("PRODUCT_DEFINITION_FORMATION('','',#{product})"));
        let definition = out.add(format!(
            "PRODUCT_DEFINITION('design','',#{formation},#{definition_context})"
        ));
        let shape_of = out.add(format!("PRODUCT_DEFINITION_SHAPE('','',#{definition})"));
        out.add(format!(
            "SHAPE_DEFINITION_REPRESENTATION(#{shape_of},#{shape})"
        ));
        products.push(product);
    }
    if !products.is_empty() {
        let list: Vec<String> = products.iter().map(|p| format!("#{p}")).collect();
        out.add(format!(
            "PRODUCT_RELATED_PRODUCT_CATEGORY('part',$,({}))",
            list.join(",")
        ));
    }
    let mut text = String::from("ISO-10303-21;\nHEADER;\n");
    text += &format!("FILE_DESCRIPTION(({}),'2;1');\n", string(description));
    text += "FILE_NAME('','',(''),(''),'Limo CAD','Limo kernel','');\n";
    text += "FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }'));\n";
    text += "ENDSEC;\nDATA;\n";
    for (i, line) in out.lines.iter().enumerate() {
        text += &format!("#{}={line};\n", i + 1);
    }
    text += "ENDSEC;\nEND-ISO-10303-21;\n";
    text
}

/// Faces grouped by the skin they belong to: faces that share an edge are
/// in one group.
fn connected(body: &Body) -> Vec<Vec<u32>> {
    let nf = body.face_count();
    let mut parent: Vec<u32> = (0..nf as u32).collect();
    fn find(parent: &mut [u32], mut i: u32) -> u32 {
        while parent[i as usize] != i {
            parent[i as usize] = parent[parent[i as usize] as usize];
            i = parent[i as usize];
        }
        i
    }
    for e in 0..body.edge_count() as u32 {
        let (a, b) = (
            find(&mut parent, body.h_face(2 * e)),
            find(&mut parent, body.h_face(2 * e + 1)),
        );
        if a != b {
            parent[a.max(b) as usize] = a.min(b);
        }
    }
    let mut groups: Vec<Vec<u32>> = Vec::new();
    let mut slot: HashMap<u32, usize> = HashMap::new();
    for f in 0..nf as u32 {
        let root = find(&mut parent, f);
        let at = *slot.entry(root).or_insert_with(|| {
            groups.push(Vec::new());
            groups.len() - 1
        });
        groups[at].push(f);
    }
    groups
}

// ---------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------

/// One value in the argument list of an entity.
#[derive(Clone, Debug, PartialEq)]
enum Arg {
    Ref(usize),
    Number(f64),
    Text(String),
    /// `.T.`, `.F.`, `.MILLI.` and the like, without the dots.
    Word(String),
    List(Vec<Arg>),
    /// `NAME(...)` inside an argument list.
    Typed(String, Vec<Arg>),
    /// `$` or `*`.
    Unset,
}

impl Arg {
    fn reference(&self) -> Option<usize> {
        match self {
            Arg::Ref(id) => Some(*id),
            _ => None,
        }
    }
    fn number(&self) -> Option<f64> {
        match self {
            Arg::Number(x) => Some(*x),
            Arg::Typed(_, inner) => inner.first().and_then(Arg::number),
            _ => None,
        }
    }
    fn list(&self) -> &[Arg] {
        match self {
            Arg::List(items) => items,
            _ => &[],
        }
    }
    fn flag(&self) -> bool {
        matches!(self, Arg::Word(w) if w == "T")
    }
}

/// An entity as written: one name with its arguments, or several for a
/// complex entity.
type Entity = Vec<(String, Vec<Arg>)>;

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn skip(&mut self) {
        loop {
            while self.at < self.bytes.len() && self.bytes[self.at].is_ascii_whitespace() {
                self.at += 1;
            }
            if self.bytes[self.at..].starts_with(b"/*") {
                match self.bytes[self.at..].windows(2).position(|w| w == b"*/") {
                    Some(end) => self.at += end + 2,
                    None => self.at = self.bytes.len(),
                }
            } else {
                return;
            }
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip();
        self.bytes.get(self.at).copied()
    }

    fn eat(&mut self, byte: u8) -> Result<(), String> {
        if self.peek() == Some(byte) {
            self.at += 1;
            Ok(())
        } else {
            Err(format!(
                "the STEP file is malformed near byte {} (expected '{}')",
                self.at, byte as char
            ))
        }
    }

    fn name(&mut self) -> String {
        self.skip();
        let start = self.at;
        while self.at < self.bytes.len()
            && (self.bytes[self.at].is_ascii_alphanumeric() || self.bytes[self.at] == b'_')
        {
            self.at += 1;
        }
        String::from_utf8_lossy(&self.bytes[start..self.at]).to_ascii_uppercase()
    }

    fn integer(&mut self) -> Result<usize, String> {
        self.skip();
        let start = self.at;
        while self.at < self.bytes.len() && self.bytes[self.at].is_ascii_digit() {
            self.at += 1;
        }
        std::str::from_utf8(&self.bytes[start..self.at])
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| format!("the STEP file is malformed near byte {start}"))
    }

    fn list(&mut self) -> Result<Vec<Arg>, String> {
        self.eat(b'(')?;
        let mut items = Vec::new();
        if self.peek() == Some(b')') {
            self.at += 1;
            return Ok(items);
        }
        loop {
            items.push(self.arg()?);
            match self.peek() {
                Some(b',') => self.at += 1,
                Some(b')') => {
                    self.at += 1;
                    return Ok(items);
                }
                _ => return Err(format!("the STEP file is malformed near byte {}", self.at)),
            }
        }
    }

    fn arg(&mut self) -> Result<Arg, String> {
        match self.peek() {
            Some(b'#') => {
                self.at += 1;
                Ok(Arg::Ref(self.integer()?))
            }
            Some(b'$') | Some(b'*') => {
                self.at += 1;
                Ok(Arg::Unset)
            }
            Some(b'(') => Ok(Arg::List(self.list()?)),
            Some(b'\'') => {
                self.at += 1;
                let mut text = Vec::new();
                loop {
                    match self.bytes.get(self.at) {
                        None => return Err("the STEP file ends inside a string".into()),
                        Some(b'\'') if self.bytes.get(self.at + 1) == Some(&b'\'') => {
                            text.push(b'\'');
                            self.at += 2;
                        }
                        Some(b'\'') => {
                            self.at += 1;
                            break;
                        }
                        Some(&b) => {
                            text.push(b);
                            self.at += 1;
                        }
                    }
                }
                Ok(Arg::Text(String::from_utf8_lossy(&text).into_owned()))
            }
            Some(b'.') => {
                self.at += 1;
                let word = self.name();
                self.eat(b'.')?;
                Ok(Arg::Word(word))
            }
            Some(b'"') => {
                // Binary data: nothing here reads it.
                self.at += 1;
                while self.at < self.bytes.len() && self.bytes[self.at] != b'"' {
                    self.at += 1;
                }
                self.at += 1;
                Ok(Arg::Unset)
            }
            Some(b) if b.is_ascii_digit() || b == b'-' || b == b'+' => {
                let start = self.at;
                self.at += 1;
                while self.at < self.bytes.len()
                    && (self.bytes[self.at].is_ascii_digit()
                        || matches!(self.bytes[self.at], b'.' | b'e' | b'E' | b'-' | b'+'))
                {
                    self.at += 1;
                }
                let text = String::from_utf8_lossy(&self.bytes[start..self.at]);
                // `1.` and `1.E-7` are STEP's way of writing reals.
                let text = text.replace(".E", ".0E").replace(".e", ".0e");
                let text = text.strip_suffix('.').unwrap_or(&text);
                text.parse()
                    .map(Arg::Number)
                    .map_err(|_| format!("the STEP file has a number that cannot be read: {text}"))
            }
            Some(b) if b.is_ascii_alphabetic() => {
                let name = self.name();
                Ok(Arg::Typed(name, self.list()?))
            }
            _ => Err(format!("the STEP file is malformed near byte {}", self.at)),
        }
    }
}

/// Every entity of the data section, by its number.
fn entities(text: &str) -> Result<HashMap<usize, Entity>, String> {
    let bytes = text.as_bytes();
    if !text.trim_start().starts_with("ISO-10303-21") {
        return Err("this is not a STEP file".into());
    }
    let data = text
        .find("DATA;")
        .ok_or("the STEP file has no data section")?
        + "DATA;".len();
    let mut reader = Reader { bytes, at: data };
    let mut found = HashMap::new();
    while let Some(b'#') = reader.peek() {
        reader.at += 1;
        let id = reader.integer()?;
        reader.eat(b'=')?;
        let mut parts = Vec::new();
        if reader.peek() == Some(b'(') {
            reader.at += 1;
            while reader.peek() != Some(b')') {
                let name = reader.name();
                if name.is_empty() {
                    return Err(format!(
                        "the STEP file is malformed near byte {}",
                        reader.at
                    ));
                }
                parts.push((name, reader.list()?));
            }
            reader.at += 1;
        } else {
            let name = reader.name();
            parts.push((name, reader.list()?));
        }
        reader.eat(b';')?;
        found.insert(id, parts);
    }
    Ok(found)
}

struct Model {
    all: HashMap<usize, Entity>,
    /// Millimetres per unit of length in the file.
    scale: f64,
}

impl Model {
    /// The arguments of a plain entity, when it has one of these names.
    fn get(&self, id: usize, names: &[&str]) -> Option<&[Arg]> {
        let parts = self.all.get(&id)?;
        parts
            .iter()
            .find(|(name, _)| names.contains(&name.as_str()))
            .map(|(_, args)| args.as_slice())
    }

    fn name(&self, id: usize) -> &str {
        self.all
            .get(&id)
            .and_then(|parts| parts.first())
            .map_or("nothing", |(name, _)| name.as_str())
    }

    fn triple(&self, id: usize, names: &[&str]) -> Result<V3, String> {
        let args = self
            .get(id, names)
            .ok_or_else(|| format!("#{id} is not a {}", names[0].to_lowercase()))?;
        let c = args.get(1).map(Arg::list).unwrap_or(&[]);
        let at = |k: usize| c.get(k).and_then(Arg::number).unwrap_or(0.0);
        Ok(v3(at(0), at(1), at(2)))
    }

    fn point(&self, id: usize) -> Result<V3, String> {
        Ok(self.triple(id, &["CARTESIAN_POINT"])? * self.scale)
    }

    fn frame(&self, id: usize) -> Result<Frame, String> {
        let args = self
            .get(id, &["AXIS2_PLACEMENT_3D"])
            .ok_or_else(|| format!("#{id} is not a placement"))?;
        let o = self.point(
            args.get(1)
                .and_then(Arg::reference)
                .ok_or("a placement has no origin")?,
        )?;
        let z = match args.get(2).and_then(Arg::reference) {
            Some(id) => self.triple(id, &["DIRECTION"])?,
            None => V3::Z,
        };
        let x = match args.get(3).and_then(Arg::reference) {
            Some(id) => self.triple(id, &["DIRECTION"])?,
            None => z.any_perp(),
        };
        if z.len() < 1.0e-12 {
            return Err("a placement has no axis".into());
        }
        Ok(Frame::from_zx(o, z, x))
    }

    fn radius(&self, arg: Option<&Arg>) -> Result<f64, String> {
        arg.and_then(Arg::number)
            .map(|r| r * self.scale)
            .ok_or_else(|| "a radius is missing".to_string())
    }

    fn surface(&self, id: usize) -> Result<Surface, String> {
        let known = [
            "PLANE",
            "CYLINDRICAL_SURFACE",
            "CONICAL_SURFACE",
            "SPHERICAL_SURFACE",
            "TOROIDAL_SURFACE",
        ];
        let parts = self.all.get(&id).ok_or("a face has no surface")?;
        if let Some(spline) = self.spline_surface(id)? {
            return Ok(spline);
        }
        let (name, args) = parts
            .iter()
            .find(|(name, _)| known.contains(&name.as_str()))
            .ok_or_else(|| {
                format!(
                    "the file has a surface that cannot be read ({}); planes, cylinders, cones, \
                     spheres, tori and splines can",
                    self.name(id).to_lowercase()
                )
            })?;
        let f = self.frame(
            args.get(1)
                .and_then(Arg::reference)
                .ok_or("a surface is not placed")?,
        )?;
        Ok(match name.as_str() {
            "PLANE" => Surface {
                kind: SurfKind::Plane,
                f,
                r: 0.0,
                s: 0.0,
                t: 0.0,
                form: None,
            },
            "CYLINDRICAL_SURFACE" => Surface {
                kind: SurfKind::Cylinder,
                f,
                r: self.radius(args.get(2))?,
                s: 0.0,
                t: 0.0,
                form: None,
            },
            "CONICAL_SURFACE" => {
                let radius = self.radius(args.get(2))?;
                let half = args
                    .get(3)
                    .and_then(Arg::number)
                    .ok_or("a cone has no angle")?;
                if !(half > 1.0e-9 && half < 0.5 * PI - 1.0e-9) {
                    return Err("a cone has an angle that is no cone's".into());
                }
                Surface {
                    kind: SurfKind::Cone,
                    f: Frame {
                        o: f.o - f.z * (radius / half.tan()),
                        ..f
                    },
                    r: 0.0,
                    s: half,
                    t: 0.0,
                    form: None,
                }
            }
            "SPHERICAL_SURFACE" => Surface {
                kind: SurfKind::Sphere,
                f,
                r: self.radius(args.get(2))?,
                s: 0.0,
                t: 0.0,
                form: None,
            },
            _ => Surface {
                kind: SurfKind::Torus,
                f,
                r: self.radius(args.get(2))?,
                s: self.radius(args.get(3))?,
                t: 0.0,
                form: None,
            },
        })
    }

    /// The curve of an edge, looking through the wrappers that tie a curve
    /// to its surfaces.
    fn curve(&self, mut id: usize) -> Result<Curve, String> {
        for _ in 0..4 {
            match self.get(id, &["SURFACE_CURVE", "SEAM_CURVE", "INTERSECTION_CURVE"]) {
                Some(args) => {
                    id = args
                        .get(1)
                        .and_then(Arg::reference)
                        .ok_or("a curve is missing")?
                }
                None => break,
            }
        }
        if let Some(args) = self.get(id, &["LINE"]) {
            let o = self.point(
                args.get(1)
                    .and_then(Arg::reference)
                    .ok_or("a line has no point")?,
            )?;
            let vector = self
                .get(
                    args.get(2)
                        .and_then(Arg::reference)
                        .ok_or("a line has no direction")?,
                    &["VECTOR"],
                )
                .ok_or("a line has no direction")?;
            let d = self.triple(
                vector
                    .get(1)
                    .and_then(Arg::reference)
                    .ok_or("a line has no direction")?,
                &["DIRECTION"],
            )?;
            return Ok(Curve::line(o, d));
        }
        if let Some(args) = self.get(id, &["CIRCLE"]) {
            let f = self.frame(
                args.get(1)
                    .and_then(Arg::reference)
                    .ok_or("a circle is not placed")?,
            )?;
            return Ok(Curve::circle(f.o, f.z, f.x, self.radius(args.get(2))?));
        }
        if let Some(args) = self.get(id, &["ELLIPSE"]) {
            let f = self.frame(
                args.get(1)
                    .and_then(Arg::reference)
                    .ok_or("an ellipse is not placed")?,
            )?;
            let (a, b) = (self.radius(args.get(2))?, self.radius(args.get(3))?);
            // The first axis of a STEP ellipse need not be the longer one.
            return Ok(if a >= b {
                Curve::ellipse(f.o, f.z, f.x, a, b)
            } else {
                Curve::ellipse(f.o, f.z, f.y, b, a)
            });
        }
        // A spline is often a line or an arc by another name: some systems
        // write every curve they have touched that way. Otherwise it is
        // read as the spline it is.
        if let Some(found) = self.spline(id)? {
            return Ok(found);
        }
        Err(format!(
            "the file has a curve that cannot be read ({}); lines, circles, ellipses and \
             splines can",
            self.name(id).to_lowercase()
        ))
    }
}

impl Model {
    /// The line or circle a spline curve runs along, if it is one.
    fn spline(&self, id: usize) -> Result<Option<Curve>, String> {
        let Some(parts) = self.all.get(&id) else {
            return Ok(None);
        };
        let part = |name: &str| {
            parts
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, a)| a.as_slice())
        };
        // On its own the entity starts with a name; as one part of a
        // complex entity it does not.
        let (degree, poles, knots) = if let Some(args) = part("B_SPLINE_CURVE_WITH_KNOTS") {
            if parts.len() == 1 {
                (args.get(1), args.get(2), Some((args.get(6), args.get(7))))
            } else {
                let base = part("B_SPLINE_CURVE").unwrap_or(&[]);
                (base.first(), base.get(1), Some((args.first(), args.get(1))))
            }
        } else if let Some(args) = part("B_SPLINE_CURVE") {
            if parts.len() == 1 {
                (args.get(1), args.get(2), None)
            } else {
                (args.first(), args.get(1), None)
            }
        } else {
            return Ok(None);
        };
        let degree = degree
            .and_then(Arg::number)
            .ok_or("a spline has no degree")? as usize;
        let mut points = Vec::new();
        for pole in poles.map(Arg::list).unwrap_or(&[]) {
            points.push(self.point(pole.reference().ok_or("a spline has no points")?)?);
        }
        let n = points.len();
        if degree == 0 || n <= degree {
            return Err("the STEP file has a spline with too few points".into());
        }
        let weights: Vec<f64> = match part("RATIONAL_B_SPLINE_CURVE") {
            Some(args) => args
                .first()
                .map(Arg::list)
                .unwrap_or(&[])
                .iter()
                .filter_map(Arg::number)
                .collect(),
            None => vec![1.0; n],
        };
        // The knots written out in full; without any, one Bezier span.
        let mut full: Vec<f64> = Vec::new();
        if let Some((mults, values)) = knots {
            let mults = mults.map(Arg::list).unwrap_or(&[]);
            let values = values.map(Arg::list).unwrap_or(&[]);
            for (m, k) in mults.iter().zip(values) {
                let (m, k) = (
                    m.number().unwrap_or(1.0) as usize,
                    k.number().unwrap_or(0.0),
                );
                full.extend(std::iter::repeat_n(k, m));
            }
        }
        if full.len() != n + degree + 1 || weights.len() != n {
            if n == degree + 1 && weights.len() == n {
                full = [vec![0.0; n], vec![1.0; n]].concat();
            } else {
                return Err("the STEP file has a spline whose knots do not fit its points".into());
            }
        }
        // De Boor, on weighted points.
        let at = |u: f64| -> V3 {
            let mut span = degree;
            while span + 1 < n && u >= full[span + 1] {
                span += 1;
            }
            let mut d: Vec<(V3, f64)> = (0..=degree)
                .map(|j| {
                    let i = span - degree + j;
                    (points[i] * weights[i], weights[i])
                })
                .collect();
            for r in 1..=degree {
                for j in (r..=degree).rev() {
                    let i = span - degree + j;
                    let width = full[i + degree + 1 - r] - full[i];
                    let a = if width > 0.0 {
                        (u - full[i]) / width
                    } else {
                        0.0
                    };
                    d[j] = (
                        d[j - 1].0 * (1.0 - a) + d[j].0 * a,
                        d[j - 1].1 * (1.0 - a) + d[j].1 * a,
                    );
                }
            }
            d[degree].0 / d[degree].1
        };
        let (first, last) = (full[degree], full[n]);
        const SAMPLES: usize = 16;
        let samples: Vec<V3> = (0..=SAMPLES)
            .map(|i| at(first + (last - first) * i as f64 / SAMPLES as f64))
            .collect();
        let size = samples
            .iter()
            .fold(0.0_f64, |m, p| m.max(p.dist(samples[0])))
            .max(1.0e-9);
        let close = 1.0e-7 * size.max(1.0);
        // A line: every sample on the chord, in order.
        let (a, b) = (samples[0], samples[SAMPLES]);
        if a.dist(b) > close {
            let d = (b - a).unit();
            if samples.iter().all(|p| (*p - a).cross(d).len() <= close) {
                return Ok(Some(Curve::line(a, d)));
            }
        }
        // A circle: every sample as far from one centre, in one plane.
        let (m, q) = (samples[SAMPLES / 3], samples[2 * SAMPLES / 3]);
        if let Some((centre, radius, axis)) = crate::make::arc_circle(a, m, q) {
            let round = samples.iter().all(|p| {
                (p.dist(centre) - radius).abs() <= close && (*p - centre).dot(axis).abs() <= close
            });
            if round {
                return Ok(Some(Curve::circle(centre, axis, a - centre, radius)));
            }
        }
        // Neither: the spline itself.
        let mut rows = vec![points
            .iter()
            .zip(&weights)
            .map(|(p, &w)| [p.x * w, p.y * w, p.z * w, w])
            .collect::<Vec<Hom>>()];
        let clamped = full[..=degree].iter().all(|k| *k == full[0])
            && full[n..].iter().all(|k| *k == full[n]);
        if !clamped {
            full = form::clamp(&full, degree, &mut rows)?;
        }
        let form = SplineCurve::from_hom(degree, full, rows.remove(0))
            .map_err(|e| format!("the STEP file has a spline curve that cannot be used: {e}"))?;
        Ok(Some(Curve::spline(form)))
    }

    /// Knots written out in full from STEP's list of values and list of how
    /// often each occurs; for a single Bezier span that came without any,
    /// the knots of one.
    fn knots(
        mults: Option<&Arg>,
        values: Option<&Arg>,
        degree: usize,
        n: usize,
    ) -> Result<Vec<f64>, String> {
        let mut full: Vec<f64> = Vec::new();
        let (mults, values) = (
            mults.map(Arg::list).unwrap_or(&[]),
            values.map(Arg::list).unwrap_or(&[]),
        );
        for (m, k) in mults.iter().zip(values) {
            let (m, k) = (
                m.number().unwrap_or(1.0) as usize,
                k.number().unwrap_or(0.0),
            );
            full.extend(std::iter::repeat_n(k, m));
        }
        if full.len() != n + degree + 1 {
            if full.is_empty() && n == degree + 1 {
                full = [vec![0.0; n], vec![1.0; n]].concat();
            } else {
                return Err("the STEP file has a spline whose knots do not fit its points".into());
            }
        }
        Ok(full)
    }

    /// A spline surface as it is written: degrees, a net of control points
    /// with their weights if it is rational, and knots. `None` for any
    /// other kind of surface.
    fn spline_surface(&self, id: usize) -> Result<Option<Surface>, String> {
        let Some(parts) = self.all.get(&id) else {
            return Ok(None);
        };
        let part = |name: &str| {
            parts
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, a)| a.as_slice())
        };
        // On its own an entity starts with a name; as one part of a
        // complex entity it does not.
        let alone = parts.len() == 1;
        let skip = usize::from(alone);
        let (head, knots) = if let Some(args) = part("B_SPLINE_SURFACE_WITH_KNOTS") {
            if alone {
                (args.get(1..).unwrap_or(&[]), args.get(8..))
            } else {
                (part("B_SPLINE_SURFACE").unwrap_or(&[]), Some(args))
            }
        } else if let Some(args) = part("B_SPLINE_SURFACE").or(part("BEZIER_SURFACE")) {
            (args.get(skip..).unwrap_or(&[]), None)
        } else {
            return Ok(None);
        };
        let degree = |k: usize| -> Result<usize, String> {
            head.get(k)
                .and_then(Arg::number)
                .map(|d| d as usize)
                .filter(|d| (1..=form::MAX_DEGREE).contains(d))
                .ok_or_else(|| {
                    "the STEP file has a spline surface of a degree that cannot be read".into()
                })
        };
        let (du, dv) = (degree(0)?, degree(1)?);
        let net = head.get(2).map(Arg::list).unwrap_or(&[]);
        let (nu, nv) = (net.len(), net.first().map_or(0, |row| row.list().len()));
        if nu <= du || nv <= dv || net.iter().any(|row| row.list().len() != nv) {
            return Err("the STEP file has a spline surface with too few points".into());
        }
        let weights = part("RATIONAL_B_SPLINE_SURFACE")
            .and_then(|args| args.first())
            .map(Arg::list);
        if weights.is_some_and(|w| w.len() != nu || w.iter().any(|row| row.list().len() != nv)) {
            return Err("the STEP file has a spline surface whose weights do not fit".into());
        }
        // The control points, by `u` then `v`, as the file has them.
        let mut by_u: Vec<Vec<Hom>> = Vec::with_capacity(nu);
        for (i, row) in net.iter().enumerate() {
            let mut out = Vec::with_capacity(nv);
            for (j, pole) in row.list().iter().enumerate() {
                let p = self.point(pole.reference().ok_or("a spline has no points")?)?;
                let w = match weights {
                    Some(w) => w[i].list()[j]
                        .number()
                        .filter(|w| *w > 0.0)
                        .ok_or("the STEP file has a spline surface with a weight that is none")?,
                    None => 1.0,
                };
                out.push([p.x * w, p.y * w, p.z * w, w]);
            }
            by_u.push(out);
        }
        let k = |at: usize| knots.and_then(|k| k.get(at));
        let mut ku = Self::knots(k(0), k(2), du, nu)?;
        let mut kv = Self::knots(k(1), k(3), dv, nv)?;
        let clamped = |knots: &[f64], degree: usize, n: usize| {
            knots[..=degree].iter().all(|k| *k == knots[0])
                && knots[n..].iter().all(|k| *k == knots[n])
        };
        // Along `v` each row of the net is a curve of its own.
        if !clamped(&kv, dv, nv) {
            kv = form::clamp(&kv, dv, &mut by_u)?;
        }
        let nv = by_u[0].len();
        // Along `u` the curves are the columns.
        let mut by_v: Vec<Vec<Hom>> = (0..nv)
            .map(|j| by_u.iter().map(|row| row[j]).collect())
            .collect();
        if !clamped(&ku, du, nu) {
            ku = form::clamp(&ku, du, &mut by_v)?;
        }
        let nu = by_v[0].len();
        let hom: Vec<Hom> = by_v.into_iter().flatten().collect();
        let form = SplineSurface::from_hom([du, dv], [ku, kv], [nu, nv], hom)
            .map_err(|e| format!("the STEP file has a spline surface that cannot be used: {e}"))?;
        Ok(Some(Surface::spline(form)))
    }
}

/// Bodies of a STEP file, one per solid, in millimetres.
pub fn read(text: &str) -> Result<Vec<Body>, String> {
    let all = entities(text)?;
    // Millimetres, unless the file says otherwise.
    let mut scale = 1.0;
    for parts in all.values() {
        if !parts.iter().any(|(name, _)| name == "LENGTH_UNIT") {
            continue;
        }
        if let Some((_, args)) = parts.iter().find(|(name, _)| name == "SI_UNIT") {
            scale = match args.first() {
                Some(Arg::Word(prefix)) if prefix == "MILLI" => 1.0,
                Some(Arg::Word(prefix)) if prefix == "CENTI" => 10.0,
                Some(Arg::Word(prefix)) if prefix == "MICRO" => 1.0e-3,
                Some(Arg::Unset) => 1000.0,
                _ => 1.0,
            };
        } else if let Some((_, args)) = parts
            .iter()
            .find(|(name, _)| name == "CONVERSION_BASED_UNIT")
        {
            if let Some(Arg::Text(unit)) = args.first() {
                scale = match unit.to_ascii_uppercase().as_str() {
                    "INCH" => 25.4,
                    "FOOT" => 304.8,
                    _ => 1.0,
                };
            }
        }
    }
    let model = Model { all, scale };
    let mut solids: Vec<usize> = model
        .all
        .iter()
        .filter(|(_, parts)| {
            parts
                .iter()
                .any(|(name, _)| name == "MANIFOLD_SOLID_BREP" || name == "BREP_WITH_VOIDS")
        })
        .map(|(id, _)| *id)
        .collect();
    solids.sort_unstable();
    if solids.is_empty() {
        return Err("the STEP file holds no solid".into());
    }
    let mut bodies = Vec::with_capacity(solids.len());
    for id in solids {
        // `BREP_WITH_VOIDS(name, outer, (voids))` on its own, or as one part
        // of a complex entity next to `MANIFOLD_SOLID_BREP(name, outer)`.
        let plain = model.get(id, &["MANIFOLD_SOLID_BREP"]);
        let with_voids = model.get(id, &["BREP_WITH_VOIDS"]);
        let (outer, voids) = match (plain, with_voids) {
            (Some(args), Some(voids)) => (args.get(1), voids.first()),
            (Some(args), None) => (args.get(1), None),
            (None, Some(args)) => (args.get(1), args.get(2)),
            (None, None) => (None, None),
        };
        let outer = outer
            .and_then(Arg::reference)
            .ok_or("a solid has no shell")?;
        let mut body = model.shell(outer, false)?;
        for hollow in voids.map(Arg::list).unwrap_or(&[]) {
            let hollow = hollow
                .reference()
                .ok_or("a solid lists a void that is none")?;
            body.append(&model.shell(hollow, true)?);
        }
        bodies.push(body);
    }
    Ok(bodies)
}

impl Model {
    /// One closed shell as a body: with its material inside, or for a
    /// hollow with the material outside.
    fn shell(&self, mut id: usize, hollow: bool) -> Result<Body, String> {
        // A void is a shell seen through `ORIENTED_CLOSED_SHELL`; which way
        // it faces is settled by its volume below, whatever the file says.
        if let Some(args) = self.get(id, &["ORIENTED_CLOSED_SHELL"]) {
            id = args
                .get(2)
                .and_then(Arg::reference)
                .ok_or("a void has no shell")?;
        }
        let faces = self
            .get(id, &["CLOSED_SHELL", "OPEN_SHELL"])
            .ok_or_else(|| format!("#{id} is not a shell"))?
            .get(1)
            .map(Arg::list)
            .unwrap_or(&[]);
        let mut body = Body::new();
        let mut vertices: HashMap<usize, u32> = HashMap::new();
        // Edge of the file to edge of the body, and whether the body's
        // edge runs the way the file's does.
        let mut edges: HashMap<usize, (u32, bool)> = HashMap::new();
        let mut used_hes: std::collections::HashSet<u32> = std::collections::HashSet::new();
        for (index, face) in faces.iter().enumerate() {
            let face = face
                .reference()
                .ok_or("a shell lists something that is no face")?;
            let args = self
                .get(face, &["ADVANCED_FACE", "FACE_SURFACE"])
                .ok_or_else(|| format!("#{face} is not a face"))?;
            let surface = self.surface(
                args.get(2)
                    .and_then(Arg::reference)
                    .ok_or("a face has no surface")?,
            )?;
            let sense = args.get(3).is_some_and(Arg::flag);
            let mut loops: Vec<Vec<u32>> = Vec::new();
            for bound in args.get(1).map(Arg::list).unwrap_or(&[]) {
                let bound = bound.reference().ok_or("a face has a bound that is none")?;
                let bound_args = self
                    .get(bound, &["FACE_BOUND", "FACE_OUTER_BOUND"])
                    .ok_or_else(|| format!("#{bound} is not a face bound"))?;
                let ring = bound_args
                    .get(1)
                    .and_then(Arg::reference)
                    .ok_or("a bound has no loop")?;
                let Some(ring_args) = self.get(ring, &["EDGE_LOOP"]) else {
                    // A loop of one vertex bounds nothing.
                    continue;
                };
                let mut hes: Vec<u32> = Vec::new();
                for item in ring_args.get(1).map(Arg::list).unwrap_or(&[]) {
                    let item = item
                        .reference()
                        .ok_or("a loop lists something that is no edge")?;
                    let oriented = self
                        .get(item, &["ORIENTED_EDGE"])
                        .ok_or_else(|| format!("#{item} is not an oriented edge"))?;
                    let source = oriented
                        .get(3)
                        .and_then(Arg::reference)
                        .ok_or("an edge use has no edge")?;
                    let along = oriented.get(4).is_some_and(Arg::flag);
                    let (edge, same) = match edges.get(&source) {
                        Some(known) => *known,
                        None => {
                            let made = self.edge(source, &mut body, &mut vertices)?;
                            edges.insert(source, made);
                            made
                        }
                    };
                    hes.push(2 * edge + u32::from(along != same));
                }
                if !bound_args.get(2).is_some_and(Arg::flag) {
                    hes.reverse();
                    for h in &mut hes {
                        *h ^= 1;
                    }
                }
                if hes.iter().any(|h| !used_hes.insert(*h)) {
                    return Err("the STEP file uses one side of an edge twice".into());
                }
                if !hes.is_empty() {
                    loops.push(hes);
                }
            }
            let surf = body.add_surf(surface);
            body.add_tagged_face(surf, sense, &loops, index as u64 + 1);
        }
        // Seams and circles cut in two go; then the shell must close.
        let mut body = unify(&body);
        body.check()
            .map_err(|e| format!("the STEP solid is not a closed solid ({e})"))?;
        // Which way round it is: by its exact volume, or where it has
        // splines, which take a while to weigh, by that of its mesh.
        let free = body.surfs.iter().any(|s| s.kind == SurfKind::Spline);
        let volume = if free {
            crate::tess::tessellate(&body, &crate::tess::TessOptions::default()).volume()
        } else {
            props::volume(&body)
        };
        if (volume < 0.0) != hollow {
            // Facing the wrong way for what it is.
            let mut turned = Body::new();
            turned.surfs = body.surfs.clone();
            turned.curves = body.curves.clone();
            turned.verts = body.verts.clone();
            for e in 0..body.edge_count() {
                let ([t0, t1], [v0, v1]) = (body.e_t[e], body.e_v[e]);
                turned.add_edge(body.e_curve[e], t0, t1, v0, v1);
            }
            for f in 0..body.face_count() as u32 {
                let loops: Vec<Vec<u32>> = body
                    .face_loops(f)
                    .map(|l| {
                        let mut hes: Vec<u32> = body.loop_hes(l).map(|h| h ^ 1).collect();
                        hes.reverse();
                        hes
                    })
                    .collect();
                turned.add_tagged_face(
                    body.f_surf[f as usize],
                    !body.f_sense[f as usize],
                    &loops,
                    body.f_tag[f as usize],
                );
            }
            body = turned;
        }
        Ok(body)
    }

    /// Add the edge `id` of the file to `body`: its number there, and
    /// whether it runs the same way.
    fn edge(
        &self,
        id: usize,
        body: &mut Body,
        vertices: &mut HashMap<usize, u32>,
    ) -> Result<(u32, bool), String> {
        let args = self
            .get(id, &["EDGE_CURVE"])
            .ok_or_else(|| format!("#{id} is not an edge"))?;
        let mut vertex = |arg: Option<&Arg>, body: &mut Body| -> Result<(u32, V3), String> {
            let v = arg.and_then(Arg::reference).ok_or("an edge has no end")?;
            let point = self
                .get(v, &["VERTEX_POINT"])
                .and_then(|a| a.get(1))
                .and_then(Arg::reference)
                .ok_or("an edge ends on something that is no vertex")?;
            let p = self.point(point)?;
            let index = *vertices.entry(v).or_insert_with(|| body.add_vert(p));
            Ok((index, p))
        };
        let (mut v0, mut p0) = vertex(args.get(1), body)?;
        let (mut v1, mut p1) = vertex(args.get(2), body)?;
        let mut curve = self.curve(
            args.get(3)
                .and_then(Arg::reference)
                .ok_or("an edge has no curve")?,
        )?;
        // The body's edge runs the way its curve does.
        let same = args.get(4).is_some_and(Arg::flag);
        if !same {
            std::mem::swap(&mut v0, &mut v1);
            std::mem::swap(&mut p0, &mut p1);
        }
        let (t0, t1) = if curve.kind == CurveKind::Line {
            (curve.param(p0), curve.param(p1))
        } else if let Some((lo, hi)) = curve.bounds().filter(|_| curve.kind == CurveKind::Spline) {
            // An open spline: between its ends, or all of it where both
            // ends of the edge are one vertex.
            if v0 == v1 {
                (lo, hi)
            } else {
                (curve.param(p0), curve.param(p1))
            }
        } else if v0 == v1 {
            // All the way round, starting at the vertex.
            if curve.kind == CurveKind::Circle {
                curve = Curve::circle(curve.f.o, curve.f.z, p0 - curve.f.o, curve.r);
                (0.0, TAU)
            } else {
                let t = curve.param(p0);
                (t, t + TAU)
            }
        } else {
            let t0 = curve.param(p0);
            (t0, wrap_from(curve.param(p1), t0))
        };
        if t1 <= t0 {
            return Err("the STEP file has an edge of no length".into());
        }
        let c = body.add_curve(curve);
        Ok((body.add_edge(c, t0, t1, v0, v1), same))
    }
}

/// Bytes of base64 text, as STEP files travel inside a recompute plan.
pub fn from_base64(text: &str) -> Result<Vec<u8>, String> {
    let digit = |b: u8| -> Option<u32> {
        Some(match b {
            b'A'..=b'Z' => u32::from(b - b'A'),
            b'a'..=b'z' => u32::from(b - b'a') + 26,
            b'0'..=b'9' => u32::from(b - b'0') + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        })
    };
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0u32);
    for b in text.bytes() {
        if b == b'=' || b.is_ascii_whitespace() {
            continue;
        }
        acc = (acc << 6) | digit(b).ok_or("STEP import contains invalid base64 data")?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Ok(out)
}
