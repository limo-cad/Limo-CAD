//! The kernel behind Limo CAD's host-neutral solid contract: a recompute
//! plan goes in, an evaluated scene comes out.
//!
//! This crate is the one place where the kernel's types meet the
//! application's. The kernel (`limo-cad-kernel`) depends on nothing and
//! knows nothing of jobs, features or scenes; whatever does is here, and
//! it reaches the kernel only through what the kernel makes public.
//!
//! Bodies are immutable values, so the session keeps the state after every
//! job of the previous plan. A new plan resumes from the longest unchanged
//! prefix: editing feature `k` replays from `k`, not from the start, and an
//! untouched body keeps its geometry identity and its cached display mesh.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use limo_cad_core::{BodyId, FeatureId, PlaneBasis};
use limo_cad_solid::{iso_metric_thread_envelope, rounded_thread_diameters, ThreadFit};
use limo_cad_solid::{
    stable_face_id, CircularCurveDto, CombineOperation, ConicalSurfaceDto, CylindricalSurfaceDto,
    ExtrudeOperation, HistorySupportQuery, HoleBottomStyle, HoleExtent, HoleStyle, HoleThreadDto,
    HoleThreadHand, HoleThreadRepresentation, KernelBodyDto, KernelCurveDto, KernelEdgeDto,
    KernelFaceDto, KernelFeatureErrorDto, KernelHoleJobDto, KernelJobDto,
    KernelPlanarFaceSourceDto, KernelProfileDto, KernelSceneDto, KernelTransformDto,
    PlanarFaceSignatureDto, Point3Dto, RecomputePlanDto, SweepOrientation, SweepTransition,
};
use limo_cad_solid::{DisplayMeshWarningDto, EdgeMarkDto, FaceMarkDto, FeatureKeysDto};

use limo_cad_kernel::boolean::{boolean, Op, NO_OVERLAP};
use limo_cad_kernel::geom::{CurveKind, SurfKind};
use limo_cad_kernel::make::{self, Region, Seg};
use limo_cad_kernel::math::*;
use limo_cad_kernel::name::{mix, tag_of, Names, Role};
use limo_cad_kernel::props::{self, planar_props};
use limo_cad_kernel::tess::{edge_polyline, tessellate, Mesh, TessOptions};
use limo_cad_kernel::topo::Body;
use limo_cad_kernel::trim::{classify, edge_closest, EdgeMesh, FaceTrim, PtClass};
use limo_cad_kernel::{blend, loft, offset, step, thread};

