//! The kernel behind Limo CAD's host-neutral solid contract: a recompute
//! plan goes in, an evaluated scene comes out.
//!
//! Bodies are immutable values, so the session keeps the state after every
//! job of the previous plan. A new plan resumes from the longest unchanged
//! prefix: editing feature `k` replays from `k`, not from the start, and an
//! untouched body keeps its geometry identity and its cached display mesh.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use limo_cad_core::{BodyId, FeatureId, PlaneBasis};
use limo_cad_solid::{
    stable_face_id, CircularCurveDto, CombineOperation, ConicalSurfaceDto, CylindricalSurfaceDto,
    ExtrudeOperation, HistorySupportQuery, HoleBottomStyle, HoleExtent, HoleStyle,
    HoleThreadRepresentation, KernelBodyDto, KernelCurveDto, KernelEdgeDto, KernelFaceDto,
    KernelFeatureErrorDto, KernelHoleJobDto, KernelJobDto, KernelPlanarFaceSourceDto,
    KernelProfileDto, KernelSceneDto, KernelTransformDto, PlanarFaceSignatureDto, Point3Dto,
    RecomputePlanDto,
};

use crate::blend;
use crate::boolean::{boolean, Op};
use crate::geom::{CurveKind, SurfKind};
use crate::make::{self, Region, Seg};
use crate::math::*;
use crate::props::planar_props;
use crate::tess::{edge_polyline, tessellate, Mesh, TessOptions};
use crate::topo::Body;
use crate::trim::{EdgeMesh, FaceTrim};

/// A body value with the identity of its geometry.
#[derive(Clone)]
struct Slot {
    body: Arc<Body>,
    generation: u64,
}

type State = BTreeMap<u64, Slot>;

/// Stateful modelling session.
pub struct Kernel {
    state: State,
    /// Jobs of the last plan that applied, each with the state after it.
    history: Vec<(KernelJobDto, State)>,
    clock: u64,
    /// Display data by geometry generation.
    display: HashMap<u64, Arc<KernelBodyDto>>,
    pub options: TessOptions,
    /// Jobs actually executed by the last recompute (the rest were reused).
    pub last_applied_jobs: usize,
}

impl Default for Kernel {
    fn default() -> Self {
        Kernel::new()
    }
}

fn p3(p: &Point3Dto) -> V3 {
    v3(p.x, p.y, p.z)
}

fn dto(p: V3) -> Point3Dto {
    Point3Dto {
        x: p.x,
        y: p.y,
        z: p.z,
    }
}

fn unsupported(what: &str) -> String {
    format!("{what} is not supported by the Limo kernel yet")
}

impl Kernel {
    pub fn new() -> Kernel {
        Kernel {
            state: State::new(),
            history: Vec::new(),
            clock: 0,
            display: HashMap::new(),
            options: TessOptions::default(),
            last_applied_jobs: 0,
        }
    }

    pub fn reset(&mut self) {
        self.state.clear();
        self.history.clear();
        self.display.clear();
    }

    pub fn body_ids(&self) -> Vec<u64> {
        self.state.keys().copied().collect()
    }

    pub fn body(&self, id: u64) -> Option<&Body> {
        self.state.get(&id).map(|slot| &*slot.body)
    }

    /// Geometry generation of every live body. Equal generations mean
    /// identical geometry.
    pub fn generations(&self) -> BTreeMap<u64, u64> {
        self.state
            .iter()
            .map(|(id, slot)| (*id, slot.generation))
            .collect()
    }

    /// True when the last recompute applied exactly these jobs.
    pub fn completed_jobs_match(&self, plan: &RecomputePlanDto) -> bool {
        plan.errors.is_empty()
            && self.history.len() == plan.jobs.len()
            && self.history.iter().zip(&plan.jobs).all(|(h, j)| h.0 == *j)
    }

    pub fn recompute(&mut self, plan: &RecomputePlanDto) -> KernelSceneDto {
        self.recompute_with_supports(plan, &[]).0
    }

