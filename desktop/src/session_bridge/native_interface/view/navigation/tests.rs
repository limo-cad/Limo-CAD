use super::*;
use crate::{
    native_viewport::interface_shell::{self, InterfaceOccluder},
    native_viewport::winit_host::Modifiers,
    session_bridge::native_interface::{controller::NativeServices, tests::Fixture},
};
use bevy::{
    input::{
        gestures::PinchGesture,
        keyboard::{Key, KeyCode, KeyboardInput},
        mouse::{MouseButtonInput, MouseWheel},
    },
    prelude::*,
    ui::{ComputedStackIndex, UiGlobalTransform},
    window::{CursorMoved, WindowFocused},
};

fn setup(fixture: &Fixture) -> (App, NativeInterfaceHandle) {
    let (_, handle, _, _) = interface_shell::tests::fixture();
    let mut frame = handle.frame().unwrap();
    frame.context = fixture.owner();
    handle.present(frame).unwrap();
    let mut app = native_viewport::interface_scene_fixture();
    native_viewport::apply_interface_model(app.world_mut(), model_snapshot(&fixture.engine))
        .unwrap();
    interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
    (app, handle)
}

fn input(handle: &NativeInterfaceHandle, cursor: [f32; 2], event: WindowEvent) -> NativeHostInput {
    NativeHostInput {
        ui_scale: 1.,
        context: handle.frame().map(|f| f.context),
        cursor: Some(Vec2::from_array(cursor)),
        modifiers: Modifiers::default(),
        event,
        consumed: false,
        actions: vec![],
    }
}
fn button(button: MouseButton, state: ButtonState) -> WindowEvent {
    WindowEvent::MouseButtonInput(MouseButtonInput {
        button,
        state,
        window: Entity::PLACEHOLDER,
    })
}
fn moved(point: [f32; 2]) -> WindowEvent {
    WindowEvent::CursorMoved(CursorMoved {
        window: Entity::PLACEHOLDER,
        position: Vec2::from_array(point),
        delta: None,
    })
}
fn wheel(unit: MouseScrollUnit, x: f32, y: f32) -> WindowEvent {
    WindowEvent::MouseWheel(MouseWheel {
        unit,
        x,
        y,
        window: Entity::PLACEHOLDER,
        phase: bevy::input::touch::TouchPhase::Moved,
    })
}
fn camera(world: &World) -> ViewportCamera {
    native_viewport::interface_camera_snapshot(world).1
}
fn distance(camera: ViewportCamera) -> f32 {
    Vec3::from_array(camera.position).distance(Vec3::from_array(camera.target))
}

#[test]
fn pointer_pan_orbit_wheel_and_pinch_move_only_the_camera() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, handle) = setup(&fixture);
    let before = fixture.engine.engine_call("project_export_model", "");
    let receipt = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &fixture.owner())
        .unwrap();
    let original = camera(app.world());
    let send = |world: &mut World, p, e| navigate(world, &handle, &input(&handle, p, e)).unwrap();
    assert!(send(
        app.world_mut(),
        [300., 300.],
        button(MouseButton::Middle, ButtonState::Pressed)
    ));
    let mut movement = input(&handle, [340., 325.], moved([340., 325.]));
    movement.modifiers.shift = true;
    assert!(navigate(app.world_mut(), &handle, &movement).unwrap());
    let panned = camera(app.world());
    let eye_shift = Vec3::from_array(panned.position) - Vec3::from_array(original.position);
    let target_shift = Vec3::from_array(panned.target) - Vec3::from_array(original.target);
    assert!(eye_shift.length() > 0.1);
    assert!((eye_shift - target_shift).length() < 1e-4);
    assert!(send(
        app.world_mut(),
        [340., 325.],
        button(MouseButton::Middle, ButtonState::Released)
    ));
    assert!(!send(app.world_mut(), [360., 350.], moved([360., 350.])));
    assert_eq!(camera(app.world()), panned);

    for button_kind in [MouseButton::Right, MouseButton::Middle] {
        let start = camera(app.world());
        let mut press = input(
            &handle,
            [300., 300.],
            button(button_kind, ButtonState::Pressed),
        );
        press.modifiers.shift = button_kind == MouseButton::Middle;
        assert!(navigate(app.world_mut(), &handle, &press).unwrap());
        assert!(send(app.world_mut(), [370., 330.], moved([370., 330.])));
        let orbited = camera(app.world());
        assert_ne!(orbited.position, start.position);
        assert_eq!(orbited.target, start.target);
        assert!((distance(orbited) - distance(start)).abs() < 1e-3);
        assert!(send(
            app.world_mut(),
            [370., 330.],
            button(button_kind, ButtonState::Released)
        ));
    }
    let start = camera(app.world());
    assert!(send(
        app.world_mut(),
        [300., 300.],
        wheel(MouseScrollUnit::Line, 0., 3.)
    ));
    assert!(distance(camera(app.world())) < distance(start));
    let zoomed = camera(app.world());
    assert!(send(
        app.world_mut(),
        [300., 300.],
        wheel(MouseScrollUnit::Pixel, 20., -10.)
    ));
    assert_ne!(camera(app.world()).target, zoomed.target);
    assert!((distance(camera(app.world())) - distance(zoomed)).abs() < 1e-3);
    let start = camera(app.world());
    assert!(send(
        app.world_mut(),
        [300., 300.],
        WindowEvent::PinchGesture(PinchGesture(0.15))
    ));
    assert!(distance(camera(app.world())) < distance(start));
    assert_eq!(
        fixture.engine.engine_call("project_export_model", ""),
        before
    );
    assert_eq!(
        fixture
            .bridge
            .native_document_receipt(&fixture.engine, &fixture.owner())
            .unwrap(),
        receipt
    );
}

