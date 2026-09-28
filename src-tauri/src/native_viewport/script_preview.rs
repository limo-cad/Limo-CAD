//! Immutable teaching snapshots rendered by the production CAD scene systems.
//! One windowless worker owns its Bevy world. It sleeps while idle and has no
//! AppState, publisher, native child view, or access to the live camera/picker.
use super::super::DEFAULT_VERTICAL_FOV_DEGREES;
use super::*;
use bevy::render::{
    pipelined_rendering::PipelinedRenderingPlugin,
    render_resource::{
        CachedPipelineState, Extent3d, PipelineCache, PollType, TextureDimension, TextureFormat,
        TextureUsages,
    },
    renderer::RenderDevice,
    view::screenshot::{Screenshot, ScreenshotCaptured},
    RenderApp,
};
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, sync::mpsc, time::Duration};

const MAX_CACHE_BYTES: usize = 32 * 1024 * 1024;
const MAX_DOCUMENT_BYTES: usize = 16 * 1024 * 1024;
const MAX_DOCUMENTS: usize = 8;
const MAX_VIEWS: usize = 8;
const REQUEST_LIFETIME: Duration = Duration::from_secs(20);

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct Frame {
    pub caption: String,
    pub scene: SolidSceneDto,
}

struct PreviewDocument {
    frames: Vec<Frame>,
    center: Vec3,
    radius: f32,
    bytes: usize,
}

impl PreviewDocument {
    fn new(frames: Vec<Frame>) -> Result<Self, String> {
        if frames.is_empty() || frames.len() > 16 {
            return Err("A feature preview needs between 1 and 16 frames".into());
        }
        let mut low = Vec3::splat(f32::INFINITY);
        let mut high = Vec3::splat(f32::NEG_INFINITY);
        let mut triangles = 0;
        let mut points = 0;
        let mut bodies = 0;
        let mut faces = 0;
        let mut edges = 0;
        for frame in &frames {
            if frame.caption.len() > 4000 || !frame.scene.errors.is_empty() {
                return Err("A preview frame has an invalid caption or failed geometry".into());
            }
            for body in &frame.scene.bodies {
                let mesh = &body.mesh;
                bodies += 1;
                faces += body.faces.len();
                edges += body.edges.len();
                triangles += mesh.indices.len() / 3;
                points += mesh.positions.len() / 3
                    + body
                        .edges
                        .iter()
                        .map(|edge| edge.points.len())
                        .sum::<usize>();
                if bodies > 256
                    || faces > 10_000
                    || edges > 10_000
                    || triangles > 100_000
                    || mesh.positions.len() % 3 != 0
                    || mesh.indices.len() < 3
                    || points > 300_000
                    || mesh.positions.len() < 9
                    || mesh.normals.len() != mesh.positions.len()
                    || mesh.normals.iter().any(|value| !value.is_finite())
                    || mesh.indices.len() % 3 != 0
                    || mesh
                        .indices
                        .iter()
                        .any(|index| *index as usize >= mesh.positions.len() / 3)
                    || mesh
                        .positions
                        .iter()
                        .any(|value| !value.is_finite() || value.abs() > 10_000.0)
                {
                    return Err("This geometry exceeds the bounded feature preview; run the script in a new design".into());
                }
                // Kernel face ranges partition the index buffer. Checking the
                // partition also bounds face-boundary extraction work: repeated
                // overlapping ranges must not multiply a small mesh's cost.
                let mut ranges = body
                    .faces
                    .iter()
                    .map(|face| (face.first_index as usize, face.index_count as usize))
                    .collect::<Vec<_>>();
                ranges.sort_unstable();
                let mut previous_end = 0;
                for (start, count) in ranges {
                    let end = start
                        .checked_add(count)
                        .ok_or("Invalid preview face range")?;
                    if start % 3 != 0
                        || count % 3 != 0
                        || start < previous_end
                        || end > mesh.indices.len()
                    {
                        return Err("Invalid preview face range".into());
                    }
                    previous_end = end;
                }
                if body
                    .edges
                    .iter()
                    .flat_map(|edge| &edge.points)
                    .any(|point| {
                        [point.x, point.y, point.z]
                            .iter()
                            .any(|value| !value.is_finite() || value.abs() > 10_000.0)
                    })
                {
                    return Err("Invalid preview edge points".into());
                }
                for point in mesh.positions.chunks_exact(3) {
                    let point = Vec3::new(point[0], point[1], point[2]);
                    low = low.min(point);
                    high = high.max(point);
                }
            }
        }
        let bytes = serde_json::to_vec(&frames)
            .map_err(|error| error.to_string())?
            .len();
        if bytes > MAX_DOCUMENT_BYTES {
            return Err("The feature preview is too large; run the script in a new design".into());
        }
        let (center, radius) = if low.is_finite() {
            ((low + high) * 0.5, low.distance(high).max(2.0) * 0.5)
        } else {
            (Vec3::ZERO, 1.0)
        };
        Ok(Self {
            frames,
            center,
            radius,
            bytes,
        })
    }