    /// Recompute and report which face-hosted sketches still have their
    /// planar support at the point in history where they were created.
    pub fn recompute_with_supports(
        &mut self,
        plan: &RecomputePlanDto,
        queries: &[HistorySupportQuery],
    ) -> (KernelSceneDto, BTreeSet<FeatureId>) {
        let reused = self
            .history
            .iter()
            .zip(&plan.jobs)
            .take_while(|(h, j)| h.0 == **j)
            .count();
        self.history.truncate(reused);
        self.state = self
            .history
            .last()
            .map(|(_, state)| state.clone())
            .unwrap_or_default();
        self.last_applied_jobs = 0;
        let mut errors = plan.errors.clone();
        for job in &plan.jobs[reused..] {
            self.last_applied_jobs += 1;
            match self.apply(job) {
                Ok(()) => self.history.push((job.clone(), self.state.clone())),
                Err(message) => {
                    errors.push(KernelFeatureErrorDto {
                        feature_id: job.feature_id(),
                        message,
                    });
                    break;
                }
            }
        }
        let mut verified = BTreeSet::new();
        if errors.is_empty() {
            for query in queries {
                let Some(at) = plan
                    .jobs
                    .iter()
                    .rposition(|job| job.feature_id() == query.after_feature)
                else {
                    continue;
                };
                let Some((_, state)) = self.history.get(at) else {
                    continue;
                };
                let found = state.iter().any(|(id, slot)| {
                    (0..slot.body.face_count() as u32).any(|f| {
                        slot.body.f_surface(f).is_plane()
                            && stable_face_id(BodyId(*id), &format!("face:{f}")) == query.face_id
                    })
                });
                if found {
                    verified.insert(query.sketch_id);
                }
            }
        }
        (self.scene(errors), verified)
    }

    /// The evaluated scene of the current state.
    pub fn scene(&mut self, errors: Vec<KernelFeatureErrorDto>) -> KernelSceneDto {
        let live: BTreeSet<u64> = self.state.values().map(|slot| slot.generation).collect();
        // History states can come back; keep their display data too.
        let retained: BTreeSet<u64> = self
            .history
            .iter()
            .flat_map(|(_, state)| state.values().map(|slot| slot.generation))
            .chain(live.iter().copied())
            .collect();
        self.display
            .retain(|generation, _| retained.contains(generation));
        let mut bodies = Vec::with_capacity(self.state.len());
        for (id, slot) in &self.state {
            let options = self.options;
            let shared = self
                .display
                .entry(slot.generation)
                .or_insert_with(|| Arc::new(body_dto(0, &slot.body, &options)));
            let mut body = (**shared).clone();
            body.body_id = BodyId(*id);
            bodies.push(body);
        }
        KernelSceneDto { bodies, errors }
    }

    /// Tessellate one body at an explicit precision (mesh export).
    pub fn tessellate(&self, id: u64, linear: f64, angular: f64) -> Option<Mesh> {
        let defaults = TessOptions::default();
        self.body(id).map(|body| {
            tessellate(
                body,
                &TessOptions {
                    linear: if linear > 0.0 {
                        linear
                    } else {
                        defaults.linear
                    },
                    angular: if angular > 0.0 {
                        angular
                    } else {
                        defaults.angular
                    },
                },
            )
        })
    }

    fn put(&mut self, id: u64, body: Body) {
        self.clock += 1;
        self.state.insert(
            id,
            Slot {
                body: Arc::new(body),
                generation: self.clock,
            },
        );
    }

    fn get(&self, id: BodyId, role: &str) -> Result<Arc<Body>, String> {
        self.state
            .get(&id.0)
            .map(|slot| slot.body.clone())
            .ok_or_else(|| format!("{role} body is missing"))
    }

