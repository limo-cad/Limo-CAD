use super::super::*;
use super::{fields, runtime};
use crate::native_viewport::winit_host::HostInputState;
use crate::session_bridge::native_interface::tests::Fixture;
use bevy::ui::{ComputedStackIndex, UiGlobalTransform};

fn setup(fixture: &Fixture) -> (App, NativeInterfaceHandle, NativeServices) {
    let owner = fixture.owner();
    for (operation, arguments) in [
        (
            "drawing_create_sheet",
            json!({"name":"Paper input","format":"a4","orientation":"landscape"}),
        ),
        (
            "drawing_add_note",
            json!({"sheet_id":1,"text":"Saved note","position":[20.,30.]}),
        ),
    ] {
        fixture
            .bridge
            .apply_native_mutation(&fixture.engine, &owner, operation, &arguments, || Ok(()))
            .unwrap();
    }
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let (_, handle, _, _) = interface_shell::tests::fixture();
    let mut app = native_viewport::interface_scene_fixture();
    let world = app.world_mut();
    world.init_resource::<Assets<Image>>();
    world.init_resource::<ViewportUiAssets>();
    world.init_resource::<HostInputState>();
    world.spawn((Window::default(), PrimaryWindow));
    let camera = world.spawn(InterfaceCamera).id();
    let mut state = Workbench::default();
    state.refresh_owner(&owner);
    state.workspace = Workspace::Drawing;
    state.widgets.begin();
    drawing_paper::paint(
        world,
        camera,
        &services,
        &mut state,
        (1360., 860., 240.),
        &HashMap::new(),
    )
    .unwrap();
    state.widgets.finish(world);
    let clip = state
        .paper_view
        .as_ref()
        .unwrap()
        .navigation
        .transform()
        .clip;
    world
        .entity_mut(state.widgets.entity("drawing-content-clip").unwrap())
        .insert((
            ComputedNode {
                size: Vec2::new(clip.width as f32, clip.height as f32),
                inverse_scale_factor: 1.,
                ..default()
            },
            UiGlobalTransform::from_translation(Vec2::new(
                (clip.x + clip.width * 0.5) as f32,
                (clip.y + clip.height * 0.5) as f32,
            )),
            ComputedStackIndex(7),
            InheritedVisibility::VISIBLE,
        ));
    runtime::synchronize(world, camera, &services, &owner, (860., 240.), true, &state).unwrap();
    let mut frame = handle.frame().unwrap();
    frame.context = owner;
    frame.client = InterfaceRect {
        x: 0.,
        y: 0.,
        width: 1360.,
        height: 860.,
    };
    frame.surface = frame.client;
    frame.canvases = vec![drawing_paper::canvas(&state).unwrap()];
    handle.present(frame).unwrap();
    world.insert_resource(state);
    interface_shell::tests::publish_layout_once(world, handle.clone());
    (app, handle, services)
}

fn point(world: &World, paper: [f64; 2]) -> [f64; 2] {
    drawing_paper::transform(world.resource::<Workbench>())
        .unwrap()
        .to_screen(paper)
}

fn drive(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    request: Value,
) -> Result<Value, String> {
    crate::native_editor::mcp::drive(
        world,
        handle,
        services,
        &services
            .bridge
            .native_document_context("main", &services.engine)
            .unwrap(),
        &request,
    )
}

fn note_handle(world: &mut World, handle: &NativeInterfaceHandle) -> [f64; 2] {
    let cursor = point(world, [20., 30.]);
    let entity = world
        .resource::<runtime::Editor>()
        .widgets
        .entity("drawing-annotation-1")
        .unwrap();
    world.entity_mut(entity).insert((
        ComputedNode {
            size: Vec2::new(80., 24.),
            inverse_scale_factor: 1.,
            ..default()
        },
        UiGlobalTransform::from_translation(Vec2::new(cursor[0] as f32, cursor[1] as f32)),
        ComputedStackIndex(20),
        InheritedVisibility::VISIBLE,
    ));
    interface_shell::tests::publish_layout_once(world, handle.clone());
    assert!(drawing_canvas_control(world, handle, cursor));
    cursor
}

#[test]
fn mcp_paper_click_places_a_note_with_native_history() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, handle, services) = setup(&fixture);
    let before = fixture.engine.engine_call("project_export_model", "");
    let world = app.world_mut();
    let cursor = point(world, [70., 80.]);
    assert!(handle.owns_pointer(cursor));
    assert!(handle.canvas_owns_pointer("drawing", cursor));
    {
        let mut editor = world.resource_mut::<runtime::Editor>();
        editor.tool = Some(runtime::Tool::Note);
        editor.fields = fields::note_creation([70., 80.]);
    }
    let result = drive(
        world,
        &handle,
        &services,
        json!({"gesture":"click","point":cursor}),
    )
    .unwrap();
    assert_eq!(result["handled"], true);
    let drawing = fixture.engine.drawing_snapshot();
    assert_eq!(drawing.sheets[0].annotations.len(), 2);
    let limo_cad_sketch::DrawingAnnotationDto::Note { position, .. } =
        &drawing.sheets[0].annotations[1]
    else {
        panic!("Expected a note")
    };
    assert!((position[0] - 70.).abs() < 1e-4 && (position[1] - 80.).abs() < 1e-4);
    let after = fixture.engine.engine_call("project_export_model", "");
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &fixture.owner(), false, || Ok(()))
        .unwrap();
    assert_eq!(
        fixture.engine.engine_call("project_export_model", ""),
        before
    );
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &fixture.owner(), true, || Ok(()))
        .unwrap();
    assert_eq!(
        fixture.engine.engine_call("project_export_model", ""),
        after
    );
}

