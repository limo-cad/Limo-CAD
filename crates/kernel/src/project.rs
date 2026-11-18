//! Line drawings: what an eye at infinity sees of a set of solids.
//!
//! The lines of a drawing are the edges of the solids and the outlines of
//! their curved faces: the curves along which a face turns away from the
//! eye. On a cylinder, cone, sphere or torus those are known by formula.
//! On a free-form skin, the flank of a thread or a rolling-ball round they
//! are where the normal is square to the line of sight, found the way a
//! contour is drawn on a map: over a grid of the face's own parameters,
//! with every crossing closed in on. Each line is followed in small steps, and at
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
            SurfKind::Plane => {}
            // No formula for these: the outline is followed over the
            // face's own parameters.
            SurfKind::Helicoid | SurfKind::Coil | SurfKind::Spline | SurfKind::Pipe => {
                out.extend(turnings(body, f, trim.clone(), d, deflection));
            }
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
            let spans = stretches(&inside, from, to, samples);
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

/// The stretches of `[from, to]` over which `inside` holds, looked at in
/// `samples` places and closed in on where it changes.
fn stretches(inside: &dyn Fn(f64) -> bool, from: f64, to: f64, samples: usize) -> Vec<(f64, f64)> {
    let t = |k: usize| from + (to - from) * k as f64 / samples as f64;
    let mut spans: Vec<(f64, f64)> = Vec::new();
    let mut start: Option<f64> = inside(from).then_some(from);
    let mut was = start.is_some();
    for k in 1..=samples {
        let now = inside(t(k));
        if was == now {
            continue;
        }
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
        was = now;
    }
    if let Some(begun) = start {
        spans.push((begun, to));
    }
    spans
}

