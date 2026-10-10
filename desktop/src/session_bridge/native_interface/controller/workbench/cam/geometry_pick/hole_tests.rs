use super::*;
use crate::session_bridge::{native_interface::tests::Fixture, parse_engine_envelope};
use std::time::{Duration, Instant};

fn hole_scene() -> SolidSceneDto {
    serde_json::from_value(json!({"bodies":[{"id":11,"name":"Physical wall","feature_id":1,
        "mesh":{"positions":[-1.,0.,-6.,1.,0.,-6.,1.,0.,-1.,-1.,0.,-1.],"normals":[],"indices":[0,1,2,0,2,3]},
        "faces":[{"id":31,"key":"own-cylinder","first_index":0,"index_count":6,
        "cylinder":{"origin":{"x":0.,"y":0.,"z":-3.},"axis":{"x":0.,"y":0.,"z":1.},"reference":{"x":1.,"y":0.,"z":0.},"radius":1.}}],"edges":[]}],"errors":[]})).unwrap()
}
fn hole_cam(kind: &str) -> CamDocumentDto {
    let mut cam = cam("contour2d");
    cam.setups[0].operations[0] = serde_json::from_value(json!({"kind":kind,"id":7,"name":"Physical holes","tool_id":5,"enabled":true,
        "top_z":-1.,"bottom_z":-4.,"clearance_z":8.,"retract_z":2.,"feed_height_z":1.,
        "cutting":{"spindle_rpm":12000,"feed_xy":800.,"feed_z":200.},"points":[{"x":4.,"y":4.}],"holes":[],
        "pitch":1.,"major_diameter":8.,"minor_diameter":6.})).unwrap();
    cam.validate_for_editing().unwrap();
    cam
}
fn setup(
    fixture: &Fixture,
    kind: &str,
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
                scene: std::sync::Arc::new(hole_scene()),
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
            position: [0., -100., -3.],
            target: [0., 0., -3.],
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
    let cam = hole_cam(kind);
    let mut draft = Draft::new(&cam, Selection::Operation(7)).unwrap();
    operation_editor::extend(&mut draft, &cam, &hole_scene(), &[]).unwrap();
    edit(&mut draft, &cam, "/native/ui/operation_section", "geometry");
    let editor = Editor {
        owner: Some(owner.clone()),
        revision: receipt.revision,
        cam,
        tab: Tab::Toolpaths,
        draft: Some(draft),
        ..default()
    };
    toggle(app.world_mut(), &handle, &receipt, &editor).unwrap();
    app.world_mut().insert_resource(editor);
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    drain(&mut app, &handle, &services);
    (app, handle, services, receipt, Vec2::new(800., 440.))
}
fn drain(app: &mut App, handle: &NativeInterfaceHandle, services: &NativeServices) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        tick(app.world_mut(), handle, services).unwrap();
        if !loading(app.world()) && settled(app.world()).is_ok() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "hole picker worker did not settle"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
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
fn keys(world: &World) -> Vec<operation_geometry::hole_picking::FaceKey> {
    operation_geometry::hole_picking::snapshot(world.resource::<Editor>().draft.as_ref().unwrap())
        .unwrap()
        .keys
}
#[test]
fn physical_drill_and_thread_clicks_toggle_draft_only_and_share_the_single_worker_owner() {
    let _lock = crate::session_bridge::tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    for kind in ["drill", "thread"] {
        let fixture = Fixture::new();
        let before =
            parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap();
        let (mut app, handle, services, receipt, cursor) = setup(&fixture, kind);
        for expected in [1, 0, 1] {
            for press in [true, false] {
                assert!(input(
                    app.world_mut(),
                    &handle,
                    &services,
                    &pointer(&receipt.owner, cursor, press)
                )
                .unwrap());
            }
            drain(&mut app, &handle, &services);
            assert_eq!(keys(app.world()).len(), expected);
            if expected == 1 {
                let key = keys(app.world())[0];
                assert_eq!((key.body_id, key.face_id), (11, 31));
                assert_eq!(
                    overlay(app.world())
                        .unwrap()
                        .triangles
                        .iter()
                        .map(|layer| layer.positions.len())
                        .sum::<usize>(),
                    18
                );
            }
        }
        let editor = app.world().resource::<Editor>();
        let edited = editor.draft.as_ref().unwrap().edited(&editor.cam).unwrap();
        let operation = serde_json::to_value(&edited.setups[0].operations[0]).unwrap();
        assert_eq!(operation["points"], json!([{"x":4.,"y":4.}]));
        assert_eq!(operation["holes"][0]["face_key"], "11:31");
        assert_eq!(
            parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap(),
            before
        );
        cancel(app.world_mut(), &handle);
        assert!(!active(app.world()));
        assert!(overlay(app.world()).is_none());
        assert_eq!(
            fixture
                .bridge
                .native_document_receipt(&fixture.engine, &receipt.owner)
                .unwrap(),
            receipt
        );
    }
}
#[test]
fn hole_picker_rejects_form_view_and_focus_changes_before_worker_results_can_stage() {
    let _lock = crate::session_bridge::tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    for scenario in ["form", "camera", "focus", "modal"] {
        let fixture = Fixture::new();
        let (mut app, handle, services, receipt, cursor) = setup(&fixture, "drill");
        input(
            app.world_mut(),
            &handle,
            &services,
            &pointer(&receipt.owner, cursor, true),
        )
        .unwrap();
        match scenario {
            "form" => {
                let mut editor = app.world_mut().remove_resource::<Editor>().unwrap();
                edit(
                    editor.draft.as_mut().unwrap(),
                    &editor.cam,
                    "/native/ui/operation_section",
                    "parameters",
                );
                app.world_mut().insert_resource(editor);
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
            "focus" => {
                let mut event = pointer(&receipt.owner, cursor, false);
                event.context = None;
                event.event = WindowEvent::WindowFocused(bevy::window::WindowFocused {
                    window: Entity::PLACEHOLDER,
                    focused: false,
                });
                input(app.world_mut(), &handle, &services, &event).unwrap();
            }
            "modal" => {
                let mut frame = handle.frame().unwrap();
                frame.modal_stack.push("other-dialog".into());
                handle.present(frame).unwrap();
            }
            _ => unreachable!(),
        }
        if scenario != "focus" {
            assert!(tick(app.world_mut(), &handle, &services).is_err());
        }
        assert!(!active(app.world()));
        let saved = &app
            .world()
            .resource::<Editor>()
            .draft
            .as_ref()
            .unwrap()
            .record["holes"];
        assert!(saved.is_null() || saved.as_array().is_some_and(Vec::is_empty));
        assert_eq!(
            fixture
                .bridge
                .native_document_receipt(&fixture.engine, &receipt.owner)
                .unwrap(),
            receipt
        );
    }
}

#[test]
fn hole_click_release_deferred_during_inspect_and_capture_stages_exactly_once() {
    use crate::session_bridge::native_interface::controller;
    let _lock = crate::session_bridge::tests::TEST_LOCK
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    let fixture = Fixture::new();
    let (mut app, handle, services, receipt, cursor) = setup(&fixture, "drill");
    app.init_resource::<Messages<NativeHostInput>>();
    controller::worker::install(app.world_mut(), services.clone(), handle.clone()).unwrap();
    let mut controller = controller::Controller::new(
        receipt.owner.window_id.clone(),
        None,
        Arc::new(AtomicBool::new(false)),
    );
    controller.synchronized = Some((receipt.owner.clone(), receipt.revision));
    for (action, expected) in [("inspect", 1), ("capture", 0)] {
        let session_id = fixture
            .bridge
            .session_id_for_window(&receipt.owner.window_id)
            .unwrap()
            .unwrap();
        let id = format!("989-{expected}");
        let directory = crate::session_bridge::session_root()
            .join(&session_id)
            .join("controls");
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join(format!("{id}.request.json")),
            json!({"id":id,"expires_ms":now_ms()+30_000,"ui":{"action":action}}).to_string(),
        )
        .unwrap();
        let before = keys(app.world());
        controller::worker::enqueue_control_poll(
            app.world_mut(),
            receipt.owner.clone(),
            id.clone(),
        )
        .unwrap();
        controller.polled_control = Some(controller::PolledControl {
            owner: receipt.owner.clone(),
            session: session_id,
            id,
            interface_only: true,
        });
        for press in [true, false] {
            app.world_mut()
                .write_message(pointer(&receipt.owner, cursor, press));
        }
        controller::maintain_busy_window(app.world_mut(), &handle, &mut controller).unwrap();
        assert!(active(app.world()));
        assert_eq!(
            keys(app.world()),
            before,
            "query workers cannot stage pointer input"
        );
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
        drain(&mut app, &handle, &services);
        assert_eq!(
            keys(app.world()).len(),
            expected,
            "each preserved press/release toggles once"
        );
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