/// The kernel this session runs on, for whoever handles its bodies: the
/// same revision, without naming it a second time.
pub use limo_cad_kernel as kernel;

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
    /// What each job of `history` made of the edges or faces it was given,
    /// for the jobs that are given any: see [`Kernel::feature_keys`].
    reports: Vec<Option<FeatureKeysDto>>,
    /// The report of the job being applied.
    report: Option<FeatureKeysDto>,
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
            reports: Vec::new(),
            report: None,
        }
    }

    pub fn reset(&mut self) {
        self.state.clear();
        self.history.clear();
        self.reports.clear();
        self.display.clear();
    }

    /// A kernel that holds the given bodies as they are now and nothing of
    /// how they came about; all of them when no ids are given. For work
    /// that only reads: drawings, sections, export.
    pub fn snapshot(&self, ids: &[u64]) -> Kernel {
        let mut copy = Kernel::new();
        copy.options = self.options;
        copy.clock = self.clock;
        copy.state = self
            .state
            .iter()
            .filter(|(id, _)| ids.is_empty() || ids.contains(id))
            .map(|(id, slot)| (*id, slot.clone()))
            .collect();
        copy
    }

    pub fn body_ids(&self) -> Vec<u64> {
        self.state.keys().copied().collect()
    }

    pub fn body(&self, id: u64) -> Option<&Body> {
        self.state.get(&id).map(|slot| &*slot.body)
    }

    /// The names that the edges `keys` refer to have on body `id` as it
    /// is now. Names follow the faces an edge lies between, so an edge
    /// that an earlier feature did not make is found without that feature;
    /// one that it did make is gone, and says so.
    pub fn edge_names(&self, id: u64, keys: &[String]) -> Result<Vec<String>, String> {
        let body = self.body(id).ok_or("the body is missing")?;
        let names = Names::of(body);
        let mut found: Vec<String> = Vec::new();
        for e in edge_indices(&names, keys)? {
            let name = names.edges[e as usize].clone();
            if !found.contains(&name) {
                found.push(name);
            }
        }
        Ok(found)
    }

    /// What the last recompute made of the edges of each fillet and chamfer,
    /// and of the faces of each shell and external thread: every one of
    /// them by its name here and by its mark. A feature whose keys were
    /// written by the reference kernel was found by its marks, and this
    /// says what its edges or faces are called here.
    pub fn feature_keys(&self) -> Vec<FeatureKeysDto> {
        self.reports.iter().flatten().cloned().collect()
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
            && self
                .history
                .iter()
                .zip(&plan.jobs)
                .all(|(h, j)| h.0.same_work(j, follows))
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
            .take_while(|(h, j)| h.0.same_work(j, follows))
            .count();
        self.history.truncate(reused);
        self.reports.truncate(reused);
        self.state = self
            .history
            .last()
            .map(|(_, state)| state.clone())
            .unwrap_or_default();
        self.last_applied_jobs = 0;
        let mut errors = plan.errors.clone();
        for job in &plan.jobs[reused..] {
            self.last_applied_jobs += 1;
            self.report = None;
            match self.apply(job) {
                Ok(()) => {
                    self.history.push((job.clone(), self.state.clone()));
                    self.reports.push(self.report.take());
                }
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
                    let names = Names::of(&slot.body);
                    (0..slot.body.face_count() as u32).any(|f| {
                        // By name, or by the index older documents kept.
                        slot.body.f_surface(f).is_plane()
                            && (stable_face_id(BodyId(*id), &names.faces[f as usize])
                                == query.face_id
                                || stable_face_id(BodyId(*id), &format!("face:{f}"))
                                    == query.face_id)
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
        // `LIMO_CAD_KERNEL_BODIES`: leave every body of the scene in that
        // folder as text, to be read back with `Body::from_text`.
        if let Some(dir) = std::env::var_os("LIMO_CAD_KERNEL_BODIES") {
            for (id, slot) in &self.state {
                let path = std::path::Path::new(&dir).join(format!("{id}.body"));
                let _ = std::fs::write(path, slot.body.to_text());
            }
        }
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
        // Every face this job makes is tagged with the feature it belongs
        // to and its part in it, so that it can be found again by name
        // however the faces around it are renumbered.
        let feature = mix(0xfea7, job.feature_id().0);
        match job {
            KernelJobDto::Extrude(j) => {
                let normal = p3(&j.normal);
                let tools = match &j.source_face {
                    Some(source) => vec![self.face_tool(
                        feature,
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
                            let tool = profile_tool(
                                p,
                                normal,
                                j.start_offset,
                                j.end_offset,
                                j.taper_angle_deg,
                            )?;
                            Ok(stamped(tool, feature, p))
                        })
                        .collect::<Result<Vec<_>, String>>()?,
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
                    .map(|p| {
                        let tool = profile_tool(p, normal, j.start_offset, j.end_offset, 0.0)?;
                        Ok(stamped(tool, feature, p))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
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
                let modeled = j
                    .thread
                    .as_ref()
                    .filter(|t| t.representation == HoleThreadRepresentation::Modeled);
                let form = match modeled {
                    Some(thread) => Some(thread_form(thread, ThreadFit::Internal, j.diameter)?),
                    None => None,
                };
                let (mut cutter, depth) = hole_cutter(j, &target, form.as_ref())?;
                // Named by their place down the hole: mouth, wall, floor.
                cutter.retag(|tag| mix(feature, tag));
                let cut = |body: &Body, tool: &Body| {
                    boolean(body, tool, Op::Cut).map_err(|e| format!("Hole: {e}"))
                };
                let result = match (modeled, form) {
                    (Some(thread), Some(form)) => {
                        let along = p3(&j.direction).unit();
                        let start = p3(&j.center);
                        let through = matches!(j.extent, HoleExtent::ThroughAll);
                        // As far as the body goes that way, or the hole.
                        let available = if through {
                            let bounds = props::bounds(&target);
                            let mut far: f64 = 0.0;
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
                                far = far.max((p - start).dot(along));
                            }
                            if !(far.is_finite() && far > 0.0) {
                                return Err("threaded-hole target depth is degenerate".into());
                            }
                            far
                        } else {
                            depth
                        };
                        let wanted = thread.depth.filter(|d| *d > 0.0);
                        let run = thread::Run {
                            start,
                            along,
                            zero: make::reference_x(along),
                            pitch: thread.pitch,
                            depth: wanted.map_or(available, |d| d.min(available)),
                            left_hand: thread.hand == HoleThreadHand::Left,
                        };
                        let mut groove = thread::cutter(&form.internal()?, &run)?;
                        if !through || wanted.is_some_and(|d| d < available - 1.0e-7) {
                            groove = thread::clipped(&groove, &run, 0.5 * form.major, false)?;
                        }
                        groove.retag(|tag| mix(mix(feature, THREAD), tag));
                        if form.rounded.is_some() {
                            // The rounds of this form run into the bore
                            // without a corner: the bore comes first.
                            cut(&cut(&target, &cutter)?, &groove)?
                        } else {
                            cut(&cut(&target, &groove)?, &cutter)?
                        }
                    }
                    _ => cut(&target, &cutter)?,
                };
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
                for (nth, transform) in j.transforms.iter().enumerate() {
                    let x = xform(transform)?;
                    for source in &j.source_body_ids {
                        let mut copy = self.get(*source, "body transform source")?.transformed(&x);
                        // A body moved in place keeps its faces' names. A
                        // copy gets names of its own, so that copies joined
                        // into one body later can still be told apart.
                        let moved = j.result_body_ids.get(results.len()) == Some(source);
                        if !moved {
                            let instance = mix(feature, nth as u64);
                            copy.retag(|tag| mix(instance, tag));
                        }
                        results.push(copy);
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
                    let mut space = half_space(origin, normal.unit() * sign, reach)?;
                    space.retag(|tag| mix(feature, tag));
                    boolean(&target, &space, Op::Intersect)
                };
                let not_divided = "Split Body plane does not divide the target into two bodies";
                let positive = half(1.0).map_err(|_| not_divided.to_string())?;
                let negative = half(-1.0).map_err(|_| not_divided.to_string())?;
                self.put(j.target_body_id.0, positive);
                self.put(j.new_body_id.0, negative);
                Ok(())
            }
            KernelJobDto::ExternalThread(j) => {
                let target = self.get(j.target_body_id, "external thread target")?;
                if j.thread.nominal_diameter <= 0.0
                    || j.thread.pitch <= 0.0
                    || j.thread.depth.is_some_and(|d| d < 0.0)
                {
                    return Err("external thread parameters are malformed".into());
                }
                let names = Names::of(&target);
                let (found, report) = feature_faces(
                    &target,
                    &names,
                    std::slice::from_ref(&j.face_key),
                    j.face_mark.as_slice(),
                    j.feature_id,
                    "referenced external-thread cylinder no longer exists",
                )?;
                // One cylinder, one face. Where the other kernel had one
                // face and this one has several, which of them the thread
                // was meant for is not something to guess.
                let [f] = found[..] else {
                    return Err(
                        "the cylinder of this External Thread is more than one face on this kernel; reselect the face on the feature input body"
                            .into(),
                    );
                };
                self.report = Some(report);
                let surface = *target.f_surface(f);
                if surface.kind != SurfKind::Cylinder {
                    return Err("External Thread requires a cylindrical face".into());
                }
                let across = 2.0 * surface.r;
                if (across - j.thread.nominal_diameter).abs()
                    > (0.002 * j.thread.nominal_diameter).max(0.01)
                {
                    return Err("selected cylinder does not match the thread major diameter".into());
                }
                // All the way round: the face lies between two loops that
                // each go once about the axis, whatever else they do on the
                // way (a rim with a notch in it still goes round).
                let around = |l: u32| {
                    let mut turned = 0.0;
                    let mut last: Option<f64> = None;
                    for h in target.loop_hes(l) {
                        let (t0, t1) = target.h_span(h);
                        for k in 0..=8 {
                            let p = target.e_point(h >> 1, t0 + (t1 - t0) * k as f64 / 8.0);
                            let local = surface.f.local(p);
                            let angle = local.y.atan2(local.x);
                            if let Some(before) = last {
                                let step = angle - before;
                                turned += step - TAU * (step / TAU).round();
                            }
                            last = Some(angle);
                        }
                    }
                    (turned / TAU).round() != 0.0
                };
                let whole = target.face_loops(f).filter(|l| around(*l)).count() >= 2;
                if !whole {
                    return Err(
                        "External Thread requires a complete 360-degree cylindrical face".into(),
                    );
                }
                if !target.f_sense[f as usize] {
                    return Err(
                        "External Thread requires an outward-facing cylindrical surface".into(),
                    );
                }
                // How far along its axis the face runs.
                let (mut lower, mut upper) = (f64::INFINITY, f64::NEG_INFINITY);
                for h in target.face_hes(f) {
                    let e = h >> 1;
                    let [t0, t1] = target.e_t[e as usize];
                    for k in 0..=8 {
                        let p = target.e_point(e, t0 + (t1 - t0) * k as f64 / 8.0);
                        let height = surface.f.local(p).z;
                        lower = lower.min(height);
                        upper = upper.max(height);
                    }
                }
                let available = upper - lower;
                if !(available.is_finite() && available > 1.0e-7) {
                    return Err("external thread cylinder has no axial length".into());
                }
                let wanted = j.thread.depth.filter(|d| *d > 0.0).unwrap_or(available);
                if wanted > available + 1.0e-6 {
                    return Err(
                        "external thread length exceeds the selected cylindrical face".into(),
                    );
                }
                if j.thread.representation == HoleThreadRepresentation::Simplified {
                    // A simplified thread is annotation only.
                    return Ok(());
                }
                let form = thread_form(&j.thread, ThreadFit::External, 0.0)?;
                let (along, from) = if j.flip {
                    (-surface.f.z, upper)
                } else {
                    (surface.f.z, lower)
                };
                let run = thread::Run {
                    start: surface.f.o + surface.f.z * from,
                    along,
                    zero: surface.f.x,
                    pitch: j.thread.pitch,
                    depth: wanted,
                    left_hand: j.thread.hand == HoleThreadHand::Left,
                };
                let mut result = (*target).clone();
                // A tolerance class that keeps the crest below the face it
                // is cut in: turn the face down to it first.
                if across - form.major > 1.0e-7 {
                    let over = (1.0e-4 * j.thread.pitch).max(1.0e-4);
                    let mut sleeve = make::lathe(
                        run.start - along * over,
                        along,
                        &[
                            (0.5 * form.major, 0.0),
                            (0.5 * across + over, 0.0),
                            (0.5 * across + over, wanted + 2.0 * over),
                            (0.5 * form.major, wanted + 2.0 * over),
                        ],
                    )?;
                    sleeve.retag(|tag| mix(mix(feature, CREST), tag));
                    result = boolean(&result, &sleeve, Op::Cut)
                        .map_err(|e| format!("External Thread: {e}"))?;
                }
                let mut groove = thread::cutter(&form.external()?, &run)?;
                groove = thread::clipped(&groove, &run, 0.5 * form.major, true)?;
                groove.retag(|tag| mix(mix(feature, THREAD), tag));
                let before = props::volume(&result);
                let result = boolean(&result, &groove, Op::Cut)
                    .map_err(|e| format!("External Thread: {e}"))?;
                let removed = before - props::volume(&result);
                if !(removed.is_finite() && removed > (1.0e-8 * before.abs()).max(1.0e-8)) {
                    return Err("modeled external thread did not remove material".into());
                }
                self.put(j.target_body_id.0, result);
                Ok(())
            }
            KernelJobDto::Revolve(j) => {
                let (origin, axis) = (p3(&j.axis_origin), p3(&j.axis_direction));
                let tools = j
                    .profiles
                    .iter()
                    .map(|p| {
                        let tool = make::revolve_region(&region_of(p), origin, axis, j.angle_rad)?;
                        Ok(stamped(tool, feature, p))
                    })
                    .collect::<Result<Vec<_>, String>>()?;
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
                let names = Names::of(&target);
                let (edges, report) =
                    finish_edges(&target, &names, &j.edge_keys, &j.edge_marks, j.feature_id)?;
                self.report = Some(report);
                // A round is named after the edge it replaces.
                let tag = |e: u32| mix(feature, tag_of(&names.edges[e as usize]));
                let result = blend::fillet_as(&target, &edges, j.radius, &tag)
                    .map_err(|e| format!("Fillet: {e}"))?;
                self.put(j.target_body_id.0, result);
                Ok(())
            }
            KernelJobDto::Chamfer(j) => {
                let target = self.get(j.target_body_id, "edge refinement target")?;
                let names = Names::of(&target);
                let (edges, report) =
                    finish_edges(&target, &names, &j.edge_keys, &j.edge_marks, j.feature_id)?;
                self.report = Some(report);
                let tag = |e: u32| mix(feature, tag_of(&names.edges[e as usize]));
                let result = blend::chamfer_as(&target, &edges, j.distance, j.angle_deg, &tag)
                    .map_err(|e| format!("Chamfer: {e}"))?;
                self.put(j.target_body_id.0, result);
                Ok(())
            }
            KernelJobDto::Sweep(j) => {
                if !j.guide_rail.is_empty() {
                    let tool = guided_tool(&j.profile, &j.path, &j.guide_rail, feature)?;
                    return self.profile_job(
                        j.operation,
                        false,
                        vec![tool],
                        &j.target_body_ids,
                        &j.result_body_ids,
                    );
                }
                let tool = sweep_tool(
                    &j.profile,
                    &j.path,
                    j.orientation == SweepOrientation::Fixed,
                    j.transition,
                    feature,
                )?;
                self.profile_job(
                    j.operation,
                    false,
                    vec![tool],
                    &j.target_body_ids,
                    &j.result_body_ids,
                )
            }
            KernelJobDto::Loft(j) => {
                if j.sections.len() < 2 {
                    return Err("Loft needs at least two sections".into());
                }
                // What steers the loft from one section to the next, if
                // anything but the sections themselves does.
                let steered = !j.centerline.is_empty() || !j.guide_rail.is_empty();
                let trail = |curves: &[KernelCurveDto], what: &str| -> Result<Vec<Seg>, String> {
                    let mut segs = Vec::new();
                    for curve in curves {
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
                            KernelCurveDto::Polyline { points, .. } => {
                                segs.extend(points.windows(2).map(|w| Seg::Line {
                                    a: p3(&w[0]),
                                    b: p3(&w[1]),
                                }))
                            }
                            KernelCurveDto::Circle { .. } => {
                                return Err(unsupported(&format!(
                                    "A full circle as the {what} of a loft"
                                )))
                            }
                        }
                    }
                    Ok(segs)
                };
                let centerline = trail(&j.centerline, "centerline")?;
                let guide_rail = trail(&j.guide_rail, "guide rail")?;
                // Two sections are joined by straight lines either way.
                let ruled = j.ruled || j.sections.len() == 2;
                let holes = j.sections[0].holes.len();
                if j.sections.iter().any(|s| s.holes.len() != holes) {
                    return Err(
                        "Loft sections must contain the same number of profile holes".into(),
                    );
                }
                let through = |pick: &dyn Fn(&KernelProfileDto) -> &KernelProfileDto,
                               part: u64|
                 -> Result<Body, String> {
                    let outlines: Vec<Vec<Seg>> =
                        j.sections.iter().map(|s| segs_of(pick(s))).collect();
                    // Flat sides and cones where the sections allow them;
                    // a free-form skin where they do not, or where the
                    // skin is to run smoothly through every section.
                    let exact = if ruled && !steered {
                        make::loft(&outlines).ok()
                    } else {
                        None
                    };
                    // The rail steers the outline it was drawn beside; a
                    // hole follows the centerline the outline follows.
                    let mut tool = match exact {
                        Some(tool) => tool,
                        None if steered => loft::along(
                            &outlines,
                            &centerline,
                            if part == 0 { &guide_rail } else { &[] },
                        )?,
                        None => loft::skin(&outlines, ruled)?,
                    };
                    // A side is named after the sketch line under it, the
                    // stretch of the loft it belongs to and, where an arc
                    // had to be cut, which piece of it this is.
                    tool.retag(|tag| match Role::of(tag) {
                        Role::Side { lp, seg } => {
                            let band = (lp % loft::BANDS) as usize;
                            let entity = entities(pick(&j.sections[band]))
                                .get(seg as usize)
                                .copied()
                                .unwrap_or(u64::from(seg));
                            mix(mix(feature, SIDE), mix(mix(part, u64::from(lp)), entity))
                        }
                        Role::Start => mix(mix(feature, START), part),
                        Role::End => mix(mix(feature, END), part),
                        Role::Other => mix(feature, tag),
                    });
                    Ok(tool)
                };
                let mut tool = through(&|s| s, 0)?;
                for hole in 0..holes {
                    let cutter = through(&|s| &s.holes[hole], hole as u64 + 1)?;
                    tool = boolean(&tool, &cutter, Op::Cut).map_err(|e| format!("Loft: {e}"))?;
                }
                self.profile_job(
                    j.operation,
                    false,
                    vec![tool],
                    &j.target_body_ids,
                    &j.result_body_ids,
                )
            }
            KernelJobDto::Shell(j) => {
                let target = self.get(j.target_body_id, "Shell target")?;
                let names = Names::of(&target);
                let (open, report) = feature_faces(
                    &target,
                    &names,
                    &j.face_keys,
                    &j.face_marks,
                    j.feature_id,
                    "referenced Shell face no longer exists",
                )?;
                self.report = Some(report);
                let result = offset::shell_as(&target, &open, j.thickness, j.inward, feature)
                    .map_err(|e| format!("Shell: {e}"))?;
                self.put(j.target_body_id.0, result);
                Ok(())
            }
            KernelJobDto::ImportStep(j) => {
                let bytes = step::from_base64(&j.data_base64)?;
                let text = String::from_utf8_lossy(&bytes);
                let mut whole = Body::new();
                for (nth, mut solid) in step::read(&text)?.into_iter().enumerate() {
                    solid.retag(|tag| mix(mix(feature, nth as u64), tag));
                    whole.append(&solid);
                }
                self.put(j.result_body_id.0, whole);
                Ok(())
            }
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
        // One target after another, as the reference kernel does: a target
        // that fails leaves the ones before it changed, and the host tells
        // from that that the geometry is no longer what it published.
        for id in targets {
            let target = self.get(*id, "boolean target")?;
            let body = boolean(&target, &tool, op).map_err(|e| format!("{name}: {e}"))?;
            self.put(id.0, body);
        }
        Ok(())
    }

    /// Extrusion tool from an existing planar face.
    fn face_tool(
        &self,
        feature: u64,
        source: &KernelPlanarFaceSourceDto,
        normal: V3,
        start: f64,
        end: f64,
        taper: f64,
    ) -> Result<Body, String> {
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
        // The saved name is only a hint; the signature is the reference.
        let names = Names::of(&body);
        let hinted = names.face(&source.face_key).filter(|f| matches(*f));
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
        let (region, edges) = face_region(&body, face)?;
        let mut tool = if taper.abs() < 1.0e-12 {
            make::extrude(&region, normal, start, end)?
        } else {
            // Scaled about the middle of the face, by its corners' mean
            // distance from there, as the reference adapter does.
            let centre = planar_props(&body, face)
                .ok_or("Extrude source face is not planar")?
                .centroid;
            let corners: Vec<V3> = body
                .face_hes(face)
                .map(|h| body.verts[body.h_origin(h) as usize])
                .collect();
            make::taper(
                &region,
                normal,
                start,
                end,
                centre,
                taper_rate(taper, centre, &corners),
            )?
        };
        // Each side is named after the edge of the face it rises from, the
        // two ends after the face itself.
        let from_face = tag_of(&names.faces[face as usize]);
        tool.retag(|tag| match Role::of(tag) {
            Role::Side { lp, seg } => {
                let e = edges[lp as usize][seg as usize];
                mix(mix(feature, SIDE), tag_of(&names.edges[e as usize]))
            }
            Role::Start => mix(mix(feature, START), from_face),
            Role::End => mix(mix(feature, END), from_face),
            Role::Other => mix(feature, tag),
        });
        Ok(tool)
    }
}

/// Parts a face can play in the feature that made it.
const START: u64 = 1;
const END: u64 = 2;
const SIDE: u64 = 3;

/// The sketch entity behind every segment of a profile loop, in the order
/// `segs_of` lists the segments.
fn entities(p: &KernelProfileDto) -> Vec<u64> {
    if p.curves.is_empty() {
        return (0..p.points.len() as u64)
            .map(|i| mix(0x90117, i))
            .collect();
    }
    let mut out = Vec::with_capacity(p.curves.len());
    for curve in &p.curves {
        match curve {
            KernelCurveDto::Line { entity_id, .. }
            | KernelCurveDto::Arc { entity_id, .. }
            | KernelCurveDto::Circle { entity_id, .. } => out.push(*entity_id),
            KernelCurveDto::Polyline { entity_id, points } => {
                out.extend((1..points.len() as u64).map(|piece| mix(*entity_id, piece)))
            }
        }
    }
    out
}

/// A piece of a sweep path, in the order and direction it is travelled.
#[derive(Clone, Copy)]
enum Leg {
    Line(V3, V3),
    Arc(V3, V3, V3),
    Ring(V3, V3),
}

impl Leg {
    /// Where it starts and the way it is heading there, and the same at
    /// its end.
    fn ends(&self) -> Option<[(V3, V3); 2]> {
        match *self {
            Leg::Line(a, b) => {
                let d = (b - a).unit();
                Some([(a, d), (b, d)])
            }
            Leg::Arc(a, m, b) => {
                let (centre, _, axis) = make::arc_circle(a, m, b)?;
                Some([
                    (a, axis.cross(a - centre).unit()),
                    (b, axis.cross(b - centre).unit()),
                ])
            }
            Leg::Ring(..) => None,
        }
    }
}

fn moved(region: &Region, by: &Xform) -> Region {
    Region {
        loops: region
            .loops
            .iter()
            .map(|segs| {
                segs.iter()
                    .map(|seg| match *seg {
                        Seg::Line { a, b } => Seg::Line {
                            a: by.point(a),
                            b: by.point(b),
                        },
                        Seg::Arc { a, m, b } => Seg::Arc {
                            a: by.point(a),
                            m: by.point(m),
                            b: by.point(b),
                        },
                        Seg::Circle { c, p, n } => Seg::Circle {
                            c: by.point(c),
                            p: by.point(p),
                            n: by.vec(n),
                        },
                    })
                    .collect()
            })
            .collect(),
    }
}

/// The path of a sweep as legs in the order they are travelled: from the
/// end that is nearer the plane of the profile.
fn sweep_legs(profile: &KernelProfileDto, path: &[KernelCurveDto]) -> Result<Vec<Leg>, String> {
    let mut legs: Vec<Leg> = Vec::new();
    for curve in path {
        match curve {
            KernelCurveDto::Line { start, end, .. } => legs.push(Leg::Line(p3(start), p3(end))),
            KernelCurveDto::Arc {
                start, mid, end, ..
            } => legs.push(Leg::Arc(p3(start), p3(mid), p3(end))),
            KernelCurveDto::Circle { center, normal, .. } => {
                legs.push(Leg::Ring(p3(center), p3(normal)))
            }
            KernelCurveDto::Polyline { points, .. } => {
                legs.extend(points.windows(2).map(|w| Leg::Line(p3(&w[0]), p3(&w[1]))))
            }
        }
    }
    if legs.is_empty() {
        return Err("Sweep needs a path".into());
    }
    let (middle, facing) = profile_plane(profile)?;
    let height = |p: V3| (p - middle).dot(facing).abs();
    let ends = |leg: &Leg| match *leg {
        Leg::Line(a, b) | Leg::Arc(a, _, b) => Some((a, b)),
        Leg::Ring(..) => None,
    };
    if let (Some((first, _)), Some((_, last))) = (ends(&legs[0]), ends(&legs[legs.len() - 1])) {
        if height(last) < height(first) {
            legs.reverse();
            for leg in &mut legs {
                match leg {
                    Leg::Line(a, b) | Leg::Arc(a, _, b) => std::mem::swap(a, b),
                    Leg::Ring(..) => {}
                }
            }
        }
    }
    Ok(legs)
}

/// A point in the plane of a profile and a unit normal to it.
fn profile_plane(profile: &KernelProfileDto) -> Result<(V3, V3), String> {
    let points: Vec<V3> = profile.points.iter().map(p3).collect();
    if points.len() < 3 {
        return Err("Sweep needs the outline of its profile".into());
    }
    let middle = points.iter().fold(V3::ZERO, |sum, p| sum + *p) / points.len() as f64;
    let mut around = V3::ZERO;
    for i in 0..points.len() {
        around += (points[i] - middle).cross(points[(i + 1) % points.len()] - middle);
    }
    Ok((middle, around.unit()))
}

/// A profile carried along a path of lines and arcs.
///
/// Kept square to the path, it makes an extrusion along each line and a
/// part turn about the middle of each arc. Where the path turns a corner,
/// `transition` says what fills it: the two legs run on until they meet in
/// a sharp corner, or the profile is swung round the corner point. Kept
/// facing one way instead (`fixed`), every point of the profile follows a
/// copy of the path, corners and all.
fn sweep_tool(
    profile: &KernelProfileDto,
    path: &[KernelCurveDto],
    fixed: bool,
    transition: SweepTransition,
    feature: u64,
) -> Result<Body, String> {
    let legs = sweep_legs(profile, path)?;
    let entity = {
        let mut loops = vec![entities(profile)];
        loops.extend(profile.holes.iter().map(entities));
        loops
    };
    let region = region_of(profile);
    let (_, facing) = profile_plane(profile)?;
    let tangents: Vec<Option<[(V3, V3); 2]>> = legs.iter().map(Leg::ends).collect();
    let corners = tangents.windows(2).any(|w| match (w[0], w[1]) {
        (Some(before), Some(after)) => before[1].1.cross(after[0].1).len() > 1.0e-6,
        _ => false,
    });
    let bends = legs.iter().any(|leg| !matches!(leg, Leg::Line(..)));
    let square = tangents[0].is_none_or(|t| t[0].1.cross(facing).len() <= 1.0e-6);
    // One outline at a time where the legs of the sweep overlap at a
    // corner: joined whole, the wall of one leg of a tube would stand in
    // the bore of the next.
    let each_alone = |make: &dyn Fn(&Region, usize) -> Result<Body, String>| {
        let mut whole: Option<Body> = None;
        for (lp, segs) in region.loops.iter().enumerate() {
            let one = make(
                &Region {
                    loops: vec![segs.clone()],
                },
                lp,
            )?;
            whole = Some(match whole {
                None => one,
                Some(outer) => boolean(&outer, &one, Op::Cut).map_err(|e| format!("Sweep: {e}"))?,
            });
        }
        whole.ok_or_else(|| "Sweep needs a profile".to_string())
    };
    if fixed && (bends || corners || !square) {
        let along: Vec<Seg> = legs
            .iter()
            .map(|leg| match *leg {
                Leg::Line(a, b) => Ok(Seg::Line { a, b }),
                Leg::Arc(a, m, b) => Ok(Seg::Arc { a, m, b }),
                Leg::Ring(..) => Err(unsupported(
                    "A sweep round a full circle with its profile kept facing one way",
                )),
            })
            .collect::<Result<_, _>>()?;
        return each_alone(&|one, lp| {
            let mut tool = loft::carried(&one.loops[0], &along)?;
            tool.retag(|tag| match Role::of(tag) {
                Role::Side { lp: band, seg } => {
                    let from = entity[lp]
                        .get(seg as usize)
                        .copied()
                        .unwrap_or(u64::from(seg));
                    mix(
                        mix(feature, SIDE),
                        mix(mix(lp as u64, u64::from(band)), from),
                    )
                }
                Role::Start => mix(mix(feature, START), lp as u64),
                Role::End => mix(mix(feature, END), lp as u64),
                Role::Other => mix(feature, tag),
            });
            Ok(tool)
        });
    }
    if corners && region.loops.len() > 1 {
        return each_alone(&|one, lp| {
            sweep_square(one, &legs, transition, feature, &entity[lp..=lp], lp as u64)
        });
    }
    sweep_square(&region, &legs, transition, feature, &entity, 0)
}

/// A profile carried along a path and steered by a rail beside it: see
/// [`loft::guided`]. Holes are swept as outlines of their own and
/// cut out.
fn guided_tool(
    profile: &KernelProfileDto,
    path: &[KernelCurveDto],
    rail: &[KernelCurveDto],
    feature: u64,
) -> Result<Body, String> {
    let plain = |curves: &[KernelCurveDto], what: &str| -> Result<Vec<Seg>, String> {
        sweep_legs(profile, curves)?
            .iter()
            .map(|leg| match *leg {
                Leg::Line(a, b) => Ok(Seg::Line { a, b }),
                Leg::Arc(a, m, b) => Ok(Seg::Arc { a, m, b }),
                Leg::Ring(..) => Err(unsupported(&format!(
                    "A full circle as the {what} of a guided sweep"
                ))),
            })
            .collect()
    };
    let (path, rail) = (plain(path, "path")?, plain(rail, "guide rail")?);
    let mut whole: Option<Body> = None;
    let outlines = std::iter::once(profile).chain(profile.holes.iter());
    for (lp, outline) in outlines.enumerate() {
        let names = entities(outline);
        let mut tool = loft::guided(&segs_of(outline), &path, &rail)?;
        tool.retag(|tag| match Role::of(tag) {
            Role::Side { lp: piece, seg } => {
                let from = names.get(seg as usize).copied().unwrap_or(u64::from(seg));
                mix(
                    mix(feature, SIDE),
                    mix(mix(lp as u64, u64::from(piece)), from),
                )
            }
            Role::Start => mix(mix(feature, START), lp as u64),
            Role::End => mix(mix(feature, END), lp as u64),
            Role::Other => mix(feature, tag),
        });
        whole = Some(match whole {
            None => tool,
            Some(outer) => boolean(&outer, &tool, Op::Cut).map_err(|e| format!("Sweep: {e}"))?,
        });
    }
    whole.ok_or_else(|| "Sweep needs a profile".to_string())
}

/// The sweep of a region kept square to its path. `entity` names the
/// sketch curve behind every segment of every loop, and `part` tells one
/// outline swept on its own from another.
fn sweep_square(
    region: &Region,
    legs: &[Leg],
    transition: SweepTransition,
    feature: u64,
    entity: &[Vec<u64>],
    part: u64,
) -> Result<Body, String> {
    let mut region = region.clone();
    let points: Vec<V3> = region
        .loops
        .iter()
        .flatten()
        .flat_map(|seg| match *seg {
            Seg::Line { a, b } => vec![a, b],
            Seg::Arc { a, m, b } => vec![a, m, b],
            Seg::Circle { c, p, n } => {
                let r = p - c;
                let q = n.unit().cross(r);
                vec![c + r, c + q, c - r, c - q]
            }
        })
        .collect();
    let middle = points.iter().fold(V3::ZERO, |sum, p| sum + *p) / points.len().max(1) as f64;
    let mut around = V3::ZERO;
    for i in 0..points.len() {
        around += (points[i] - middle).cross(points[(i + 1) % points.len()] - middle);
    }
    let mut facing = around.unit();
    let square = || "a sweep needs its profile square to the start of its path".to_string();
    // Sides are named after the sketch line swept and the piece of the
    // path it was swept along; the two ends after the sweep itself.
    let named = |tool: &mut Body, piece: u64| {
        tool.retag(|tag| match Role::of(tag) {
            Role::Side { lp, seg } => {
                let from = entity
                    .get(lp as usize)
                    .and_then(|l| l.get(seg as usize))
                    .copied()
                    .unwrap_or_else(|| mix(u64::from(lp), u64::from(seg)));
                mix(mix(feature, SIDE), mix(mix(part, piece), from))
            }
            Role::Start => mix(mix(feature, START), mix(part, piece)),
            Role::End => mix(mix(feature, END), mix(part, piece)),
            Role::Other => mix(feature, tag),
        });
    };
    let join = |whole: Option<Body>, tool: Body| -> Result<Option<Body>, String> {
        Ok(Some(match whole {
            None => tool,
            Some(so_far) => {
                boolean(&so_far, &tool, Op::Union).map_err(|e| format!("Sweep: {e}"))?
            }
        }))
    };
    let mut whole: Option<Body> = None;
    let count = legs.len() as u64;
    for (nth, leg) in legs.iter().enumerate() {
        // A corner: the path sets off another way than it arrived.
        if let Some([(start, heading), _]) = leg.ends() {
            let turn = facing.cross(heading);
            if turn.len() > 1.0e-6 {
                if nth == 0 {
                    return Err(square());
                }
                if region.loops.len() != 1 {
                    return Err(unsupported(
                        "A sweep round a corner with holes in its profile swept at once",
                    ));
                }
                let angle = turn.len().atan2(facing.dot(heading));
                if angle > 170.0_f64.to_radians() {
                    return Err("the sweep path doubles back on itself at a corner".into());
                }
                let axis = turn.unit();
                let carry = Xform::rotation(start, axis, angle);
                let reach = points_of(&region)
                    .iter()
                    .map(|p| p.dist(start))
                    .fold(0.0, f64::max);
                let turned = moved(&region, &carry);
                let fill = if transition == SweepTransition::RoundCorner {
                    swung_round(&region, start, facing, axis, angle, reach)?
                } else {
                    // Both legs run on past the corner, and what they
                    // share there is the sharp corner between them.
                    let further = reach * (0.5 * angle).tan() + 1.0;
                    let mut ahead = make::extrude(&region, facing, 0.0, further)?;
                    let mut behind = make::extrude(&turned, heading, -further, 0.0)?;
                    named(&mut ahead, count + 2 * nth as u64);
                    named(&mut behind, count + 2 * nth as u64 + 1);
                    match boolean(&ahead, &behind, Op::Intersect) {
                        Ok(shared) => Some(shared),
                        Err(e) if e.contains(NO_OVERLAP) => None,
                        Err(e) => return Err(format!("Sweep: {e}")),
                    }
                };
                if let Some(mut fill) = fill {
                    if transition == SweepTransition::RoundCorner {
                        named(&mut fill, count + 2 * nth as u64);
                    }
                    whole = join(whole, fill)?;
                }
                region = turned;
                facing = heading;
            }
        }
        let (mut tool, carry) = match *leg {
            Leg::Line(a, b) => {
                let along = b - a;
                if along.len() <= TOL {
                    return Err("the sweep path has a piece of no length".into());
                }
                (
                    make::extrude(&region, facing, 0.0, along.len())?,
                    Xform::translation(along),
                )
            }
            Leg::Arc(a, m, b) => {
                let (centre, _, axis) =
                    make::arc_circle(a, m, b).ok_or("the sweep path has a degenerate arc")?;
                let frame = Frame::from_zx(centre, axis, a - centre);
                let to = frame.local(b);
                let turn = wrap_from(to.y.atan2(to.x), 1.0e-12);
                facing = axis.cross(b - centre).unit();
                (
                    make::revolve_region(&region, centre, axis, turn)?,
                    Xform::rotation(centre, axis, turn),
                )
            }
            Leg::Ring(centre, normal) => {
                if count != 1 {
                    return Err(unsupported(
                        "A sweep path with a full circle among other curves",
                    ));
                }
                let axis = normal.unit();
                if axis.cross(middle - centre).unit().cross(facing).len() > 1.0e-6 {
                    return Err(square());
                }
                (
                    make::revolve_region(&region, centre, axis, TAU)?,
                    Xform::IDENTITY,
                )
            }
        };
        named(&mut tool, nth as u64);
        whole = join(whole, tool)?;
        region = moved(&region, &carry);
    }
    whole.ok_or_else(|| "Sweep needs a path".to_string())
}

/// Every point that the segments of a region name.
fn points_of(region: &Region) -> Vec<V3> {
    region
        .loops
        .iter()
        .flatten()
        .flat_map(|seg| match *seg {
            Seg::Line { a, b } => vec![a, b],
            Seg::Arc { a, m, b } => vec![a, m, b],
            Seg::Circle { c, p, n } => {
                let r = p - c;
                let q = n.unit().cross(r);
                vec![c + r, c + q, c - r, c - q]
            }
        })
        .collect()
}

/// What a profile sweeps on the outside of a corner when it is swung round
/// the corner point: the half of it that lies outside the turn, turned
/// about the line through the corner that the turn is about. (The inside
/// half sweeps back through the two legs.) `None` when all of the profile
/// is inside the turn.
fn swung_round(
    region: &Region,
    corner: V3,
    facing: V3,
    axis: V3,
    angle: f64,
    reach: f64,
) -> Result<Option<Body>, String> {
    let inward = axis.cross(facing);
    // The outer half of the profile is the end face of a slice of it with
    // the inner half cut away.
    let slab = make::extrude(region, facing, 0.0, 1.0)?;
    let inner = half_space(corner, inward, 4.0 * reach + 10.0)?;
    let outer = match boolean(&slab, &inner, Op::Cut) {
        Ok(outer) => outer,
        Err(e) if e.contains(NO_OVERLAP) => slab,
        Err(e) if e.contains("removes the entire body") => return Ok(None),
        Err(e) => return Err(format!("Sweep: {e}")),
    };
    let start = points_of(region)[0];
    let face = (0..outer.face_count() as u32)
        .find(|&f| {
            let s = outer.f_surface(f);
            s.is_plane()
                && outer.f_normal(f, s.f.o).dot(facing) < -1.0 + 1.0e-9
                && (s.f.o - start).dot(facing).abs() < 1.0e-7
        })
        .ok_or("the profile of a sweep could not be divided at a corner")?;
    let (half, _) = face_region(&outer, face)?;
    make::revolve_region(&half, corner, axis, angle)
        .map(Some)
        .map_err(|e| format!("Sweep: {e}"))
}

/// Tag the faces of a tool built from a sketch profile: each side after
/// the sketch entity it was swept from, so that it keeps its name when
/// other entities come and go, and the ends after the profile.
fn stamped(mut tool: Body, feature: u64, profile: &KernelProfileDto) -> Body {
    let mut loops = vec![entities(profile)];
    loops.extend(profile.holes.iter().map(entities));
    // The profile is whichever its outline's entities say it is; its place
    // in the sketch's list of profiles shifts when the sketch is edited.
    let outline = loops[0].iter().copied().min().unwrap_or(0);
    tool.retag(|tag| match Role::of(tag) {
        Role::Side { lp, seg } => {
            let entity = loops
                .get(lp as usize)
                .and_then(|l| l.get(seg as usize))
                .copied()
                .unwrap_or_else(|| mix(u64::from(lp), u64::from(seg)));
            mix(mix(feature, SIDE), entity)
        }
        Role::Start => mix(mix(feature, START), outline),
        Role::End => mix(mix(feature, END), outline),
        Role::Other => mix(feature, tag),
    });
    tool
}

/// The edges of a body that the given names refer to. A name can stand for
/// several: the pieces of an edge that something has since cut through.
fn edge_indices(names: &Names, keys: &[String]) -> Result<Vec<u32>, String> {
    let mut edges = Vec::with_capacity(keys.len());
    for key in keys {
        let found = names.edges_named(key);
        if found.is_empty() {
            return Err(format!(
                "Invalid edge topology reference '{key}'; reselect the edge on the feature input body"
            ));
        }
        edges.extend(found);
    }
    Ok(edges)
}

/// How near a point of a mark has to be to an edge to be a point of it.
/// The points come from the other kernel's drawing of the edge, which
/// keeps them on its own curve to far better than this.
const MARK_REACH: f64 = 1.0e-4;

/// A mark for edge `e`: five points of it, none of them an end.
fn mark_of(body: &Body, e: u32) -> Vec<Point3Dto> {
    let [t0, t1] = body.e_t[e as usize];
    (0..5)
        .map(|k| dto(body.e_point(e, t0 + (t1 - t0) * (2 * k + 1) as f64 / 10.0)))
        .collect()
}

/// The edges of a body that a fillet or chamfer is to work on, and what to
/// tell the host of them.
///
/// A key written here is a name, and is looked up as one. A key the
/// reference kernel wrote counts edges of a body as that kernel built it,
/// which says nothing here; the edge is then whatever passes through the
/// points of its mark, and with no mark it is refused. That may be more
/// than one edge, where this kernel cuts in two what the other keeps whole,
/// and it may be an edge longer than the one chosen, where it is the other
/// way about.
///
/// The same goes for telling a job from the one before it: see
/// [`follows`].
fn finish_edges(
    body: &Body,
    names: &Names,
    keys: &[String],
    marks: &[EdgeMarkDto],
    feature: FeatureId,
) -> Result<(Vec<u32>, FeatureKeysDto), String> {
    let mut edges: Vec<u32> = Vec::with_capacity(keys.len());
    let mut report = FeatureKeysDto::of(feature);
    for (at, key) in keys.iter().enumerate() {
        let mark = marks
            .get(at)
            .filter(|mark| marks.len() == keys.len() && !mark.points.is_empty());
        if follows(key) {
            let found = names.edges_named(key);
            if found.is_empty() {
                return Err(format!(
                    "Invalid edge topology reference '{key}'; reselect the edge on the feature input body"
                ));
            }
            report.edge_keys.push(key.clone());
            report.edge_marks.push(EdgeMarkDto {
                points: found
                    .iter()
                    .take(4)
                    .flat_map(|&e| mark_of(body, e))
                    .collect(),
            });
            edges.extend(found);
            continue;
        }
        // A count with no mark beside it, from a project older than marks,
        // leaves nothing to find the edge by. The edge that stands at that
        // count here is some other edge, so it is not taken.
        let Some(mark) = mark else {
            return Err(format!(
                "Edge reference '{key}' was written by the other kernel and carries no mark to find it by; reselect the edge on the feature input body"
            ));
        };
        let mut found: Vec<u32> = Vec::new();
        for point in &mark.points {
            let p = p3(point);
            let through: Vec<u32> = (0..body.edge_count() as u32)
                .filter(|&e| edge_closest(body, e, p).1 < MARK_REACH)
                .collect();
            if through.is_empty() {
                return Err(
                    "referenced edge no longer exists where it was marked; reselect the edge on the feature input body"
                        .into(),
                );
            }
            for e in through {
                if !found.contains(&e) {
                    found.push(e);
                }
            }
        }
        for &e in &found {
            report.edge_keys.push(names.edges[e as usize].clone());
            report.edge_marks.push(EdgeMarkDto {
                points: mark_of(body, e),
            });
        }
        edges.extend(found);
    }
    // One edge chosen twice is still one edge.
    let mut once: Vec<u32> = Vec::with_capacity(edges.len());
    for e in edges {
        if !once.contains(&e) {
            once.push(e);
        }
    }
    Ok((once, report))
}

/// How finely the rim of a face is drawn to tell what lies inside it.
/// Close to the rim the exact curve decides, so this only has to keep a
/// narrow face from being lost between two chords.
const MARK_SAG: f64 = 0.05;

/// A mark for face `f`: up to five points of it, each as far inside its
/// rim as a point of it is to be found, and none of them a point that the
/// exact rim would put outside.
fn face_mark_of(body: &Body, rims: &EdgeMesh, f: u32) -> Vec<Point3Dto> {
    let trim = FaceTrim::build(body, rims, f, MARK_SAG);
    trim.interior_points()
        .into_iter()
        .map(|(q, _)| trim.point(q))
        .filter(|&p| classify(body, &trim, f, p) == PtClass::In)
        .take(5)
        .map(dto)
        .collect()
}

/// The faces of a body that the point `p` lies on.
fn faces_through(body: &Body, rims: &EdgeMesh, p: V3) -> Vec<u32> {
    (0..body.face_count() as u32)
        .filter(|&f| {
            body.f_surface(f).sdist(p).abs() < MARK_REACH
                && classify(body, &FaceTrim::build(body, rims, f, MARK_SAG), f, p) != PtClass::Out
        })
        .collect()
}

/// The faces of a body that a shell or an external thread is to work on,
/// and what to tell the host of them.
///
/// It is for faces what [`finish_edges`] is for edges. A key written here
/// is a name, and is looked up as one; `gone` is what to say when the body
/// no longer has a face of that name. A count (`face:7`) is how the
/// reference kernel names a face, by its place among the faces of the body
/// as that kernel built it. The face that stands at the same count here is
/// as a rule another one, and a shell opened at the wrong face, or a thread
/// cut on the wrong shaft, would look like a model that worked. So a count
/// is never followed: the face is whatever the points of its mark lie on,
/// and with no mark it is refused. That may be more than one face, where
/// this kernel cuts in two what the other keeps whole, and it may be a
/// face larger than the one chosen, where it is the other way about.
fn feature_faces(
    body: &Body,
    names: &Names,
    keys: &[String],
    marks: &[FaceMarkDto],
    feature: FeatureId,
    gone: &str,
) -> Result<(Vec<u32>, FeatureKeysDto), String> {
    let rims = EdgeMesh::build(body, MARK_SAG, 16.0);
    let mut faces: Vec<u32> = Vec::with_capacity(keys.len());
    for (at, key) in keys.iter().enumerate() {
        if follows(key) {
            let f = names.face(key).ok_or_else(|| gone.to_string())?;
            if !faces.contains(&f) {
                faces.push(f);
            }
            continue;
        }
        let mark = marks
            .get(at)
            .filter(|mark| marks.len() == keys.len() && !mark.points.is_empty())
            .ok_or_else(|| {
                format!(
                    "Face reference '{key}' was written by the other kernel and carries no mark to find it by; reselect the face on the feature input body"
                )
            })?;
        for point in &mark.points {
            let through = faces_through(body, &rims, p3(point));
            if through.is_empty() {
                return Err(
                    "referenced face no longer exists where it was marked; reselect the face on the feature input body"
                        .into(),
                );
            }
            // One face chosen twice is still one face.
            for f in through {
                if !faces.contains(&f) {
                    faces.push(f);
                }
            }
        }
    }
    let mut report = FeatureKeysDto::of(feature);
    for &f in &faces {
        report.face_keys.push(names.faces[f as usize].clone());
        report.face_marks.push(FaceMarkDto {
            points: face_mark_of(body, &rims, f),
        });
    }
    Ok((faces, report))
}

/// Whether this kernel follows a key by itself: anything but a count of the
/// reference kernel's, of edges or of faces.
fn follows(key: &str) -> bool {
    !(limo_cad_solid::is_counted_edge_key(key) || limo_cad_solid::is_counted_face_key(key))
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
    if taper.abs() < 1.0e-12 {
        return make::extrude(&region_of(profile), normal, start, end);
    }
    // Scaled about the middle of the outline's points.
    let points: Vec<V3> = profile.points.iter().map(p3).collect();
    if points.is_empty() {
        return Err("a tapered extrusion needs the outline of its profile".into());
    }
    let centre = points.iter().fold(V3::ZERO, |sum, p| sum + *p) / points.len() as f64;
    make::taper(
        &region_of(profile),
        normal,
        start,
        end,
        centre,
        taper_rate(taper, centre, &points),
    )
}

/// How fast a tapered profile grows per unit of offset: the tangent of the
/// taper angle over the mean distance of `points` from `centre`, so that a
/// side at that distance leans by exactly the angle.
fn taper_rate(taper_deg: f64, centre: V3, points: &[V3]) -> f64 {
    let reach = points.iter().map(|p| p.dist(centre)).sum::<f64>() / points.len().max(1) as f64;
    taper_deg.to_radians().tan() / reach.max(1.0e-6)
}

/// The boundary of a planar face as an extrusion region, outer loop first,
/// and the edge behind every segment of it.
fn face_region(body: &Body, f: u32) -> Result<(Region, Vec<Vec<u32>>), String> {
    if !body.f_surface(f).is_plane() {
        return Err("Extrude source face is not planar".into());
    }
    let mesh = EdgeMesh::build(body, 0.05, 16.0);
    let trim = FaceTrim::build(body, &mesh, f, 0.05);
    let mut order: Vec<usize> = (0..trim.loops.len()).collect();
    order.sort_by(|&a, &b| trim.loops[b].area.total_cmp(&trim.loops[a].area));
    let loops: Vec<u32> = body.face_loops(f).collect();
    let mut region = Region::default();
    let mut edges: Vec<Vec<u32>> = Vec::new();
    for i in order {
        let mut segs = Vec::new();
        edges.push(body.loop_hes(loops[i]).map(|h| h >> 1).collect());
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
                CurveKind::Ellipse
                | CurveKind::Trace
                | CurveKind::Meet
                | CurveKind::Helix
                | CurveKind::Spline
                | CurveKind::Iso => {
                    return Err(unsupported(
                        "Extruding a face with an edge that is neither straight nor circular",
                    ))
                }
            }
        }
        region.loops.push(segs);
    }
    Ok((region, edges))
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
/// Tags of the faces a thread leaves: its groove, and the crest it was
/// turned down to.
const THREAD: u64 = 0x0741_2ead;
const CREST: u64 = 0x000c_2e57;

/// The diameters a modeled thread is cut to, and its rounds if it has any.
struct ThreadForm {
    major: f64,
    pitch_diameter: f64,
    minor: f64,
    pitch: f64,
    /// Corner radius and clearance along the axis of the rounded form.
    rounded: Option<(f64, f64)>,
}

impl ThreadForm {
    fn internal(&self) -> Result<Vec<make::PSeg>, String> {
        match self.rounded {
            Some((radius, clearance)) => {
                thread::rounded(self.major, self.minor, self.pitch, radius, clearance, true)
            }
            None => thread::iso_internal(self.major, self.pitch_diameter, self.minor, self.pitch),
        }
    }
    fn external(&self) -> Result<Vec<make::PSeg>, String> {
        match self.rounded {
            Some((radius, _)) => {
                thread::rounded(self.major, self.minor, self.pitch, radius, 0.0, false)
            }
            None => thread::iso_external(self.major, self.pitch_diameter, self.minor, self.pitch),
        }
    }
}

/// The form of a modeled thread: the rounded profile's own diameters, the
/// maximum-material limits of an ISO metric class, or the basic profile.
/// `drill` is the diameter of the hole an internal thread is cut in.
fn thread_form(thread: &HoleThreadDto, fit: ThreadFit, drill: f64) -> Result<ThreadForm, String> {
    let mut form = ThreadForm {
        major: thread.nominal_diameter,
        pitch_diameter: thread.nominal_diameter - 0.649_519_052_838_329 * thread.pitch,
        minor: match fit {
            ThreadFit::Internal => drill,
            ThreadFit::External => thread.nominal_diameter - 1.226_869_322_027_954 * thread.pitch,
        },
        pitch: thread.pitch,
        rounded: None,
    };
    if let Some([major, pitch_diameter, minor]) = rounded_thread_diameters(thread, fit)? {
        let profile = thread
            .rounded_profile
            .as_ref()
            .ok_or("custom trapezoidal threads require rounded_profile")?;
        if fit == ThreadFit::Internal && drill > minor + 1.0e-9 {
            return Err("predrill exceeds rounded thread minor diameter".into());
        }
        form.major = major;
        form.pitch_diameter = pitch_diameter;
        form.minor = minor;
        form.rounded = Some((
            profile.corner_radius,
            if fit == ThreadFit::Internal {
                profile.axial_clearance
            } else {
                0.0
            },
        ));
    } else if let Some(limits) = iso_metric_thread_envelope(thread, fit)? {
        form.major = limits.modeled_major;
        form.pitch_diameter = limits.modeled_pitch;
        form.minor = limits.modeled_minor;
    }
    if !(form.major > form.pitch_diameter && form.pitch_diameter > form.minor && form.minor > 0.0) {
        return Err("modeled thread tolerance limits are malformed".into());
    }
    Ok(form)
}

/// The solid a hole removes, and how deep it goes. A modeled thread is cut
/// in a hole of its minor diameter.
fn hole_cutter(
    j: &KernelHoleJobDto,
    target: &Body,
    form: Option<&ThreadForm>,
) -> Result<(Body, f64), String> {
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
        if thread.depth.is_some_and(|d| d < 0.0) {
            return Err("thread parameters are malformed".into());
        }
    }
    if let Some(form) = form {
        radius = 0.5 * form.minor;
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
    Ok((make::lathe(p3(&j.center), direction, &profile)?, depth))
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
    let names = Names::of(body);
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
                    edge_keys.push(names.edges[e as usize].clone());
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
                rings.push(names.edges[(hes[0] >> 1) as usize].clone());
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
            key: names.faces[f as usize].clone(),
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
            key: names.edges[e as usize].clone(),
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

    // A face the mesher could not draw is said, not passed over: the solid
    // is whole, its picture is not, and nothing may be printed from it.
    let display_warnings: Vec<DisplayMeshWarningDto> = (0..body.face_count())
        .filter(|&f| mesh.faces[f][1] == 0)
        .map(|f| DisplayMeshWarningDto {
            face_key: names.faces[f].clone(),
            message: "This face cannot be drawn yet; the exact solid is kept".into(),
        })
        .collect();
    KernelBodyDto {
        body_id: BodyId(id),
        topology_signature: topology_signature(body),
        display_warnings,
        positions,
        normals,
        indices: mesh.indices,
        faces,
        edges,
    }
}