#[test]
fn mcp_paper_annotation_drag_commits_once_and_cancels_on_host_lifecycle() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    for lifecycle in [None, Some("unfocus"), Some("scale")] {
        let fixture = Fixture::new();
        let (mut app, handle, services) = setup(&fixture);
        let before = fixture.engine.engine_call("project_export_model", "");
        let world = app.world_mut();
        let cursor = note_handle(world, &handle);
        let end = point(world, [40., 45.]);
        let mut request = json!({"gesture":"drag","canvas":"drawing","point":cursor,"to":end});
        if let Some(lifecycle) = lifecycle {
            request["lifecycle"] = json!(lifecycle);
        }
        assert_eq!(
            drive(world, &handle, &services, request).unwrap()["handled"],
            true
        );
        assert!(!runtime::pointer_active(world));
        let after = fixture.engine.engine_call("project_export_model", "");
        if lifecycle.is_some() {
            assert_eq!(
                after, before,
                "A cancelled preview must not enter document history"
            );
        } else {
            let drawing = fixture.engine.drawing_snapshot();
            let limo_cad_sketch::DrawingAnnotationDto::Note { position, .. } =
                &drawing.sheets[0].annotations[0]
            else {
                panic!("Expected a note")
            };
            assert!((position[0] - 40.).abs() < 1e-4 && (position[1] - 45.).abs() < 1e-4);
            assert_eq!(drawing.sheets[0].annotations.len(), 1);
            fixture
                .bridge
                .apply_native_history(&fixture.engine, &fixture.owner(), false, || Ok(()))
                .unwrap();
            assert_eq!(
                fixture.engine.engine_call("project_export_model", ""),
                before
            );
            fixture
                .bridge
                .apply_native_history(&fixture.engine, &fixture.owner(), true, || Ok(()))
                .unwrap();
            assert_eq!(
                fixture.engine.engine_call("project_export_model", ""),
                after
            );
        }
    }
}

#[test]
fn mcp_middle_button_pans_paper_without_placing_a_note() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, handle, services) = setup(&fixture);
    let before = fixture.engine.engine_call("project_export_model", "");
    let world = app.world_mut();
    world.resource_mut::<runtime::Editor>().tool = Some(runtime::Tool::Note);
    let cursor = point(world, [70., 80.]);
    world
        .resource_mut::<Workbench>()
        .paper_view
        .as_mut()
        .unwrap()
        .navigation
        .zoom_at(2., Some(cursor));
    let original = point(world, [0., 0.]);
    let result=drive(world,&handle,&services,json!({"gesture":"drag","button":"middle","canvas":"drawing","point":cursor,"to":[cursor[0]+40.,cursor[1]+30.]})).unwrap();
    assert_eq!(result["navigation"], true);
    let panned = point(world, [0., 0.]);
    assert!((panned[0] - original[0] - 40.).abs() < 1e-4);
    assert!((panned[1] - original[1] - 30.).abs() < 1e-4);
    assert_eq!(
        fixture.engine.engine_call("project_export_model", ""),
        before
    );
}

#[test]
fn mcp_paper_rejects_world_points_overlays_and_retired_owners() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, handle, services) = setup(&fixture);
    let before = fixture.engine.engine_call("project_export_model", "");
    let world = app.world_mut();
    let cursor = point(world, [70., 80.]);
    assert!(drive(
        world,
        &handle,
        &services,
        json!({"gesture":"click","canvas":"drawing","world":[0.,0.,0.]})
    )
    .unwrap_err()
    .contains("client point"));
    assert!(drive(
        world,
        &handle,
        &services,
        json!({"gesture":"click","canvas":"drawing","point":[-1.,-1.]})
    )
    .unwrap_err()
    .contains("outside"));
    let overlay = world
        .spawn((
            interface_shell::InterfaceOccluder,
            ComputedNode {
                size: Vec2::new(100., 40.),
                inverse_scale_factor: 1.,
                ..default()
            },
            UiGlobalTransform::from_translation(Vec2::new(cursor[0] as f32, cursor[1] as f32)),
            ComputedStackIndex(99),
            InheritedVisibility::VISIBLE,
        ))
        .id();
    interface_shell::tests::publish_layout_once(world, handle.clone());
    assert!(!handle.canvas_owns_pointer("drawing", cursor));
    assert!(drive(
        world,
        &handle,
        &services,
        json!({"gesture":"click","canvas":"drawing","point":cursor})
    )
    .unwrap_err()
    .contains("covers"));
    world.despawn(overlay);
    world.spawn((
        InterfaceControl::button("document/session", "Overlay"),
        ComputedNode {
            size: Vec2::new(100., 40.),
            inverse_scale_factor: 1.,
            ..default()
        },
        UiGlobalTransform::from_translation(Vec2::new(cursor[0] as f32, cursor[1] as f32)),
        ComputedStackIndex(99),
        InheritedVisibility::VISIBLE,
    ));
    interface_shell::tests::publish_layout_once(world, handle.clone());
    assert!(drive(
        world,
        &handle,
        &services,
        json!({"gesture":"click","canvas":"drawing","point":cursor})
    )
    .unwrap_err()
    .contains("covers"));
    let mut owner = fixture.owner();
    owner.epoch += 1;
    assert!(crate::native_editor::mcp::drive(
        world,
        &handle,
        &services,
        &owner,
        &json!({"gesture":"click","canvas":"drawing","point":cursor})
    )
    .unwrap_err()
    .contains("retired"));
    assert_eq!(
        fixture.engine.engine_call("project_export_model", ""),
        before
    );
}