    fn camera(&self, request: &RenderRequest) -> ViewportCamera {
        let direction = Vec3::new(
            request.yaw.cos() * request.pitch.cos(),
            -request.yaw.sin() * request.pitch.cos(),
            request.pitch.sin(),
        );
        let vertical = DEFAULT_VERTICAL_FOV_DEGREES.to_radians();
        let horizontal =
            2.0 * ((vertical * 0.5).tan() * request.width as f32 / request.height as f32).atan();
        let distance = self.radius / (vertical.min(horizontal) * 0.5).sin() * 1.15;
        ViewportCamera {
            position: (self.center + direction * distance).to_array(),
            target: self.center.to_array(),
            up: Vec3::Z.to_array(),
            vertical_fov_degrees: DEFAULT_VERTICAL_FOV_DEGREES,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct PreviewDescriptor {
    pub preview_id: String,
    pub captions: Vec<String>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RenderRequest {
    pub preview_id: String,
    pub view_id: String,
    pub revision: u64,
    pub frame_index: usize,
    pub width: u32,
    pub height: u32,
    pub yaw: f32,
    pub pitch: f32,
}
impl RenderRequest {
    fn validate(&self) -> Result<(), String> {
        if uuid::Uuid::parse_str(&self.view_id).is_err()
            || self.revision == 0
            || !(64..=640).contains(&self.width)
            || !(64..=400).contains(&self.height)
            || !self.yaw.is_finite()
            || self.yaw.abs() > 100_000.0
            || !self.pitch.is_finite()
            || self.pitch.abs() > 1.3
        {
            return Err("Invalid feature preview view or camera".into());
        }
        Ok(())
    }
}

#[derive(Default)]
struct ServiceState {
    documents: VecDeque<(String, Arc<PreviewDocument>)>,
    views: HashMap<String, (u64, Arc<AtomicBool>)>,
    worker: Option<mpsc::SyncSender<WorkerCommand>>,
}

#[derive(Default)]
pub(crate) struct PreviewService(Mutex<ServiceState>, Arc<AtomicUsize>);
enum WorkerCommand {
    Render(RenderJob),
    Release,
}
struct RenderJob {
    document: Arc<PreviewDocument>,
    request: RenderRequest,
    cancelled: Arc<AtomicBool>,
    reply: mpsc::SyncSender<Result<Vec<u8>, String>>,
    deadline: Instant,
}

#[derive(Debug)]
pub(crate) struct PendingRender {
    response: mpsc::Receiver<Result<Vec<u8>, String>>,
    cancelled: Arc<AtomicBool>,
    deadline: Instant,
}
impl PendingRender {
    pub fn wait(self) -> Result<Vec<u8>, String> {
        self.response
            .recv_timeout(self.deadline.saturating_duration_since(Instant::now()))
            .map_err(|error| format!("Feature preview request ended: {error}"))?
    }
}
impl Drop for PendingRender {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

fn check_request(cancelled: &AtomicBool, deadline: Instant) -> Result<(), String> {
    if cancelled.load(Ordering::Acquire) {
        return Err("Feature preview closed".into());
    }
    if Instant::now() >= deadline {
        cancelled.store(true, Ordering::Release);
        return Err("Feature preview request timed out".into());
    }
    Ok(())
}

impl PreviewService {
    pub fn open_view(&self) -> Result<String, String> {
        let mut state = self.0.lock().map_err(|_| "Preview cache lock poisoned")?;
        if state.views.len() >= MAX_VIEWS {
            return Err("Too many feature preview views are open".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        state
            .views
            .insert(id.clone(), (0, Arc::new(AtomicBool::new(false))));
        self.1.fetch_add(1, Ordering::AcqRel);
        Ok(id)
    }

    pub fn retain(&self, frames: Vec<Frame>) -> Result<PreviewDescriptor, String> {
        let document = Arc::new(PreviewDocument::new(frames)?);
        let descriptor = PreviewDescriptor {
            preview_id: uuid::Uuid::new_v4().to_string(),
            captions: document
                .frames
                .iter()
                .map(|frame| frame.caption.clone())
                .collect(),
        };
        let mut state = self.0.lock().map_err(|_| "Preview cache lock poisoned")?;
        while state.documents.len() >= MAX_DOCUMENTS
            || state
                .documents
                .iter()
                .map(|(_, doc)| doc.bytes)
                .sum::<usize>()
                + document.bytes
                > MAX_CACHE_BYTES
        {
            state.documents.pop_front();
        }
        state
            .documents
            .push_back((descriptor.preview_id.clone(), document));
        Ok(descriptor)
    }

    pub fn release(&self, preview_id: &str) {
        if let Ok(mut state) = self.0.lock() {
            state.documents.retain(|(id, _)| id != preview_id);
        }
    }

    pub fn close_view(&self, view_id: &str) {
        if let Ok(mut state) = self.0.lock() {
            if let Some((_, cancelled)) = state.views.remove(view_id) {
                cancelled.store(true, Ordering::Release);
                self.1.fetch_sub(1, Ordering::AcqRel);
                // If the queue is full its next render checks the same count.
                if let Some(worker) = &state.worker {
                    let _ = worker.try_send(WorkerCommand::Release);
                }
            }
        }
    }

    pub fn render(&self, request: RenderRequest) -> Result<PendingRender, String> {
        request.validate()?;
        let mut state = self.0.lock().map_err(|_| "Preview cache lock poisoned")?;
        let document = state
            .documents
            .iter()
            .find(|(id, _)| id == &request.preview_id)
            .map(|(_, doc)| doc.clone())
            .ok_or("Feature preview expired; reopen the example")?;
        if request.frame_index >= document.frames.len() {
            return Err("Unknown feature preview frame".into());
        }
        let (revision, _) = state
            .views
            .get(&request.view_id)
            .ok_or("Feature preview view is closed")?;
        if request.revision <= *revision {
            return Err("Superseded feature preview request".into());
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let (reply, response) = mpsc::sync_channel(1);
        let deadline = Instant::now() + REQUEST_LIFETIME;
        if state.worker.is_none() {
            let (sender, jobs) = mpsc::sync_channel::<WorkerCommand>(2);
            let active_views = self.1.clone();
            std::thread::Builder::new()
                .name("CAD feature previews".into())
                .spawn(move || render_worker(jobs, active_views))
                .map_err(|error| error.to_string())?;
            state.worker = Some(sender);
        }
        let view_id = request.view_id.clone();
        let revision = request.revision;
        state
            .worker
            .as_ref()
            .unwrap()
            .try_send(WorkerCommand::Render(RenderJob {
                document,
                request,
                cancelled: cancelled.clone(),
                reply,
                deadline,
            }))
            .map_err(|_| "Feature preview renderer is busy; try again")?;
        if let Some((_, previous)) = state.views.insert(view_id, (revision, cancelled.clone())) {
            previous.store(true, Ordering::Release);
        }
        Ok(PendingRender {
            response,
            cancelled,
            deadline,
        })
    }
}

fn render_worker(jobs: mpsc::Receiver<WorkerCommand>, active_views: Arc<AtomicUsize>) {
    let mut renderer = None;
    while let Ok(command) = jobs.recv() {
        let job = match command {
            WorkerCommand::Release => {
                if active_views.load(Ordering::Acquire) == 0 {
                    renderer = None;
                }
                continue;
            }
            WorkerCommand::Render(job) => job,
        };
        if let Err(error) = check_request(&job.cancelled, job.deadline) {
            if active_views.load(Ordering::Acquire) == 0 {
                renderer = None;
            }
            let _ = job.reply.send(Err(error));
            continue;
        }
        let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
            if renderer.is_none() {
                renderer = Some(PreviewRenderer::new()?);
            }
            renderer.as_mut().unwrap().render(
                &job.document,
                &job.request,
                &job.cancelled,
                job.deadline,
            )
        }))
        .unwrap_or_else(|failure| {
            renderer = None;
            Err(format!(
                "Feature preview renderer failed: {}",
                panic_message(failure)
            ))
        });
        // Failed/cancelled readbacks cannot survive into another view. This
        // also releases the offscreen world's GPU assets after a close.
        if result.is_err() || active_views.load(Ordering::Acquire) == 0 {
            renderer = None;
        }
        let _ = job.reply.send(result);
    }
}

#[derive(Resource, Default)]
struct CapturedImage {
    revision: u64,
    result: Option<Result<Vec<u8>, String>>,
}

struct PreviewRenderer {
    app: bevy::app::App,
    target: Handle<Image>,
    size: (u32, u32),
    revision: u64,
    geometry_revision: u64,
    model_key: Option<(String, usize)>,
}

impl PreviewRenderer {
    fn new() -> Result<Self, String> {
        let mut app = bevy::app::App::new();
        let plugins = DefaultPlugins
            .build()
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                close_when_requested: false,
                ..default()
            })
            .set(cad_render_plugin());
        // This worker already runs off the UI thread. Keep its render world
        // local so readiness can be observed before reading back an image.
        let plugins = plugins.disable::<PipelinedRenderingPlugin>();
        // dev-bevy-host enables Winit for the live window, but this renderer
        // owns only an image on its worker thread. It must never create an OS
        // event loop, either beside that live loop or in a GPU test thread.
        let plugins = plugins.disable::<bevy::winit::WinitPlugin>();
        app.add_plugins(plugins);
        install_cad_scene(&mut app);
        app.init_resource::<CapturedImage>();
        app.finish();
        app.cleanup();
        let mut image = Image::new_uninit(
            Extent3d {
                width: 300,
                height: 176,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT;
        let target = app.world_mut().resource_mut::<Assets<Image>>().add(image);
        app.insert_resource(SceneImageTarget(target.clone()));
        Ok(Self {
            app,
            target,
            size: (300, 176),
            revision: 0,
            geometry_revision: 0,
            model_key: None,
        })
    }

    fn update(&mut self, cancelled: &AtomicBool, deadline: Instant) -> Result<(), String> {
        check_request(cancelled, deadline)?;
        self.app.update();
        check_request(cancelled, deadline)?;
        self.app
            .world()
            .resource::<RenderDevice>()
            .wgpu_device()
            .poll(PollType::Wait {
                submission_index: None,
                timeout: Some(
                    deadline
                        .saturating_duration_since(Instant::now())
                        .min(Duration::from_secs(2)),
                ),
            })
            .map_err(|error| format!("Feature preview GPU wait failed: {error}"))?;
        Ok(())
    }

    fn ready_pipeline_count(&self) -> Result<Option<usize>, String> {
        let cache = self
            .app
            .sub_app(RenderApp)
            .world()
            .resource::<PipelineCache>();
        // Shader assets can be temporarily unavailable while loading. Bevy
        // keeps these in the waiting set along with asynchronous compilation.
        if cache.waiting_pipelines().next().is_some() {
            return Ok(None);
        }
        let mut count = 0;
        for pipeline in cache.pipelines() {
            match &pipeline.state {
                CachedPipelineState::Ok(_) => count += 1,
                CachedPipelineState::Err(error) => {
                    return Err(format!("Feature preview pipeline failed: {error}"));
                }
                _ => return Ok(None),
            }
        }
        Ok((count > 0).then_some(count))
    }

    fn prepare_image(&mut self, cancelled: &AtomicBool, deadline: Instant) -> Result<(), String> {
        // Extract newly spawned scene entities/assets before checking their
        // pipelines. A ready cache must survive another complete update, since
        // material preparation can queue additional specialized pipelines.
        self.update(cancelled, deadline)?;
        self.update(cancelled, deadline)?;
        let mut ready_count = None;
        loop {
            let current = self.ready_pipeline_count()?;
            if current.is_some() && current == ready_count {
                return Ok(());
            }
            ready_count = current;
            self.update(cancelled, deadline)?;
        }
    }

    fn render(
        &mut self,
        document: &PreviewDocument,
        request: &RenderRequest,
        cancelled: &AtomicBool,
        deadline: Instant,
    ) -> Result<Vec<u8>, String> {
        check_request(cancelled, deadline)?;
        if self.size != (request.width, request.height) {
            self.app
                .world_mut()
                .resource_mut::<Assets<Image>>()
                .get_mut(&self.target)
                .unwrap()
                .resize(Extent3d {
                    width: request.width,
                    height: request.height,
                    depth_or_array_layers: 1,
                });
            self.size = (request.width, request.height);
        }
        self.revision += 1;
        let revision = self.revision;
        let model_key = (request.preview_id.clone(), request.frame_index);
        let geometry_changed = self.model_key.as_ref() != Some(&model_key);
        if geometry_changed {
            self.geometry_revision += 1;
            self.model_key = Some(model_key);
        }
        let world = self.app.world_mut();
        *world.resource_mut::<CapturedImage>() = CapturedImage {
            revision,
            result: None,
        };
        if geometry_changed {
            *world.resource_mut::<ModelResource>() = ModelResource {
                session_id: "immutable-script-preview".into(),
                geometry_revision: self.geometry_revision,
                revision: self.geometry_revision,
                scene: document.frames[request.frame_index].scene.clone(),
                ..default()
            };
        }
        *world.resource_mut::<CameraResource>() = CameraResource {
            camera: document.camera(request),
            revision: self.revision,
        };
        *world.resource_mut::<ViewportSizeResource>() = ViewportSizeResource {
            logical_width: request.width as f32,
            logical_height: request.height as f32,
        };
        // Metal compiles asynchronously even when the RenderPlugin requests
        // synchronous compilation. Ticks alone cannot prove pixels are ready.
        self.prepare_image(cancelled, deadline)?;
        self.app
            .world_mut()
            .spawn(Screenshot::image(self.target.clone()))
            .observe(
                move |capture: On<ScreenshotCaptured>, mut result: ResMut<CapturedImage>| {
                    if result.revision != revision {
                        return;
                    }
                    result.result = Some(crate::native_viewport::screenshot::png_bytes(&capture.image));
                },
            );
        let mut captured = None;
        crate::native_viewport::screenshot::until_captured(|| {
            self.update(cancelled, deadline)?;
            captured = self
                .app
                .world_mut()
                .resource_mut::<CapturedImage>()
                .result
                .take();
            Ok(captured.is_some())
        })
        .map_err(|_| "Feature preview capture did not complete".to_owned())?;
        check_request(cancelled, deadline)?;
        captured.ok_or_else(|| "Feature preview capture did not complete".to_owned())?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(offset: f32) -> Frame {
        serde_json::from_value(serde_json::json!({"caption":"Immutable stock", "scene":{
            "bodies":[{"id":1,"name":"Stock","feature_id":2,"faces":[],"edges":[],"mesh":{
                "positions":[offset,0,0, offset+60.0,0,0, offset,30,0, offset,0,12],
                "normals":[0,0,1,0,0,1,0,0,1,0,0,1],"indices":[0,2,1,0,1,3,0,3,2,1,2,3]}}],"errors":[]}})).unwrap()
    }

    fn request(preview_id: String, view_id: String, revision: u64) -> RenderRequest {
        RenderRequest {
            preview_id,
            view_id,
            revision,
            frame_index: 0,
            width: 300,
            height: 176,
            yaw: std::f32::consts::FRAC_PI_4,
            pitch: std::f32::consts::PI / 6.0,
        }
    }

    #[test]
    fn immutable_preview_fits_all_frames_and_rejects_invalid_geometry() {
        let frames = vec![frame(0.0), frame(80.0)];
        let before = serde_json::to_value(&frames).unwrap();
        let document = PreviewDocument::new(frames).unwrap();
        let camera = document.camera(&request(String::new(), String::new(), 1));
        assert_eq!(camera.target, [70.0, 15.0, 6.0]);
        assert_eq!(serde_json::to_value(&document.frames).unwrap(), before);
        let distance = Vec3::from_array(camera.position).distance(document.center);
        assert!(
            distance * (camera.vertical_fov_degrees.to_radians() * 0.5).sin() > document.radius
        );
        let mut invalid = frame(0.0);
        invalid.scene.bodies[0].mesh.indices.push(999);
        assert!(PreviewDocument::new(vec![invalid]).is_err());
        let mut invalid = frame(0.0);
        invalid.scene.bodies[0].mesh.normals.clear();
        assert!(PreviewDocument::new(vec![invalid]).is_err());
        let face = |start, count| {
            serde_json::from_value(serde_json::json!({
                "id":1,"key":"face","first_index":start,"index_count":count,"plane":null
            }))
            .unwrap()
        };
        let mut invalid = frame(0.0);
        invalid.scene.bodies[0].faces = vec![face(999, 3)];
        assert!(PreviewDocument::new(vec![invalid]).is_err());
        let mut invalid = frame(0.0);
        invalid.scene.bodies[0].faces = vec![face(0, 12), face(0, 12)];
        assert!(PreviewDocument::new(vec![invalid]).is_err());
        let mut invalid = frame(0.0);
        invalid.scene.bodies = vec![invalid.scene.bodies[0].clone(); 257];
        assert!(PreviewDocument::new(vec![invalid]).is_err());
        let mut oversized = frame(0.0);
        oversized.scene.bodies[0].mesh.indices = vec![0; 300_003];
        assert!(PreviewDocument::new(vec![oversized]).is_err());
        assert!(PreviewDocument::new(vec![frame(0.0); 17]).is_err());
    }

    #[test]
    fn closed_preview_view_rejects_late_work_and_queue_stays_bounded() {
        let service = PreviewService::default();
        let descriptor = service.retain(vec![frame(0.0)]).unwrap();
        // Exercise the actual service admission/cancellation path without a GPU.
        let (worker, jobs) = mpsc::sync_channel(2);
        service.0.lock().unwrap().worker = Some(worker);
        let view = service.open_view().unwrap();
        let first = request(descriptor.preview_id.clone(), view.clone(), 1);
        let _first_reply = service.render(first.clone()).unwrap();
        assert!(service.render(first).is_err());
        let mut next = request(descriptor.preview_id.clone(), view.clone(), 2);
        let _second_reply = service.render(next.clone()).unwrap();
        next.revision = 3;
        assert!(service.render(next.clone()).unwrap_err().contains("busy"));
        let WorkerCommand::Render(stale) = jobs.try_recv().unwrap() else {
            panic!("expected render")
        };
        assert!(stale.cancelled.load(Ordering::Acquire));
        let WorkerCommand::Render(current) = jobs.try_recv().unwrap() else {
            panic!("expected render")
        };
        service.close_view(&view);
        assert!(current.cancelled.load(Ordering::Acquire));
        assert!(service.render(next).unwrap_err().contains("closed"));
        assert!(matches!(jobs.try_recv(), Ok(WorkerCommand::Release)));
        assert!(jobs.try_recv().is_err());
        let replacement = service.open_view().unwrap();
        assert_ne!(replacement, view);
        assert!(service
            .render(request(descriptor.preview_id, replacement, 1))
            .is_ok());
    }

    #[test]
    fn preview_cache_and_render_dimensions_have_explicit_bounds() {
        let service = PreviewService::default();
        let first = service.retain(vec![frame(0.0)]).unwrap();
        for _ in 0..MAX_DOCUMENTS {
            service.retain(vec![frame(0.0)]).unwrap();
        }
        assert_eq!(service.0.lock().unwrap().documents.len(), MAX_DOCUMENTS);
        let view = service.open_view().unwrap();
        assert!(service
            .render(request(first.preview_id, view.clone(), 1))
            .unwrap_err()
            .contains("expired"));
        let mut invalid = request(String::new(), view, 1);
        invalid.width = u32::MAX;
        assert!(invalid.validate().is_err());
        invalid.width = 300;
        invalid.pitch = f32::NAN;
        assert!(invalid.validate().is_err());
        for _ in 1..MAX_VIEWS {
            service.open_view().unwrap();
        }
        assert!(service.open_view().is_err());
    }

    #[test]
    fn abandoned_or_expired_preview_receipt_cancels_queued_work() {
        let (_reply, response) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let pending = PendingRender {
            response,
            cancelled: cancelled.clone(),
            deadline: Instant::now() - Duration::from_millis(1),
        };
        assert!(pending.wait().is_err());
        assert!(cancelled.load(Ordering::Acquire));
        assert!(check_request(&cancelled, Instant::now() + REQUEST_LIFETIME).is_err());
        let cancelled = AtomicBool::new(false);
        assert!(check_request(&cancelled, Instant::now() - Duration::from_millis(1)).is_err());
        assert!(cancelled.load(Ordering::Acquire));
        let (_reply, response) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        drop(PendingRender {
            response,
            cancelled: cancelled.clone(),
            deadline: Instant::now() + REQUEST_LIFETIME,
        });
        assert!(cancelled.load(Ordering::Acquire));
    }

    #[test]
    #[ignore = "requires a GPU; run explicitly when validating the native preview renderer"]
    fn native_preview_gpu_renders_real_kernel_frames_and_orbit() {
        let active = crate::state::AppState::new();
        let unchanged = (
            active.active_project_session_id(),
            serde_json::to_value(active.document_snapshot()).unwrap(),
            serde_json::to_value(active.viewport_snapshot()).unwrap(),
        );
        // Adapter fixture belongs to this layer and needs no bundled recipe.
        let source = r#"{"version":1,"name":"Native preview render fixture","steps":[
          {"call":{"group":"sketch/draw","operation":"sketch_begin","arguments":{"name":"Stock","plane":{"type":"origin_plane","plane":"xy"}}}},
          {"call":{"group":"sketch/draw","operation":"sketch_add_rectangle_locked","arguments":{"mode":"two_point","anchor":{"x":0,"y":0},"corner_hint":{"x":60,"y":30},"width_mm":60,"height_mm":30,"ctrl_held":true}}},
          {"call":{"group":"sketch/draw","operation":"sketch_finish","arguments":{}}},
          {"id":"stock","call":{"group":"solid/build","operation":"solid_extrude","arguments":{"sketch_name":"Stock","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":12},"taper_angle_deg":0,"flip":false,"target_body_ids":[]}}},
          {"id":"tall","call":{"group":"solid/build","operation":"solid_extrude","arguments":{"sketch_name":"Stock","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":36},"taper_angle_deg":0,"flip":false,"target_body_ids":[]}}}
        ],"exports":{"preview_frames":[{"caption":"Stock","scene":{"$ref":"stock","pointer":"/scene"}},{"caption":"Taller stock","scene":{"$ref":"tall","pointer":"/scene"}}]}}"#;
        let mut report = nbcad_mcp::preview_script(source).unwrap();
        let frames: Vec<Frame> =
            serde_json::from_value(report["exports"]["preview_frames"].take()).unwrap();
        let expected = serde_json::to_value(&frames).unwrap();
        let document = PreviewDocument::new(frames).unwrap();
        let mut renderer = PreviewRenderer::new().unwrap();
        let mut request = request(String::new(), String::new(), 1);
        let cancelled = AtomicBool::new(false);
        let deadline = Instant::now() + REQUEST_LIFETIME;
        let stock = renderer
            .render(&document, &request, &cancelled, deadline)
            .unwrap();
        assert_eq!(&stock[..8], b"\x89PNG\r\n\x1a\n");
        request.frame_index = 1;
        let tall = renderer
            .render(&document, &request, &cancelled, deadline)
            .unwrap();
        assert_ne!(
            stock, tall,
            "Real changed geometry must change rendered pixels"
        );
        request.yaw += 0.6;
        let geometry_revision = renderer.geometry_revision;
        let turned = renderer
            .render(&document, &request, &cancelled, deadline)
            .unwrap();
        assert_eq!(
            renderer.geometry_revision, geometry_revision,
            "Orbit keeps native geometry resident"
        );
        assert_ne!(tall, turned, "Orbit must change the native camera image");
        assert_eq!(serde_json::to_value(&document.frames).unwrap(), expected);
        assert_eq!((active.active_project_session_id(), serde_json::to_value(active.document_snapshot()).unwrap(),
            serde_json::to_value(active.viewport_snapshot()).unwrap()), unchanged,
            "Isolated construction and rendering must preserve the active native document/session snapshot");
        assert_eq!(
            renderer
                .app
                .world_mut()
                .query::<&Window>()
                .iter(renderer.app.world())
                .count(),
            0,
            "Preview must not create a native or top-level window"
        );
        cancelled.store(true, Ordering::Release);
        assert!(renderer
            .render(&document, &request, &cancelled, deadline)
            .is_err());
        assert!(renderer.ready_pipeline_count().unwrap().is_some());
        // A real deferred shader pipeline must block capture rather than
        // returning a partially rendered image after an arbitrary tick count.
        renderer
            .app
            .sub_app_mut(RenderApp)
            .world_mut()
            .resource_mut::<PipelineCache>()
            .queue_compute_pipeline(bevy::render::render_resource::ComputePipelineDescriptor {
                label: Some("Delayed preview shader regression".into()),
                ..default()
            });
        let pending = AtomicBool::new(false);
        assert!(renderer
            .prepare_image(&pending, Instant::now() + Duration::from_millis(100))
            .is_err());
        assert!(renderer.ready_pipeline_count().unwrap().is_none());
        if let Some(path) = std::env::var_os("NBCAD_PREVIEW_PROOF_DIR") {
            std::fs::create_dir_all(&path).unwrap();
            for (name, bytes) in [
                ("stock.png", stock),
                ("tall.png", tall),
                ("orbit.png", turned),
            ] {
                std::fs::write(std::path::Path::new(&path).join(name), bytes).unwrap();
            }
        }
    }

    /// Real production GPU path for model-edge strokes on an inside corner: the
    /// two faces beside a concave edge both rise towards the camera, so a stroke
    /// that only wins exact depth ties loses most of its width to them. The
    /// probe compares renders with and without edges along a concave edge and a
    /// convex one of the same plate. Kept opt-in for GPU-less hosts; set
    /// `NBCAD_PREVIEW_PROOF_DIR` to retain the rendered evidence.
    #[test]
    #[ignore = "requires a GPU; set NBCAD_PREVIEW_PROOF_DIR to retain visual evidence"]
    fn native_concave_edge_strokes_read_like_convex_ones() {
        use nbcad_core::{OriginPlane, PlaneRef};
        use nbcad_sketch::{
            RectangleMode, RectangleRequest, SegmentRequest, SetGridSnapRequest, SketchManager,
            Vec2 as P,
        };
        use nbcad_solid::{CommitKernelRequest, ExtrudeExtent, ExtrudeOperation, ExtrudeRequest};
        let extrude = |manager: &mut SketchManager,
                       kernel: &mut nbcad_occt::OcctKernel,
                       sketch: &str,
                       operation: ExtrudeOperation,
                       distance: f64,
                       flip: bool| {
            let targets = manager
                .solid_scene()
                .bodies
                .iter()
                .map(|body| body.id)
                .collect::<Vec<_>>();
            let plan = manager
                .prepare_extrude(ExtrudeRequest {
                    source_face: None,
                    sketch_name: sketch.into(),
                    profile_indices: vec![0],
                    operation,
                    extent: ExtrudeExtent::Distance { distance },
                    taper_angle_deg: 0.0,
                    flip,
                    target_body_ids: if operation == ExtrudeOperation::NewBody {
                        vec![]
                    } else {
                        targets
                    },
                })
                .unwrap();
            let scene = kernel.recompute(&plan).unwrap();
            manager
                .commit_solid(CommitKernelRequest {
                    transaction_id: plan.transaction_id,
                    scene,
                })
                .unwrap();
        };
        let begin = |manager: &mut SketchManager| {
            manager
                .begin_sketch(PlaneRef::OriginPlane {
                    plane: OriginPlane::Xy,
                })
                .unwrap();
            manager
                .set_grid_snap(SetGridSnapRequest { enabled: false })
                .unwrap();
        };
        let rectangle = |manager: &mut SketchManager, p1: P, p2: P| {
            manager
                .add_rectangle(RectangleRequest {
                    mode: RectangleMode::TwoPoint,
                    p1,
                    p2,
                    ctrl_held: true,
                })
                .unwrap();
        };

        // The reported part: a 25 mm square plate with a 5 x 20 mm strip
        // removed, leaving a 5 mm arm along the back and an inside corner at
        // (20, 20). Five millimetres thick.
        let mut manager = SketchManager::new();
        let mut kernel = nbcad_occt::OcctKernel::new().unwrap();
        begin(&mut manager);
        let outline = [
            P::new(0.0, 0.0),
            P::new(0.0, 25.0),
            P::new(25.0, 25.0),
            P::new(25.0, 20.0),
            P::new(20.0, 20.0),
            P::new(20.0, 0.0),
        ];
        for index in 0..outline.len() {
            manager
                .add_line(SegmentRequest {
                    from: outline[index],
                    to_raw: outline[(index + 1) % outline.len()],
                    ctrl_held: true,
                })
                .unwrap();
        }
        manager.end_sketch().unwrap();
        extrude(
            &mut manager,
            &mut kernel,
            "Sketch1",
            ExtrudeOperation::NewBody,
            5.0,
            false,
        );
        let plate = manager.solid_scene();

        // A 30 mm square block, 8 mm thick, with a 16 mm square pocket 4 mm
        // deep cut from its top face: every floor edge is an inside corner
        // between the floor and a wall. The face sketch's cut direction is
        // found by trying both, since only one removes material.
        let pocket = [false, true]
            .into_iter()
            .find_map(|flip| {
                let mut manager = SketchManager::new();
                let mut kernel = nbcad_occt::OcctKernel::new().unwrap();
                begin(&mut manager);
                rectangle(&mut manager, P::new(0.0, 0.0), P::new(30.0, 30.0));
                manager.end_sketch().unwrap();
                extrude(
                    &mut manager,
                    &mut kernel,
                    "Sketch1",
                    ExtrudeOperation::NewBody,
                    8.0,
                    false,
                );
                let top = manager.solid_scene().bodies[0]
                    .faces
                    .iter()
                    .find(|face| face.plane.is_some_and(|plane| plane.normal[2] > 0.9))
                    .map(|face| face.id)
                    .expect("the block has a top face");
                manager
                    .begin_sketch(PlaneRef::PlanarFace { face_id: top })
                    .unwrap();
                manager
                    .set_grid_snap(SetGridSnapRequest { enabled: false })
                    .unwrap();
                rectangle(&mut manager, P::new(-8.0, -8.0), P::new(8.0, 8.0));
                manager.end_sketch().unwrap();
                extrude(
                    &mut manager,
                    &mut kernel,
                    "Sketch2",
                    ExtrudeOperation::Cut,
                    4.0,
                    flip,
                );
                let scene = manager.solid_scene();
                (scene.bodies.len() == 1 && scene.bodies[0].faces.len() == 11).then_some(scene)
            })
            .expect("one cut direction hollows the pocket: 6 block faces, 4 walls, 1 floor");

        let mut renderer = PreviewRenderer::new().unwrap();
        let cancelled = AtomicBool::new(false);
        let output = std::env::var_os("NBCAD_PREVIEW_PROOF_DIR");
        if let Some(path) = &output {
            std::fs::create_dir_all(path).unwrap();
        }
        // Seen from the front right and above, like the report: both faces of
        // each inside corner and of its convex reference edge are visible.
        let cases = [
            (
                "inside-corner",
                plate,
                [20.0, 20.0, 0.0, 5.0],
                [20.0, 0.0, 0.0, 5.0],
            ),
            // Far floor edge of the pocket against the back top rim above it.
            (
                "pocket-floor",
                pocket,
                [7.0, 23.0, 4.0, 23.0],
                [0.0, 30.0, 8.0, 30.0],
            ),
        ];
        for (label, scene, concave_edge, convex_edge) in cases {
            let mut without_edges = scene.clone();
            without_edges.bodies[0].edges.clear();
            let document = PreviewDocument::new(vec![
                Frame {
                    caption: format!("{label} with edges"),
                    scene,
                },
                Frame {
                    caption: format!("{label} without edges"),
                    scene: without_edges,
                },
            ])
            .unwrap();
            let mut request = request(label.into(), String::new(), 1);
            request.width = 800;
            request.height = 600;
            request.yaw = std::f32::consts::FRAC_PI_4 * 0.75;
            request.pitch = 0.55;
            let mut renders = Vec::new();
            for frame_index in 0..2 {
                request.frame_index = frame_index;
                let png = renderer
                    .render(
                        &document,
                        &request,
                        &cancelled,
                        Instant::now() + Duration::from_secs(60),
                    )
                    .unwrap();
                if let Some(path) = &output {
                    std::fs::write(
                        std::path::Path::new(path).join(format!(
                            "{label}-{}.png",
                            if frame_index == 0 { "with" } else { "without" }
                        )),
                        &png,
                    )
                    .unwrap();
                }
                renders.push(decode_rgba(&png));
            }
            let (width, height, with_edges) = &renders[0];
            let (_, _, without_edges) = &renders[1];
            let camera = document.camera(&request);
            let view = camera_transform(camera).to_matrix().inverse();
            let projection = Mat4::perspective_infinite_reverse_rh(
                camera.vertical_fov_degrees.to_radians(),
                *width as f32 / *height as f32,
                0.1,
            );
            let to_pixel = |point: Vec3| {
                let clip = projection * view * point.extend(1.0);
                let ndc = clip.truncate() / clip.w;
                (
                    (ndc.x + 1.0) * 0.5 * *width as f32,
                    (1.0 - ndc.y) * 0.5 * *height as f32,
                )
            };
            // Strongest change any pixel within two pixels of the true edge shows
            // once strokes are drawn, averaged along the edge.
            let stroke_strength = |from: Vec3, to: Vec3| {
                let samples = 9;
                (1..=samples)
                    .map(|index| {
                        let (x, y) = to_pixel(from.lerp(to, index as f32 / (samples + 1) as f32));
                        let mut strongest = 0i32;
                        for dy in -2..=2 {
                            for dx in -2..=2 {
                                let (px, py) = (x.round() as i32 + dx, y.round() as i32 + dy);
                                if px < 0 || py < 0 || px >= *width as i32 || py >= *height as i32 {
                                    continue;
                                }
                                let offset = ((py as u32 * width + px as u32) * 4) as usize;
                                let change = (0..3)
                                    .map(|channel| {
                                        (i32::from(with_edges[offset + channel])
                                            - i32::from(without_edges[offset + channel]))
                                        .abs()
                                    })
                                    .max()
                                    .unwrap_or(0);
                                strongest = strongest.max(change);
                            }
                        }
                        strongest as f32
                    })
                    .sum::<f32>()
                    / samples as f32
            };
            // Edges run along one axis between the given coordinates: an axis
            // pair plus a fixed pair, the varying axis being the one that differs.
            let segment = |edge: [f32; 4]| {
                let (a, b) = if label == "inside-corner" {
                    (
                        Vec3::new(edge[0], edge[1], edge[2]),
                        Vec3::new(edge[0], edge[1], edge[3]),
                    )
                } else {
                    (
                        Vec3::new(edge[0], edge[1], edge[2]),
                        Vec3::new(edge[3], edge[1], edge[2]),
                    )
                };
                (a, b)
            };
            let (concave_from, concave_to) = segment(concave_edge);
            let (convex_from, convex_to) = segment(convex_edge);
            let concave = stroke_strength(concave_from, concave_to);
            let convex = stroke_strength(convex_from, convex_to);
            eprintln!("{label} stroke strength: concave {concave:.1}, convex {convex:.1}");
            assert!(
                convex > 40.0,
                "{label}: the convex reference edge must be a clear stroke ({convex:.1})"
            );
            assert!(
                concave >= convex * 0.8,
                "{label}: the inside-corner stroke ({concave:.1}) must read like the convex one ({convex:.1})"
            );
        }
    }

    fn decode_rgba(png: &[u8]) -> (u32, u32, Vec<u8>) {
        use bevy::asset::RenderAssetUsages;
        use bevy::image::{CompressedImageFormats, ImageSampler, ImageType};
        let image = Image::from_buffer(
            png,
            ImageType::Extension("png"),
            CompressedImageFormats::NONE,
            true,
            ImageSampler::Default,
            RenderAssetUsages::MAIN_WORLD,
        )
        .unwrap();
        let size = image.texture_descriptor.size;
        let data = image.data.expect("decoded screenshot has pixels");
        assert_eq!(
            data.len(),
            (size.width * size.height * 4) as usize,
            "RGBA8 screenshot"
        );
        (size.width, size.height, data)
    }

    /// Real production GPU path for grid stability: zoom out two decades in
    /// 6 % steps over an empty sheet and follow the brightness of two fixed
    /// world lines. Every line's colour must move smoothly as the drawn
    /// lattices change, where the earlier sheet swapped its major lines at
    /// each 1-2-5 step. Kept opt-in for GPU-less hosts; set
    /// `NBCAD_PREVIEW_PROOF_DIR` to retain every frame.
    #[test]
    #[ignore = "requires a GPU; set NBCAD_PREVIEW_PROOF_DIR to retain visual evidence"]
    fn native_ground_grid_zoom_is_continuous() {
        // A speck of a body so the grid is the only thing that moves.
        let speck: Frame = serde_json::from_value(serde_json::json!({
            "caption": "speck",
            "scene": {"bodies": [{"id": 1, "name": "Speck", "feature_id": 2, "faces": [],
                "edges": [], "mesh": {"positions": [0.0, 0.0, 0.0, 0.001, 0.0, 0.0, 0.0, 0.001, 0.0],
                "normals": [0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0], "indices": [0, 1, 2]}}],
                "errors": []}
        }))
        .unwrap();
        let mut document = PreviewDocument::new(vec![speck]).unwrap();
        document.center = Vec3::ZERO;
        let mut renderer = PreviewRenderer::new().unwrap();
        let cancelled = AtomicBool::new(false);
        let output = std::env::var_os("NBCAD_PREVIEW_PROOF_DIR");
        if let Some(path) = &output {
            std::fs::create_dir_all(path).unwrap();
        }
        let mut request = request("grid-zoom".into(), String::new(), 1);
        request.width = 640;
        request.height = 400;
        request.pitch = 0.6;
        let viewport = ViewportSizeResource {
            logical_width: 640.0,
            logical_height: 400.0,
        };
        let [(_, ground, _), ..] = origin_plane_bases();
        // Brightest pixel within a pixel of each sample along a world line,
        // averaged: the line's brightness in this frame.
        let line_brightness = |pixels: &[u8], camera: ViewportCamera, x: f32| {
            let view = camera_transform(camera).to_matrix().inverse();
            let projection = Mat4::perspective_infinite_reverse_rh(
                camera.vertical_fov_degrees.to_radians(),
                640.0 / 400.0,
                0.1,
            );
            let samples = 9;
            (0..samples)
                .map(|sample| {
                    let y = (sample as f32 / (samples - 1) as f32 - 0.5) * 12.0;
                    let clip = projection * view * Vec3::new(x, y, 0.0).extend(1.0);
                    let ndc = clip.truncate() / clip.w;
                    let (px, py) = (
                        ((ndc.x + 1.0) * 0.5 * 640.0).round() as i32,
                        ((1.0 - ndc.y) * 0.5 * 400.0).round() as i32,
                    );
                    let mut brightest = 0u8;
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let (sx, sy) = (px + dx, py + dy);
                            if sx < 0 || sy < 0 || sx >= 640 || sy >= 400 {
                                continue;
                            }
                            let offset = ((sy * 640 + sx) * 4) as usize;
                            brightest = brightest
                                .max(pixels[offset..offset + 3].iter().copied().max().unwrap());
                        }
                    }
                    f32::from(brightest)
                })
                .sum::<f32>()
                / samples as f32
        };
        let mut previous: Option<[f32; 2]> = None;
        let mut largest_jump = 0.0f32;
        let mut finest_changes = 0;
        let mut last_finest = None;
        for step in 0..80 {
            document.radius = 30.0 * 1.06f32.powi(step);
            let camera = document.camera(&request);
            let layout = grid_layout(camera, viewport, &ground);
            let png = renderer
                .render(
                    &document,
                    &request,
                    &cancelled,
                    Instant::now() + Duration::from_secs(60),
                )
                .unwrap();
            if let Some(path) = &output {
                std::fs::write(
                    std::path::Path::new(path)
                        .join(format!("grid-zoom-{step:03}-finest{}.png", layout.finest)),
                    &png,
                )
                .unwrap();
            }
            let (_, _, pixels) = decode_rgba(&png);
            let brightness = [
                line_brightness(&pixels, camera, 20.0),
                line_brightness(&pixels, camera, 50.0),
            ];
            if last_finest.is_some_and(|finest| finest != layout.finest) {
                finest_changes += 1;
            }
            last_finest = Some(layout.finest);
            if let Some(previous) = previous {
                for (line, (now, before)) in brightness.iter().zip(previous).enumerate() {
                    let jump = (now - before).abs();
                    largest_jump = largest_jump.max(jump);
                    assert!(
                        jump <= 10.0,
                        "line {} jumped {before:.1} -> {now:.1} at step {step} (finest {} mm)",
                        [20, 50][line],
                        layout.finest
                    );
                }
            }
            eprintln!(
                "grid zoom step {step:>2} radius {:>7.1} finest {:>5} mm: 20 mm line {:>5.1}, 50 mm line {:>5.1}",
                document.radius, layout.finest, brightness[0], brightness[1]
            );
            previous = Some(brightness);
        }
        assert!(
            finest_changes >= 5,
            "the sweep must cross several lattice changes, crossed {finest_changes}"
        );
        eprintln!("largest frame-to-frame line brightness change: {largest_jump:.1}");
    }