#[test]
fn navigation_respects_controls_panels_modals_and_document_incarnations() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, handle) = setup(&fixture);
    let before = camera(app.world());
    let mut event = input(&handle, [300., 300.], wheel(MouseScrollUnit::Line, 0., 2.));
    event.context.as_mut().unwrap().epoch += 1;
    assert!(!navigate(app.world_mut(), &handle, &event).unwrap());
    event.context = Some(fixture.owner());
    event.cursor = Some(Vec2::new(99., 300.));
    assert!(!navigate(app.world_mut(), &handle, &event).unwrap());
    event.cursor = Some(Vec2::new(300., 300.));
    event.consumed = true;
    assert!(!navigate(app.world_mut(), &handle, &event).unwrap());
    event.consumed = false;

    let overlay = app
        .world_mut()
        .spawn((
            InterfaceOccluder,
            ComputedNode {
                size: Vec2::splat(100.),
                inverse_scale_factor: 1.,
                ..default()
            },
            UiGlobalTransform::from_translation(Vec2::new(200., 200.)),
            ComputedStackIndex(2),
            InheritedVisibility::VISIBLE,
        ))
        .id();
    interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
    assert!(handle.owns_pointer([300., 300.]));
    assert!(!navigate(app.world_mut(), &handle, &event).unwrap());
    app.world_mut().despawn(overlay);
    let mut frame = handle.frame().unwrap();
    frame.modal_stack.push("file-dialog".into());
    frame.surfaces.push(limo_cad_interface::Surface {
        name: "file-dialog".into(),
        text: None,
    });
    handle.present(frame).unwrap();
    interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
    assert!(!navigate(app.world_mut(), &handle, &event).unwrap());
    assert_eq!(camera(app.world()), before);

    let mut frame = handle.frame().unwrap();
    frame.modal_stack.clear();
    handle.present(frame).unwrap();
    interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
    let press = input(
        &handle,
        [300., 300.],
        button(MouseButton::Right, ButtonState::Pressed),
    );
    navigate(app.world_mut(), &handle, &press).unwrap();
    let mut frame = handle.frame().unwrap();
    frame.context.epoch += 1;
    handle.present(frame).unwrap();
    interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
    let event = input(&handle, [360., 350.], moved([360., 350.]));
    assert!(!navigate(app.world_mut(), &handle, &event).unwrap());
    assert_eq!(camera(app.world()), before);
}

#[test]
fn escape_and_focus_loss_cancel_drags_and_pointer_navigation_cancels_timed_motion() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, handle) = setup(&fixture);
    let owner = fixture.owner();
    let revision = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &owner)
        .unwrap()
        .revision;
    let transition = super::super::request(app.world_mut(), &owner, revision,
        &json!({"view":"front","duration_ms":500,"expires_ms":crate::session_bridge::now_ms()+10_000})).unwrap()["camera_pending"].as_u64().unwrap();
    let before = camera(app.world());
    let press = input(
        &handle,
        [300., 300.],
        button(MouseButton::Middle, ButtonState::Pressed),
    );
    assert!(navigate(app.world_mut(), &handle, &press).unwrap());
    assert!(super::super::poll(app.world_mut(), transition)
        .unwrap()
        .unwrap_err()
        .contains("pointer navigation"));
    for cancel in [
        WindowEvent::KeyboardInput(KeyboardInput {
            key_code: KeyCode::Escape,
            logical_key: Key::Escape,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        }),
        WindowEvent::WindowFocused(WindowFocused {
            window: Entity::PLACEHOLDER,
            focused: false,
        }),
    ] {
        navigate(app.world_mut(), &handle, &press).unwrap();
        navigate(
            app.world_mut(),
            &handle,
            &input(&handle, [300., 300.], cancel),
        )
        .unwrap();
        assert!(!navigate(
            app.world_mut(),
            &handle,
            &input(&handle, [400., 400.], moved([400., 400.]))
        )
        .unwrap());
        assert_eq!(camera(app.world()), before);
    }
}

