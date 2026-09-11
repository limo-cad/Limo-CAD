//! Immutable teaching snapshots rendered by the production CAD scene systems.
//! One windowless worker owns its Bevy world. It sleeps while idle and has no
//! AppState, publisher, native child view, or access to the live camera/picker.
use super::super::DEFAULT_VERTICAL_FOV_DEGREES;
use super::*;
use bevy::{
    image::ImageFormat,
    render::{
        pipelined_rendering::PipelinedRenderingPlugin,
        render_resource::{
            CachedPipelineState, Extent3d, PipelineCache, PollType, TextureDimension,
            TextureFormat, TextureUsages,
        },
        renderer::RenderDevice,
        view::screenshot::{Screenshot, ScreenshotCaptured},
        RenderApp,
    },
};
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, io::Cursor, sync::mpsc, time::Duration};

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
                    result.result = Some((|| {
                        let image = capture
                            .image
                            .clone()
                            .try_into_dynamic()
                            .map_err(|error| error.to_string())?;
                        let mut png = Cursor::new(Vec::new());
                        image
                            .write_to(&mut png, ImageFormat::Png.as_image_crate_format().unwrap())
                            .map_err(|error| error.to_string())?;
                        Ok(png.into_inner())
                    })());
                },
            );
        for _ in 0..12 {
            self.update(cancelled, deadline)?;
            if let Some(result) = self
                .app
                .world_mut()
                .resource_mut::<CapturedImage>()
                .result
                .take()
            {
                check_request(cancelled, deadline)?;
                return result;
            }
        }
        Err("Feature preview capture did not complete".into())
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
}