    /// Real production GPU path for the ground grid: a small part and one two
    /// hundred times larger must both sit on a legible sheet that follows the
    /// zoom. Kept opt-in for GPU-less hosts; set `NBCAD_PREVIEW_PROOF_DIR` to
    /// retain the rendered evidence.
    #[test]
    #[ignore = "requires a GPU; set NBCAD_PREVIEW_PROOF_DIR to retain visual evidence"]
    fn native_ground_grid_visual_matrix() {
        let block = |size: f32| -> Frame {
            let (x, y, z) = (size, size * 0.6, size * 0.2);
            let corners: [[f32; 3]; 8] = [
                [0.0, 0.0, 0.0],
                [x, 0.0, 0.0],
                [x, y, 0.0],
                [0.0, y, 0.0],
                [0.0, 0.0, z],
                [x, 0.0, z],
                [x, y, z],
                [0.0, y, z],
            ];
            let positions: Vec<f32> = corners.iter().flatten().copied().collect();
            let normals: Vec<f32> = corners.iter().flat_map(|_| [0.0, 0.0, 1.0]).collect();
            let indices: Vec<u32> = vec![
                0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2,
                7, 6, 3, 0, 4, 3, 4, 7,
            ];
            serde_json::from_value(serde_json::json!({
                "caption": format!("{size} mm block"),
                "scene": {"bodies": [{"id": 1, "name": "Block", "feature_id": 2, "faces": [],
                    "edges": [], "mesh": {"positions": positions, "normals": normals,
                    "indices": indices}}], "errors": []}
            }))
            .unwrap()
        };
        let mut renderer = PreviewRenderer::new().unwrap();
        let cancelled = AtomicBool::new(false);
        let output = std::env::var_os("NBCAD_PREVIEW_PROOF_DIR");
        if let Some(path) = &output {
            std::fs::create_dir_all(path).unwrap();
        }
        for (label, size) in [("small", 20.0), ("large", 4_000.0)] {
            let mut document = PreviewDocument::new(vec![block(size)]).unwrap();
            let base_radius = document.radius;
            let mut request = request(format!("grid-{label}"), String::new(), 1);
            request.width = 800;
            request.height = 600;
            for (zoom_label, zoom, pitch) in [
                ("fit", 1.0, 0.6),
                ("far", 4.0, 0.6),
                ("close", 0.25, 0.6),
                ("grazing", 1.0, 0.12),
            ] {
                document.radius = base_radius * zoom;
                request.pitch = pitch;
                let png = renderer
                    .render(
                        &document,
                        &request,
                        &cancelled,
                        Instant::now() + Duration::from_secs(60),
                    )
                    .unwrap();
                assert!(
                    png.len() > 5_000,
                    "a scene must be rendered, not an empty target"
                );
                if let Some(path) = &output {
                    std::fs::write(
                        std::path::Path::new(path).join(format!("grid-{label}-{zoom_label}.png")),
                        png,
                    )
                    .unwrap();
                }
            }
        }
    }

