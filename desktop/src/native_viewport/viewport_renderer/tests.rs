use super::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};
use std::time::{Duration, Instant};

#[derive(Resource)]
pub(super) struct CompilationGate {
    pub(super) receiver: Mutex<mpsc::Receiver<()>>,
    pub(super) release: mpsc::SyncSender<()>,
    pub(super) entered: mpsc::Sender<()>,
}

#[derive(Resource, Default)]
struct Captured(Option<Image>);

fn capture(app: &mut App, target: &Handle<Image>, name: &str) -> Vec<u8> {
    // Capture a prepared UI frame, rather than the first clear-only frame
    // before the interface's own 2D pipelines and image assets are ready.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let pipelines = app.sub_app(RenderApp).world().resource::<PipelineCache>();
        let count = pipelines.pipelines().count();
        let pending = pipelines.waiting_pipelines().next().is_some();
        app.update();
        let pipelines = app.sub_app(RenderApp).world().resource::<PipelineCache>();
        if !pending
            && pipelines.waiting_pipelines().next().is_none()
            && count == pipelines.pipelines().count()
        {
            break;
        }
        assert!(Instant::now() < deadline, "interface preparation timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
    app.world_mut().resource_mut::<Captured>().0 = None;
    app.world_mut()
        .spawn(Screenshot::image(target.clone()))
        .observe(
            |event: On<ScreenshotCaptured>, mut captured: ResMut<Captured>| {
                captured.0 = Some(event.image.clone());
            },
        );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        app.update();
        assert!(app.should_exit().is_none(), "interface renderer failed");
        app.world()
            .resource::<RenderDevice>()
            .wgpu_device()
            .poll(PollType::Poll)
            .unwrap();
        if let Some(image) = app.world_mut().resource_mut::<Captured>().0.take() {
            let image = image.try_into_dynamic().unwrap().to_rgba8();
            if let Ok(directory) = std::env::var("LIMO_VIEWPORT_RENDER_PROOFS") {
                image
                    .save(std::path::Path::new(&directory).join(format!("{name}.png")))
                    .unwrap();
            }
            return image.into_raw();
        }
        assert!(Instant::now() < deadline, "interface capture timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn ready(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !app.world().resource::<ViewportReady>().0 {
        app.update();
        assert!(app.should_exit().is_none(), "viewport renderer failed");
        assert!(Instant::now() < deadline, "viewport completion timed out");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn changed(a: &[u8], b: &[u8]) -> usize {
    a.as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_chunks::<4>().0.iter())
        .filter(|(a, b)| a != b)
        .count()
}

#[test]
#[ignore = "requires a native graphics adapter"]
fn native_viewport_grid_restores_without_pointer_motion_or_another_scene_request() {
    use crate::session_bridge::native_interface::controller::workbench::{self, Command};

    let mut app = App::new();
    app.add_plugins(
        interface_plugins()
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>()
            .set(WindowPlugin {
                primary_window: Some(Window {
                    resolution: bevy::window::WindowResolution::new(320, 200),
                    ..default()
                }),
                exit_condition: ExitCondition::DontExit,
                ..default()
            }),
    );
    app.insert_resource(NativeInterfaceHandle::new(|| {}))
        .init_resource::<Captured>();
    install(&mut app);
    app.world_mut().resource_mut::<ModelResource>().session_id = "grid-owner".into();
    bind_document_geometry(app.world_mut());
    *app.world_mut().resource_mut::<ViewportSizeResource>() = ViewportSizeResource {
        logical_width: 320.,
        logical_height: 200.,
    };
    {
        let mut palette = app.world_mut().resource_mut::<PaletteResource>();
        palette.0.grid_fine = [0.9, 0.9, 0.9];
        palette.0.grid_major = [1., 1., 1.];
    }
    let original_camera = app.world().resource::<CameraResource>().camera;
    app.finish();
    app.cleanup();
    app.main_mut().update();
    let target = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(target_image(UVec2::new(320, 200)));
    let camera = app
        .world_mut()
        .query_filtered::<Entity, With<NativeOverlayCamera>>()
        .single(app.world())
        .unwrap();
    app.world_mut()
        .entity_mut(camera)
        .insert(bevy::camera::RenderTarget::Image(target.clone().into()));
    ready(&mut app);
    let showing = capture(&mut app, &target, "grid-initial");
    for cycle in 0..3 {
        workbench::execute(app.world_mut(), &Command::ToggleGrid).unwrap();
        app.update();
        ready(&mut app);
        let hidden = capture(&mut app, &target, "grid-hidden");
        assert!(changed(&showing, &hidden) > 200, "Grid must disappear");

        workbench::execute(app.world_mut(), &Command::ToggleGrid).unwrap();
        app.update();
        ready(&mut app);
        let sent = app.world().resource::<Worker>().sent.clone();
        let restored = capture(&mut app, &target, "grid-restored");
        assert_eq!(
            changed(&showing, &restored),
            0,
            "Cycle {cycle}: the completed viewport frame must restore the grid without pointer input"
        );
        assert_eq!(
            app.world().resource::<CameraResource>().camera,
            original_camera
        );
        assert!(
            app.world().resource::<Worker>().sent == sent,
            "Capturing the completed frame must not submit another CAD scene request"
        );
        assert!(!app.world().resource::<Worker>().in_flight);
    }
}

#[test]
fn camera_snapshots_keep_document_and_presentation_state_resident() {
    let mut world = World::new();
    world.init_resource::<ModelResource>();
    world.init_resource::<CameraResource>();
    world.init_resource::<PreviewResource>();
    world.init_resource::<CamStockResource>();
    world.init_resource::<PaletteResource>();
    world.init_resource::<PresentationResource>();
    world.init_resource::<section_view::State>();
    world.init_resource::<DocumentGeometryIndex>();
    world.init_resource::<ViewportSizeResource>();
    let window = world.spawn(Window::default()).id();
    world.insert_resource(HostSurface {
        window,
        image: default(),
        node: None,
        displayed: None,
    });
    let first = Stamp::capture(&world).unwrap();
    let full = Snapshot::capture(&world, first.clone(), None);
    assert!(Arc::ptr_eq(
        &full.model.unwrap().document,
        &world.resource::<ModelResource>().document
    ));
    world.increment_change_tick();
    world.resource_mut::<CameraResource>().camera.position[0] += 1.;
    let next = Stamp::capture(&world).unwrap();
    let delta = Snapshot::capture(&world, next, Some(&first));
    assert!(delta.camera.is_some());
    assert!(delta.model.is_none());
    assert!(delta.edge_cache.is_none());
    assert!(delta.presentation.is_none());
    assert!(delta.preview.is_none());
    assert!(delta.stock.is_none());
    assert!(delta.palette.is_none());
    assert!(delta.section.is_none());
    assert!(delta.retained_sessions.is_none());
}

#[test]
#[ignore = "requires a native graphics adapter"]
fn native_ui_renders_and_updates_before_3d_compilation_and_composites_owned_frames() {
    let mut app = App::new();
    app.add_plugins(
        interface_plugins()
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>()
            .set(WindowPlugin {
                primary_window: Some(Window {
                    resolution: bevy::window::WindowResolution::new(320, 200),
                    ..default()
                }),
                exit_condition: ExitCondition::DontExit,
                ..default()
            }),
    );
    let handle = NativeInterfaceHandle::new(|| {});
    app.insert_resource(handle.clone())
        .init_resource::<Captured>();
    install(&mut app);
    let (release, receiver) = mpsc::sync_channel(1);
    let (entered, compiling) = mpsc::channel();
    app.insert_resource(CompilationGate {
        receiver: Mutex::new(receiver),
        release: release.clone(),
        entered,
    });
    app.world_mut().resource_mut::<ModelResource>().session_id = "first-owner".into();
    bind_document_geometry(app.world_mut());
    *app.world_mut().resource_mut::<ViewportSizeResource>() = ViewportSizeResource {
        logical_width: 320.,
        logical_height: 200.,
    };
    app.finish();
    app.cleanup();
    assert!(!app.is_plugin_added::<bevy::pbr::PbrPlugin>());
    // Bevy disables wgpu's default features. Without wgpu/std its error-scope
    // thread IDs are a single global stub, so independently nested shader
    // validation in the two renderers can panic. Require the documented native
    // scope ownership deterministically, rather than relying on a timing race.
    let device = app.world().resource::<RenderDevice>().clone();
    let interface_scope = device
        .wgpu_device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    let (entered, worker_entered) = mpsc::channel();
    let (release_scope, scope_released) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        let scope = device
            .wgpu_device()
            .push_error_scope(wgpu::ErrorFilter::Validation);
        entered.send(()).unwrap();
        let _ = scope_released.recv();
        bevy::tasks::block_on(scope.pop())
    });
    worker_entered.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(bevy::tasks::block_on(interface_scope.pop()).is_none());
    release_scope.send(()).unwrap();
    assert!(thread.join().unwrap().is_none());
    app.main_mut().update();
    let target = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(target_image(UVec2::new(320, 200)));
    let camera = app
        .world_mut()
        .query_filtered::<Entity, With<NativeOverlayCamera>>()
        .single(app.world())
        .unwrap();
    app.world_mut()
        .entity_mut(camera)
        .insert(bevy::camera::RenderTarget::Image(target.clone().into()));
    let button = app
        .world_mut()
        .spawn((
            UiTargetCamera(camera),
            Node {
                position_type: PositionType::Absolute,
                left: px(8),
                top: px(8),
                width: px(180),
                height: px(36),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgb(0.1, 0.8, 0.2)),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new("File  Edit  Design"),
                TextFont::from_font_size(16.0),
                TextColor(Color::WHITE),
            ));
        })
        .id();
    app.update();
    compiling.recv_timeout(Duration::from_secs(5)).unwrap();
    let before = capture(&mut app, &target, "menus-before-3d");
    assert!(
        before
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[1] > 150 && p[1] > p[0].saturating_add(60))
            .count()
            > 2000,
        "button background must be rendered"
    );
    assert!(
        before
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0] > 200 && p[1] > 200 && p[2] > 200)
            .count()
            > 40,
        "menu text must be rendered"
    );
    assert!(
        !app.world().resource::<ViewportReady>().0,
        "3D worker is still blocked"
    );
    assert!(app.world().resource::<Worker>().in_flight);
    let pipelines = app.sub_app(RenderApp).world().resource::<PipelineCache>();
    assert!(pipelines.waiting_pipelines().next().is_none());
    let labels: Vec<_> = pipelines
        .pipelines()
        .map(|p| format!("{:?}", p.descriptor))
        .collect();
    assert!(
        labels.iter().all(|label| !label.contains("atmosphere")
            && !label.contains("mesh_preprocess")
            && !label.contains("pbr_pipeline")),
        "interface must have no 3D pipelines: {labels:?}"
    );
    *app.world_mut().get_mut::<BackgroundColor>(button).unwrap() =
        BackgroundColor(Color::srgb(0.1, 0.2, 0.9));
    let interactive = capture(&mut app, &target, "menus-update-before-3d");
    assert!(
        changed(&before, &interactive) > 2000,
        "UI must keep drawing updates while 3D is blocked"
    );
    assert!(!app.world().resource::<ViewportReady>().0);

    app.world_mut().resource_mut::<CameraResource>().camera = ViewportCamera {
        position: [0., 0., 10.],
        target: [0.; 3],
        up: [0., 1., 0.],
        vertical_fov_degrees: 90.,
    };
    app.world_mut()
        .resource_mut::<PresentationResource>()
        .0
        .measurement_lines = Arc::new(vec![ViewportMeasurementLine {
        points: [-1., 0., 0., 1., 0., 0.],
        color: [1., 0.78, 0.12, 1.],
        label: Some("2.000 mm".into()),
    }]);
    let measured = capture(&mut app, &target, "measurement-value-before-3d");
    let region = |pixels: &[u8]| -> Vec<[u8; 4]> {
        pixels
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter(|(i, _)| (110..210).contains(&(i % 320)) && (60..105).contains(&(i / 320)))
            .map(|(_, pixel)| *pixel)
            .collect()
    };
    let ink = app
        .world()
        .resource::<PaletteResource>()
        .0
        .ink
        .map(|value| (value * 255.).round() as u8);
    assert!(
        region(&measured)
            .iter()
            .filter(|p| (0..3).all(|channel| p[channel].abs_diff(ink[channel]) < 20))
            .count()
            > 30,
        "Measurement glyphs must render in the guide's screen region"
    );
    assert!(
        region(&measured)
            .iter()
            .zip(region(&interactive))
            .filter(|(a, b)| **a != *b)
            .count()
            > 100,
        "The retained 2D renderer must draw measurement readouts before CAD compilation completes"
    );
    assert!(!app.world().resource::<ViewportReady>().0);
    app.world_mut()
        .resource_mut::<CameraResource>()
        .camera
        .position[0] = 2.;
    app.world_mut()
        .resource_mut::<CameraResource>()
        .camera
        .target[0] = 2.;
    let moved = capture(&mut app, &target, "measurement-value-camera-moved");
    assert!(
        changed(&measured, &moved) > 100,
        "Readout pixels must follow camera movement"
    );
    app.world_mut()
        .resource_mut::<PresentationResource>()
        .0
        .measurement_lines = Arc::default();
    let cleared = capture(&mut app, &target, "measurement-value-cleared");
    assert_eq!(
        region(&cleared),
        region(&interactive),
        "Clearing guides must retire their value pixels"
    );

    // Replace the owner while the first snapshot is still pending. Its eventual
    // completion must neither display old geometry nor acknowledge this owner.
    let source = r#"{"version":1,"name":"Independent viewport render fixture","steps":[
      {"call":{"group":"sketch/draw","operation":"sketch_begin","arguments":{"name":"Box","plane":{"type":"origin_plane","plane":"xy"}}}},
      {"call":{"group":"sketch/draw","operation":"sketch_add_rectangle_locked","arguments":{"mode":"two_point","anchor":{"x":0,"y":0},"corner_hint":{"x":60,"y":30},"width_mm":60,"height_mm":30,"ctrl_held":true}}},
      {"call":{"group":"sketch/draw","operation":"sketch_finish","arguments":{}}},
      {"id":"box","call":{"group":"solid/build","operation":"solid_extrude","arguments":{"sketch_name":"Box","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":36},"taper_angle_deg":0,"flip":false,"target_body_ids":[]}}}
    ],"exports":{"scene":{"$ref":"box","pointer":"/scene"}}}"#;
    let mut report = limo_cad_mcp::preview_script(source).unwrap();
    let scene: SolidSceneDto = serde_json::from_value(report["exports"]["scene"].take()).unwrap();
    let document = Arc::new(limo_cad_native_engine::NativeViewportDocument {
        scene: Arc::new(scene),
        ..default()
    });
    apply_interface_model(
        app.world_mut(),
        ViewportModel {
            session_id: "second-owner".into(),
            geometry_revision: 1,
            document: document.clone(),
            body_poses: Arc::default(),
            instance_body_poses: Arc::default(),
        },
    )
    .unwrap();
    release.send(()).unwrap();
    ready(&mut app);
    let frame = app
        .world()
        .resource::<HostSurface>()
        .displayed
        .as_ref()
        .unwrap();
    assert_eq!(frame.owner, "second-owner");
    assert!(Arc::ptr_eq(
        &app.world().resource::<ModelResource>().document,
        &document
    ));
    let composed = capture(&mut app, &target, "kernel-composited");
    assert!(
        changed(&interactive, &composed) > 2000,
        "the real CAD scene must be visible through the shared GPU texture"
    );
    let mut camera = app.world().resource::<CameraResource>().camera;
    camera.position = [180., -180., 140.];
    camera.target = [30., 15., 18.];
    camera.vertical_fov_degrees = 35.;
    apply_camera_state(app.world_mut(), camera);
    app.update();
    ready(&mut app);
    let turned = capture(&mut app, &target, "kernel-camera-update");
    assert!(
        changed(&composed, &turned) > 1000,
        "camera update must reach the viewport without replacing the source model"
    );
    assert!(Arc::ptr_eq(
        &app.world().resource::<ModelResource>().document,
        &document
    ));

    // The worker retains the production GPU stock material and receives both
    // the immutable stock mesh and the changing cutter/path presentation.
    let body = &document.scene.bodies[0];
    let mesh = &body.mesh;
    let positions = mesh
        .indices
        .iter()
        .flat_map(|index| {
            let index = *index as usize * 3;
            mesh.positions[index..index + 3].iter().copied()
        })
        .collect();
    let normals = mesh
        .indices
        .iter()
        .flat_map(|index| {
            let index = *index as usize * 3;
            mesh.normals[index..index + 3].iter().copied()
        })
        .collect();
    apply_interface_cam_stock(
        app.world_mut(),
        "second-owner",
        Some(ViewportCamStock {
            positions: Arc::new(positions),
            normals: Arc::new(normals),
            time_seconds: Some(0.),
        }),
    )
    .unwrap();
    apply_preview_state(
        app.world_mut(),
        Arc::new(ViewportPreview {
            lines: vec![super::super::super::ViewportLineLayer {
                color: [0.2, 0.4, 1., 1.],
                segments: vec![-10., 15., 31., 70., 15., 31.].into(),
                playback: Some(super::super::super::ViewportLinePlayback {
                    path_id: 7,
                    single_tool: true,
                    removes_stock: true,
                    completed_color: [0.2, 0.4, 1., 1.],
                    segment_times: vec![0., 1.].into(),
                }),
                ..default()
            }],
            ..default()
        }),
    );
    let stock_presentation = ViewportPresentation {
        hidden_body_ids: vec![body.id.0],
        cam_stock_visible: true,
        cam_tool: Some(ViewportCamTool {
            tip: [30., 15., 31.],
            axis: [0., 0., 1.],
            geometry: limo_cad_cam::CamCutterGeometryDto {
                kind: limo_cad_cam::CamToolKind::FlatEndMill,
                diameter: 10.,
                flute_length: 60.,
                overall_length: 80.,
                point_angle_degrees: None,
                corner_radius: None,
                corner_chamfer: None,
            },
        }),
        cam_path_progress: Some(super::super::super::ViewportCamPathProgress {
            path_id: 7,
            time_seconds: 0.5,
            position: [30., 15., 31.],
        }),
        ..default()
    };
    apply_presentation_state(app.world_mut(), stock_presentation.clone());
    app.update();
    ready(&mut app);
    let stock = capture(&mut app, &target, "stock-composited");
    let mut cut_presentation = stock_presentation.clone();
    cut_presentation.cam_gpu_stock_removal = true;
    apply_presentation_state(app.world_mut(), cut_presentation);
    app.update();
    ready(&mut app);
    let cut = capture(&mut app, &target, "stock-removal-composited");
    assert!(
        changed(&stock, &cut) > 40,
        "stock removal must reach the shared texture"
    );
    apply_presentation_state(app.world_mut(), stock_presentation);
    app.update();
    ready(&mut app);
    let restored = capture(&mut app, &target, "stock-restored-composited");
    assert_eq!(stock, restored, "turning removal off must restore stock");

    *app.world_mut().resource_mut::<ViewportSizeResource>() = ViewportSizeResource {
        logical_width: 400.,
        logical_height: 240.,
    };
    *app.world_mut()
        .resource_mut::<Assets<Image>>()
        .get_mut(&target)
        .unwrap() = target_image(UVec2::new(400, 240));
    app.update();
    ready(&mut app);
    let resized = capture(&mut app, &target, "viewport-resized");
    assert_eq!(resized.len(), 400 * 240 * 4);
    assert_eq!(
        app.world()
            .resource::<HostSurface>()
            .displayed
            .as_ref()
            .unwrap()
            .stamp
            .size,
        UVec2::new(400, 240)
    );
}

