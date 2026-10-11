//! Independent CAD rendering over the interface's GPU device and queue.
//!
//! The interface owns input, document state, picking and all UI. A windowless
//! worker receives immutable scene snapshots and owns the existing CAD renderer.
//! Only GPU texture handles cross back; no pixels or geometry are read back.
//! Its compilation queue cannot hold up a menu frame or a Winit input update.

use super::*;
use bevy::{
    app::AppExit,
    asset::AssetId,
    render::{
        render_asset::{prepare_assets, RenderAssets},
        render_resource::{
            Extent3d, PipelineCache, PollType, TextureDimension, TextureFormat, TextureUsages,
        },
        renderer::{RenderAdapter, RenderAdapterInfo, RenderDevice, RenderInstance, RenderQueue},
        settings::RenderResources,
        texture::GpuImage,
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
    },
    ui::UiSystems,
    window::{ExitCondition, PrimaryWindow, WindowPlugin},
};
use std::{
    panic::AssertUnwindSafe,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::JoinHandle,
};

#[derive(Clone, PartialEq)]
struct Stamp {
    changed: [u32; 8],
    size: UVec2,
    scale: f32,
}

impl Stamp {
    fn capture(world: &World) -> Option<Self> {
        let surface = world.resource::<HostSurface>();
        let window = world.get::<Window>(surface.window)?;
        let size = world.resource::<ViewportSizeResource>();
        let scale = window.scale_factor();
        Some(Self {
            changed: [
                world
                    .get_resource_ref::<ModelResource>()?
                    .last_changed()
                    .get(),
                world
                    .get_resource_ref::<CameraResource>()?
                    .last_changed()
                    .get(),
                world
                    .get_resource_ref::<PreviewResource>()?
                    .last_changed()
                    .get(),
                world
                    .get_resource_ref::<CamStockResource>()?
                    .last_changed()
                    .get(),
                world
                    .get_resource_ref::<PaletteResource>()?
                    .last_changed()
                    .get(),
                world
                    .get_resource_ref::<PresentationResource>()?
                    .last_changed()
                    .get(),
                world
                    .get_resource_ref::<section_view::State>()?
                    .last_changed()
                    .get(),
                world
                    .get_resource_ref::<DocumentGeometryIndex>()?
                    .last_changed()
                    .get(),
            ],
            size: UVec2::new(
                (size.logical_width * scale).round().max(1.) as u32,
                (size.logical_height * scale).round().max(1.) as u32,
            ),
            scale,
        })
    }
}

struct Snapshot {
    stamp: Stamp,
    owner: String,
    model: Option<ModelResource>,
    edge_cache: Option<ModelEdgeCache>,
    camera: Option<CameraResource>,
    preview: Option<PreviewResource>,
    stock: Option<CamStockResource>,
    palette: Option<PaletteResource>,
    presentation: Option<PresentationResource>,
    section: Option<section_view::State>,
    viewport: ViewportSizeResource,
    retained_sessions: Option<Vec<String>>,
}

impl Snapshot {
    fn capture(world: &World, stamp: Stamp, previous: Option<&Stamp>) -> Self {
        // One request can be outstanding. Its completion precedes the next
        // snapshot, so unchanged state remains resident in the worker. Orbit
        // and resize never copy document metadata or presentation collections.
        let changed =
            |index| previous.is_none_or(|previous| previous.changed[index] != stamp.changed[index]);
        let model = world.resource::<ModelResource>();
        Self {
            owner: model.session_id.clone(),
            model: changed(0).then(|| ModelResource {
                session_id: model.session_id.clone(),
                geometry_revision: model.geometry_revision,
                document: model.document.clone(),
                body_poses: model.body_poses.clone(),
                instance_body_poses: model.instance_body_poses.clone(),
                instance_revision: model.instance_revision,
                revision: model.revision,
                transient_model: model.transient_model,
                ..default()
            }),
            camera: changed(1).then(|| *world.resource::<CameraResource>()),
            edge_cache: changed(0)
                .then(|| {
                    model
                        .cache_entity
                        .and_then(|entity| world.get::<ModelEdgeCache>(entity))
                        .cloned()
                })
                .flatten(),
            preview: changed(2).then(|| world.resource::<PreviewResource>().clone()),
            stock: changed(3).then(|| world.resource::<CamStockResource>().clone()),
            palette: changed(4).then(|| *world.resource::<PaletteResource>()),
            presentation: changed(5).then(|| world.resource::<PresentationResource>().clone()),
            section: changed(6).then(|| world.resource::<section_view::State>().clone()),
            viewport: *world.resource::<ViewportSizeResource>(),
            retained_sessions: changed(7).then(|| {
                world
                    .resource::<DocumentGeometryIndex>()
                    .0
                    .keys()
                    .cloned()
                    .collect()
            }),
            stamp,
        }
    }
}