    /// Real production GPU path: projected partial arcs on both face normals,
    /// thin solids, multiple zooms and palettes. Kept opt-in for GPU-less hosts.
    #[test]
    #[ignore = "requires a GPU; set NBCAD_PREVIEW_PROOF_DIR to retain visual evidence"]
    fn native_sketch_boundary_visual_matrix() {
        use nbcad_core::{OriginPlane, PlaneRef};
        use nbcad_sketch::{
            ArcCenterRequest, SegmentRequest, SetGridSnapRequest, SketchManager, Vec2 as P,
        };
        use nbcad_solid::{CommitKernelRequest, ExtrudeExtent, ExtrudeOperation, ExtrudeRequest};
        let mut manager = SketchManager::new();
        manager
            .begin_sketch(PlaneRef::OriginPlane {
                plane: OriginPlane::Xy,
            })
            .unwrap();
        manager
            .set_grid_snap(SetGridSnapRequest { enabled: false })
            .unwrap();
        manager
            .add_arc_center(ArcCenterRequest {
                center: P::ZERO,
                start: P::new(25.0, 0.0),
                sweep: P::new(0.0, -25.0),
                sweep_rad: Some(1.5 * std::f64::consts::PI),
                ctrl_held: true,
                radius_mm: None,
                radius_text: None,
                angle_text: None,
            })
            .unwrap();
        manager
            .add_line(SegmentRequest {
                from: P::new(0.0, -25.0),
                to_raw: P::new(25.0, 0.0),
                ctrl_held: true,
            })
            .unwrap();
        manager.end_sketch().unwrap();
        let plan = manager
            .prepare_extrude(ExtrudeRequest {
                source_face: None,
                sketch_name: "Sketch1".into(),
                profile_indices: vec![0],
                operation: ExtrudeOperation::NewBody,
                extent: ExtrudeExtent::Distance { distance: 0.15 },
                taper_angle_deg: 0.0,
                flip: false,
                target_body_ids: vec![],
            })
            .unwrap();
        let mut kernel = nbcad_occt::OcctKernel::new().unwrap();
        let scene = kernel.recompute(&plan).unwrap();
        manager
            .commit_solid(CommitKernelRequest {
                transaction_id: plan.transaction_id,
                scene,
            })
            .unwrap();
        // A second, smaller solid lies fully behind the thin plate. Its edges
        // are a deterministic occlusion probe, not just a visual impression.
        manager
            .begin_sketch(PlaneRef::OriginPlane {
                plane: OriginPlane::Xy,
            })
            .unwrap();
        manager
            .add_rectangle(nbcad_sketch::RectangleRequest {
                mode: nbcad_sketch::RectangleMode::TwoPoint,
                p1: P::new(-8.0, 2.0),
                p2: P::new(-2.0, 8.0),
                ctrl_held: true,
            })
            .unwrap();
        manager.end_sketch().unwrap();
        let plan = manager
            .prepare_extrude(ExtrudeRequest {
                source_face: None,
                sketch_name: "Sketch2".into(),
                profile_indices: vec![0],
                operation: ExtrudeOperation::NewBody,
                extent: ExtrudeExtent::Distance { distance: 0.01 },
                taper_angle_deg: 0.0,
                flip: false,
                target_body_ids: vec![],
            })
            .unwrap();
        let scene = kernel.recompute(&plan).unwrap();
        manager
            .commit_solid(CommitKernelRequest {
                transaction_id: plan.transaction_id,
                scene,
            })
            .unwrap();
        let scene = manager.solid_scene();
        let mut renderer = PreviewRenderer::new().unwrap();
        let cancelled = AtomicBool::new(false);
        let output = std::env::var_os("NBCAD_PREVIEW_PROOF_DIR");
        if let Some(path) = &output {
            std::fs::create_dir_all(path).unwrap();
        }
        for bottom in [false, true] {
            let face = scene.bodies[0]
                .faces
                .iter()
                .find(|f| {
                    f.plane.is_some_and(|p| {
                        if bottom {
                            p.normal[2] < -0.9
                        } else {
                            p.normal[2] > 0.9
                        }
                    })
                })
                .unwrap();
            let sketch = manager
                .begin_sketch(PlaneRef::PlanarFace { face_id: face.id })
                .unwrap();
            assert!(sketch
                .projected_edges
                .iter()
                .any(|edge| edge.circle.is_some() && edge.points.len() > 3));
            let mut document = PreviewDocument::new(vec![Frame {
                caption: "Thin partial circular plate".into(),
                scene: scene.clone(),
            }])
            .unwrap();
            let base_radius = document.radius;
            let mut request = request(format!("boundary-{bottom}"), String::new(), 1);
            request.width = 800;
            request.height = 600;
            request.pitch = if bottom { -1.15 } else { 1.15 };
            renderer
                .render(
                    &document,
                    &request,
                    &cancelled,
                    Instant::now() + Duration::from_secs(60),
                )
                .unwrap();
            {
                let world = renderer.app.world_mut();
                let mut model = world.resource_mut::<ModelResource>();
                model.active_sketch = Some(sketch);
                model.geometry_revision += 1;
                model.revision += 1;
                world.resource_mut::<PresentationResource>().0.mode = ViewportMode::Sketch;
            }
            for light in [false, true] {
                let mut palette = ViewportPalette::default();
                if light {
                    palette.background = [0.95, 0.96, 0.98];
                    palette.grid_fine = [0.82, 0.83, 0.85];
                    palette.grid_major = [0.70, 0.71, 0.73];
                    palette.projected = [0.47, 0.19, 0.72];
                }
                renderer.app.world_mut().resource_mut::<PaletteResource>().0 = palette;
                *renderer.app.world_mut().resource_mut::<ClearColor>() =
                    ClearColor(rgb(palette.background));
                for (label, zoom, pitch) in [
                    ("face", 1.0, 1.15),
                    ("close", 0.7, 1.15),
                    ("grazing", 1.0, 0.08),
                ] {
                    document.radius = base_radius * zoom;
                    request.pitch = if bottom { -pitch } else { pitch };
                    let png = renderer
                        .render(
                            &document,
                            &request,
                            &cancelled,
                            Instant::now() + Duration::from_secs(60),
                        )
                        .unwrap();
                    assert!(
                        png.len() > 5_000,
                        "a scene must be rendered, not an empty target"
                    );
                    let name = format!(
                        "boundary-{}-{}-{label}.png",
                        if bottom { "bottom" } else { "top" },
                        if light { "light" } else { "dark" }
                    );
                    if let Some(path) = &output {
                        std::fs::write(std::path::Path::new(path).join(name), png).unwrap();
                    }
                }
            }
            manager.end_sketch().unwrap();
        }
        *renderer
            .app
            .world_mut()
            .resource_mut::<PresentationResource>() = PresentationResource::default();
        let mut without_hidden_edges = scene.clone();
        without_hidden_edges.bodies[1].edges.clear();
        let mut document = PreviewDocument::new(vec![
            Frame {
                caption: "Hidden edges present".into(),
                scene,
            },
            Frame {
                caption: "Hidden edges removed".into(),
                scene: without_hidden_edges,
            },
        ])
        .unwrap();
        let radius = document.radius;
        let mut request = request("occlusion".into(), String::new(), 1);
        request.width = 800;
        request.height = 600;
        request.pitch = 1.3;
        for zoom in [0.7, 1.0, 2.5] {
            document.radius = radius * zoom;
            request.frame_index = 0;
            let with_edges = renderer
                .render(
                    &document,
                    &request,
                    &cancelled,
                    Instant::now() + Duration::from_secs(60),
                )
                .unwrap();
            request.frame_index = 1;
            let without_edges = renderer
                .render(
                    &document,
                    &request,
                    &cancelled,
                    Instant::now() + Duration::from_secs(60),
                )
                .unwrap();
            if let Some(path) = &output {
                std::fs::write(
                    std::path::Path::new(path).join(format!("occlusion-{zoom}-with.png")),
                    &with_edges,
                )
                .unwrap();
                std::fs::write(
                    std::path::Path::new(path).join(format!("occlusion-{zoom}-without.png")),
                    &without_edges,
                )
                .unwrap();
            }
            assert!(
                with_edges == without_edges,
                "hidden body edges leaked through the 0.15 mm plate at zoom {zoom}"
            );
        }
    }
}