    /// Apply one job. The state changes only when the whole job succeeds.
    pub fn apply(&mut self, job: &KernelJobDto) -> Result<(), String> {
        match job {
            KernelJobDto::Extrude(j) => {
                let normal = p3(&j.normal);
                let tools = match &j.source_face {
                    Some(source) => vec![self.face_tool(
                        source,
                        normal,
                        j.start_offset,
                        j.end_offset,
                        j.taper_angle_deg,
                    )?],
                    None => j
                        .profiles
                        .iter()
                        .map(|p| {
                            profile_tool(p, normal, j.start_offset, j.end_offset, j.taper_angle_deg)
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                };
                self.profile_job(
                    j.operation,
                    j.require_single_solid,
                    tools,
                    &j.target_body_ids,
                    &j.result_body_ids,
                )
            }
            KernelJobDto::Rib(j) => {
                let normal = p3(&j.normal);
                let tools = j
                    .profiles
                    .iter()
                    .map(|p| profile_tool(p, normal, j.start_offset, j.end_offset, 0.0))
                    .collect::<Result<Vec<_>, _>>()?;
                self.profile_job(
                    j.operation,
                    false,
                    tools,
                    &j.target_body_ids,
                    &j.result_body_ids,
                )
            }
            KernelJobDto::Hole(j) => {
                let target = self.get(j.target_body_id, "hole target")?;
                let cutter = hole_cutter(j, &target)?;
                let result =
                    boolean(&target, &cutter, Op::Cut).map_err(|e| format!("Hole: {e}"))?;
                self.put(j.target_body_id.0, result);
                Ok(())
            }
            KernelJobDto::Transform(j) => {
                if j.source_body_ids.is_empty()
                    || j.transforms.is_empty()
                    || j.result_body_ids.len() != j.source_body_ids.len() * j.transforms.len()
                {
                    return Err("body transform buffers are malformed".into());
                }
                let mut results = Vec::with_capacity(j.result_body_ids.len());
                for transform in &j.transforms {
                    let x = xform(transform)?;
                    for source in &j.source_body_ids {
                        results.push(self.get(*source, "body transform source")?.transformed(&x));
                    }
                }
                for (id, body) in j.result_body_ids.iter().zip(results) {
                    self.put(id.0, body);
                }
                Ok(())
            }
            KernelJobDto::Combine(j) => {
                if j.tool_body_ids.is_empty() {
                    return Err("Combine needs a target and at least one tool body".into());
                }
                let mut result = (*self.get(j.target_body_id, "Combine target")?).clone();
                let (op, name) = match j.operation {
                    CombineOperation::Join => (Op::Union, "Join"),
                    CombineOperation::Cut => (Op::Cut, "Cut"),
                    CombineOperation::Intersect => (Op::Intersect, "Intersect"),
                };
                for tool in &j.tool_body_ids {
                    let tool = self.get(*tool, "Combine tool")?;
                    result =
                        boolean(&result, &tool, op).map_err(|e| format!("Combine {name}: {e}"))?;
                }
                self.put(j.target_body_id.0, result);
                if !j.keep_tools {
                    for tool in &j.tool_body_ids {
                        self.state.remove(&tool.0);
                    }
                }
                Ok(())
            }
            KernelJobDto::SplitBody(j) => {
                let target = self.get(j.target_body_id, "Split Body target")?;
                let normal = p3(&j.plane_normal);
                if normal.len2() < 1.0e-18 {
                    return Err("Split Body plane normal is degenerate".into());
                }
                let origin = p3(&j.plane_origin);
                let reach = target.bbox().diagonal() + origin.dist(target.bbox().lo) + 1.0;
                let half = |sign: f64| -> Result<Body, String> {
                    let space = half_space(origin, normal.unit() * sign, reach)?;
                    boolean(&target, &space, Op::Intersect)
                };
                let not_divided = "Split Body plane does not divide the target into two bodies";
                let positive = half(1.0).map_err(|_| not_divided.to_string())?;
                let negative = half(-1.0).map_err(|_| not_divided.to_string())?;
                self.put(j.target_body_id.0, positive);
                self.put(j.new_body_id.0, negative);
                Ok(())
            }
            KernelJobDto::ExternalThread(j) => match j.thread.representation {
                // A simplified thread is annotation only.
                HoleThreadRepresentation::Simplified => self
                    .get(j.target_body_id, "external thread target")
                    .map(|_| ()),
                HoleThreadRepresentation::Modeled => Err(unsupported("A modeled thread")),
            },
            KernelJobDto::Revolve(j) => {
                let (origin, axis) = (p3(&j.axis_origin), p3(&j.axis_direction));
                let tools = j
                    .profiles
                    .iter()
                    .map(|p| make::revolve_region(&region_of(p), origin, axis, j.angle_rad))
                    .collect::<Result<Vec<_>, _>>()?;
                self.profile_job(
                    j.operation,
                    j.require_single_solid,
                    tools,
                    &j.target_body_ids,
                    &j.result_body_ids,
                )
            }
            KernelJobDto::Fillet(j) => {
                let target = self.get(j.target_body_id, "edge refinement target")?;
                let edges = edge_indices(&j.edge_keys)?;
                let result =
                    blend::fillet(&target, &edges, j.radius).map_err(|e| format!("Fillet: {e}"))?;
                self.put(j.target_body_id.0, result);
                Ok(())
            }
            KernelJobDto::Chamfer(j) => {
                let target = self.get(j.target_body_id, "edge refinement target")?;
                let edges = edge_indices(&j.edge_keys)?;
                let result = blend::chamfer(&target, &edges, j.distance, j.angle_deg)
                    .map_err(|e| format!("Chamfer: {e}"))?;
                self.put(j.target_body_id.0, result);
                Ok(())
            }
            KernelJobDto::Sweep(_) => Err(unsupported("Sweep")),
            KernelJobDto::Loft(_) => Err(unsupported("Loft")),
            KernelJobDto::Shell(_) => Err(unsupported("Shell")),
            KernelJobDto::ImportStep(_) => Err(unsupported("STEP import")),
        }
    }

    fn profile_job(
        &mut self,
        operation: ExtrudeOperation,
        require_single_solid: bool,
        tools: Vec<Body>,
        targets: &[BodyId],
        results: &[BodyId],
    ) -> Result<(), String> {
        if tools.is_empty() {
            return Err("the feature has no profile".into());
        }
        if operation == ExtrudeOperation::NewBody {
            if results.len() != tools.len() {
                return Err("New Body output count does not match profiles".into());
            }
            for (id, tool) in results.iter().zip(tools) {
                self.put(id.0, tool);
            }
            return Ok(());
        }
        let fuse = |tools: Vec<Body>| -> Result<Body, String> {
            let mut iter = tools.into_iter();
            let mut merged = iter.next().unwrap();
            for tool in iter {
                merged = boolean(&merged, &tool, Op::Union)?;
            }
            Ok(merged)
        };
        if operation == ExtrudeOperation::Join && targets.is_empty() {
            if tools.len() < 2 || results.len() != 1 {
                return Err("Join Profiles needs multiple profiles and one output body".into());
            }
            let merged = fuse(tools)?;
            if require_single_solid && lump_count(&merged) != 1 {
                return Err(
                    "Merge profiles requires contacting regions that form one solid; \
                            separate regions and point-only contact cannot be merged"
                        .into(),
                );
            }
            self.put(results[0].0, merged);
            return Ok(());
        }
        if targets.is_empty() {
            return Err("boolean solid feature has no target body".into());
        }
        let tool = fuse(tools)?;
        let (op, name) = match operation {
            ExtrudeOperation::Join => (Op::Union, "Join"),
            ExtrudeOperation::Cut => (Op::Cut, "Cut"),
            ExtrudeOperation::Intersect => (Op::Intersect, "Intersect"),
            ExtrudeOperation::NewBody => unreachable!(),
        };
        let mut done = Vec::with_capacity(targets.len());
        for id in targets {
            let target = self.get(*id, "boolean target")?;
            done.push((
                id.0,
                boolean(&target, &tool, op).map_err(|e| format!("{name}: {e}"))?,
            ));
        }
        for (id, body) in done {
            self.put(id, body);
        }
        Ok(())
    }

    /// Extrusion tool from an existing planar face.
    fn face_tool(
        &self,
        source: &KernelPlanarFaceSourceDto,
        normal: V3,
        start: f64,
        end: f64,
        taper: f64,
    ) -> Result<Body, String> {
        if taper.abs() >= 1.0e-12 {
            return Err(unsupported("A tapered extrusion of a face"));
        }
        let body = self.get(source.body_id, "Extrude source")?;
        let matches = |f: u32| {
            let Some(props) = planar_props(&body, f) else {
                return false;
            };
            let s = &source.signature;
            let close = |a: f64, b: f64| (a - b).abs() <= a.abs().max(b.abs()).max(1.0) * 1.0e-6;
            let n = body.f_normal(f, props.centroid);
            close(props.area, s.area)
                && close(props.perimeter, s.perimeter)
                && props.loops == s.wire_count
                && props.centroid.dist(p3(&s.centroid)) <= 1.0e-6 * props.perimeter.max(1.0)
                && n.dot(p3(&s.normal).unit()) > 1.0 - 1.0e-9
        };
        // The saved ordinal is only a hint; the signature is the reference.
        let hinted = source
            .face_key
            .strip_prefix("face:")
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|f| (*f as usize) < body.face_count() && matches(*f));
        let face = match hinted {
            Some(f) => f,
            None => {
                let found: Vec<u32> = (0..body.face_count() as u32)
                    .filter(|f| matches(*f))
                    .collect();
                if found.len() != 1 {
                    return Err(
                        "the planar face used by this Extrude changed; reselect its source".into(),
                    );
                }
                found[0]
            }
        };
        let region = face_region(&body, face)?;
        make::extrude(&region, normal, start, end)
    }
}

/// Edge ordinals from topology keys of the form `edge:N`.
fn edge_indices(keys: &[String]) -> Result<Vec<u32>, String> {
    keys.iter()
        .map(|key| {
            key.strip_prefix("edge:")
                .and_then(|v| v.parse::<u32>().ok())
                .ok_or_else(|| {
                    format!(
                        "Invalid edge topology reference '{key}'; reselect the edge on the feature input body"
                    )
                })
        })
        .collect()
}

fn xform(t: &KernelTransformDto) -> Result<Xform, String> {
    match t {
        KernelTransformDto::Mirror { origin, normal } => {
            let n = p3(normal);
            if n.len2() < 1.0e-18 {
                return Err("Mirror plane normal is degenerate".into());
            }
            Ok(Xform::mirror(p3(origin), n))
        }
        KernelTransformDto::Translate { vector } => Ok(Xform::translation(p3(vector))),
        KernelTransformDto::Rotate {
            origin,
            axis,
            angle_rad,
        } => {
            let a = p3(axis);
            if a.len2() < 1.0e-18 {
                return Err("Circular Pattern axis is degenerate".into());
            }
            Ok(Xform::rotation(p3(origin), a, *angle_rad))
        }
        KernelTransformDto::Rigid {
            translation,
            rotation,
            pivot,
        } => Xform::rigid(p3(translation), *rotation, p3(pivot))
            .ok_or_else(|| "Move/Copy rotation is degenerate".to_string()),
    }
}

fn segs_of(profile: &KernelProfileDto) -> Vec<Seg> {
    if profile.curves.is_empty() {
        let n = profile.points.len();
        return (0..n)
            .map(|i| Seg::Line {
                a: p3(&profile.points[i]),
                b: p3(&profile.points[(i + 1) % n]),
            })
            .collect();
    }
    let mut segs = Vec::with_capacity(profile.curves.len());
    for curve in &profile.curves {
        match curve {
            KernelCurveDto::Line { start, end, .. } => segs.push(Seg::Line {
                a: p3(start),
                b: p3(end),
            }),
            KernelCurveDto::Arc {
                start, mid, end, ..
            } => segs.push(Seg::Arc {
                a: p3(start),
                m: p3(mid),
                b: p3(end),
            }),
            KernelCurveDto::Circle {
                center,
                axis_point,
                normal,
                ..
            } => segs.push(Seg::Circle {
                c: p3(center),
                p: p3(axis_point),
                n: p3(normal),
            }),
            KernelCurveDto::Polyline { points, .. } => {
                for w in points.windows(2) {
                    segs.push(Seg::Line {
                        a: p3(&w[0]),
                        b: p3(&w[1]),
                    });
                }
            }
        }
    }
    segs
}

fn region_of(profile: &KernelProfileDto) -> Region {
    let mut loops = vec![segs_of(profile)];
    loops.extend(profile.holes.iter().map(segs_of));
    Region { loops }
}

fn profile_tool(
    profile: &KernelProfileDto,
    normal: V3,
    start: f64,
    end: f64,
    taper: f64,
) -> Result<Body, String> {
    if taper.abs() >= 1.0e-12 {
        return Err(unsupported("A tapered extrusion"));
    }
    make::extrude(&region_of(profile), normal, start, end)
}

/// The boundary of a planar face as an extrusion region, outer loop first.
fn face_region(body: &Body, f: u32) -> Result<Region, String> {
    if !body.f_surface(f).is_plane() {
        return Err("Extrude source face is not planar".into());
    }
    let mesh = EdgeMesh::build(body, 0.05, 16.0);
    let trim = FaceTrim::build(body, &mesh, f, 0.05);
    let mut order: Vec<usize> = (0..trim.loops.len()).collect();
    order.sort_by(|&a, &b| trim.loops[b].area.total_cmp(&trim.loops[a].area));
    let loops: Vec<u32> = body.face_loops(f).collect();
    let mut region = Region::default();
    for i in order {
        let mut segs = Vec::new();
        for h in body.loop_hes(loops[i]) {
            let e = h >> 1;
            let c = body.e_curve_ref(e);
            let (t0, t1) = body.h_span(h);
            let (a, b) = (c.point(t0), c.point(t1));
            match c.kind {
                CurveKind::Line => segs.push(Seg::Line { a, b }),
                CurveKind::Circle if body.e_closed(e) => {
                    let n = if h & 1 == 0 { c.f.z } else { -c.f.z };
                    segs.push(Seg::Circle { c: c.f.o, p: a, n });
                }
                CurveKind::Circle => segs.push(Seg::Arc {
                    a,
                    m: c.point(0.5 * (t0 + t1)),
                    b,
                }),
                CurveKind::Ellipse | CurveKind::Trace => {
                    return Err(unsupported("Extruding a face with an elliptical edge"))
                }
            }
        }
        region.loops.push(segs);
    }
    Ok(region)
}

/// A block covering everything on the `normal` side of a plane within
/// `reach` of `origin`.
fn half_space(origin: V3, normal: V3, reach: f64) -> Result<Body, String> {
    let f = Frame::from_z(origin, normal);
    let corner = |sx: f64, sy: f64| origin + f.x * (sx * reach) + f.y * (sy * reach);
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
    make::extrude(&region, normal, 0.0, reach)
}

/// Cutter for a hole: one solid of revolution, built without Booleans.
fn hole_cutter(j: &KernelHoleJobDto, target: &Body) -> Result<Body, String> {
    if j.diameter.is_nan() || j.diameter <= 0.0 {
        return Err("hole parameters are malformed".into());
    }
    let direction = p3(&j.direction);
    if direction.len2() < 1.0e-18 {
        return Err("hole direction is degenerate".into());
    }
    let direction = direction.unit();
    let through = matches!(j.extent, HoleExtent::ThroughAll);
    let mut radius = 0.5 * j.diameter;
    if let Some(thread) = &j.thread {
        if thread.nominal_diameter <= j.diameter || thread.pitch <= 0.0 {
            return Err("thread parameters are malformed".into());
        }
        if thread.representation == HoleThreadRepresentation::Modeled {
            return Err(unsupported("A modeled thread"));
        }
    }
    // Start just outside the support face; a through cut also clears the far
    // side. The blind stop stays at the requested depth from the face.
    let overlap = 1.0e-4;
    let depth = match j.extent {
        HoleExtent::Distance { depth } => {
            if depth.is_nan() || depth <= 0.0 {
                return Err("hole parameters are malformed".into());
            }
            depth
        }
        HoleExtent::ThroughAll => target.bbox().diagonal() + 1.0 + overlap,
    };
    if !radius.is_finite() || !depth.is_finite() {
        return Err("hole parameters are malformed".into());
    }
    radius = radius.max(0.0);
    let mut profile: Vec<(f64, f64)> = vec![(0.0, -overlap)];
    match j.style {
        HoleStyle::Simple => profile.push((radius, -overlap)),
        HoleStyle::Counterbore => {
            let bore = 0.5 * j.counterbore_diameter;
            let deep = j.counterbore_depth;
            if bore.is_nan() || bore <= radius || deep.is_nan() || deep <= 0.0 {
                return Err("counterbore dimensions are invalid".into());
            }
            let bottom = if through {
                j.counterbore_depth + overlap
            } else {
                j.counterbore_depth
            };
            profile.extend([(bore, -overlap), (bore, bottom), (radius, bottom)]);
        }
        HoleStyle::Countersink => {
            let large = 0.5 * j.countersink_diameter;
            let half_angle = j.countersink_angle_deg.to_radians() * 0.5;
            let sink = (large - radius) / half_angle.tan();
            if !sink.is_finite() || sink <= 0.0 {
                return Err("countersink dimensions are invalid".into());
            }
            profile.extend([
                (large + overlap * half_angle.tan(), -overlap),
                (radius, sink),
            ]);
        }
    }
    profile.push((radius, depth));
    if !through && j.bottom_style == HoleBottomStyle::DrillPoint {
        let tip = radius / (j.drill_point_angle_deg.to_radians() * 0.5).tan();
        if !tip.is_finite() || tip <= 0.0 {
            return Err("drill point angle is invalid".into());
        }
        profile.push((0.0, depth + tip));
    } else {
        profile.push((0.0, depth));
    }
    make::lathe(p3(&j.center), direction, &profile)
}

/// Connected shells of a body as face lists.
pub fn shells(body: &Body) -> Vec<Vec<u32>> {
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
    let mut groups: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for f in 0..nf as u32 {
        groups.entry(find(&mut parent, f)).or_default().push(f);
    }
    groups.into_values().collect()
}

/// Signed volume enclosed by each shell (negative for a void).
fn shell_volumes(body: &Body, mesh: &Mesh, groups: &[Vec<u32>]) -> Vec<f64> {
    groups
        .iter()
        .map(|faces| {
            let mut six = 0.0;
            for &f in faces {
                let [first, count] = mesh.faces[f as usize];
                for t in mesh.indices[first as usize..(first + count) as usize]
                    .as_chunks::<3>()
                    .0
                {
                    let (a, b, c) = (
                        mesh.positions[t[0] as usize],
                        mesh.positions[t[1] as usize],
                        mesh.positions[t[2] as usize],
                    );
                    six += a.dot(b.cross(c));
                }
            }
            let _ = body;
            six / 6.0
        })
        .collect()
}

/// Number of separate solids in a body.
pub fn lump_count(body: &Body) -> usize {
    let mesh = tessellate(body, &TessOptions::default());
    let groups = shells(body);
    shell_volumes(body, &mesh, &groups)
        .iter()
        .filter(|v| **v > 0.0)
        .count()
}

fn topology_signature(body: &Body) -> String {
    let mut hash = 14_695_981_039_346_656_037_u64;
    let mut mix = |value: u64| {
        for byte in value.to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(1_099_511_628_211);
        }
    };
    mix(2);
    mix(body.verts.len() as u64);
    mix(body.edge_count() as u64);
    mix(body.face_count() as u64);
    for e in 0..body.edge_count() {
        mix(body.e_curve_ref(e as u32).kind as u64);
        mix(u64::from(body.e_v[e][0]));
        mix(u64::from(body.e_v[e][1]));
    }
    for f in 0..body.face_count() as u32 {
        mix(body.f_surface(f).kind as u64);
        mix(u64::from(body.f_sense[f as usize]));
        for l in body.face_loops(f) {
            mix(0xf1);
            for h in body.loop_hes(l) {
                mix(u64::from(h));
            }
            mix(0xf2);
        }
        mix(0xf3);
    }
    format!("connectivity-v1:{hash}")
}

