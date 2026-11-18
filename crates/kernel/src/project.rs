//! Line drawings: what an eye at infinity sees of a set of solids.
//!
//! The lines of a drawing are the edges of the solids and the outlines of
//! their curved faces: the curves along which a cylinder, cone, sphere or
//! torus turns away from the eye. Each is followed in small steps, and at
//! every step a ray towards the eye says whether something stands in the
//! way. The steps are joined into runs that are seen and runs that are
//! hidden, and the changes between them are closed in on by halving.
//!
//! Faces are sorted into a grid over the picture plane, so a ray only tries
//! the few faces that can lie in front of its point.
//!
//! A section cuts the solids with a plane first, and reports the outlines
//! of the cut faces on top of the drawing of what is left.

use crate::boolean::{boolean, Op};
use crate::geom::{Curve, CurveKind, SurfKind};
use crate::make::{self, Region, Seg};
use crate::math::*;
use crate::roots::{curve_surface, Hits};
use crate::topo::Body;
use crate::trim::{classify, EdgeMesh, FaceTrim, PtClass};

/// The picture plane: unit vectors towards the eye, to the right and up.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub toward: V3,
    pub right: V3,
    pub up: V3,
}

impl View {
    /// Looking from `toward`, with `up` as near to upright as it can be.
    pub fn new(toward: V3, up: V3) -> Option<View> {
        let toward = toward.unit();
        let right = up.cross(toward);
        if !toward.is_finite() || right.len2() < 1.0e-18 {
            return None;
        }
        let right = right.unit();
        Some(View {
            toward,
            right,
            up: toward.cross(right),
        })
    }

    /// Where a point of the model is on paper.
    pub fn flat(&self, p: V3) -> V2 {
        v2(p.dot(self.right), p.dot(self.up))
    }

    /// How near a point is to the eye; larger is nearer.
    fn depth(&self, p: V3) -> f64 {
        p.dot(self.toward)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DrawOptions {
    /// Report the hidden runs as well.
    pub hidden: bool,
    /// Draw edges where two faces meet smoothly, such as the ends of a
    /// round.
    pub smooth_edges: bool,
    /// How far a drawn curve may stray from the true one, in model units.
    pub deflection: f64,
}

impl Default for DrawOptions {
    fn default() -> Self {
        DrawOptions {
            hidden: true,
            smooth_edges: false,
            deflection: 0.02,
        }
    }
}

/// Runs of line on paper.
#[derive(Clone, Debug, Default)]
pub struct Drawing {
    pub visible: Vec<Vec<V2>>,
    pub hidden: Vec<Vec<V2>>,
}

/// One face ready to be looked through: where it is, and where it is on
/// paper.
struct Pane<'a> {
    body: &'a Body,
    face: u32,
    trim: FaceTrim,
    lo: V2,
    hi: V2,
    /// Its part nearest the eye.
    near: f64,
}

struct Scene<'a> {
    view: View,
    panes: Vec<Pane<'a>>,
    /// Panes by cell of a grid over the paper.
    cells: Vec<Vec<u32>>,
    lo: V2,
    cell: V2,
    across: [usize; 2],
    /// A length below which two points are one.
    fine: f64,
}