#[test]
fn worker_shutdown_services_pending_main_thread_surface_work() {
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
    let (done, completed) = mpsc::channel();
    let main = std::thread::spawn(move || {
        let executor = MainThreadExecutor::new();
        let main_thread = std::thread::current().id();
        let submission = surface_submission::SharedSubmission::default();
        let surface_submission = submission.clone();
        let render_executor = executor.clone();
        let (queued, pending) = mpsc::channel();
        let render = std::thread::spawn(move || {
            surface_submission.run(|| {
                let task = render_executor.0.spawn(async move {
                    assert_eq!(std::thread::current().id(), main_thread);
                });
                queued.send(()).unwrap();
                bevy::tasks::block_on(task);
            });
        });
        pending.recv_timeout(Duration::from_secs(5)).unwrap();
        let cad = std::thread::spawn(move || submission.run(|| {}));
        // A plain join deadlocks: CAD awaits surface work queued exclusively
        // on this synthetic main thread. Teardown must service that work.
        drop(Worker {
            requests: None,
            frames: Mutex::new(None),
            thread: Some(cad),
            sent: None,
            in_flight: false,
            stopped: Arc::new(AtomicBool::new(false)),
            main_thread_executor: Some(executor),
            gate_release: None,
        });
        render.join().unwrap();
        done.send(()).unwrap();
    });
    completed
        .recv_timeout(Duration::from_secs(5))
        .expect("CAD shutdown did not service the pending surface work");
    main.join().unwrap();
}

