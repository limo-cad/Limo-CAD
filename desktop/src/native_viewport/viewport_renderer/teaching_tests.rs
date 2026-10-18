//! End-to-end GPU ownership and main-view isolation through the production worker.
use super::*;
use crate::native_viewport::script_preview::{PreviewService, RenderRequest, RenderedTexture};
use std::time::{Duration, Instant};

fn receive(
    app: &mut App,
    pending: crate::native_viewport::script_preview::PendingRender,
) -> Result<RenderedTexture, String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        app.update();
        assert!(app.should_exit().is_none());
        match pending.try_recv()? {
            Some(texture) => return Ok(texture),
            None => assert!(Instant::now() < deadline, "teaching frame timed out"),
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
#[ignore = "requires native GPU; exercises shared worker and retained GPU leases"]
fn native_shared_teaching_preserves_main_and_exhausted_targets_do_not_block_main() {
    shared_worker_regression(false);
}

#[test]
#[ignore = "requires native GPU; injects real shader failures into the shared worker"]
fn native_shared_teaching_shader_failure_recovers_main_and_main_failure_is_fatal() {
    shared_worker_regression(true);
}

fn shared_worker_regression(shader_failure: bool) {
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
        .init_resource::<tests::Captured>();
    install(&mut app);
    app.world_mut().resource_mut::<ModelResource>().session_id = "live-proof".into();
    let live_scene = serde_json::from_value(serde_json::json!({
        "bodies":[{"id":1,"name":"Distinct live solid","feature_id":2,"faces":[],
        "edges":[{"id":1,"key":"live-edge","points":[{"x":-20.,"y":-15.,"z":0.},{"x":40.,"y":-15.,"z":0.}]}],
        "mesh":{"positions":[-20,-15,0, 40,-15,0, -20,45,0, -20,-15,50],
        "normals":[0,0,1,0,0,1,0,0,1,0,0,1],"indices":[0,2,1,0,1,3,0,3,2,1,2,3]}}],"errors":[]
    })).unwrap();
    app.world_mut().resource_mut::<ModelResource>().document =
        Arc::new(limo_cad_native_engine::NativeViewportDocument {
            scene: Arc::new(live_scene),
            ..default()
        });
    app.world_mut()
        .resource_mut::<PresentationResource>()
        .0
        .selected_edge_ids = vec![1];
    let original_document = app.world().resource::<ModelResource>().document.clone();
    app.world_mut().resource_mut::<ModelResource>().revision = 1;
    app.world_mut()
        .resource_mut::<ModelResource>()
        .geometry_revision = 1;
    bind_document_geometry(app.world_mut());
    *app.world_mut().resource_mut::<ViewportSizeResource>() = ViewportSizeResource {
        logical_width: 320.,
        logical_height: 200.,
    };
    let original_camera = *app.world().resource::<CameraResource>();
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
    tests::ready(&mut app);
    let original = tests::capture(&mut app, &target, "shared-main-before");
    let service = PreviewService::default();
    connect_preview(app.world(), &service).unwrap();
    let frame = serde_json::from_value(serde_json::json!({"caption":"teaching", "scene":{
        "bodies":[{"id":1,"name":"Stock","feature_id":2,"faces":[],"edges":[],"mesh":{
        "positions":[0,0,0, 10,0,0, 0,10,0, 0,0,10],"normals":[0,0,1,0,0,1,0,0,1,0,0,1],
        "indices":[0,2,1,0,1,3,0,3,2,1,2,3]}}],"errors":[]}
    }))
    .unwrap();
    let descriptor = service.retain(vec![frame]).unwrap();
    let view = service.open_view().unwrap();
    let request = |revision| RenderRequest {
        preview_id: descriptor.preview_id.clone(),
        view_id: view.clone(),
        revision,
        frame_index: 0,
        width: 300,
        height: 176,
        yaw: 0.5,
        pitch: 0.5,
        palette: None,
    };
    let mut held = Vec::new();
    for revision in 1..=5 {
        let pending = service.render(request(revision)).unwrap();
        held.push(receive(&mut app, pending).unwrap());
    }
    assert!(held
        .windows(2)
        .all(|pair| pair[0].image.texture_view.id() != pair[1].image.texture_view.id()));
    let exhausted = service.render(request(6)).unwrap();
    assert!(receive(&mut app, exhausted)
        .err()
        .unwrap()
        .contains("occupied"));
    assert!(Arc::ptr_eq(
        &original_document,
        &app.world().resource::<ModelResource>().document
    ));
    assert_eq!(
        app.world()
            .resource::<PresentationResource>()
            .0
            .selected_edge_ids,
        vec![1]
    );
    assert_eq!(
        app.world().resource::<CameraResource>().camera,
        original_camera.camera
    );
    // Force a fresh main render at the same pose; a retained old texture is insufficient proof.
    app.world_mut().resource_mut::<CameraResource>().revision += 1;
    app.update();
    tests::ready(&mut app);
    let after_teaching = tests::capture(&mut app, &target, "shared-main-after-teaching");
    assert_eq!(
        tests::changed(&original, &after_teaching),
        0,
        "teaching altered live pixels"
    );
    {
        let mut camera = app.world_mut().resource_mut::<CameraResource>();
        camera.camera.position[0] += 25.;
        camera.revision += 1;
    }
    app.update();
    tests::ready(&mut app);
    let orbited = tests::capture(&mut app, &target, "shared-main-orbit");
    assert!(
        tests::changed(&original, &orbited) > 100,
        "main must advance while all teaching targets are leased"
    );
    {
        let mut camera = app.world_mut().resource_mut::<CameraResource>();
        camera.camera = original_camera.camera;
        camera.revision += 1;
    }
    app.update();
    tests::ready(&mut app);
    let restored = tests::capture(&mut app, &target, "shared-main-restored");
    assert_eq!(
        tests::changed(&original, &restored),
        0,
        "main state failed to restore after teaching"
    );
    if shader_failure {
        // Keep four old-device texture leases across App reconstruction.
        held.pop();
        let queue = app
            .world()
            .resource::<Worker>()
            .requests
            .as_ref()
            .unwrap()
            .clone();
        queue.inject_shader_failure.store(1, Ordering::Release);
        let pending = service.render(request(7)).unwrap();
        let failure = receive(&mut app, pending).err().unwrap();
        assert!(
            failure.contains("Teaching viewport shader failed"),
            "{failure}"
        );
        app.world_mut().resource_mut::<CameraResource>().revision += 1;
        app.update();
        tests::ready(&mut app);
        let recovered = tests::capture(&mut app, &target, "shared-main-shader-recovery");
        assert_eq!(
            tests::changed(&restored, &recovered),
            0,
            "recovery lost unchanged live state"
        );
        let disabled = service.render(request(8)).unwrap();
        assert!(receive(&mut app, disabled)
            .err()
            .unwrap()
            .contains("disabled"));
        // Real main compile errors still terminate; recovery is not an ignore list.
        queue.inject_shader_failure.store(2, Ordering::Release);
        app.world_mut().resource_mut::<CameraResource>().revision += 1;
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            app.update();
            if app.should_exit().is_some() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "main shader failure was not fatal"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    service.close_view(&view);
    let started = Instant::now();
    drop(app);
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "shutdown waited for external teaching leases"
    );
    assert_eq!(
        held.len(),
        if shader_failure { 4 } else { 5 },
        "leases deliberately outlive the renderer host"
    );
    drop(held);
}

// Fault injection uses the real compiler/cache, not a manufactured error return.
// 1 targets teaching and 2 targets main; the trigger belongs to one worker only.
pub(super) fn inject_invalid_shader(app: &mut App) {
    let shader = app
        .world_mut()
        .resource_mut::<Assets<bevy::shader::Shader>>()
        .add(bevy::shader::Shader::from_wgsl(
            "this is deliberately invalid WGSL",
            "teaching-failure-test.wgsl",
        ));
    app.sub_app(RenderApp)
        .world()
        .resource::<PipelineCache>()
        .queue_compute_pipeline(bevy::render::render_resource::ComputePipelineDescriptor {
            shader,
            entry_point: Some("main".into()),
            ..default()
        });
}

#[test]
fn native_shared_recovery_merges_deltas_without_resurrecting_cleared_state() {
    let mut world = World::new();
    world.init_resource::<ModelResource>();
    world.init_resource::<CameraResource>();
    world.init_resource::<PreviewResource>();
    world.init_resource::<CamStockResource>();
    world.init_resource::<PaletteResource>();
    world.init_resource::<PresentationResource>();
    world.init_resource::<section_view::State>();
    world.init_resource::<ViewportSizeResource>();
    world.init_resource::<DocumentGeometryIndex>();
    world.resource_mut::<ModelResource>().session_id = "before".into();
    world
        .resource_mut::<PresentationResource>()
        .0
        .selected_edge_ids = vec![17];
    let stamp = Stamp {
        changed: [0; 8],
        size: UVec2::new(320, 200),
        scale: 1.,
    };
    let previous = Snapshot::capture(&world, stamp.clone(), None);
    let mut delta = Snapshot::capture(&world, stamp.clone(), Some(&stamp));
    // A new empty model/presentation/session list means deletion, not omission.
    delta.model = Some(ModelResource {
        session_id: "after".into(),
        ..default()
    });
    delta.presentation = Some(PresentationResource::default());
    delta.retained_sessions = Some(Vec::new());
    let mut camera = *world.resource::<CameraResource>();
    camera.revision += 1;
    delta.camera = Some(camera);
    delta.restore_unchanged(previous);
    assert_eq!(delta.model.unwrap().session_id, "after");
    assert!(delta.edge_cache.is_none());
    assert!(delta.presentation.unwrap().0.selected_edge_ids.is_empty());
    assert!(delta.retained_sessions.unwrap().is_empty());
    assert_eq!(delta.camera.unwrap().revision, camera.revision);
    assert!(delta.preview.is_some());
    assert!(delta.stock.is_some());
    assert!(delta.palette.is_some());
    assert!(delta.section.is_some());
}