struct Frame {
    stamp: Stamp,
    owner: String,
    image: GpuImage,
    lease: Arc<()>,
}

struct Target {
    image: Handle<Image>,
    lease: std::sync::Weak<()>,
}

/// A full rendering receipt requires the matching viewport texture as well as
/// the UI. Menus can already be displayed and receive input while this is false.
#[derive(Resource, Default)]
pub(crate) struct ViewportReady(pub bool);

#[derive(Resource)]
struct HostSurface {
    window: Entity,
    image: Handle<Image>,
    node: Option<Entity>,
    displayed: Option<Frame>,
}

#[derive(Resource)]
struct Worker {
    requests: Option<mpsc::SyncSender<Snapshot>>,
    frames: Mutex<Option<mpsc::Receiver<Result<Frame, String>>>>,
    thread: Option<JoinHandle<()>>,
    sent: Option<Stamp>,
    in_flight: bool,
    stopped: Arc<AtomicBool>,
    #[cfg(test)]
    gate_release: Option<mpsc::SyncSender<()>>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Close the request channel before joining. The worker drops its GPU
        // world on its owning thread; no non-send window data lives there.
        self.requests.take();
        self.stopped.store(true, Ordering::Release);
        #[cfg(test)]
        if let Some(release) = &self.gate_release {
            let _ = release.try_send(());
        }
        self.frames.get_mut().unwrap().take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct ViewportRendererPlugin;

pub(crate) fn interface_plugins() -> bevy::app::PluginGroupBuilder {
    DefaultPlugins
        .build()
        .set(cad_render_plugin())
        .disable::<bevy::pbr::PbrPlugin>()
        .disable::<bevy::gizmos_render::GizmoRenderPlugin>()
}

pub(crate) fn install(app: &mut App) {
    init_cad_state(app);
    app.insert_resource(SharedPickState(Arc::new(Mutex::new(PickState::default()))))
        .init_resource::<ViewportReady>()
        .add_systems(
            Startup,
            (ui::load_system_font, setup_interface_canvas).chain(),
        )
        .add_systems(
            Update,
            (
                rebuild_native_annotations,
                rebuild_native_hud,
                update_native_hud_orientation,
            )
                .chain()
                .after(interface_shell::InterfaceReduction),
        )
        .add_systems(PostUpdate, exchange_frames.before(UiSystems::Layout))
        .add_plugins(ViewportRendererPlugin);
}

impl Plugin for ViewportRendererPlugin {
    fn build(&self, app: &mut App) {
        let render = app.sub_app_mut(RenderApp);
        render
            .add_systems(ExtractSchedule, extract_texture)
            .add_systems(
                Render,
                prepare_texture
                    .in_set(RenderSystems::PrepareAssets)
                    .after(prepare_assets::<GpuImage>),
            );
    }

