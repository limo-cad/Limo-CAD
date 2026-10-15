use super::*;

#[test]
#[ignore = "requires a native graphics adapter"]
fn sketch_origin_planes_render_on_first_open_and_after_cancel() {
    let mut app = App::new();
    app.add_plugins(
        interface_plugins()
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>()
            .set(WindowPlugin {
                primary_window: Some(Window {
                    resolution: bevy::window::WindowResolution::new(640, 400),
                    ..default()
                }),
                exit_condition: ExitCondition::DontExit,
                ..default()
            }),
    );
    app.insert_resource(NativeInterfaceHandle::new(|| {}))
        .init_resource::<Captured>();
    install(&mut app);
    app.world_mut().resource_mut::<ModelResource>().session_id = "origin-plane-owner".into();
    bind_document_geometry(app.world_mut());
    *app.world_mut().resource_mut::<ViewportSizeResource>() = ViewportSizeResource {
        logical_width: 640.,
        logical_height: 400.,
    };
    app.world_mut()
        .resource_mut::<PresentationResource>()
        .0
        .hide_ground_grid = true;
    app.finish();
    app.cleanup();
    app.main_mut().update();
    let target = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(target_image(UVec2::new(640, 400)));
    let camera = app
        .world_mut()
        .query_filtered::<Entity, With<NativeOverlayCamera>>()
        .single(app.world())
        .unwrap();
    app.world_mut()
        .entity_mut(camera)
        .insert(bevy::camera::RenderTarget::Image(target.clone().into()));
    ready(&mut app);
    let solid = capture(&mut app, &target, "origin-planes-hidden");

    for cycle in 0..2 {
        app.world_mut()
            .resource_mut::<PresentationResource>()
            .0
            .mode = ViewportMode::PickPlane;
        app.update();
        ready(&mut app);
        let picking = capture(&mut app, &target, "origin-planes-visible");
        assert!(
            changed(&solid, &picking) > 3000,
            "Cycle {cycle}: all three filled origin planes must appear without pointer motion"
        );
        for plane in [
            ViewportOriginPlane::Xy,
            ViewportOriginPlane::Xz,
            ViewportOriginPlane::Yz,
        ] {
            app.world_mut()
                .resource_mut::<PresentationResource>()
                .0
                .hovered_origin_plane = Some(plane);
            app.update();
            ready(&mut app);
            let hovered = capture(&mut app, &target, &format!("origin-plane-{plane:?}-hover"));
            assert!(
                changed(&picking, &hovered) > 800,
                "{plane:?}: the plane's filled area must brighten on hover"
            );
        }
        {
            let mut presentation = app.world_mut().resource_mut::<PresentationResource>();
            presentation.0.mode = ViewportMode::Solid;
            presentation.0.hovered_origin_plane = None;
        }
        app.update();
        ready(&mut app);
        let cancelled = capture(&mut app, &target, "origin-planes-cancelled");
        assert_eq!(
            changed(&solid, &cancelled),
            0,
            "Cancel must hide the planes"
        );
    }
}