/// Display data and exact metadata of one body.
pub fn body_dto(id: u64, body: &Body, options: &TessOptions) -> KernelBodyDto {
    let mesh = tessellate(body, options);
    let mut positions = Vec::with_capacity(mesh.positions.len() * 3);
    let mut normals = Vec::with_capacity(mesh.normals.len() * 3);
    for p in &mesh.positions {
        positions.extend([p.x as f32, p.y as f32, p.z as f32]);
    }
    for n in &mesh.normals {
        normals.extend([n.x as f32, n.y as f32, n.z as f32]);
    }

    // Exterior evidence: only for a single solid, whose one outward shell is
    // unambiguous.
    let groups = shells(body);
    let volumes = shell_volumes(body, &mesh, &groups);
    let mut outer: Vec<Option<bool>> = vec![None; body.face_count()];
    if volumes.iter().filter(|v| **v > 0.0).count() == 1 {
        for (faces, volume) in groups.iter().zip(&volumes) {
            for &f in faces {
                outer[f as usize] = Some(*volume > 0.0);
            }
        }
    }

    let mut faces = Vec::with_capacity(body.face_count());
    for f in 0..body.face_count() as u32 {
        let s = body.f_surface(f);
        let sense = body.f_sense[f as usize];
        let mut edge_keys: Vec<String> = Vec::new();
        let mut seen: Vec<u32> = Vec::new();
        let mut rings: Vec<String> = Vec::new();
        let loop_count = body.face_loops(f).len();
        for l in body.face_loops(f) {
            let hes: Vec<u32> = body.loop_hes(l).collect();
            for &h in &hes {
                let e = h >> 1;
                if !seen.contains(&e) {
                    seen.push(e);
                    edge_keys.push(format!("edge:{e}"));
                }
            }
            // A full circle that is not the only loop is the mouth of a hole.
            if s.is_plane()
                && loop_count > 1
                && outer[f as usize] == Some(true)
                && hes.len() == 1
                && body.e_closed(hes[0] >> 1)
                && body.e_curve_ref(hes[0] >> 1).kind == CurveKind::Circle
            {
                rings.push(format!("edge:{}", hes[0] >> 1));
            }
        }
        // The outer boundary of a disc-shaped face is not a hole mouth.
        if rings.len() == loop_count {
            rings.clear();
        }
        let plane = s.is_plane().then(|| {
            let normal = if sense { s.f.z } else { -s.f.z };
            PlaneBasis {
                origin: s.f.o.arr(),
                u: s.f.x.arr(),
                v: normal.cross(s.f.x).arr(),
                normal: normal.arr(),
            }
        });
        let signature = planar_props(body, f).map(|p| PlanarFaceSignatureDto {
            centroid: dto(p.centroid),
            normal: dto(if sense { s.f.z } else { -s.f.z }),
            area: p.area,
            perimeter: p.perimeter,
            wire_count: p.loops,
            edge_count: p.edges,
        });
        let [first_index, index_count] = mesh.faces[f as usize];
        faces.push(KernelFaceDto {
            key: format!("face:{f}"),
            first_index,
            index_count,
            plane,
            signature,
            cylinder: (s.kind == SurfKind::Cylinder).then(|| CylindricalSurfaceDto {
                origin: dto(s.f.o),
                axis: dto(s.f.z),
                reference: dto(s.f.x),
                radius: s.r,
            }),
            edge_keys,
            linear_seam_edge_keys: Vec::new(),
            planar_inner_ring_edge_keys: rings,
            outer_shell: outer[f as usize],
            cone: (s.kind == SurfKind::Cone).then(|| ConicalSurfaceDto {
                axis: dto(s.f.z),
                semi_angle: s.s,
            }),
        });
    }

    let mut edges = Vec::with_capacity(body.edge_count());
    for e in 0..body.edge_count() as u32 {
        let c = body.e_curve_ref(e);
        let points = edge_polyline(body, e, 0.01).into_iter().map(dto).collect();
        let [t0, t1] = body.e_t[e as usize];
        let refinable = body.e_sharp(e);
        edges.push(KernelEdgeDto {
            key: format!("edge:{e}"),
            points,
            circle: (c.kind == CurveKind::Circle).then(|| CircularCurveDto {
                center: dto(c.f.o),
                normal: dto(c.f.z),
                reference: dto(c.f.x),
                radius: c.r,
                closed: (t1 - t0 - TAU).abs() <= 1.0e-7,
            }),
            refinable,
        });
    }

    KernelBodyDto {
        body_id: BodyId(id),
        topology_signature: topology_signature(body),
        display_warnings: Vec::new(),
        positions,
        normals,
        indices: mesh.indices,
        faces,
        edges,
    }
}