    fn finish(&self, app: &mut App) {
        let render = app.sub_app(RenderApp).world();
        let resources = RenderResources(
            render.resource::<RenderDevice>().clone(),
            render.resource::<RenderQueue>().clone(),
            render.resource::<RenderAdapterInfo>().clone(),
            render.resource::<RenderAdapter>().clone(),
            render.resource::<RenderInstance>().clone(),
        );
        let handle = app.world().resource::<NativeInterfaceHandle>().clone();
        let (requests, jobs) = mpsc::sync_channel(1);
        let (replies, frames) = mpsc::sync_channel(1);
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stopped = stopped.clone();
        #[cfg(test)]
        let gate = app.world_mut().remove_resource::<tests::CompilationGate>();
        #[cfg(test)]
        let gate_release = gate.as_ref().map(|gate| gate.release.clone());
        let thread = std::thread::Builder::new()
            .name("CAD viewport".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    #[cfg(test)]
                    if let Some(gate) = gate {
                        if gate.receiver.lock().unwrap().recv().is_err() {
                            return Ok(());
                        }
                    }
                    render_worker(resources, jobs, &replies, &handle, &worker_stopped)
                }));
                let error = match result {
                    Ok(Ok(())) => return,
                    Ok(Err(error)) => error,
                    Err(_) => "CAD viewport renderer panicked".into(),
                };
                let _ = replies.send(Err(error));
                handle.request_redraw();
            })
            .expect("create CAD viewport renderer");
        let window = app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(app.world())
            .expect("primary CAD window");
        let image = app.world().resource::<Assets<Image>>().reserve_handle();
        app.insert_resource(HostSurface {
            window,
            image,
            node: None,
            displayed: None,
        })
        .insert_resource(Worker {
            requests: Some(requests),
            frames: Mutex::new(Some(frames)),
            thread: Some(thread),
            sent: None,
            in_flight: false,
            stopped,
            #[cfg(test)]
            gate_release,
        });
    }
}

fn setup_interface_canvas(mut commands: Commands, mut surface: ResMut<HostSurface>) {
    let camera = commands
        .spawn((
            Name::new("CAD canvas interface camera"),
            NativeViewportCamera,
            NativeOverlayCamera,
            IsDefaultUiCamera,
            Camera2d,
            VIEWPORT_MSAA,
            BoxShadowSamples(6),
            RenderLayers::none(),
            Camera {
                order: 0,
                ..default()
            },
        ))
        .id();
    surface.node = Some(
        commands
            .spawn((
                Name::new("CAD viewport texture"),
                UiTargetCamera(camera),
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    display: Display::None,
                    ..default()
                },
                ZIndex(-1),
                bevy::picking::Pickable::IGNORE,
                ImageNode::new(surface.image.clone()),
            ))
            .id(),
    );
}

fn exchange_frames(world: &mut World) {
    let Some(stamp) = Stamp::capture(world) else {
        return;
    };
    let owner = world.resource::<ModelResource>().session_id.clone();
    world.resource_scope(|world, mut worker: Mut<Worker>| {
        let frames: Vec<_> = worker
            .frames
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .try_iter()
            .collect();
        for frame in frames {
            worker.in_flight = false;
            match frame {
                // During continuous input, present this owner's latest
                // completed frame and immediately request the newest state.
                // Exact-stamp readiness below still fences render receipts.
                // Reject another document's pixels and incompatible sizes.
                Ok(frame) if frame.owner == owner && frame.stamp.size == stamp.size => {
                    world.resource_mut::<HostSurface>().displayed = Some(frame);
                    world
                        .resource::<NativeInterfaceHandle>()
                        .invalidate_presentation();
                }
                Ok(_) => {}
                Err(error) => {
                    eprintln!("{error}");
                    world.write_message(AppExit::error());
                    return;
                }
            }
        }
        if !worker.in_flight && worker.sent.as_ref() != Some(&stamp) {
            let snapshot = Snapshot::capture(world, stamp.clone(), worker.sent.as_ref());
            match worker.requests.as_ref().unwrap().try_send(snapshot) {
                Ok(()) => {
                    worker.sent = Some(stamp.clone());
                    worker.in_flight = true;
                }
                Err(mpsc::TrySendError::Full(_)) => {}
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    world.write_message(AppExit::error());
                }
            }
        }
    });
    let surface = world.resource::<HostSurface>();
    let ready = surface
        .displayed
        .as_ref()
        .is_some_and(|frame| frame.owner == owner && frame.stamp == stamp);
    let visible = surface
        .displayed
        .as_ref()
        .is_some_and(|frame| frame.owner == owner && frame.stamp.size == stamp.size);
    let node = surface.node;
    world.resource_mut::<ViewportReady>().0 = ready;
    if let Some(mut node) = node.and_then(|entity| world.get_mut::<Node>(entity)) {
        let display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
}

