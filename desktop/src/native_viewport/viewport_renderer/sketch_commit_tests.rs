use super::tests::{capture, changed, ready, Captured};
use super::*;
use crate::state::AppState;
use serde_json::{json, Value};

fn call(engine: &AppState, operation: &str, arguments: Value) {
    crate::session_bridge::parse_engine_envelope(
        engine.engine_call(operation, &arguments.to_string()),
    )
    .unwrap();
}

#[test]
#[ignore = "requires a native graphics adapter"]
fn committed_sketch_primitives_render_without_a_followup_pointer_frame() {
    let mut app = App::new();
    app.add_plugins(
        interface_plugins()
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>()
            .set(WindowPlugin {
                primary_window: Some(Window {
                    resolution: bevy::window::WindowResolution::new(400, 300),
                    ..default()
                }),
                exit_condition: ExitCondition::DontExit,
                ..default()
            }),
    );
    app.insert_resource(NativeInterfaceHandle::new(|| {}))
        .init_resource::<Captured>();
    install(&mut app);
    let engine = AppState::new();
    call(
        &engine,
        "begin_sketch",
        json!({"type":"origin_plane","plane":"xy"}),
    );
    apply_interface_model(app.world_mut(), engine.viewport_frame()).unwrap();
    let owner = engine.viewport_frame().session_id.clone();
    *app.world_mut().resource_mut::<ViewportSizeResource>() = ViewportSizeResource {
        logical_width: 400.,
        logical_height: 300.,
    };
    apply_interface_view(
        app.world_mut(),
        &owner,
        Some(ViewportCamera {
            position: [0., 0., 160.],
            target: [0.; 3],
            up: [0., 1., 0.],
            ..default()
        }),
        Some(ViewportPresentation {
            mode: ViewportMode::Sketch,
            hide_sketch_grid: true,
            ..default()
        }),
    )
    .unwrap();
    app.finish();
    app.cleanup();
    app.main_mut().update();
    let target = app
        .world_mut()
        .resource_mut::<Assets<Image>>()
        .add(target_image(UVec2::new(400, 300)));
    let camera = app
        .world_mut()
        .query_filtered::<Entity, With<NativeOverlayCamera>>()
        .single(app.world())
        .unwrap();
    app.world_mut()
        .entity_mut(camera)
        .insert(bevy::camera::RenderTarget::Image(target.clone().into()));
    ready(&mut app);

    let p1 = json!({"x":-12.,"y":-8.});
    let p2 = json!({"x":12.,"y":8.});
    let p3 = json!({"x":18.,"y":-6.});
    for (index, (operation, arguments)) in [
        (
            "add_rectangle",
            json!({"mode":"two_point","p1":p1,"p2":p2,"ctrl_held":true}),
        ),
        (
            "add_rectangle",
            json!({"mode":"center","p1":p1,"p2":p2,"ctrl_held":true}),
        ),
        ("add_line", json!({"from":p1,"to_raw":p2,"ctrl_held":true})),
        (
            "add_line_midpoint",
            json!({"mid_raw":p1,"end_raw":p2,"ctrl_held":true}),
        ),
        ("add_point", json!({"position":p2,"ctrl_held":true})),
        (
            "add_circle",
            json!({"mode":"center_diameter","p1":p1,"p2":p2,"ctrl_held":true}),
        ),
        (
            "add_circle",
            json!({"mode":"two_point","p1":p1,"p2":p2,"ctrl_held":true}),
        ),
        (
            "add_arc_3pt",
            json!({"p1":p1,"p2":p2,"p3":p3,"ctrl_held":true}),
        ),
        (
            "add_arc_center",
            json!({"center":p1,"start":p2,"sweep":p3,"ctrl_held":true}),
        ),
        (
            "add_slot",
            json!({"mode":"center_to_center","p1":p1,"p2":p2,"cursor":p3,"width_mm":6.,"ctrl_held":true}),
        ),
        (
            "add_slot",
            json!({"mode":"overall","p1":p1,"p2":p2,"cursor":p3,"width_mm":6.,"ctrl_held":true}),
        ),
        (
            "add_slot",
            json!({"mode":"center_point","p1":p1,"p2":p2,"cursor":p3,"width_mm":6.,"ctrl_held":true}),
        ),
        ("add_spline", json!({"points":[p1,p2,p3]})),
    ].into_iter().enumerate() {
        let empty = capture(&mut app, &target, "sketch-empty");
        call(&engine, operation, arguments);
        apply_interface_model(app.world_mut(), engine.viewport_frame()).unwrap();
        app.update();
        ready(&mut app);
        let first = capture(&mut app, &target, &format!("{index:02}-{operation}"));
        assert!(
            changed(&empty, &first) > 20,
            "{operation}: the committed primitive must be visible immediately"
        );
        // Force the extra scene request a later mouse move used to provide.
        // Identical geometry must already have identical pixels before it.
        app.world_mut()
            .resource_mut::<CameraResource>()
            .set_changed();
        app.update();
        ready(&mut app);
        let second = capture(&mut app, &target, "sketch-followup");
        assert_eq!(
            changed(&first, &second),
            0,
            "{operation}: waiting for another pointer frame must not reveal missing geometry"
        );
        call(&engine, "undo", Value::Null);
        apply_interface_model(app.world_mut(), engine.viewport_frame()).unwrap();
        app.update();
        ready(&mut app);
    }
}