impl<'a> Scene<'a> {
    fn new(bodies: &[&'a Body], meshes: &[EdgeMesh], view: View, deflection: f64) -> Scene<'a> {
        let mut panes = Vec::new();
        let (mut lo, mut hi) = (
            v2(f64::INFINITY, f64::INFINITY),
            v2(f64::NEG_INFINITY, f64::NEG_INFINITY),
        );
        for (body, mesh) in bodies.iter().zip(meshes) {
            for face in 0..body.face_count() as u32 {
                let trim = FaceTrim::build(body, mesh, face, deflection);
                let bounds = trim.bbox();
                let (mut flo, mut fhi, mut near) = (
                    v2(f64::INFINITY, f64::INFINITY),
                    v2(f64::NEG_INFINITY, f64::NEG_INFINITY),
                    f64::NEG_INFINITY,
                );
                for corner in 0..8 {
                    let p = v3(
                        if corner & 1 == 0 {
                            bounds.lo.x
                        } else {
                            bounds.hi.x
                        },
                        if corner & 2 == 0 {
                            bounds.lo.y
                        } else {
                            bounds.hi.y
                        },
                        if corner & 4 == 0 {
                            bounds.lo.z
                        } else {
                            bounds.hi.z
                        },
                    );
                    let q = view.flat(p);
                    flo = v2(flo.x.min(q.x), flo.y.min(q.y));
                    fhi = v2(fhi.x.max(q.x), fhi.y.max(q.y));
                    near = near.max(view.depth(p));
                }
                lo = v2(lo.x.min(flo.x), lo.y.min(flo.y));
                hi = v2(hi.x.max(fhi.x), hi.y.max(fhi.y));
                panes.push(Pane {
                    body,
                    face,
                    trim,
                    lo: flo,
                    hi: fhi,
                    near,
                });
            }
        }
        let side = ((panes.len() as f64).sqrt().ceil() as usize).clamp(1, 256);
        let across = [side, side];
        let size = v2((hi.x - lo.x).max(1.0e-9), (hi.y - lo.y).max(1.0e-9));
        let cell = v2(size.x / side as f64, size.y / side as f64);
        let mut cells: Vec<Vec<u32>> = vec![Vec::new(); side * side];
        let column = |x: f64| (((x - lo.x) / cell.x) as usize).min(side - 1);
        let row = |y: f64| (((y - lo.y) / cell.y) as usize).min(side - 1);
        for (k, pane) in panes.iter().enumerate() {
            for j in row(pane.lo.y)..=row(pane.hi.y) {
                for i in column(pane.lo.x)..=column(pane.hi.x) {
                    cells[j * side + i].push(k as u32);
                }
            }
        }
        Scene {
            view,
            panes,
            cells,
            lo,
            cell,
            across,
            fine: 1.0e-6 * (1.0 + size.len()),
        }
    }

    /// Whether something stands between `p` and the eye.
    fn hidden(&self, p: V3) -> bool {
        let q = self.view.flat(p);
        let i = ((q.x - self.lo.x) / self.cell.x).floor();
        let j = ((q.y - self.lo.y) / self.cell.y).floor();
        if i < 0.0 || j < 0.0 || i as usize >= self.across[0] || j as usize >= self.across[1] {
            return false;
        }
        let depth = self.view.depth(p);
        let ray = Curve::line(p, self.view.toward);
        for &k in &self.cells[j as usize * self.across[0] + i as usize] {
            let pane = &self.panes[k as usize];
            if pane.near <= depth + self.fine
                || q.x < pane.lo.x
                || q.x > pane.hi.x
                || q.y < pane.lo.y
                || q.y > pane.hi.y
            {
                continue;
            }
            let reach = pane.near - depth + 1.0;
            let Hits::Roots(roots) =
                curve_surface(&ray, self.fine, reach, pane.body.f_surface(pane.face))
            else {
                // The line of sight runs in the face's own surface. Seen
                // edge on like that the face is a line on paper, and the
                // point is behind that line if the face carries on towards
                // the eye from it: the far rim of a rod seen from its end.
                let ahead = ray.point(100.0 * self.fine);
                if classify(pane.body, &pane.trim, pane.face, ahead) != PtClass::Out {
                    return true;
                }
                continue;
            };
            for root in roots {
                // A face the ray only brushes hides nothing either.
                // An edge stops a ray as well as the middle of a face does.
                if !root.tangent
                    && classify(pane.body, &pane.trim, pane.face, ray.point(root.t)) != PtClass::Out
                {
                    return true;
                }
            }
        }
        false
    }

    /// Cut a path into runs that are seen and runs that are not.
    fn sort(&self, path: &Path, step: f64, drawing: &mut Drawing, hidden: bool) {
        // Short pieces, each judged at its middle. The middle is taken on
        // the path itself: the middle of a chord lies inside a round solid,
        // where everything is hidden.
        let span = path.to - path.from;
        let parts = if path.straight {
            let length = (path.at)(path.from).dist((path.at)(path.to));
            ((length / step).ceil() as usize).clamp(1, 4096)
        } else {
            path.pieces.max(1)
        };
        let t = |k: usize| path.from + span * k as f64 / parts as f64;
        let covered: Vec<bool> = (0..parts)
            .map(|k| self.hidden((path.at)(0.5 * (t(k) + t(k + 1)))))
            .collect();
        let emit =
            |from: f64, to: f64, first: usize, last: usize, state: bool, out: &mut Drawing| {
                if !hidden && state {
                    return;
                }
                // The ends of the run, and for a curve the points between.
                let mut flat = vec![self.view.flat((path.at)(from))];
                if !path.straight {
                    flat.extend((first + 1..=last).map(|k| self.view.flat((path.at)(t(k)))));
                }
                flat.push(self.view.flat((path.at)(to)));
                // A line seen end on is a point on paper.
                let long = flat.windows(2).any(|w| (w[1] - w[0]).len() > self.fine);
                if long {
                    if state {
                        out.hidden.push(flat);
                    } else {
                        out.visible.push(flat);
                    }
                }
            };
        let mut from = path.from;
        let mut first = 0;
        for k in 1..=parts {
            if k < parts && covered[k] == covered[k - 1] {
                continue;
            }
            let state = covered[k - 1];
            let to = if k == parts {
                path.to
            } else {
                // The change lies between the middles of the two pieces:
                // halve that stretch until it is pinned down.
                let (mut a, mut b) = (0.5 * (t(k - 1) + t(k)), 0.5 * (t(k) + t(k + 1)));
                for _ in 0..30 {
                    let mid = 0.5 * (a + b);
                    if self.hidden((path.at)(mid)) == state {
                        a = mid;
                    } else {
                        b = mid;
                    }
                }
                0.5 * (a + b)
            };
            // Whole pieces strictly inside the run.
            let last = if k == parts {
                parts - 1
            } else if to > t(k) {
                k
            } else {
                k - 1
            };
            let inner_first = if from < t(first) {
                first.saturating_sub(1)
            } else {
                first
            };
            emit(from, to, inner_first, last, state, drawing);
            from = to;
            first = if to > t(k) { k + 1 } else { k };
        }
    }
}

/// A line to draw: a point for every parameter between two ends.
struct Path<'c> {
    at: Box<dyn Fn(f64) -> V3 + 'c>,
    from: f64,
    to: f64,
    /// Straight in space: two points say it all.
    straight: bool,
    /// How many chords a curved one takes.
    pieces: usize,
}

/// The outlines of the curved faces of `body`: for each face, the lines
/// on it along which it turns away from the eye.
fn outlines<'b>(body: &'b Body, mesh: &EdgeMesh, view: &View, deflection: f64) -> Vec<Path<'b>> {
    let d = view.toward;
    let mut out: Vec<Path<'b>> = Vec::new();
    for f in 0..body.face_count() as u32 {
        let s = *body.f_surface(f);
        if s.kind == SurfKind::Plane {
            continue;
        }
        let trim = FaceTrim::build(body, mesh, f, deflection);
        // Chords to a full turn of radius `r`.
        let turn = |r: f64| {
            let step = if deflection >= r {
                1.0
            } else {
                2.0 * (1.0 - deflection / r).acos()
            };
            ((TAU / step.clamp(1.0e-3, 0.5)).ceil() as usize).max(12)
        };
        let (z, dz) = (s.f.z, d.dot(s.f.z));
        let across = d - z * dz;
        // Whole curves of the surface: a point for every parameter, the
        // range of the parameter, whether straight, and chords needed.
        type Whole = (Box<dyn Fn(f64) -> V3>, f64, f64, bool, usize);
        let mut curves: Vec<Whole> = Vec::new();
        match s.kind {
            // A thread shows by its edges, the helices along crest and
            // root; the outline of its flanks is not drawn.
            SurfKind::Plane
            | SurfKind::Helicoid
            | SurfKind::Coil
            | SurfKind::Spline
            | SurfKind::Pipe => {}
            SurfKind::Cylinder => {
                if across.len() > 1.0e-9 {
                    let side = z.cross(across).unit();
                    for sign in [1.0, -1.0] {
                        let foot = s.f.o + side * (sign * s.r);
                        curves.push((
                            Box::new(move |t| foot + z * t),
                            trim.lo.y,
                            trim.hi.y,
                            true,
                            1,
                        ));
                    }
                }
            }
            SurfKind::Cone => {
                // Rulings whose normal is square to the line of sight.
                let lean = s.s.tan() * dz;
                if across.len() > lean.abs() + 1.0e-12 {
                    let middle = across.unit();
                    let open = (lean / across.len()).acos();
                    for sign in [1.0, -1.0] {
                        let out = Xform::rotation(V3::ZERO, z, sign * open).vec(middle);
                        let along = z + out * s.s.tan();
                        let tip = s.f.o;
                        curves.push((
                            Box::new(move |t| tip + along * t),
                            trim.lo.y.max(0.0),
                            trim.hi.y.max(0.0),
                            true,
                            1,
                        ));
                    }
                }
            }
            SurfKind::Sphere => {
                let e1 = d.any_perp();
                let e2 = d.cross(e1);
                curves.push((
                    Box::new(move |a| s.f.o + (e1 * a.cos() + e2 * a.sin()) * s.r),
                    0.0,
                    TAU,
                    false,
                    turn(s.r),
                ));
            }
            SurfKind::Torus => {
                // Round the axis, the two places on the tube where it
                // turns away.
                for branch in [0.0, PI] {
                    curves.push((
                        Box::new(move |u| {
                            let out = s.f.x * u.cos() + s.f.y * u.sin();
                            s.point(u, (-out.dot(d)).atan2(dz) + branch)
                        }),
                        0.0,
                        TAU,
                        false,
                        turn(s.r + s.s),
                    ));
                }
            }
        }
        for (at, from, to, straight, pieces) in curves {
            if to - from <= 1.0e-12 {
                continue;
            }
            // The stretches of the curve that lie on the face.
            let inside = |t: f64| {
                let p = at(t);
                trim.contains_uv(trim.uv(p))
            };
            let samples = if straight { 64 } else { pieces.max(48) };
            let t = |k: usize| from + (to - from) * k as f64 / samples as f64;
            let mut spans: Vec<(f64, f64)> = Vec::new();
            let mut start: Option<f64> = inside(from).then_some(from);
            for k in 1..=samples {
                let (was, now) = (inside(t(k - 1)), inside(t(k)));
                if was == now {
                    continue;
                }
                // Where the curve crosses the face's boundary.
                let (mut a, mut b) = (t(k - 1), t(k));
                for _ in 0..40 {
                    let mid = 0.5 * (a + b);
                    if inside(mid) == was {
                        a = mid;
                    } else {
                        b = mid;
                    }
                }
                let change = 0.5 * (a + b);
                match start.take() {
                    Some(begun) => spans.push((begun, change)),
                    None => start = Some(change),
                }
            }
            if let Some(begun) = start {
                spans.push((begun, to));
            }
            let at: std::rc::Rc<dyn Fn(f64) -> V3> = std::rc::Rc::from(at);
            for (a, b) in spans {
                if b - a <= 1.0e-9 * (to - from) {
                    continue;
                }
                let at = at.clone();
                let share = ((b - a) / (to - from) * pieces as f64).ceil() as usize;
                out.push(Path {
                    at: Box::new(move |t| at(t)),
                    from: a,
                    to: b,
                    straight,
                    pieces: share.max(1),
                });
            }
        }
    }
    out
}