#[derive(Resource)]
struct ExtractedTexture {
    id: AssetId<Image>,
    image: GpuImage,
    // Keep the worker from writing this texture until this render world's
    // previous frame has submitted its commands. This also covers pipelining
    // and discarded completions during rapid document/camera changes.
    _lease: Arc<()>,
}

fn extract_texture(mut commands: Commands, surface: Extract<Res<HostSurface>>) {
    if let Some(frame) = &surface.displayed {
        commands.insert_resource(ExtractedTexture {
            id: surface.image.id(),
            image: frame.image.clone(),
            _lease: frame.lease.clone(),
        });
    }
}

fn prepare_texture(
    texture: Option<Res<ExtractedTexture>>,
    mut images: ResMut<RenderAssets<GpuImage>>,
    mut bindings: ResMut<bevy::ui_render::ImageNodeBindGroups>,
) {
    let Some(texture) = texture else {
        return;
    };
    if images
        .get(texture.id)
        .is_none_or(|image| image.texture_view.id() != texture.image.texture_view.id())
    {
        images.insert(texture.id, texture.image.clone());
        bindings.values.remove(&texture.id);
    }
}

fn target_image(size: UVec2) -> Image {
    let mut image = Image::new_uninit(
        Extent3d {
            width: size.x,
            height: size.y,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT;
    image
}

fn render_worker(
    resources: RenderResources,
    jobs: mpsc::Receiver<Snapshot>,
    replies: &mpsc::SyncSender<Result<Frame, String>>,
    handle: &NativeInterfaceHandle,
    stopped: &AtomicBool,
) -> Result<(), String> {
    if stopped.load(Ordering::Acquire) {
        return Ok(());
    }
    let mut app = App::new();
    let plugins = DefaultPlugins
        .build()
        .disable::<bevy::winit::WinitPlugin>()
        .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>()
        .disable::<bevy::ui_render::UiRenderPlugin>()
        .set(WindowPlugin {
            primary_window: None,
            exit_condition: ExitCondition::DontExit,
            ..default()
        })
        .set(RenderPlugin {
            synchronous_pipeline_compilation: true,
            render_creation: RenderCreation::Manual(Box::new(resources)),
            ..default()
        });
    #[cfg(feature = "dev-native-ime-trace")]
    let plugins = plugins.disable::<bevy::log::LogPlugin>();
    app.add_plugins(plugins);
    install_cad_renderer(&mut app, false);
    app.finish();
    app.cleanup();
    // The main world and its pipelined render world may each retain a distinct
    // completed frame. A third texture lets CAD render without overwriting
    // either. Only an unleased texture can be reused; there is no readback.
    let mut targets: [Target; 3] = std::array::from_fn(|_| Target {
        image: app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(target_image(UVec2::ONE)),
        lease: default(),
    });
    app.insert_resource(SceneImageTarget(targets[0].image.clone()));
    app.main_mut().update();
    let mut last: Option<Stamp> = None;
    while let Ok(mut snapshot) = jobs.recv() {
        for newer in jobs.try_iter() {
            snapshot = newer;
        }
        let owner = snapshot.owner;
        let stamp = snapshot.stamp.clone();
        let target = targets
            .iter_mut()
            .find(|target| target.lease.strong_count() == 0)
            .ok_or("CAD viewport has no free texture")?;
        let lease = Arc::new(());
        target.lease = Arc::downgrade(&lease);
        let target = &target.image;
        let world = app.world_mut();
        if let Some(model) = snapshot.model {
            *world.resource_mut::<ModelResource>() = model;
            if let Some(cache) = snapshot.edge_cache {
                let session = world.resource::<ModelResource>().session_id.clone();
                let entity = if let Some(entity) =
                    world.resource::<DocumentGeometryIndex>().0.get(&session)
                {
                    *entity
                } else {
                    let entity = world.spawn(BodyMeshCache::default()).id();
                    world
                        .resource_mut::<DocumentGeometryIndex>()
                        .0
                        .insert(session, entity);
                    entity
                };
                world.entity_mut(entity).insert(cache);
                world.resource_mut::<ModelResource>().cache_entity = Some(entity);
            } else {
                bind_document_geometry(world);
            }
        }
        if let Some(camera) = snapshot.camera {
            *world.resource_mut::<CameraResource>() = camera;
        }
        if let Some(preview) = snapshot.preview {
            *world.resource_mut::<PreviewResource>() = preview;
        }
        if let Some(stock) = snapshot.stock {
            *world.resource_mut::<CamStockResource>() = stock;
        }
        if let Some(palette) = snapshot.palette {
            *world.resource_mut::<PaletteResource>() = palette;
            *world.resource_mut::<ClearColor>() = ClearColor(rgb(palette.0.background));
        }
        if let Some(presentation) = snapshot.presentation {
            *world.resource_mut::<PresentationResource>() = presentation;
        }
        if let Some(section) = snapshot.section {
            world
                .resource_mut::<section_view::State>()
                .apply_snapshot(section);
        }
        if let Some(retained_sessions) = snapshot.retained_sessions {
            let retired: Vec<_> = world
                .resource::<DocumentGeometryIndex>()
                .0
                .keys()
                .filter(|session| !retained_sessions.contains(session))
                .cloned()
                .collect();
            for session in retired {
                drop_cached_model_session(world, &session);
            }
        }
        if last
            .as_ref()
            .is_none_or(|last| last.size != stamp.size || last.scale != stamp.scale)
        {
            world
                .resource_mut::<ViewportSizeResource>()
                .set_if_neq(snapshot.viewport);
            configure_viewport_line_widths(
                &mut world.resource_mut::<GizmoConfigStore>(),
                snapshot.viewport.logical_width,
                snapshot.viewport.logical_height,
                stamp.scale,
            );
        }
        {
            let mut images = world.resource_mut::<Assets<Image>>();
            let mut image = images.get_mut(target).unwrap();
            if image.size() != stamp.size {
                *image = target_image(stamp.size);
            }
        }
        for mut camera in world.query_filtered::<(&mut Camera, &mut bevy::camera::RenderTarget), With<NativeViewportCamera>>().iter_mut(world) {
            camera.0.viewport = None;
            *camera.1 = bevy::camera::RenderTarget::Image(bevy::camera::ImageRenderTarget { handle: target.clone(), scale_factor: stamp.scale });
        }
        loop {
            if stopped.load(Ordering::Acquire) {
                return Ok(());
            }
            let pipelines = app.sub_app(RenderApp).world().resource::<PipelineCache>();
            let before = pipelines.pipelines().count();
            let pending_before = pipelines.waiting_pipelines().next().is_some();
            app.update();
            if app.should_exit().is_some() {
                return Err("CAD viewport reported a rendering error".into());
            }
            let pipelines = app.sub_app(RenderApp).world().resource::<PipelineCache>();
            for pipeline in pipelines.pipelines() {
                if let bevy::render::render_resource::CachedPipelineState::Err(error) =
                    &pipeline.state
                {
                    if !matches!(
                        error,
                        bevy::shader::ShaderCacheError::ShaderNotLoaded(_)
                            | bevy::shader::ShaderCacheError::ShaderImportNotYetAvailable
                    ) {
                        return Err(format!("CAD viewport shader failed: {error:?}"));
                    }
                }
            }
            if pipelines.waiting_pipelines().next().is_none()
                && !pending_before
                && before == pipelines.pipelines().count()
            {
                break;
            }
            // Waiting shaders arrive through the asset loader. Yield the worker
            // rather than waking or blocking the interface's event loop.
            std::thread::yield_now();
        }
        let image = app
            .sub_app(RenderApp)
            .world()
            .resource::<RenderAssets<GpuImage>>()
            .get(target)
            .ok_or("CAD viewport target was not prepared")?
            .clone();
        app.world()
            .resource::<RenderDevice>()
            .wgpu_device()
            .poll(PollType::Poll)
            .map_err(|error| error.to_string())?;
        last = Some(stamp.clone());
        if replies
            .send(Ok(Frame {
                stamp,
                owner,
                image,
                lease,
            }))
            .is_err()
        {
            break;
        }
        handle.request_redraw();
    }
    Ok(())
}

#[cfg(test)]
#[path = "viewport_renderer/tests.rs"]
mod tests;
