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
    service.close_view(&view);
    let started = Instant::now();
    drop(app);
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "shutdown waited for external teaching leases"
    );
    assert_eq!(
        held.len(),
        5,
        "leases deliberately outlive the renderer host"
    );
    drop(held);
}