/// The lines along which a face without a formula for them turns away
/// from an eye looking along `-d`: where its normal is square to `d`.
///
/// The face's own parameters are laid out as a grid, fine enough for the
/// turns of a thread and the spans of a spline. Where the normal leans
/// towards the eye at one end of a grid line and away at the other, the
/// place between where it does neither is closed in on, and those places
/// are joined cell by cell into lines. A point anywhere along such a line
/// is then put back on the outline itself: a chord between two points of
/// it lies inside a round solid, where everything is hidden.
///
/// A line that runs along the very edge of the face is left to the edge:
/// see [`turns_away`].
fn turnings<'b>(body: &'b Body, f: u32, trim: FaceTrim, d: V3, deflection: f64) -> Vec<Path<'b>> {
    let (lo, hi) = (trim.lo, trim.hi);
    let size = hi - lo;
    if !(size.x > 0.0 && size.y > 0.0 && size.x.is_finite() && size.y.is_finite()) {
        return Vec::new();
    }
    let surf = trim.surf;
    // The surface's own parameters at a place of the face's.
    let own = move |trim: &FaceTrim, q: V2| {
        let q = if trim.turned { v2(-q.y, q.x) } else { q };
        (if trim.flip { -q.x } else { q.x }, q.y)
    };
    // How far the normal leans towards the eye.
    let lean = std::rc::Rc::new({
        let trim = trim.clone();
        move |q: V2| {
            let (u, v) = own(&trim, q);
            let n = surf.du(u, v).cross(surf.dv(u, v));
            let len = n.len();
            if len > 0.0 {
                n.dot(d) / len
            } else {
                0.0
            }
        }
    });
    // Cells: as many as it takes to see every turn about an axis and
    // every span of a spline, and no coarser than a sixtieth of the face.
    let middle = (lo + hi) * 0.5;
    let metric = trim.metric(middle);
    let lengths = v2(size.x * metric.x, size.y * metric.y);
    let longest = lengths.x.max(lengths.y).max(1.0e-9);
    let by_length = |length: f64| (length / (longest / 60.0)).ceil();
    let turn = TAU / 24.0;
    let (by_u, by_v) = match surf.kind {
        SurfKind::Helicoid => (size.x / turn, 4.0),
        SurfKind::Coil => (size.x / turn, size.y / turn),
        SurfKind::Pipe => (16.0, size.y / turn),
        SurfKind::Spline => {
            let form = surf.spline_form();
            let spans = |k: usize| form.knots[k].windows(2).filter(|w| w[1] > w[0]).count() as f64;
            let (su, sv) = (4.0 * spans(0), 4.0 * spans(1));
            if trim.turned {
                (sv, su)
            } else {
                (su, sv)
            }
        }
        _ => (8.0, 8.0),
    };
    let cells =
        |wanted: f64, length: f64| (wanted.max(by_length(length)).ceil() as usize).clamp(6, 768);
    let n = [cells(by_u, lengths.x), cells(by_v, lengths.y)];
    let step = v2(size.x / n[0] as f64, size.y / n[1] as f64);
    let node = |i: usize, j: usize| v2(lo.x + i as f64 * step.x, lo.y + j as f64 * step.y);
    let values: Vec<f64> = (0..=n[1])
        .flat_map(|j| (0..=n[0]).map(move |i| (i, j)))
        .map(|(i, j)| lean(node(i, j)))
        .collect();
    let value = |i: usize, j: usize| values[j * (n[0] + 1) + i];
    // A grid edge by number: from node `(i, j)` across, or up.
    let key = |i: usize, j: usize, up: bool| ((i as u64) << 33) | ((j as u64) << 1) | u64::from(up);
    let mut links: Vec<(u64, u64)> = Vec::new();
    for j in 0..n[1] {
        for i in 0..n[0] {
            let above = [
                value(i, j) > 0.0,
                value(i + 1, j) > 0.0,
                value(i + 1, j + 1) > 0.0,
                value(i, j + 1) > 0.0,
            ];
            let sides = [
                (above[0] != above[1], key(i, j, false)),
                (above[1] != above[2], key(i + 1, j, true)),
                (above[3] != above[2], key(i, j + 1, false)),
                (above[0] != above[3], key(i, j, true)),
            ];
            let cut: Vec<u64> = sides.iter().filter(|s| s.0).map(|s| s.1).collect();
            match cut.len() {
                2 => links.push((cut[0], cut[1])),
                4 => {
                    // A saddle: the middle says which corners are joined.
                    let mid = lean(node(i, j) + step * 0.5) > 0.0;
                    if mid == above[0] {
                        links.push((cut[0], cut[1]));
                        links.push((cut[2], cut[3]));
                    } else {
                        links.push((cut[0], cut[3]));
                        links.push((cut[1], cut[2]));
                    }
                }
                _ => {}
            }
        }
    }
    // Where on its grid edge the normal is square to the line of sight.
    let crossing = |key: u64| -> V2 {
        let (i, j, up) = (
            (key >> 33) as usize,
            ((key >> 1) & 0xffff_ffff) as usize,
            key & 1 == 1,
        );
        let (a, b) = (node(i, j), if up { node(i, j + 1) } else { node(i + 1, j) });
        let below = lean(a) > 0.0;
        let (mut near, mut far) = (0.0, 1.0);
        for _ in 0..48 {
            let mid = 0.5 * (near + far);
            if (lean(a + (b - a) * mid) > 0.0) == below {
                near = mid;
            } else {
                far = mid;
            }
        }
        a + (b - a) * (0.5 * (near + far))
    };
    let trim = std::rc::Rc::new(trim);
    let mut out: Vec<Path<'b>> = Vec::new();
    for chain in crate::meet::chains(&links) {
        let points: Vec<V2> = chain.into_iter().map(crossing).collect();
        if points.len() < 2 {
            continue;
        }
        let points = std::rc::Rc::new(points);
        // A place along the line, back on the outline: between two of its
        // points, then down the slope of the lean to where there is none.
        let place = {
            let (points, lean) = (points.clone(), lean.clone());
            move |t: f64| -> V2 {
                let last = points.len() - 1;
                let k = (t.floor().max(0.0) as usize).min(last - 1);
                let mut q =
                    points[k] + (points[k + 1] - points[k]) * (t - k as f64).clamp(0.0, 1.0);
                let (hu, hv) = (1.0e-4 * step.x, 1.0e-4 * step.y);
                for _ in 0..3 {
                    let here = lean(q);
                    let slope = v2(
                        (lean(v2(q.x + hu, q.y)) - lean(v2(q.x - hu, q.y))) / (2.0 * hu) * step.x,
                        (lean(v2(q.x, q.y + hv)) - lean(v2(q.x, q.y - hv))) / (2.0 * hv) * step.y,
                    );
                    let steep = slope.dot(slope);
                    if steep < 1.0e-24 {
                        break;
                    }
                    // No further than the cell it is in.
                    let fix = slope * (here / steep);
                    if fix.len() > 1.0 {
                        break;
                    }
                    q = v2(q.x - fix.x * step.x, q.y - fix.y * step.y);
                }
                q
            }
        };
        let (from, to) = (0.0, (points.len() - 1) as f64);
        // Chords enough that none strays further than a drawn curve may.
        let mut sag: f64 = 0.0;
        for k in 0..points.len() - 1 {
            let (a, b) = (trim.point(points[k]), trim.point(points[k + 1]));
            let middle = trim.point(place(k as f64 + 0.5));
            sag = sag.max(middle.dist((a + b) * 0.5));
        }
        let finer = (sag / deflection).sqrt().ceil().clamp(1.0, 8.0) as usize;
        let pieces = (points.len() - 1) * finer;
        let inside = {
            let (trim, place) = (trim.clone(), place.clone());
            move |t: f64| classify(body, &trim, f, trim.point(place(t))) == PtClass::In
        };
        for (a, b) in stretches(&inside, from, to, pieces.max(48)) {
            if b - a <= 1.0e-9 * (to - from) {
                continue;
            }
            let (trim, place) = (trim.clone(), place.clone());
            let share = ((b - a) / (to - from) * pieces as f64).ceil() as usize;
            out.push(Path {
                at: Box::new(move |t| trim.point(place(t))),
                from: a,
                to: b,
                straight: false,
                pieces: share.max(1),
            });
        }
    }
    out
}

