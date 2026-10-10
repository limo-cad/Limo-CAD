//! One bounded worker per window. Shared DTO resolution never runs in a
//! pointer handler; cancellation drops ownership without joining the worker.
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc,
};

pub(super) const MAX_POINTS: usize = 131_072;
/// Bound the immutable source before the shared adapters clone/tessellate it.
/// Closed resolution reads all scoped source edges, including edges omitted
/// from the 2D picker choices, so those must count against the same budget.
pub(super) fn preflight(
    context: &operation_geometry::Context,
    source: ChainSource,
) -> Result<(), String> {
    let (mut edges, mut points) = (0usize, 0usize);
    let mut add = |count: usize| -> Result<(), String> {
        edges = edges
            .checked_add(1)
            .ok_or("Geometry edge budget overflow")?;
        points = points
            .checked_add(count)
            .ok_or("Geometry point budget overflow")?;
        if edges > 20_000 {
            return Err(
                "Viewport picking supports at most 20000 source edges; use the geometry fields"
                    .into(),
            );
        }
        if points > MAX_POINTS {
            return Err(
                "Viewport picking exceeds its source point budget; use the geometry fields".into(),
            );
        }
        Ok(())
    };
    match source {
        ChainSource::Model => {
            for body in context.scene.bodies.iter().filter(|body| {
                context.setup.body_ids.is_empty() || context.setup.body_ids.contains(&body.id)
            }) {
                for edge in &body.edges {
                    add(edge.points.len())?;
                }
            }
        }
        ChainSource::Sketch => {
            use limo_cad_sketch::EntityDto;
            for entity in context.sketches.iter().flat_map(|sketch| &sketch.entities) {
                let count = match entity {
                    EntityDto::Point { .. } => continue,
                    EntityDto::Line { .. } => 2,
                    EntityDto::Circle { .. } => 72,
                    EntityDto::Spline { tessellation, .. } => tessellation.len(),
                    EntityDto::Arc {
                        start_angle,
                        end_angle,
                        ..
                    } => {
                        let mut sweep = end_angle - start_angle;
                        if !sweep.is_finite() || sweep.abs() > std::f64::consts::TAU * 2. {
                            return Err(
                                "Sketch arc angles exceed the bounded viewport picker range".into(),
                            );
                        }
                        while sweep <= 0. {
                            sweep += std::f64::consts::TAU;
                        }
                        ((sweep.to_degrees() / 5.).ceil() as usize).max(4) + 1
                    }
                };
                add(count)?;
            }
        }
    }
    Ok(())
}
pub(super) struct Candidate {
    pub key: String,
    pub points: Vec<[f64; 3]>,
    pub closed: bool,
    pub planar: bool,
}
pub(super) enum ResultMessage {
    Points(Result<Vec<super::point_target::Candidate>, String>),
    Candidates(Result<Vec<Candidate>, String>),
    Chain(Result<limo_cad_core::edge_chain::Chain, String>),
    Holes(Result<Vec<super::holes::Candidate>, String>),
    Face(
        super::holes::Request,
        Result<Option<super::holes::FaceKey>, String>,
    ),
}
pub(super) enum Request {
    Chain(String),
    Face(super::holes::Request),
}
pub(super) struct Worker {
    pub(super) sender: Option<mpsc::SyncSender<Request>>,
    pub(super) receiver: Mutex<mpsc::Receiver<ResultMessage>>,
    pub(super) cancelled: Arc<AtomicBool>,
}
impl Worker {
    pub fn cancel(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.sender = None;
    }
    pub fn receive(&self) -> Result<ResultMessage, mpsc::TryRecvError> {
        self.receiver.lock().unwrap().try_recv()
    }
    pub fn resolve(&self, key: String) -> Result<(), String> {
        self.sender
            .as_ref()
            .ok_or("Geometry picking was cancelled")?
            .try_send(Request::Chain(key))
            .map_err(|_| "The geometry resolver is busy".to_string())
    }
    pub fn face(&self, request: super::holes::Request) -> Result<(), String> {
        self.sender
            .as_ref()
            .ok_or("Geometry picking was cancelled")?
            .try_send(Request::Face(request))
            .map_err(|_| "The geometry resolver is busy".to_string())
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel();
    }
}
pub(super) fn candidates(
    context: &operation_geometry::Context,
    selection: &SelectionState,
    transforms: &HashMap<u64, Transform>,
) -> Result<Vec<Candidate>, String> {
    preflight(context, selection.source)?;
    let allowed = if selection.source == ChainSource::Model {
        &context.model_options
    } else {
        &context.sketch_options
    };
    if allowed.len() > 20_000 {
        return Err(
            "Viewport picking supports at most 20000 edges; use the geometry fields".into(),
        );
    }
    let allowed = allowed
        .iter()
        .map(|option| option.value.as_str())
        .collect::<std::collections::HashSet<_>>();
    let edges = limo_cad_sketch::edge_chain_candidates(
        &context.scene,
        &context.sketches,
        selection.source,
        &context.setup.body_ids,
    );
    let mut total = 0usize;
    edges
        .into_iter()
        .filter(|edge| allowed.contains(edge.key.as_str()))
        .map(|edge| {
            total = total
                .checked_add(edge.points.len())
                .ok_or("Geometry preview exceeds its point budget")?;
            if total > MAX_POINTS {
                return Err(
                    "Viewport picking exceeds its point budget; use the geometry fields".into(),
                );
            }
            if edge.points.len() < 2 || edge.points.iter().flatten().any(|p| !p.is_finite()) {
                return Err("Selected geometry has invalid preview points".into());
            }
            let height = operation_geometry::project(edge.points[0], &context.setup)[2];
            let planar = edge.points.iter().all(|point| {
                (operation_geometry::project(*point, &context.setup)[2] - height).abs()
                    <= limo_cad_core::edge_chain::JOIN_TOLERANCE
            });
            let transform = edge
                .key
                .strip_prefix("edge:")
                .and_then(|key| key.split_once(':'))
                .and_then(|(body, _)| body.parse::<u64>().ok())
                .and_then(|body| transforms.get(&body));
            let points: Vec<_> = edge
                .points
                .into_iter()
                .map(|point| {
                    transform.map_or(point, |transform| {
                        transform
                            .transform_point(Vec3::from_array(point.map(|v| v as f32)))
                            .as_dvec3()
                            .to_array()
                    })
                })
                .collect();
            if points
                .iter()
                .flatten()
                .any(|point| !(*point as f32).is_finite())
            {
                return Err(
                    "Geometry preview is outside the renderer's finite coordinate range".into(),
                );
            }
            Ok(Candidate {
                key: edge.key,
                points,
                closed: edge.closed,
                planar,
            })
        })
        .collect()
}
pub(super) fn start(
    context: operation_geometry::Context,
    selection: SelectionState,
    transforms: HashMap<u64, Transform>,
    handle: NativeInterfaceHandle,
) -> Result<Worker, String> {
    let (sender, requests) = mpsc::sync_channel::<Request>(1);
    let (send, receiver) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancel = cancelled.clone();
    std::thread::Builder::new()
        .name("cad-native-cam-pick".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                candidates(&context, &selection, &transforms)
            }))
            .unwrap_or_else(|_| Err("Geometry preview worker stopped unexpectedly".into()));
            let failed = result.is_err();
            if cancel.load(Ordering::Acquire) {
                return;
            }
            if send.send(ResultMessage::Candidates(result)).is_err() {
                return;
            }
            handle.request_redraw();
            if failed {
                return;
            }
            while let Ok(Request::Chain(key)) = requests.recv() {
                if cancel.load(Ordering::Acquire) {
                    return;
                }
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    limo_cad_sketch::resolve_edge_chain(
                        &context.scene,
                        &context.sketches,
                        &limo_cad_sketch::EdgeChainRequest {
                            source: selection.source,
                            body_ids: context.setup.body_ids.clone(),
                            normal: Some(context.setup.wcs.z_axis),
                            keys: vec![key],
                            mode: ChainMode::Closed,
                            reversed: selection.reversed,
                        },
                    )
                }))
                .unwrap_or_else(|_| Err("Geometry resolver stopped unexpectedly".into()));
                if cancel.load(Ordering::Acquire) {
                    return;
                }
                if send.send(ResultMessage::Chain(result)).is_err() {
                    return;
                }
                handle.request_redraw();
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(Worker {
        sender: Some(sender),
        receiver: Mutex::new(receiver),
        cancelled,
    })
}