#[test]
fn extreme_gestures_stay_finite_in_every_standard_orientation() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, handle) = setup(&fixture);
    for direction in [
        ViewDirection::Front,
        ViewDirection::Back,
        ViewDirection::Left,
        ViewDirection::Right,
        ViewDirection::Top,
        ViewDirection::Bottom,
    ] {
        let (axis, up) = direction.axes();
        let start = ViewportCamera {
            position: (axis * 100.).to_array(),
            target: [0.; 3],
            up: up.to_array(),
            ..ViewportCamera::default()
        };
        native_viewport::apply_interface_view(
            app.world_mut(),
            &fixture.owner().document_id,
            Some(start),
            None,
        )
        .unwrap();
        let press = input(
            &handle,
            [300., 300.],
            button(MouseButton::Right, ButtonState::Pressed),
        );
        navigate(app.world_mut(), &handle, &press).unwrap();
        let event = input(&handle, [f32::MAX, -f32::MAX], moved([f32::MAX, -f32::MAX]));
        navigate(app.world_mut(), &handle, &event).unwrap();
        super::super::motion::pose(camera(app.world())).unwrap();
        assert!((distance(camera(app.world())) - 100.).abs() < 1e-3);
        let release = input(
            &handle,
            [300., 300.],
            button(MouseButton::Right, ButtonState::Released),
        );
        navigate(app.world_mut(), &handle, &release).unwrap();
        let event = input(
            &handle,
            [300., 300.],
            wheel(MouseScrollUnit::Line, 0., f32::NAN),
        );
        let before = camera(app.world());
        assert!(!navigate(app.world_mut(), &handle, &event).unwrap());
        assert_eq!(camera(app.world()), before);
    }
}

#[test]
fn trackpad_pan_and_orbit_have_the_same_scale_on_retina_displays() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, handle) = setup(&fixture);
    let initial = camera(app.world());
    for shift in [false, true] {
        let mut outputs = Vec::new();
        for scale in [1., 2.] {
            let window = app
                .world_mut()
                .spawn(Window {
                    resolution: bevy::window::WindowResolution::new(1000, 800)
                        .with_scale_factor_override(scale),
                    ..default()
                })
                .id();
            native_viewport::apply_interface_view(
                app.world_mut(),
                &fixture.owner().document_id,
                Some(initial),
                None,
            )
            .unwrap();
            let event = WindowEvent::MouseWheel(MouseWheel {
                unit: MouseScrollUnit::Pixel,
                x: 20. * scale,
                y: 10. * scale,
                window,
                phase: bevy::input::touch::TouchPhase::Moved,
            });
            let mut input = input(&handle, [300., 300.], event);
            input.modifiers.shift = shift;
            assert!(navigate(app.world_mut(), &handle, &input).unwrap());
            outputs.push(camera(app.world()));
            app.world_mut().despawn(window);
        }
        assert_ne!(outputs[0], initial);
        assert_eq!(outputs[0], outputs[1]);
    }
}

#[test]
fn toolbar_modes_capture_left_drag_and_window_zoom_without_editing_geometry() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, handle) = setup(&fixture);
    let before = fixture.engine.engine_call("project_export_model", "");
    for tool in [
        NavigationTool::Pan,
        NavigationTool::Orbit,
        NavigationTool::Zoom,
        NavigationTool::ZoomWindow,
    ] {
        workbench::execute(app.world_mut(), &workbench::Command::Navigation(tool)).unwrap();
        let original = camera(app.world());
        let send =
            |world: &mut World, p, e| navigate(world, &handle, &input(&handle, p, e)).unwrap();
        assert!(send(
            app.world_mut(),
            [300., 300.],
            button(MouseButton::Left, ButtonState::Pressed)
        ));
        assert!(send(app.world_mut(), [390., 370.], moved([390., 370.])));
        assert!(send(
            app.world_mut(),
            [390., 370.],
            button(MouseButton::Left, ButtonState::Released)
        ));
        assert_ne!(camera(app.world()), original, "{tool:?}");
        assert!(app
            .world()
            .get_resource::<workbench::NavigationRectangle>()
            .is_none_or(|r| r.0.is_none()));
    }
    assert_eq!(
        before,
        fixture.engine.engine_call("project_export_model", "")
    );
    assert!(navigate(
        app.world_mut(),
        &handle,
        &input(
            &handle,
            [300., 300.],
            WindowEvent::KeyboardInput(KeyboardInput {
                key_code: KeyCode::Escape,
                logical_key: Key::Escape,
                state: ButtonState::Pressed,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            })
        )
    )
    .unwrap());
    assert_eq!(workbench::navigation(app.world()), NavigationTool::Select);
    assert!(!navigate(
        app.world_mut(),
        &handle,
        &input(
            &handle,
            [300., 300.],
            button(MouseButton::Left, ButtonState::Pressed)
        )
    )
    .unwrap());
}