/// The drawing of some solids seen from `view`.
pub fn draw(bodies: &[&Body], view: &View, options: &DrawOptions) -> Drawing {
    let deflection = options.deflection.clamp(1.0e-4, 10.0);
    let meshes: Vec<EdgeMesh> = bodies
        .iter()
        .map(|body| EdgeMesh::build_for_faces(body, deflection, 48.0))
        .collect();
    let scene = Scene::new(bodies, &meshes, *view, deflection);
    let size = bodies.iter().fold(0.0, |far: f64, body| {
        let b = body.bbox();
        far.max(b.diagonal())
    });
    let step = (8.0 * deflection).max(size / 600.0);
    let mut drawing = Drawing::default();
    for (body, mesh) in bodies.iter().zip(&meshes) {
        for e in 0..body.edge_count() as u32 {
            if !options.smooth_edges && !body.e_sharp(e) {
                continue;
            }
            let curve = body.e_curve_ref(e);
            let [from, to] = body.e_t[e as usize];
            let path = Path {
                at: Box::new(move |t| curve.point(t)),
                from,
                to,
                straight: curve.kind == CurveKind::Line,
                pieces: mesh.edge(e).len() - 1,
            };
            scene.sort(&path, step, &mut drawing, options.hidden);
        }
        for path in outlines(body, mesh, view, deflection) {
            scene.sort(&path, step, &mut drawing, options.hidden);
        }
    }
    drawing
}

