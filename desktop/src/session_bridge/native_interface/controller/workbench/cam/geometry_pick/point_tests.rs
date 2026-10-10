use super::*;
use crate::session_bridge::{native_interface::tests::Fixture, parse_engine_envelope};
use std::{
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

pub(super) fn setup(
    fixture: &Fixture,
) -> (
    App,
    NativeInterfaceHandle,
    NativeServices,
    DocumentReceipt,
    Vec2,
) {
    let owner = fixture.owner();
    let receipt = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &owner)
        .unwrap();
    let bounds = InterfaceRect {
        x: 240.,
        y: 120.,
        width: 1120.,
        height: 640.,
    };
    let mut app = projection_app(&owner, bounds);
    native_viewport::apply_interface_model(
        app.world_mut(),
        ViewportModel {
            session_id: owner.document_id.clone(),
            geometry_revision: 2,
            body_poses: std::sync::Arc::default(),
            instance_body_poses: std::sync::Arc::default(),
            document: std::sync::Arc::new(limo_cad_native_engine::NativeViewportDocument {
                scene: std::sync::Arc::new(scene()),
                active_sketch: None,
                finished_sketches: vec![],
                datum_planes: vec![],
                profile_catalog: vec![],
                body_appearances: vec![],
                ..Default::default()
            }),
        },
    )
    .unwrap();
    native_viewport::apply_interface_view(
        app.world_mut(),
        &owner.document_id,
        Some(ViewportCamera {
            position: [150., 100., 130.],
            target: [15., 25., -6.],
            up: [0., 0., 1.],
            vertical_fov_degrees: 45.,
        }),
        None,
    )
    .unwrap();
    let handle = NativeInterfaceHandle::new(|| {});
    publish(app.world_mut(), &handle, owner.clone(), bounds);
    app.world_mut()
        .insert_resource(super::super::super::super::Workbench {
            owner: Some(owner.clone()),
            workspace: Workspace::Cam,
            ..default()
        });
    let mut cam = cam("contour2d");
    cam.setups[0].operations.clear();
    cam.height_expressions.clear();
    let mut draft = Draft::new(&cam, Selection::Setup(3)).unwrap();
    super::super::super::setup::extend(&mut draft, &cam, &scene(), &[]).unwrap();
    form::set(&mut draft, "/native/setup/origin", "stock_box_point");
    let editor = Editor {
        owner: Some(owner.clone()),
        revision: receipt.revision,
        cam,
        tab: Tab::Setups,
        draft: Some(draft),
        ..default()
    };
    toggle(app.world_mut(), &handle, &receipt, &editor).unwrap();
    app.world_mut().insert_resource(editor);
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    while loading(app.world()) {
        tick(app.world_mut(), &handle, &services).unwrap();
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    let projected =
        native_viewport::interface_world_point(app.world(), &owner.document_id, [30., 50., 0.])
            .unwrap()
            .unwrap();
    let cursor = Vec2::new(
        projected[0] + bounds.x as f32,
        projected[1] + bounds.y as f32,
    );
    (app, handle, services, receipt, cursor)
}
fn pointer(owner: &DocumentContext, cursor: Vec2, press: bool) -> NativeHostInput {
    NativeHostInput {
        ui_scale: 1.,
        context: Some(owner.clone()),
        cursor: Some(cursor),
        modifiers: default(),
        consumed: false,
        actions: vec![],
        event: WindowEvent::MouseButtonInput(bevy::input::mouse::MouseButtonInput {
            window: Entity::PLACEHOLDER,
            button: MouseButton::Left,
            state: if press {
                ButtonState::Pressed
            } else {
                ButtonState::Released
            },
        }),
    }
}
fn anchors(world: &World) -> Vec<String> {
    let draft = world.resource::<Editor>().draft.as_ref().unwrap();
    ["x", "y", "z"]
        .into_iter()
        .map(|axis| {
            form::text(draft, &format!("/native/setup/anchor/{axis}"))
                .unwrap()
                .into()
        })
        .collect()
}
#[test]
fn wcs_projected_handle_stages_only_on_complete_release_and_retires_one_picker_owner() {
    let _lock = crate::session_bridge::tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let fixture = Fixture::new();
    let before =
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap();
    let (mut app, handle, services, receipt, cursor) = setup(&fixture);
    let baseline = anchors(app.world());
    input(
        app.world_mut(),
        &handle,
        &services,
        &pointer(&receipt.owner, cursor, true),
    )
    .unwrap();
    let mut left = pointer(&receipt.owner, cursor, false);
    left.event = WindowEvent::CursorLeft(bevy::window::CursorLeft {
        window: Entity::PLACEHOLDER,
    });
    input(app.world_mut(), &handle, &services, &left).unwrap();
    input(
        app.world_mut(),
        &handle,
        &services,
        &pointer(&receipt.owner, cursor, false),
    )
    .unwrap();
    assert!(active(app.world()));
    assert_eq!(
        anchors(app.world()),
        baseline,
        "A release outside cannot use a stale client coordinate"
    );
    assert_eq!(
        overlay(app.world())
            .unwrap()
            .points
            .iter()
            .map(|layer| layer.positions.len() / 3)
            .sum::<usize>(),
        27
    );
    assert!(input(
        app.world_mut(),
        &handle,
        &services,
        &pointer(&receipt.owner, cursor, true)
    )
    .unwrap());
    assert_eq!(anchors(app.world()), baseline);
    assert!(active(app.world()));
    assert!(settled(app.world()).is_err());
    assert!(input(
        app.world_mut(),
        &handle,
        &services,
        &pointer(&receipt.owner, cursor, false)
    )
    .unwrap());
    assert_eq!(anchors(app.world()), vec!["max", "max", "max"]);
    assert!(!active(app.world()));
    assert!(overlay(app.world()).is_none());
    assert!(!input(
        app.world_mut(),
        &handle,
        &services,
        &pointer(&receipt.owner, cursor, false)
    )
    .unwrap());
    assert_eq!(
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap(),
        before
    );
    assert_eq!(
        fixture
            .bridge
            .native_document_receipt(&fixture.engine, &receipt.owner)
            .unwrap(),
        receipt
    );
}
#[test]
fn wcs_release_cannot_cross_form_camera_focus_modal_or_source_changes() {
    let _lock = crate::session_bridge::tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    for reason in ["form", "camera", "focus", "modal", "source", "escape"] {
        let fixture = Fixture::new();
        let (mut app, handle, services, receipt, cursor) = setup(&fixture);
        let before = anchors(app.world());
        input(
            app.world_mut(),
            &handle,
            &services,
            &pointer(&receipt.owner, cursor, true),
        )
        .unwrap();
        match reason {
            "form" => {
                let mut editor = app.world_mut().resource_mut::<Editor>();
                form::set(
                    editor.draft.as_mut().unwrap(),
                    "/native/setup/offset/x_max",
                    "9",
                );
            }
            "camera" => {
                native_viewport::apply_interface_view(
                    app.world_mut(),
                    &receipt.owner.document_id,
                    Some(ViewportCamera::default()),
                    None,
                )
                .unwrap();
            }
            "modal" => {
                let mut frame = handle.frame().unwrap();
                frame.modal_stack.push("dialog".into());
                handle.present(frame).unwrap();
            }
            "source" => {
                native_viewport::apply_interface_model(
                    app.world_mut(),
                    ViewportModel {
                        session_id: receipt.owner.document_id.clone(),
                        geometry_revision: 3,
                        body_poses: std::sync::Arc::default(),
                        instance_body_poses: std::sync::Arc::default(),
                        document: std::sync::Arc::new(
                            limo_cad_native_engine::NativeViewportDocument {
                                scene: std::sync::Arc::new(scene()),
                                active_sketch: None,
                                finished_sketches: vec![],
                                datum_planes: vec![],
                                profile_catalog: vec![],
                                body_appearances: vec![],
                                ..Default::default()
                            },
                        ),
                    },
                )
                .unwrap();
            }
            "focus" => {
                let mut event = pointer(&receipt.owner, cursor, false);
                event.context = None;
                event.event = WindowEvent::WindowFocused(bevy::window::WindowFocused {
                    window: Entity::PLACEHOLDER,
                    focused: false,
                });
                input(app.world_mut(), &handle, &services, &event).unwrap();
            }
            "escape" => {
                let mut event = pointer(&receipt.owner, cursor, false);
                event.event = WindowEvent::KeyboardInput(bevy::input::keyboard::KeyboardInput {
                    window: Entity::PLACEHOLDER,
                    logical_key: Key::Escape,
                    key_code: bevy::input::keyboard::KeyCode::Escape,
                    state: ButtonState::Pressed,
                    text: None,
                    repeat: false,
                });
                assert!(input(app.world_mut(), &handle, &services, &event).unwrap());
            }
            _ => unreachable!(),
        }
        if !matches!(reason, "focus" | "escape") {
            assert!(
                input(
                    app.world_mut(),
                    &handle,
                    &services,
                    &pointer(&receipt.owner, cursor, false)
                )
                .is_err(),
                "{reason}"
            );
        }
        assert!(!active(app.world()), "{reason}");
        assert_eq!(anchors(app.world()), before, "{reason}");
    }
}
#[test]
fn wcs_inspect_and_capture_polls_preserve_release_and_stage_once_after_idle() {
    use crate::session_bridge::native_interface::controller;
    let _lock = crate::session_bridge::tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    for action in ["inspect", "capture"] {
        let fixture = Fixture::new();
        let (mut app, handle, services, receipt, cursor) = setup(&fixture);
        app.init_resource::<Messages<NativeHostInput>>();
        controller::worker::install(app.world_mut(), services.clone(), handle.clone()).unwrap();
        let mut controller = controller::Controller::new(
            receipt.owner.window_id.clone(),
            None,
            Arc::new(AtomicBool::new(false)),
        );
        controller.synchronized = Some((receipt.owner.clone(), receipt.revision));
        let session = fixture
            .bridge
            .session_id_for_window(&receipt.owner.window_id)
            .unwrap()
            .unwrap();
        let id = format!("991-{}", usize::from(action == "capture"));
        let directory = crate::session_bridge::session_root()
            .join(&session)
            .join("controls");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(format!("{id}.request.json")),
            json!({"id":id,"expires_ms":now_ms()+30_000,"ui":{"action":action}}).to_string(),
        )
        .unwrap();
        controller::worker::enqueue_control_poll(
            app.world_mut(),
            receipt.owner.clone(),
            id.clone(),
        )
        .unwrap();
        controller.polled_control = Some(controller::PolledControl {
            owner: receipt.owner.clone(),
            session,
            id,
            interface_only: true,
        });
        let before = anchors(app.world());
        for press in [true, false] {
            app.world_mut()
                .write_message(pointer(&receipt.owner, cursor, press));
        }
        controller::maintain_busy_window(app.world_mut(), &handle, &mut controller).unwrap();
        assert!(active(app.world()));
        assert_eq!(anchors(app.world()), before);
        assert_eq!(
            controller.deferred_pointer.as_ref().unwrap().events.len(),
            2
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(outcome) = controller::worker::poll(app.world_mut(), &services) {
                assert_eq!(
                    outcome.value.unwrap()["control_request"]["ui"]["action"],
                    action
                );
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        controller.polled_control = None;
        let replay = controller::take_deferred_pointer_input(
            app.world_mut(),
            &handle,
            &services,
            &mut controller,
        )
        .unwrap();
        assert_eq!(replay.len(), 2);
        for event in replay {
            input(app.world_mut(), &handle, &services, &event).unwrap();
        }
        assert_eq!(anchors(app.world()), vec!["max", "max", "max"]);
        assert!(!active(app.world()));
        assert!(controller::take_deferred_pointer_input(
            app.world_mut(),
            &handle,
            &services,
            &mut controller
        )
        .unwrap()
        .is_empty());
        assert_eq!(
            fixture
                .bridge
                .native_document_receipt(&fixture.engine, &receipt.owner)
                .unwrap(),
            receipt
        );
    }
}