#[test]
fn mcp_toolbar_drags_match_native_navigation_without_editing_geometry() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut native, native_handle) = setup(&fixture);
    let (mut mcp, mcp_handle) = setup(&fixture);
    mcp.world_mut()
        .init_resource::<crate::native_viewport::winit_host::HostInputState>();
    mcp.world_mut()
        .spawn((Window::default(), bevy::window::PrimaryWindow));
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let owner = fixture.owner();
    let before = fixture.engine.engine_call("project_export_model", "");
    let receipt = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &owner)
        .unwrap();
    for tool in [
        NavigationTool::Pan,
        NavigationTool::Orbit,
        NavigationTool::Zoom,
        NavigationTool::ZoomWindow,
    ] {
        for app in [&mut native, &mut mcp] {
            workbench::execute(app.world_mut(), &workbench::Command::Navigation(tool)).unwrap();
        }
        let original = camera(mcp.world());
        for (cursor, event) in [
            (
                [300., 300.],
                button(MouseButton::Left, ButtonState::Pressed),
            ),
            ([390., 370.], moved([390., 370.])),
            (
                [390., 370.],
                button(MouseButton::Left, ButtonState::Released),
            ),
        ] {
            assert!(navigate(
                native.world_mut(),
                &native_handle,
                &input(&native_handle, cursor, event),
            )
            .unwrap());
        }
        let value = crate::native_editor::mcp::drive(
            mcp.world_mut(),
            &mcp_handle,
            &services,
            &owner,
            &json!({"gesture":"drag","point":[300.,300.],"to":[390.,370.]}),
        )
        .unwrap();
        assert_eq!(value["navigation"], true, "{tool:?}");
        assert_ne!(camera(mcp.world()), original, "{tool:?}");
        assert_eq!(camera(mcp.world()), camera(native.world()), "{tool:?}");
        assert!(!pointer_active(mcp.world()));
        assert!(mcp
            .world()
            .get_resource::<workbench::NavigationRectangle>()
            .is_none_or(|rectangle| rectangle.0.is_none()));
    }
    assert_eq!(
        fixture.engine.engine_call("project_export_model", ""),
        before
    );
    assert_eq!(
        fixture
            .bridge
            .native_document_receipt(&fixture.engine, &owner)
            .unwrap(),
        receipt,
    );
}

#[test]
fn mcp_navigation_preserves_an_unfinished_sketch_line() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner,
            "sketch_begin",
            &json!({"name":"Navigation draft","plane":{"type":"origin_plane","plane":"xy"}}),
            || Ok(()),
        )
        .unwrap();
    let (mut app, handle) = setup(&fixture);
    app.world_mut()
        .init_resource::<crate::native_viewport::winit_host::HostInputState>();
    app.world_mut()
        .spawn((Window::default(), bevy::window::PrimaryWindow));
    native_viewport::apply_interface_viewport(
        app.world_mut(),
        handle.frame().unwrap().canvases[0].bounds,
        1.,
    )
    .unwrap();
    let mut top = camera(app.world());
    top.position = [0., 0., 100.];
    top.target = [0., 0., 0.];
    top.up = [0., 1., 0.];
    native_viewport::apply_interface_view(app.world_mut(), &owner.document_id, Some(top), None)
        .unwrap();
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    crate::native_editor::execute(
        app.world_mut(),
        &fixture.engine,
        &fixture.bridge,
        &owner,
        crate::native_editor::EditorCommand::Tool(crate::native_editor::CreateTool::Line),
        || Ok(()),
    )
    .unwrap();
    crate::native_editor::mcp::drive(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        &json!({"gesture":"click","point":[300.,300.]}),
    )
    .unwrap();
    let before = crate::native_editor::active(&fixture.engine)
        .unwrap()
        .unwrap();
    let instruction = crate::native_editor::status(app.world());
    assert!(before.entities.is_empty());
    workbench::execute(
        app.world_mut(),
        &workbench::Command::Navigation(NavigationTool::Zoom),
    )
    .unwrap();
    let original = camera(app.world());
    let value = crate::native_editor::mcp::drive(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        &json!({"gesture":"drag","point":[300.,300.],"to":[390.,370.]}),
    )
    .unwrap();
    assert_eq!(value["navigation"], true);
    assert_ne!(camera(app.world()), original);
    assert_eq!(
        serde_json::to_value(crate::native_editor::active(&fixture.engine).unwrap()).unwrap(),
        serde_json::to_value(Some(before)).unwrap(),
    );
    assert_eq!(crate::native_editor::status(app.world()), instruction);
    assert!(!pointer_active(app.world()));
}