/// A body cut by a plane.
pub struct Cut {
    /// What is left behind the plane (`None`: nothing).
    pub kept: Option<Body>,
    /// The outlines of the cut faces, as closed lines of points. Empty
    /// when the plane cuts no material.
    pub outlines: Vec<Vec<V3>>,
    /// The plane touches the body without cutting into it.
    pub touches: bool,
    /// Where it touches: the outlines of the body's own faces that lie in
    /// the plane.
    pub contact: Vec<Vec<V3>>,
}

/// Faces a cutting plane leaves are tagged with this.
const CUT: u64 = 0xc07_c07;

/// Cut `body` with the plane through `point` square to `normal`, keeping
/// what lies behind it (and, with `depth`, no further back than that).
pub fn cut(
    body: &Body,
    point: V3,
    normal: V3,
    depth: Option<f64>,
    deflection: f64,
) -> Result<Cut, String> {
    let n = normal.unit();
    if !n.is_finite() || n.len2() < 0.5 {
        return Err("the cutting plane has no direction".into());
    }
    let bounds = body.bbox();
    let reach = bounds.diagonal() + point.dist((bounds.lo + bounds.hi) * 0.5) + 1.0;
    // How far the body reaches in front of the plane and behind it.
    let (mut front, mut back) = (f64::NEG_INFINITY, f64::INFINITY);
    for corner in 0..8 {
        let p = v3(
            if corner & 1 == 0 {
                bounds.lo.x
            } else {
                bounds.hi.x
            },
            if corner & 2 == 0 {
                bounds.lo.y
            } else {
                bounds.hi.y
            },
            if corner & 4 == 0 {
                bounds.lo.z
            } else {
                bounds.hi.z
            },
        );
        let h = (p - point).dot(n);
        front = front.max(h);
        back = back.min(h);
    }
    // A slab of everything behind the plane.
    let slab = |at: V3, facing: V3| -> Result<Body, String> {
        let f = Frame::from_z(at, facing);
        let corner = |sx: f64, sy: f64| at + f.x * (sx * reach) + f.y * (sy * reach);
        let p = [
            corner(-1.0, -1.0),
            corner(1.0, -1.0),
            corner(1.0, 1.0),
            corner(-1.0, 1.0),
        ];
        let region = Region {
            loops: vec![(0..4)
                .map(|i| Seg::Line {
                    a: p[i],
                    b: p[(i + 1) % 4],
                })
                .collect()],
        };
        let mut slab = make::extrude(&region, -facing, 0.0, 2.0 * reach)?;
        slab.retag(|_| CUT);
        Ok(slab)
    };
    let mut kept = if back >= -TOL {
        // All of it is in front.
        None
    } else if front <= TOL {
        Some(body.clone())
    } else {
        Some(boolean(body, &slab(point, n)?, Op::Intersect)?)
    };
    if let (Some(depth), Some(whole)) = (depth, kept.as_ref()) {
        if back < -depth - TOL {
            let far = point - n * depth;
            kept = if front <= -depth + TOL {
                None
            } else {
                Some(boolean(whole, &slab(far, -n)?, Op::Intersect)?)
            };
        }
    }
    // The loops of a face, as closed lines of points.
    let rims = |of: &Body, f: u32, into: &mut Vec<Vec<V3>>| {
        for l in of.face_loops(f) {
            let mut line: Vec<V3> = Vec::new();
            for h in of.loop_hes(l) {
                let mut points = crate::tess::edge_polyline(of, h >> 1, deflection);
                if h & 1 == 1 {
                    points.reverse();
                }
                points.pop();
                line.extend(points);
            }
            if let Some(first) = line.first().copied() {
                line.push(first);
                into.push(line);
            }
        }
    };
    let in_plane = |of: &Body, f: u32| {
        let s = of.f_surface(f);
        s.is_plane() && s.f.z.cross(n).len() < 1.0e-9 && (s.f.o - point).dot(n).abs() < 1.0e-6
    };
    let mut outlines = Vec::new();
    if let Some(kept) = &kept {
        for f in 0..kept.face_count() as u32 {
            // The face the first plane left: on it, looking forwards.
            if kept.f_tag[f as usize] == CUT
                && in_plane(kept, f)
                && kept.f_normal(f, kept.f_surface(f).f.o).dot(n) > 0.5
            {
                rims(kept, f, &mut outlines);
            }
        }
    }
    let touches = outlines.is_empty() && front >= -TOL && back <= TOL;
    let mut contact = Vec::new();
    if touches {
        for f in 0..body.face_count() as u32 {
            if in_plane(body, f) {
                rims(body, f, &mut contact);
            }
        }
    }
    Ok(Cut {
        kept,
        outlines,
        touches,
        contact,
    })
}