/// Whether the skin turns away from the eye along edge `e` at `t`: the
/// face on one side of it faces the eye, a short way in, and the face on
/// the other side faces away.
///
/// Two patches of one smooth skin meet in an edge that is no line of the
/// drawing, unless the skin happens to turn away from the eye exactly
/// there. From the front, the side and above it often does: skins are
/// drawn round the same axes that drawings are looked along.
fn turns_away(body: &Body, e: u32, t: f64, d: V3, reach: f64) -> bool {
    let curve = body.e_curve_ref(e);
    let foot = curve.point(t);
    let travel = curve.deriv(t);
    let mut facing = [0.0; 2];
    for (side, slot) in facing.iter_mut().enumerate() {
        let f = body.h_face(2 * e + side as u32);
        // The face lies to the left of its own way along the edge.
        let way = if side == 0 { travel } else { -travel };
        let into = body.f_normal(f, foot).cross(way);
        if !(into.is_finite() && into.len2() > 0.0) {
            return false;
        }
        let within = body.f_surface(f).project(foot + into.unit() * reach);
        *slot = body.f_normal(f, within).dot(d);
    }
    facing[0] * facing[1] < 0.0
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
            let curve = body.e_curve_ref(e);
            let [from, to] = body.e_t[e as usize];
            if !options.smooth_edges && !body.e_sharp(e) {
                // A smooth edge is drawn only where it is the outline: on
                // skins that have no formula for theirs.
                let free = [2 * e, 2 * e + 1].iter().any(|&h| {
                    matches!(
                        body.f_surface(body.h_face(h)).kind,
                        SurfKind::Spline | SurfKind::Pipe | SurfKind::Helicoid | SurfKind::Coil
                    )
                });
                if !free {
                    continue;
                }
                let pieces = mesh.edge(e).len() - 1;
                let reach = 1.0e-3 * (1.0 + size);
                let outline = |t: f64| turns_away(body, e, t, view.toward, reach);
                for (a, b) in stretches(&outline, from, to, pieces.max(16)) {
                    // A crossing of the outline and the edge is a point,
                    // not a stretch of either.
                    if curve.point(a).dist(curve.point(b)) < 20.0 * reach {
                        continue;
                    }
                    let share = ((b - a) / (to - from) * pieces as f64).ceil() as usize;
                    let path = Path {
                        at: Box::new(move |t| curve.point(t)),
                        from: a,
                        to: b,
                        straight: curve.kind == CurveKind::Line,
                        pieces: share.max(1),
                    };
                    scene.sort(&path, step, &mut drawing, options.hidden);
                }
                continue;
            }
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
