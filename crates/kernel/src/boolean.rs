//! Regularised Boolean operations on solids.
//!
//! 1. **Intersect.** Every pair of faces whose boxes overlap contributes the
//!    curves where their surfaces meet, clipped to both faces. Faces on the
//!    same surface imprint their boundaries on each other instead.
//! 2. **Arrange.** All vertices and edges of both bodies and of the section
//!    curves go through one registry that merges coincident points and
//!    splits every edge at every vertex lying on it. From here on, shared
//!    geometry is shared topology and sewing needs no tolerance.
//! 3. **Split.** Each face is retraced from its boundary and the section
//!    edges inside it, by walking around vertices in angular order.
//! 4. **Classify.** One interior point per piece is tested against the other
//!    solid: inside, outside, or on a coincident face.
//! 5. **Select and sew.** The pieces the operation keeps become the result,
//!    which is then simplified ([`crate::simplify`]).

use std::collections::{HashMap, HashSet};

use crate::geom::{Curve, SurfKind, Surface};
use crate::math::*;
use crate::roots::{curve_curve, curve_surface, Hits};
use crate::simplify::unify;
use crate::ssi::{intersect, Ssi};
use crate::topo::{Body, NONE};
use crate::trim::{
    classify, edge_closest, image, loop_points, EdgeMesh, FaceTrim, PtClass, TrimLoop,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Union,
    Cut,
    Intersect,
}

/// Chord tolerance of the trimming polylines used for classification.
const SAG: f64 = 0.01;
const PER_TURN: f64 = 32.0;
/// Points closer than this are one vertex.
const VTOL: f64 = 4.0 * TOL;
const CELL: f64 = 1.0e-4;
/// A point this close to a curve lies on it.
const ON_CURVE: f64 = 10.0 * TOL;

/// Shared vertex and edge registry of one Boolean.
struct Arr {
    /// Geometry only: curves, vertices and edges, no faces.
    g: Body,
    cells: HashMap<[i64; 3], Vec<u32>>,
    by_verts: HashMap<(u32, u32), Vec<u32>>,
}

impl Arr {
    fn new() -> Arr {
        Arr {
            g: Body::new(),
            cells: HashMap::new(),
            by_verts: HashMap::new(),
        }
    }

    fn vert(&mut self, p: V3) -> u32 {
        let cell = |v: f64| (v / CELL).floor() as i64;
        let (cx, cy, cz) = (cell(p.x), cell(p.y), cell(p.z));
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(list) = self.cells.get(&[cx + dx, cy + dy, cz + dz]) {
                        for &i in list {
                            if self.g.verts[i as usize].dist(p) < VTOL {
                                return i;
                            }
                        }
                    }
                }
            }
        }
        let i = self.g.add_vert(p);
        self.cells.entry([cx, cy, cz]).or_default().push(i);
        i
    }

    /// Register the span `[t0, t1]` of `c` between two vertices and return
    /// the half-edge that travels `t0 → t1`. A geometrically identical edge
    /// registered earlier is reused.
    fn edge(&mut self, c: &Curve, t0: f64, t1: f64, v0: u32, v1: u32) -> u32 {
        let key = (v0.min(v1), v0.max(v1));
        let mid_t = 0.5 * (t0 + t1);
        let mid = c.point(mid_t);
        if let Some(list) = self.by_verts.get(&key) {
            for &e in list {
                let [a0, a1] = self.g.e_t[e as usize];
                let ec = self.g.e_curve_ref(e);
                let emid_t = 0.5 * (a0 + a1);
                if ec.point(emid_t).dist(mid) > ON_CURVE {
                    continue;
                }
                let same = if v0 != v1 {
                    self.g.e_v[e as usize][0] == v0
                } else {
                    ec.deriv(emid_t).dot(c.deriv(mid_t)) > 0.0
                };
                return 2 * e + u32::from(!same);
            }
        }
        let ci = self.g.add_curve(*c);
        let e = self.g.add_edge(ci, t0, t1, v0, v1);
        self.by_verts.entry(key).or_default().push(e);
        2 * e
    }
}

/// One operand with its trimming data.
struct Side<'a> {
    body: &'a Body,
    trims: Vec<FaceTrim>,
    boxes: Vec<Aabb>,
    /// Bounds of every edge.
    eboxes: Vec<Aabb>,
    bbox: Aabb,
    /// Per face: faces of the other body on the same surface, and whether
    /// the outward normals agree.
    partners: Vec<Vec<(u32, bool)>>,
    /// Per original edge: registry half-edges from `t0` to `t1`.
    sub: Vec<Vec<u32>>,
    /// Per face: registry edges strictly inside it.
    interior: Vec<Vec<u32>>,
    /// Edge against a surface of the other body.
    hits: HashMap<(u32, u32), Hits>,
}