#[test]
fn texture_pool_waits_for_deferred_extraction() {
    use bevy::ecs::schedule::{ScheduleLabel, SystemSet};
    use bevy::render::extract_plugin::ExtractPlugin;

    // Exercise Bevy's actual deferred extraction schedule without a GPU. These
    // resources retain the same leases as HostSurface and ExtractedTexture.
    #[derive(Resource)]
    struct Displayed(Arc<()>);
    #[derive(Resource)]
    struct Extracted(Arc<()>);
    fn extract_lease(mut commands: Commands, frame: Extract<Res<Displayed>>) {
        commands.insert_resource(Extracted(frame.0.clone()));
    }
    let mut app = App::new();
    app.add_plugins(ExtractPlugin::<RenderApp>::new(
        |_, _| {},
        Render::base_schedule,
        Render.intern(),
        RenderSystems::ExtractCommands.intern(),
        RenderSystems::PostCleanup.intern(),
    ));
    let mut render = app.remove_sub_app(RenderApp).unwrap();
    render.update_schedule = Some(Render.intern());
    render.add_systems(ExtractSchedule, extract_lease);
    let mut targets: [Target; 3] = std::array::from_fn(|_| Target {
        image: default(),
        lease: default(),
    });
    for (index, target) in targets.iter_mut().enumerate() {
        let lease = Arc::new(());
        target.lease = Arc::downgrade(&lease);
        app.world_mut().insert_resource(Displayed(lease));
        if index < 2 {
            render.extract(app.world_mut());
        }
        if index == 0 {
            render.update();
        }
    }
    // A is in the render world, B in queued extraction commands, C in the main
    // world. The next CAD job must wait, rather than return a fatal error.
    assert_eq!(
        targets.each_ref().map(|target| target.lease.strong_count()),
        [1, 1, 1]
    );
    assert!(std::sync::Weak::ptr_eq(
        &targets[0].lease,
        &Arc::downgrade(&render.world().resource::<Extracted>().0),
    ));
    let released = targets[0].lease.clone();
    let (done, completed) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let stopped = AtomicBool::new(false);
        let acquired = wait_for_target(&mut targets, &stopped)
            .map(|target| std::sync::Weak::ptr_eq(&target.lease, &released));
        done.send(acquired).unwrap();
    });
    assert_eq!(
        completed.recv_timeout(Duration::from_millis(50)),
        Err(mpsc::RecvTimeoutError::Timeout),
        "a fully leased pool must wait without failing or reusing a live texture",
    );
    // Applying B frees exactly A, while B and C stay leased.
    render.update();
    assert_eq!(
        completed.recv_timeout(Duration::from_secs(5)).unwrap(),
        Some(true),
        "the pending job must acquire only the released texture",
    );
    worker.join().unwrap();
}

#[test]
fn texture_pool_wait_is_cancelled_on_shutdown() {
    let leases: [Arc<()>; 3] = std::array::from_fn(|_| Arc::new(()));
    let mut targets = leases.each_ref().map(|lease| Target {
        image: default(),
        lease: Arc::downgrade(lease),
    });
    let stopped = Arc::new(AtomicBool::new(false));
    let worker_stopped = stopped.clone();
    let (done, completed) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let cancelled = wait_for_target(&mut targets, &worker_stopped).is_none();
        done.send(cancelled).unwrap();
    });
    assert_eq!(
        completed.recv_timeout(Duration::from_millis(50)),
        Err(mpsc::RecvTimeoutError::Timeout),
    );
    stopped.store(true, Ordering::Release);
    assert!(completed.recv_timeout(Duration::from_secs(5)).unwrap());
    worker.join().unwrap();
    assert!(leases.iter().all(|lease| Arc::strong_count(lease) == 1));
    // Shutdown also wins when an unused target is available.
    let mut available = [Target {
        image: default(),
        lease: default(),
    }];
    assert!(wait_for_target(&mut available, &stopped).is_none());
}