impl<'a> Side<'a> {
    fn new(body: &'a Body) -> Side<'a> {
        let mesh = EdgeMesh::build(body, SAG, PER_TURN);
        let trims: Vec<FaceTrim> = (0..body.face_count() as u32)
            .map(|f| FaceTrim::build(body, &mesh, f, SAG))
            .collect();
        let boxes: Vec<Aabb> = trims.iter().map(FaceTrim::bbox).collect();
        let mut bbox = Aabb::EMPTY;
        for b in &boxes {
            bbox.union(b);
        }
        let eboxes: Vec<Aabb> = (0..body.edge_count() as u32)
            .map(|e| {
                let mut b = Aabb::EMPTY;
                for p in mesh.edge(e) {
                    b.add(*p);
                }
                b.grown(SAG + 10.0 * TOL)
            })
            .collect();
        Side {
            body,
            trims,
            boxes,
            eboxes,
            bbox,
            partners: vec![Vec::new(); body.face_count()],
            sub: Vec::new(),
            interior: vec![Vec::new(); body.face_count()],
            hits: HashMap::new(),
        }
    }
}

/// A piece of intersection curve lying in both bodies' boundaries.
struct Seg {
    curve: Curve,
    t0: f64,
    t1: f64,
    /// Face of each body the piece lies strictly inside (not on its edge).
    in_a: Option<u32>,
    in_b: Option<u32>,
}

/// A face, or part of one, in terms of registry half-edges.
struct Piece {
    orig: u32,
    loops: Vec<Vec<u32>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cls {
    In,
    Out,
    OnSame,
    OnOpp,
}

pub fn boolean(a: &Body, b: &Body, op: Op) -> Result<Body, String> {
    let result = evaluate(a, b, op);
    if let (Err(error), Some(dir)) = (&result, std::env::var_os("LIMO_CAD_KERNEL_DUMP")) {
        // Opt-in bug capture: keep the operands of a failed operation.
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::path::PathBuf::from(dir);
        let stem = format!("fail-{}-{n}-{op:?}", std::process::id());
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(format!("{stem}-a.body")), a.to_text());
        let _ = std::fs::write(dir.join(format!("{stem}-b.body")), b.to_text());
        let _ = std::fs::write(dir.join(format!("{stem}.txt")), error);
    }
    result
}

fn evaluate(a: &Body, b: &Body, op: Op) -> Result<Body, String> {
    if a.is_empty() {
        return match op {
            Op::Union => Ok(b.clone()),
            _ => Err("the target body is empty".into()),
        };
    }
    if b.is_empty() {
        return match op {
            Op::Intersect => Err("the bodies do not overlap".into()),
            _ => Ok(a.clone()),
        };
    }
    let timing = std::env::var_os("LIMO_CAD_KERNEL_PROFILE").is_some();
    let mut clock = std::time::Instant::now();
    let mut lap = |name: &str| {
        if timing {
            eprintln!("  {name}: {:?}", clock.elapsed());
            clock = std::time::Instant::now();
        }
    };
    let mut sa = Side::new(a);
    let mut sb = Side::new(b);
    if !sa.bbox.overlaps(&sb.bbox) {
        return disjoint(a, b, op);
    }
    lap("trims");

    // 1. Intersection curves and coincident faces.
    let mut segs: Vec<Seg> = Vec::new();
    let mut relation: HashMap<(u32, u32), Ssi> = HashMap::new();
    for fa in 0..a.face_count() as u32 {
        if !sa.boxes[fa as usize].overlaps(&sb.bbox) {
            continue;
        }
        for fb in 0..b.face_count() as u32 {
            if !sa.boxes[fa as usize].overlaps(&sb.boxes[fb as usize]) {
                continue;
            }
            let key = (a.f_surf[fa as usize], b.f_surf[fb as usize]);
            let rel = relation
                .entry(key)
                .or_insert_with(|| intersect(a.f_surface(fa), b.f_surface(fb)))
                .clone();
            match rel {
                Ssi::None => {}
                Ssi::Unsupported(what) => {
                    if separated(&sa, fa, b.f_surface(fb)) || separated(&sb, fb, a.f_surface(fa)) {
                        continue;
                    }
                    return Err(format!(
                        "this needs the intersection of {what}, which the Limo kernel does not support yet"
                    ));
                }
                Ssi::Same(same) => {
                    let agree = same == (a.f_sense[fa as usize] == b.f_sense[fb as usize]);
                    sa.partners[fa as usize].push((fb, agree));
                    sb.partners[fb as usize].push((fa, agree));
                    imprint(&sb, fb, &sa, fa, false, &mut segs);
                    imprint(&sa, fa, &sb, fb, true, &mut segs);
                }
                Ssi::Curves(curves) => {
                    for c in &curves {
                        section(&mut sa, fa, &mut sb, fb, c, &mut segs);
                    }
                }
            }
        }
    }

    lap("intersect");
    // 2. One registry for every vertex and edge.
    let mut arr = Arr::new();
    let va: Vec<(u32, V3)> = a.verts.iter().map(|p| (arr.vert(*p), *p)).collect();
    let vb: Vec<(u32, V3)> = b.verts.iter().map(|p| (arr.vert(*p), *p)).collect();
    let mut ends: Vec<(u32, V3)> = Vec::new();
    for s in &segs {
        for t in [s.t0, s.t1] {
            let p = s.curve.point(t);
            ends.push((arr.vert(p), p));
        }
    }
    let dedup = |mut list: Vec<(u32, V3)>| {
        list.sort_by_key(|x| x.0);
        list.dedup_by_key(|x| x.0);
        list
    };
    let reach_a = sa.bbox.grown(10.0 * VTOL);
    let reach_b = sb.bbox.grown(10.0 * VTOL);
    let for_a = dedup(
        vb.iter()
            .chain(&ends)
            .filter(|x| reach_a.contains(x.1))
            .copied()
            .collect(),
    );
    let for_b = dedup(
        va.iter()
            .chain(&ends)
            .filter(|x| reach_b.contains(x.1))
            .copied()
            .collect(),
    );
    let for_segs = dedup(va.iter().chain(&vb).chain(&ends).copied().collect());
    sa.sub = (0..a.edge_count() as u32)
        .map(|e| {
            let [t0, t1] = a.e_t[e as usize];
            split(&mut arr, a.e_curve_ref(e), t0, t1, &for_a)
        })
        .collect();
    sb.sub = (0..b.edge_count() as u32)
        .map(|e| {
            let [t0, t1] = b.e_t[e as usize];
            split(&mut arr, b.e_curve_ref(e), t0, t1, &for_b)
        })
        .collect();
    for s in &segs {
        for h in split(&mut arr, &s.curve, s.t0, s.t1, &for_segs) {
            if let Some(f) = s.in_a {
                sa.interior[f as usize].push(h >> 1);
            }
            if let Some(f) = s.in_b {
                sb.interior[f as usize].push(h >> 1);
            }
        }
    }
    let gmesh = EdgeMesh::build(&arr.g, SAG, PER_TURN);

    lap("arrange");
    // 3-4. Split and classify.
    let pieces_a = pieces(&arr, &gmesh, &sa)?;
    let pieces_b = pieces(&arr, &gmesh, &sb)?;
    lap("split");
    let cls_a = classify_pieces(&arr, &gmesh, &pieces_a, &sa, &sb)?;
    let cls_b = classify_pieces(&arr, &gmesh, &pieces_b, &sb, &sa)?;
    lap("classify");

    // 5. Select and sew.
    let keep_a = |c: Cls| match op {
        Op::Union => matches!(c, Cls::Out | Cls::OnSame),
        Op::Cut => matches!(c, Cls::Out | Cls::OnOpp),
        Op::Intersect => matches!(c, Cls::In | Cls::OnSame),
    };
    let keep_b = |c: Cls| match op {
        Op::Union => c == Cls::Out,
        Op::Cut | Op::Intersect => c == Cls::In,
    };
    if std::env::var_os("LIMO_CAD_KERNEL_TRACE").is_some() {
        for (name, list, cls) in [("A", &pieces_a, &cls_a), ("B", &pieces_b, &cls_b)] {
            for (piece, c) in list.iter().zip(cls.iter()) {
                eprintln!("{name} face {} -> {c:?}", piece.orig);
                for lp in &piece.loops {
                    let text: Vec<String> = lp
                        .iter()
                        .map(|&h| {
                            let p = arr.g.verts[arr.g.h_origin(h) as usize];
                            format!("h{h}({:.3},{:.3},{:.3})", p.x, p.y, p.z)
                        })
                        .collect();
                    eprintln!("    {}", text.join(" "));
                }
            }
        }
    }
    let mut out = Body::new();
    let mut edge_map: Vec<u32> = vec![NONE; arr.g.edge_count()];
    let mut vert_map: Vec<u32> = vec![NONE; arr.g.verts.len()];
    let mut add = |out: &mut Body, src: &Body, piece: &Piece, reversed: bool| -> Result<(), String> {
        let (surf, same) = out.intern_surf(*src.f_surface(piece.orig));
        let sense = (src.f_sense[piece.orig as usize] == same) != reversed;
        let mut loops: Vec<Vec<u32>> = Vec::with_capacity(piece.loops.len());
        for lp in &piece.loops {
            let mut mapped = Vec::with_capacity(lp.len());
            for &h in lp {
                let e = (h >> 1) as usize;
                if edge_map[e] == NONE {
                    let mut vs = arr.g.e_v[e];
                    for v in &mut vs {
                        if vert_map[*v as usize] == NONE {
                            vert_map[*v as usize] = out.add_vert(arr.g.verts[*v as usize]);
                        }
                        *v = vert_map[*v as usize];
                    }
                    let c = out.add_curve(*arr.g.e_curve_ref(e as u32));
                    let [t0, t1] = arr.g.e_t[e];
                    edge_map[e] = out.add_edge(c, t0, t1, vs[0], vs[1]);
                }
                let oh = 2 * edge_map[e] + ((h & 1) ^ u32::from(reversed));
                if out.h_loop[oh as usize] != NONE
                    || mapped.contains(&oh)
                    || loops.iter().any(|l| l.contains(&oh))
                {
                    return Err(
                        "the result is not a closed solid (bodies touch along an edge or the geometry is ambiguous)"
                            .into(),
                    );
                }
                mapped.push(oh);
            }
            if reversed {
                mapped.reverse();
            }
            loops.push(mapped);
        }
        // Reserve the half-edges immediately so a later duplicate is caught.
        out.add_face(surf, sense, &loops);
        Ok(())
    };
    for (piece, &c) in pieces_a.iter().zip(&cls_a) {
        if keep_a(c) {
            add(&mut out, a, piece, false)?;
        }
    }
    for (piece, &c) in pieces_b.iter().zip(&cls_b) {
        if keep_b(c) {
            add(&mut out, b, piece, op == Op::Cut)?;
        }
    }
    if out.is_empty() {
        return Err(match op {
            Op::Cut => "the cut removes the entire body".into(),
            Op::Intersect => "the bodies do not overlap".into(),
            Op::Union => "the union is empty".into(),
        });
    }
    out.check()
        .map_err(|e| format!("the result is not a closed solid ({e})"))?;
    lap("sew");
    let result = unify(&out);
    result
        .check()
        .map_err(|e| format!("simplifying the result failed ({e})"))?;
    Ok(result)
}

fn disjoint(a: &Body, b: &Body, op: Op) -> Result<Body, String> {
    match op {
        Op::Union => {
            let mut out = a.clone();
            out.append(b);
            Ok(out)
        }
        Op::Cut => Ok(a.clone()),
        Op::Intersect => Err("the bodies do not overlap".into()),
    }
}

/// True when every sample of face `f` is clearly on one side of `s`.
fn separated(side: &Side, f: u32, s: &Surface) -> bool {
    let trim = &side.trims[f as usize];
    let mut sign = 0.0;
    let mut check = |p: V3| {
        let d = s.sdist(p);
        if d.abs() < 4.0 * trim.sag {
            return false;
        }
        if sign == 0.0 {
            sign = d.signum();
        }
        d.signum() == sign
    };
    for lp in &trim.loops {
        if !lp.p3.iter().all(|p| check(*p)) {
            return false;
        }
    }
    trim.interior_points()
        .iter()
        .all(|(q, _)| check(trim.point(*q)))
}

fn dedup_params(c: &Curve, params: &mut Vec<f64>) {
    params.sort_by(f64::total_cmp);
    let mut kept: Vec<f64> = Vec::with_capacity(params.len());
    for &s in params.iter() {
        let near = kept
            .last()
            .is_some_and(|&l| (s - l) * c.deriv(s).len() <= 2.0 * TOL);
        if !near {
            kept.push(s);
        }
    }
    *params = kept;
}

/// Parameters on `c` where it meets the boundary of face `f`.
fn boundary_params(
    side: &mut Side,
    f: u32,
    other: &Surface,
    other_id: u32,
    other_box: &Aabb,
    c: &Curve,
    out: &mut Vec<f64>,
) {
    let body = side.body;
    for h in body.face_hes(f) {
        let e = h >> 1;
        // An edge that stays clear of the other face cannot bound the curve
        // inside it.
        if !side.eboxes[e as usize].overlaps(other_box) {
            continue;
        }
        let hits = side.hits.entry((e, other_id)).or_insert_with(|| {
            let [t0, t1] = body.e_t[e as usize];
            curve_surface(body.e_curve_ref(e), t0, t1, other)
        });
        match hits {
            Hits::Coincident => {
                // The edge runs along the other surface; if it also runs
                // along this curve, its ends bound the overlap.
                if c.dist(body.e_mid(e)) < ON_CURVE {
                    for v in body.e_v[e as usize] {
                        out.push(c.param(body.verts[v as usize]));
                    }
                }
            }
            Hits::Roots(roots) => {
                for r in roots.iter() {
                    let p = body.e_point(e, r.t);
                    if c.dist(p) < ON_CURVE {
                        out.push(c.param(p));
                    }
                }
            }
        }
    }
}

/// Clip intersection curve `c` of faces `fa` and `fb` to both faces.
fn section(sa: &mut Side, fa: u32, sb: &mut Side, fb: u32, c: &Curve, segs: &mut Vec<Seg>) {
    let (surf_a, surf_b) = (*sa.body.f_surface(fa), *sb.body.f_surface(fb));
    let mut params: Vec<f64> = Vec::new();
    let (box_a, box_b) = (sa.boxes[fa as usize], sb.boxes[fb as usize]);
    boundary_params(sa, fa, &surf_b, sb.body.f_surf[fb as usize], &box_b, c, &mut params);
    boundary_params(sb, fb, &surf_a, sa.body.f_surf[fa as usize], &box_a, c, &mut params);
    let periodic = c.periodic();
    if periodic {
        for p in &mut params {
            *p = wrap_from(*p, 0.0);
        }
    }
    // A bounded curve also ends where it ends.
    if let Some((lo, hi)) = c.bounds() {
        params.extend([lo, hi]);
    }
    dedup_params(c, &mut params);
    if periodic && params.len() > 1 {
        let (first, last) = (params[0], *params.last().unwrap());
        if (first + TAU - last) * c.deriv(first).len() <= 2.0 * TOL {
            params.pop();
        }
    }
    let mut spans: Vec<(f64, f64)> = Vec::new();
    if periodic {
        if params.is_empty() {
            spans.push((0.0, TAU));
        }
        for i in 0..params.len() {
            let next = if i + 1 < params.len() {
                params[i + 1]
            } else {
                params[0] + TAU
            };
            spans.push((params[i], next));
        }
    } else {
        for w in params.windows(2) {
            spans.push((w[0], w[1]));
        }
    }
    let tracing = std::env::var_os("LIMO_CAD_KERNEL_TRACE").is_some();
    if tracing {
        eprintln!(
            "section A{fa} x B{fb} {:?} at {:?}: params {params:?}",
            c.kind,
            c.point(0.0)
        );
    }
    for (t0, t1) in spans {
        let mid = c.point(0.5 * (t0 + t1));
        if tracing {
            eprintln!(
                "   span {t0}..{t1} mid {mid:?}: A {:?} B {:?}",
                classify(sa.body, &sa.trims[fa as usize], fa, mid),
                classify(sb.body, &sb.trims[fb as usize], fb, mid)
            );
        }
        let ca = classify(sa.body, &sa.trims[fa as usize], fa, mid);
        if ca == PtClass::Out {
            continue;
        }
        let cb = classify(sb.body, &sb.trims[fb as usize], fb, mid);
        if cb == PtClass::Out {
            continue;
        }
        segs.push(Seg {
            curve: *c,
            t0,
            t1,
            in_a: (ca == PtClass::In).then_some(fa),
            in_b: (cb == PtClass::In).then_some(fb),
        });
    }
}

/// Faces `fs` (source) and `fd` (destination) share a surface: the parts of
/// the source boundary strictly inside the destination face split it.
fn imprint(src: &Side, fs: u32, dst: &Side, fd: u32, dst_is_b: bool, segs: &mut Vec<Seg>) {
    let surf = dst.body.f_surface(fd);
    let mut seen: Vec<u32> = Vec::new();
    for h in src.body.face_hes(fs) {
        let e = h >> 1;
        if seen.contains(&e) {
            continue;
        }
        seen.push(e);
        let c = *src.body.e_curve_ref(e);
        let [t0, t1] = src.body.e_t[e as usize];
        let mut params = vec![t0, t1];
        if !src.eboxes[e as usize].overlaps(&dst.boxes[fd as usize]) {
            continue;
        }
        for hd in dst.body.face_hes(fd) {
            let ed = hd >> 1;
            if !src.eboxes[e as usize].overlaps(&dst.eboxes[ed as usize]) {
                continue;
            }
            match curve_curve(&c, t0, t1, dst.body.e_curve_ref(ed), surf) {
                Hits::Coincident => {
                    for v in dst.body.e_v[ed as usize] {
                        let p = dst.body.verts[v as usize];
                        if c.dist(p) >= ON_CURVE {
                            continue;
                        }
                        let mut t = c.param(p);
                        if c.periodic() {
                            t = wrap_from(t, t0);
                        }
                        if t > t0 && t < t1 {
                            params.push(t);
                        }
                    }
                }
                Hits::Roots(roots) => {
                    for r in roots {
                        let (_, d, _) = edge_closest(dst.body, ed, c.point(r.t));
                        if d < ON_CURVE {
                            params.push(r.t);
                        }
                    }
                }
            }
        }
        dedup_params(&c, &mut params);
        for w in params.windows(2) {
            let mid = c.point(0.5 * (w[0] + w[1]));
            if classify(dst.body, &dst.trims[fd as usize], fd, mid) != PtClass::In {
                continue;
            }
            segs.push(Seg {
                curve: c,
                t0: w[0],
                t1: w[1],
                in_a: (!dst_is_b).then_some(fd),
                in_b: dst_is_b.then_some(fd),
            });
        }
    }
}

/// Register the span `[t0, t1]` of `c`, split at every candidate vertex that
/// lies on it. Returns registry half-edges in order from `t0` to `t1`.
fn split(arr: &mut Arr, c: &Curve, t0: f64, t1: f64, cands: &[(u32, V3)]) -> Vec<u32> {
    let mut stops: Vec<(f64, u32)> = vec![
        (t0, arr.vert(c.point(t0))),
        (t1, arr.vert(c.point(t1))),
    ];
    if !cands.is_empty() {
        let mut bounds = Aabb::EMPTY;
        let curved = c.kind != crate::geom::CurveKind::Line;
        let n = if curved { 16 } else { 1 };
        for i in 0..=n {
            bounds.add(c.point(t0 + (t1 - t0) * i as f64 / n as f64));
        }
        let bulge = if curved {
            // Chord sag of one sampling step, with a margin for curves that
            // bend faster than a circle of their radius.
            2.0 * c.r.abs().max(c.s.abs()) * (1.0 - (0.5 * (t1 - t0) / n as f64).cos())
        } else {
            0.0
        };
        let bounds = bounds.grown(bulge + 2.0 * VTOL);
        for &(v, p) in cands {
            if !bounds.contains(p) {
                continue;
            }
            let mut t = c.param(p);
            if c.periodic() {
                t = wrap_from(t, t0);
            }
            let margin = 2.0 * VTOL / c.deriv(t).len().max(1.0e-12);
            if t < t0 + margin || t > t1 - margin || c.point(t).dist(p) > VTOL {
                continue;
            }
            stops.push((t, v));
        }
        stops.sort_by(|x, y| x.0.total_cmp(&y.0));
    }
    let mut out = Vec::with_capacity(stops.len() - 1);
    let mut prev = stops[0];
    for &stop in &stops[1..] {
        let length = c.length(prev.0, stop.0);
        if stop.1 == prev.1 && length < 2.0 * VTOL {
            continue;
        }
        out.push(arr.edge(c, prev.0, stop.0, prev.1, stop.1));
        prev = stop;
    }
    out
}

/// Split every face of one operand.
fn pieces(arr: &Arr, gmesh: &EdgeMesh, side: &Side) -> Result<Vec<Piece>, String> {
    let body = side.body;
    let mut out = Vec::with_capacity(body.face_count());
    for f in 0..body.face_count() as u32 {
        let boundary: Vec<Vec<u32>> = body
            .face_loops(f)
            .map(|l| {
                let mut hes = Vec::new();
                for h in body.loop_hes(l) {
                    let subs = &side.sub[(h >> 1) as usize];
                    if h & 1 == 0 {
                        hes.extend_from_slice(subs);
                    } else {
                        hes.extend(subs.iter().rev().map(|x| x ^ 1));
                    }
                }
                hes
            })
            .collect();
        let split = split_face(
            arr,
            gmesh,
            body.f_surface(f),
            body.f_sense[f as usize],
            boundary,
            &side.interior[f as usize],
        )
        .map_err(|e| format!("could not split a face along the intersection ({e})"))?;
        out.extend(split.into_iter().map(|loops| Piece { orig: f, loops }));
    }
    Ok(out)
}

/// Unit direction in which half-edge `h` leaves its origin, measured along a
/// short chord so that tangent curves still sort correctly.
fn chord_dir(g: &Body, h: u32, delta: f64) -> V3 {
    let e = h >> 1;
    let c = g.e_curve_ref(e);
    let [t0, t1] = g.e_t[e as usize];
    let half = 0.5 * (t1 - t0);
    if h & 1 == 0 {
        let dt = (delta / c.deriv(t0).len().max(1.0e-12)).min(half);
        (c.point(t0 + dt) - c.point(t0)).unit()
    } else {
        let dt = (delta / c.deriv(t1).len().max(1.0e-12)).min(half);
        (c.point(t1 - dt) - c.point(t1)).unit()
    }
}

/// Walk the half-edges into loops with the face on the left: at each vertex
/// continue with the first edge clockwise from the one just travelled.
fn trace(arr: &Arr, surf: &Surface, sense: bool, hes: &[u32]) -> Result<Vec<Vec<u32>>, String> {
    let g = &arr.g;
    let mut outgoing: HashMap<u32, Vec<u32>> = HashMap::new();
    let mut shortest: HashMap<u32, f64> = HashMap::new();
    for &h in hes {
        outgoing.entry(g.h_origin(h)).or_default().push(h);
        let len = g.e_length(h >> 1);
        for v in [g.h_origin(h), g.h_dest(h)] {
            let m = shortest.entry(v).or_insert(f64::INFINITY);
            *m = m.min(len);
        }
    }
    let angle = |h: u32, v: u32| -> f64 {
        let dir = chord_dir(g, h, 1.0e-2 * shortest[&v]);
        let mut n = surf.normal(g.verts[v as usize]);
        if !sense {
            n = -n;
        }
        let e1 = n.any_perp();
        let e2 = n.cross(e1);
        dir.dot(e2).atan2(dir.dot(e1))
    };
    let mut angles: HashMap<u32, f64> = HashMap::with_capacity(hes.len());
    for &h in hes {
        angles.insert(h, angle(h, g.h_origin(h)));
    }
    let next = |h: u32| -> Result<u32, String> {
        let w = g.h_dest(h);
        let back = angle(h ^ 1, w);
        let mut best: Option<(f64, u32)> = None;
        for &cand in outgoing.get(&w).map(Vec::as_slice).unwrap_or(&[]) {
            let turn = if cand == h ^ 1 {
                TAU
            } else {
                let t = wrap_from(back - angles[&cand], 0.0);
                if !(1.0e-9..=TAU - 1.0e-9).contains(&t) {
                    return Err("two edges leave a vertex in the same direction".into());
                }
                t
            };
            if best.is_none_or(|(b, _)| turn < b) {
                best = Some((turn, cand));
            }
        }
        best.map(|(_, c)| c)
            .ok_or_else(|| "an intersection edge ends inside a face".to_string())
    };
    let mut used: HashSet<u32> = HashSet::with_capacity(hes.len());
    let mut loops = Vec::new();
    for &start in hes {
        if used.contains(&start) {
            continue;
        }
        let mut lp = Vec::new();
        let mut h = start;
        loop {
            if !used.insert(h) {
                return Err("face boundary does not close".into());
            }
            lp.push(h);
            h = next(h)?;
            if h == start {
                break;
            }
            if lp.len() > hes.len() {
                return Err("face boundary does not close".into());
            }
        }
        loops.push(lp);
    }
    Ok(loops)
}

fn inside_polygon(uv: &[V2], q: V2) -> bool {
    let mut inside = false;
    for w in uv.windows(2) {
        let (a, b) = (w[0], w[1]);
        if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

/// Height of a u-winding loop at parameter `u`.
fn v_at(lp: &TrimLoop, u: f64) -> Option<f64> {
    let x = image(u, lp.lo.x);
    for w in lp.uv.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (lo, hi) = if a.x < b.x { (a.x, b.x) } else { (b.x, a.x) };
        if lo == hi {
            continue;
        }
        if x >= lo && x < hi {
            return Some(a.y + (x - a.x) / (b.x - a.x) * (b.y - a.y));
        }
    }
    None
}

#[derive(Clone, Copy)]
enum Kind {
    /// Inside one counter-clockwise loop.
    Disk(usize),
    /// Between two u-winding loops; the upper one may sit a period higher
    /// when the band runs through the parameter seam of a torus tube.
    Band(usize, usize, f64),
    /// From a u-winding loop to the pole or apex above / below it.
    Above(usize),
    Below(usize),
    /// The whole closed surface minus holes.
    Whole,
}

/// Group traced loops into connected regions of the surface.
fn group(
    gmesh: &EdgeMesh,
    surf: &Surface,
    sense: bool,
    loops: &[Vec<u32>],
) -> Result<Vec<Vec<usize>>, String> {
    let polys: Vec<TrimLoop> = loops
        .iter()
        .map(|hes| {
            FaceTrim::from_points(*surf, !sense, vec![loop_points(gmesh, hes.iter().copied())], SAG)
                .loops
                .pop()
                .ok_or_else(|| "empty loop".to_string())
        })
        .collect::<Result<_, _>>()?;
    let mut regions: Vec<(Kind, Vec<usize>)> = Vec::new();
    let mut holes: Vec<usize> = Vec::new();
    let mut winding: Vec<usize> = Vec::new();
    for (i, p) in polys.iter().enumerate() {
        if p.winds_v() {
            return Err("splitting a torus face across its tube is not supported yet".into());
        }
        if p.winds_u() {
            winding.push(i);
        } else if p.area > 0.0 {
            regions.push((Kind::Disk(i), vec![i]));
        } else {
            holes.push(i);
        }
    }
    let mean_v = |i: usize| polys[i].uv.iter().map(|q| q.y).sum::<f64>() / polys[i].uv.len() as f64;
    // Both directions of one section circle sit at the same height: the one
    // travelling in -u closes the band below and must come first.
    winding.sort_by(|&x, &y| {
        let (vx, vy) = (mean_v(x), mean_v(y));
        if (vx - vy).abs() <= 1.0e-7 * (1.0 + vx.abs()) {
            polys[x].shift.x.total_cmp(&polys[y].shift.x)
        } else {
            vx.total_cmp(&vy)
        }
    });
    let mut lift: Vec<f64> = vec![0.0; winding.len()];
    if surf.kind == SurfKind::Torus {
        let leading = winding
            .iter()
            .take_while(|&&i| polys[i].shift.x < 0.0)
            .count();
        if leading > 0 && leading < winding.len() {
            winding.rotate_left(leading);
            let n = winding.len();
            for slot in &mut lift[n - leading..] {
                *slot = TAU;
            }
        }
    }
    let mut k = 0;
    while k < winding.len() {
        let i = winding[k];
        if polys[i].shift.x > 0.0 {
            if k + 1 < winding.len() && polys[winding[k + 1]].shift.x < 0.0 {
                let j = winding[k + 1];
                regions.push((Kind::Band(i, j, lift[k + 1] - lift[k]), vec![i, j]));
                k += 2;
                continue;
            }
            if surf.kind != SurfKind::Sphere {
                return Err("a face has no upper boundary".into());
            }
            regions.push((Kind::Above(i), vec![i]));
        } else {
            if !matches!(surf.kind, SurfKind::Sphere | SurfKind::Cone) {
                return Err("a face has no lower boundary".into());
            }
            regions.push((Kind::Below(i), vec![i]));
        }
        k += 1;
    }
    let shifts: &[f64] = if surf.u_periodic() {
        &[0.0, TAU, -TAU]
    } else {
        &[0.0]
    };
    let holds = |kind: Kind, q: V2| -> bool {
        match kind {
            Kind::Disk(i) => shifts
                .iter()
                .any(|s| inside_polygon(&polys[i].uv, v2(q.x + s, q.y))),
            Kind::Band(lo, hi, rise) => match (v_at(&polys[lo], q.x), v_at(&polys[hi], q.x)) {
                (Some(a), Some(b)) => {
                    let turns: &[f64] = if surf.v_periodic() {
                        &[0.0, TAU, -TAU]
                    } else {
                        &[0.0]
                    };
                    turns.iter().any(|t| q.y + t > a && q.y + t < b + rise)
                }
                _ => false,
            },
            Kind::Above(i) => v_at(&polys[i], q.x).is_some_and(|a| q.y > a),
            Kind::Below(i) => v_at(&polys[i], q.x).is_some_and(|a| q.y < a),
            Kind::Whole => true,
        }
    };
    let size = |kind: Kind| match kind {
        Kind::Disk(i) => polys[i].area.abs(),
        _ => f64::INFINITY,
    };
    let closed = matches!(surf.kind, SurfKind::Sphere | SurfKind::Torus);
    let mut whole: Option<usize> = None;
    for h in holes {
        let uv = &polys[h].uv;
        let n = uv.len() - 1;
        // Probe just to the left of the loop, strictly inside its region: the
        // same curve travelled the other way bounds a different region.
        let probes: Vec<V2> = [0, n / 3, 2 * n / 3]
            .iter()
            .map(|&i| {
                let (p, q) = (uv[i], uv[(i + 1).min(n)]);
                let along = q - p;
                (p + q) * 0.5 + v2(-along.y, along.x) * 1.0e-3
            })
            .collect();
        let mut best: Option<(f64, usize)> = None;
        for (r, (kind, _)) in regions.iter().enumerate() {
            if matches!(kind, Kind::Whole) {
                continue;
            }
            let votes = probes.iter().filter(|q| holds(*kind, **q)).count();
            if votes * 2 > probes.len() && best.is_none_or(|(s, _)| size(*kind) < s) {
                best = Some((size(*kind), r));
            }
        }
        match best {
            Some((_, r)) => regions[r].1.push(h),
            None if closed => {
                let r = *whole.get_or_insert_with(|| {
                    regions.push((Kind::Whole, Vec::new()));
                    regions.len() - 1
                });
                regions[r].1.push(h);
            }
            None => return Err("a hole lies outside every boundary".into()),
        }
    }
    Ok(regions.into_iter().map(|(_, loops)| loops).collect())
}

/// Split one face along the registry edges inside it.
fn split_face(
    arr: &Arr,
    gmesh: &EdgeMesh,
    surf: &Surface,
    sense: bool,
    boundary: Vec<Vec<u32>>,
    interior: &[u32],
) -> Result<Vec<Vec<Vec<u32>>>, String> {
    if interior.is_empty() {
        return Ok(vec![boundary]);
    }
    let rim: HashSet<u32> = boundary.iter().flatten().map(|h| h >> 1).collect();
    let mut inner: Vec<u32> = interior.to_vec();
    inner.sort_unstable();
    inner.dedup();
    inner.retain(|e| !rim.contains(e));
    loop {
        if inner.is_empty() {
            return Ok(vec![boundary]);
        }
        let hes: Vec<u32> = boundary
            .iter()
            .flatten()
            .copied()
            .chain(inner.iter().flat_map(|&e| [2 * e, 2 * e + 1]))
            .collect();
        let loops = trace(arr, surf, sense, &hes)?;
        let regions = group(gmesh, surf, sense, &loops)?;
        let mut region_of: HashMap<u32, usize> = HashMap::with_capacity(hes.len());
        for (r, members) in regions.iter().enumerate() {
            for &l in members {
                for &h in &loops[l] {
                    region_of.insert(h, r);
                }
            }
        }
        if std::env::var_os("LIMO_CAD_KERNEL_TRACE").is_some() {
            eprintln!("split {:?} sense {sense}: inner {inner:?}", surf.kind);
            for (i, lp) in loops.iter().enumerate() {
                eprintln!("  loop {i}: {lp:?}");
            }
            eprintln!("  regions {regions:?}");
        }
        // An edge with the same region on both sides separates nothing.
        let slits: Vec<u32> = inner
            .iter()
            .copied()
            .filter(|&e| region_of.get(&(2 * e)) == region_of.get(&(2 * e + 1)))
            .collect();
        if slits.is_empty() {
            return Ok(regions
                .into_iter()
                .map(|members| members.into_iter().map(|l| loops[l].clone()).collect())
                .collect());
        }
        inner.retain(|e| !slits.contains(e));
    }
}

/// Is `p` inside the solid? `None` when `p` lies on its boundary or every
/// probe direction is degenerate.
fn point_in_solid(side: &Side, p: V3) -> Option<bool> {
    if !side.bbox.contains(p) {
        return Some(false);
    }
    let body = side.body;
    let far = side.bbox.diagonal() + p.dist((side.bbox.lo + side.bbox.hi) * 0.5) + 1.0;
    const DIRS: [V3; 6] = [
        v3(0.531_2, 0.719_3, 0.447_9),
        v3(-0.687_1, 0.231_8, 0.688_6),
        v3(0.211_3, -0.854_1, 0.475_2),
        v3(-0.393_7, -0.512_9, -0.762_8),
        v3(0.862_4, -0.127_7, -0.489_9),
        v3(-0.115_6, 0.918_3, -0.378_7),
    ];
    'dirs: for d in DIRS {
        let d = d.unit();
        let ray = Curve::line(p, d);
        let mut crossings = 0;
        for f in 0..body.face_count() as u32 {
            if !side.boxes[f as usize].hit_by_ray(p, d) {
                continue;
            }
            let roots = match curve_surface(&ray, 0.0, far, body.f_surface(f)) {
                Hits::Coincident => continue 'dirs,
                Hits::Roots(roots) => roots,
            };
            for r in roots {
                let q = ray.point(r.t);
                match classify(body, &side.trims[f as usize], f, q) {
                    PtClass::Out => {}
                    PtClass::On => continue 'dirs,
                    PtClass::In => {
                        if r.t < 20.0 * TOL {
                            // The start point itself is on the boundary.
                            return None;
                        }
                        if r.tangent {
                            continue 'dirs;
                        }
                        crossings += 1;
                    }
                }
            }
        }
        return Some(crossings % 2 == 1);
    }
    None
}

fn classify_pieces(
    arr: &Arr,
    gmesh: &EdgeMesh,
    list: &[Piece],
    this: &Side,
    other: &Side,
) -> Result<Vec<Cls>, String> {
    let mut out = Vec::with_capacity(list.len());
    for piece in list {
        let f = piece.orig;
        let trim = FaceTrim::from_points(
            *this.body.f_surface(f),
            !this.body.f_sense[f as usize],
            piece
                .loops
                .iter()
                .map(|hes| loop_points(gmesh, hes.iter().copied()))
                .collect(),
            SAG,
        );
        let mut decided: Option<Cls> = None;
        for (q, _) in trim.interior_points() {
            let p = trim.point(q);
            let mut on: Option<Cls> = None;
            for &(partner, agree) in &this.partners[f as usize] {
                if classify(other.body, &other.trims[partner as usize], partner, p) != PtClass::Out {
                    on = Some(if agree { Cls::OnSame } else { Cls::OnOpp });
                    break;
                }
            }
            if on.is_some() {
                decided = on;
                break;
            }
            if let Some(inside) = point_in_solid(other, p) {
                decided = Some(if inside { Cls::In } else { Cls::Out });
                break;
            }
        }
        if decided.is_none() && std::env::var_os("LIMO_CAD_KERNEL_TRACE").is_some() {
            eprintln!(
                "undecided piece of face {f} ({:?}), {} loops, candidates:",
                this.body.f_surface(f).kind,
                piece.loops.len()
            );
            for (q, score) in trim.interior_points() {
                let p = trim.point(q);
                eprintln!("   {p:?} clearance {score:.4} -> {:?}", point_in_solid(other, p));
            }
            for lp in &piece.loops {
                let text: Vec<String> = lp
                    .iter()
                    .map(|&h| {
                        let p = arr.g.verts[arr.g.h_origin(h) as usize];
                        format!("({:.4},{:.4},{:.4})", p.x, p.y, p.z)
                    })
                    .collect();
                eprintln!("   loop {}", text.join(" "));
            }
        }
        out.push(decided.ok_or_else(|| {
            "could not tell whether part of a face is inside the other body".to_string()
        })?);
    }
    Ok(out)
}
