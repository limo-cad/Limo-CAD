use super::super::tests::Fixture;
use super::*;
use crate::session_bridge::parse_engine_envelope;
use std::fs;

mod playback_priority;
mod sibling_menus;
mod window_close;

/// Finish the actual control admitted by a fixture's poll worker. Discarding
/// its outcome leaves the window's one-use admission pending for the next poll.
pub(super) fn finish_polled_control_fixture(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    state: &mut Controller,
    request: &Value,
) {
    // Ordinary controller update initializes file state before dispatch, even
    // when a capture request will reject its missing destination.
    files::initialize(world, state.workspace.clone());
    let polled = state.polled_control.take().expect("fixture polled control");
    assert_eq!(request["id"], polled.id);
    assert_eq!(request["session_id"], polled.session);
    let ticket =
        native_control::ticket_for(&services.bridge, &polled.owner, &polled.session, &polled.id)
            .unwrap();
    assert!(native_control::has_pending(&services.bridge, &polled.owner.window_id).unwrap());
    start_control_with_ticket(
        world,
        handle,
        services,
        state,
        &polled.owner,
        request,
        Some(ticket),
    )
    .unwrap();

    assert!(!world.contains_resource::<Controller>());
    world.insert_resource(services.clone());
    world.insert_resource(handle.clone());
    let placeholder = Controller::new(
        state.window_id.clone(),
        None,
        Arc::new(AtomicBool::new(false)),
    );
    world.insert_resource(std::mem::replace(state, placeholder));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        complete_control(world);
        if world.resource::<Controller>().pending.is_none() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "fixture completion timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    *state = world.remove_resource::<Controller>().unwrap();
    assert!(!native_control::has_pending(&services.bridge, &polled.owner.window_id).unwrap());
    let controls = crate::session_bridge::session_root()
        .join(&polled.session)
        .join("controls");
    let response: Value = serde_json::from_str(
        &fs::read_to_string(controls.join(format!("{}.result.json", polled.id))).unwrap(),
    )
    .unwrap();
    assert_eq!(response["request_id"], polled.id);
    assert_eq!(response["session_id"], polled.session);
    assert_eq!(response["control_admission"]["request_id"], polled.id);
    assert_eq!(
        response["control_admission"]["document_id"],
        polled.owner.document_id
    );
    if request["ui"]["action"] == "capture" {
        // These scheduling fixtures have no capture destination or GPU. The
        // actual rejection must still complete and retire the admitted ticket.
        assert_eq!(response["status"], "failed");
        assert_eq!(response["error"], "Capture requires an absolute PNG path");
    } else {
        assert_eq!(response["status"], "applied");
    }
    assert!(!controls
        .join(format!("{}.request.json", polled.id))
        .exists());
    assert!(!controls
        .join(format!("{}.admitted.json", polled.id))
        .exists());
}

#[test]
fn retained_close_receipt_retry_exits_in_the_same_frame_without_replaying_close() {
    crate::session_bridge::tests::with_isolated_session_test(|| {
        let fixture = Fixture::new();
        let (mut app, handle, _) = prepare(&fixture);
        app.add_message::<AppExit>();
        let services = app.world().resource::<NativeServices>().clone();
        let owner = fixture.owner();
        let session = fixture
            .bridge
            .session_id_for_window("main")
            .unwrap()
            .unwrap();
        let controls = crate::session_bridge::session_root()
            .join(&session)
            .join("controls");
        fs::create_dir_all(&controls).unwrap();
        let id = "701-1";
        fs::write(
            controls.join(format!("{id}.request.json")),
            json!({"id":id,"expires_ms":now_ms()+30_000,
                "ui":{"action":"window","mode":"close"}})
            .to_string(),
        )
        .unwrap();
        let request = crate::session_bridge::control_for_window_owned(
            &fixture.bridge,
            "main",
            &fixture.engine,
            None,
            Some((&owner, id)),
        )
        .unwrap();
        let ticket = native_control::ticket_for(&fixture.bridge, &owner, &session, id).unwrap();
        let before = serde_json::to_string(&fixture.engine.document_snapshot()).unwrap();
        app.world_mut()
            .resource_scope(|world, mut state: Mut<Controller>| {
                files::initialize(world, state.workspace.clone());
                start_control_with_ticket(
                    world,
                    &handle,
                    &services,
                    &mut state,
                    &owner,
                    &request,
                    Some(ticket),
                )
                .unwrap();
                assert!(state.exit_after_receipt);
                assert!(!state.close_pending);
            });
        let result_path = controls.join(format!("{id}.result.json"));
        fs::create_dir(&result_path).unwrap();
        complete_control(app.world_mut());
        let mut original = {
            let state = app.world().resource::<Controller>();
            let pending = state
                .pending
                .as_ref()
                .expect("failed publication retains close outcome");
            assert!(pending.delivery_only);
            assert!(!state.stop_watcher.load(Ordering::Acquire));
            pending.response.clone()
        };
        let first_publication_ms = original
            .as_object_mut()
            .unwrap()
            .remove("receipt_published_ms");
        assert!(first_publication_ms.unwrap().as_u64().is_some());
        assert!(app.should_exit().is_none());
        assert!(native_control::has_pending(&fixture.bridge, "main").unwrap());
        assert!(fixture
            .bridge
            .session_id_for_window("main")
            .unwrap()
            .is_some());
        // A second failed delivery also retains the original close, not a new dispatch.
        complete_control(app.world_mut());
        let mut retained = app
            .world()
            .resource::<Controller>()
            .pending
            .as_ref()
            .unwrap()
            .response
            .clone();
        assert!(retained
            .as_object_mut()
            .unwrap()
            .remove("receipt_published_ms")
            .unwrap()
            .as_u64()
            .is_some());
        assert_eq!(retained, original);
        assert!(app.should_exit().is_none());
        fs::remove_dir(&result_path).unwrap();
        complete_control(app.world_mut());
        // No app.update or additional event is permitted between delivery and exit.
        assert_eq!(app.should_exit(), Some(AppExit::Success));
        assert!(app.world().resource::<Controller>().pending.is_none());
        assert!(app
            .world()
            .resource::<Controller>()
            .stop_watcher
            .load(Ordering::Acquire));
        assert!(!native_control::has_pending(&fixture.bridge, "main").unwrap());
        assert!(fixture
            .bridge
            .session_id_for_window("main")
            .unwrap()
            .is_none());
        let mut response: Value =
            serde_json::from_str(&fs::read_to_string(&result_path).unwrap()).unwrap();
        assert!(response
            .as_object_mut()
            .unwrap()
            .remove("receipt_published_ms")
            .unwrap()
            .as_u64()
            .is_some());
        assert_eq!(response, original);
        assert_eq!(response["status"], "applied");
        assert_eq!(response["awaiting_input"], false);
        assert_eq!(response["control_admission"]["request_id"], id);
        assert!(!controls.join(format!("{id}.admitted.json")).exists());
        assert_eq!(
            serde_json::to_string(&fixture.engine.document_snapshot()).unwrap(),
            before
        );
    });
}

#[test]
fn rejected_native_receipt_retries_on_frame_and_prevents_early_shutdown() {
    crate::session_bridge::tests::with_isolated_session_test(|| {
        let fixture = Fixture::new();
        let (mut app, handle, _) = prepare(&fixture);
        app.add_message::<AppExit>();
        let services = app.world().resource::<NativeServices>().clone();
        let owner = fixture.owner();
        let session = fixture
            .bridge
            .session_id_for_window("main")
            .unwrap()
            .unwrap();
        let controls = crate::session_bridge::session_root()
            .join(&session)
            .join("controls");
        fs::create_dir_all(&controls).unwrap();
        let id = "700-1";
        fs::write(
            controls.join(format!("{id}.request.json")),
            json!({"id":id,"expires_ms":now_ms()+30_000,"ui":{"action":"inspect"}}).to_string(),
        )
        .unwrap();
        let request = crate::session_bridge::control_for_window_owned(
            &fixture.bridge,
            "main",
            &fixture.engine,
            None,
            Some((&owner, id)),
        )
        .unwrap();
        let ticket = native_control::ticket_for(&fixture.bridge, &owner, &session, id).unwrap();
        // Retiring the original document makes the dispatch guard reject it.
        fixture
            .bridge
            .apply_native_mutation(
                &fixture.engine,
                &owner,
                "cad_new_project",
                &json!({}),
                || Ok(()),
            )
            .unwrap();
        let before = serde_json::to_string(&fixture.engine.document_snapshot()).unwrap();
        let result_path = controls.join(format!("{id}.result.json"));
        fs::create_dir(&result_path).unwrap();
        app.world_mut()
            .resource_scope(|world, mut state: Mut<Controller>| {
                assert!(start_control_with_ticket(
                    world,
                    &handle,
                    &services,
                    &mut state,
                    &owner,
                    &request,
                    Some(ticket)
                )
                .is_err());
                assert!(state.pending.is_none());
                state.exit_after_receipt = true;
            });
        complete_control(app.world_mut());
        assert!(native_control::has_pending(&fixture.bridge, "main").unwrap());
        assert!(
            fixture
                .bridge
                .session_id_for_window("main")
                .unwrap()
                .is_some(),
            "A pending receipt must retain its window publisher"
        );
        assert!(!app
            .world()
            .resource::<Controller>()
            .stop_watcher
            .load(Ordering::Acquire));
        fs::remove_dir(&result_path).unwrap();
        complete_control(app.world_mut());
        let response: Value =
            serde_json::from_str(&fs::read_to_string(&result_path).unwrap()).unwrap();
        assert!(response["status"] == "failed" && response["mutation_applied"] == false);
        assert!(
            serde_json::to_string(&fixture.engine.document_snapshot()).unwrap() == before,
            "Receipt retry must not execute a model action"
        );
        assert!(app
            .world()
            .resource::<Controller>()
            .stop_watcher
            .load(Ordering::Acquire));
        assert!(fixture
            .bridge
            .session_id_for_window("main")
            .unwrap()
            .is_none());
    });
}

#[test]
fn mcp_and_keyboard_history_use_the_same_guarded_native_controls() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let view = json!({"name":"Review","camera":{"position":[30,40,50],
        "target":[0,0,0],"up":[0,0,1]},"visible_body_ids":[]});
    let saved = fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "upsert_named_view",
            &view,
            || Ok(()),
        )
        .unwrap()
        .value["views"]
        .clone();
    let mut app = native_viewport::interface_scene_fixture();
    app.insert_resource(ViewportUiAssets::default());
    app.world_mut().spawn((Window::default(), PrimaryWindow));
    app.world_mut().spawn(InterfaceCamera);
    let handle = NativeInterfaceHandle::new(|| {});
    let mut state = Controller::new("main".into(), None, Arc::new(AtomicBool::new(false)));
    synchronize(app.world_mut(), &handle, &services, &mut state).unwrap();
    for (index, key) in ["undo", "redo"].into_iter().enumerate() {
        app.world_mut().entity_mut(state.controls[key]).insert((
            ComputedNode {
                size: Vec2::new(78., 32.),
                inverse_scale_factor: 1.,
                ..default()
            },
            bevy::ui::UiGlobalTransform::from_translation(Vec2::new(
                400. + index as f32 * 80.,
                400.,
            )),
            bevy::ui::ComputedStackIndex(1),
            InheritedVisibility::VISIBLE,
        ));
    }
    interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
    let owner = fixture.owner();
    let (view_state, history) =
        inspect_document(app.world(), &handle, &services, &state, &owner).unwrap();
    assert!(view_state["camera"]["position"].is_array());
    assert_eq!(view_state["visible_body_ids"], json!([]));
    assert_eq!(history, json!({"can_undo":true,"can_redo":false}));

    let mut frame = handle.frame().unwrap();
    frame.modal_stack = vec!["guarded-editor".into()];
    handle.present(frame).unwrap();
    interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
    let (_, history) = inspect_document(app.world(), &handle, &services, &state, &owner).unwrap();
    assert_eq!(history["can_undo"], false);
    let request = |command| {
        json!({"expires_ms":now_ms()+30_000,
        "ui":{"action":"history","command":command}})
    };
    assert!(apply_control(
        app.world_mut(),
        &handle,
        &services,
        &mut state,
        &owner,
        &request("undo")
    )
    .is_err());
    assert_eq!(
        parse_engine_envelope(fixture.engine.engine_call("named_views", "")).unwrap()["views"],
        saved
    );
    synchronize(app.world_mut(), &handle, &services, &mut state).unwrap();
    interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
    for (command, expected) in [("undo", json!([])), ("redo", saved.clone())] {
        apply_control(
            app.world_mut(),
            &handle,
            &services,
            &mut state,
            &fixture.owner(),
            &request(command),
        )
        .unwrap();
        assert_eq!(
            parse_engine_envelope(fixture.engine.engine_call("named_views", "")).unwrap()["views"],
            expected
        );
        synchronize(app.world_mut(), &handle, &services, &mut state).unwrap();
        interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
    }
    for (redo, expected) in [(false, json!([])), (true, saved.clone())] {
        use bevy::input::keyboard::{Key, KeyCode, KeyboardInput};
        let event = NativeHostInput {
            ui_scale: 1.,
            context: Some(fixture.owner()),
            cursor: None,
            modifiers: crate::native_viewport::winit_host::Modifiers {
                meta: cfg!(target_os = "macos"),
                ctrl: !cfg!(target_os = "macos"),
                shift: redo,
                ..default()
            },
            event: WindowEvent::KeyboardInput(KeyboardInput {
                key_code: KeyCode::KeyZ,
                logical_key: Key::Character(if redo { "\u{1a}" } else { "z" }.into()),
                text: None,
                state: bevy::input::ButtonState::Pressed,
                repeat: false,
                window: Entity::PLACEHOLDER,
            }),
            consumed: false,
            actions: vec![],
        };
        let mut blocked = event.clone();
        blocked.consumed = true;
        assert!(history::shortcut_action(app.world_mut(), &handle, &blocked)
            .unwrap()
            .is_none());
        blocked = event.clone();
        blocked.context.as_mut().unwrap().epoch += 1;
        assert!(history::shortcut_action(app.world_mut(), &handle, &blocked)
            .unwrap()
            .is_none());
        let mut modal = handle.frame().unwrap();
        modal.modal_stack.push("guarded-editor".into());
        handle.present(modal).unwrap();
        assert!(history::shortcut_action(app.world_mut(), &handle, &event)
            .unwrap()
            .is_none());
        synchronize(app.world_mut(), &handle, &services, &mut state).unwrap();
        interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
        let action = history::shortcut_action(app.world_mut(), &handle, &event)
            .unwrap()
            .unwrap();
        apply_queued_control(app.world_mut(), &handle, &services, &mut state, &action).unwrap();
        assert_eq!(
            parse_engine_envelope(fixture.engine.engine_call("named_views", "")).unwrap()["views"],
            expected
        );
        synchronize(app.world_mut(), &handle, &services, &mut state).unwrap();
        interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
    }
    assert!(apply_control(
        app.world_mut(),
        &handle,
        &services,
        &mut state,
        &owner,
        &request("undo")
    )
    .is_err());
    app.insert_resource(services);
    app.insert_resource(handle);
    app.insert_resource(state);
    let path = pending(&fixture, &mut app, "11-1");
    app.world_mut()
        .resource_mut::<Controller>()
        .pending
        .as_mut()
        .unwrap()
        .inspect = true;
    complete_control(app.world_mut());
    let response: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(response["status"], "applied");
    assert!(response["view_state"]["camera"]["position"].is_array());
    assert_eq!(
        response["state"]["history"],
        json!({"can_undo":true,"can_redo":false})
    );
}

#[test]
fn minimized_native_window_keeps_a_valid_inspectable_viewport() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let mut app = native_viewport::interface_scene_fixture();
    app.insert_resource(ViewportUiAssets::default());
    app.world_mut().spawn((
        Window {
            resolution: bevy::window::WindowResolution::new(0, 0),
            ..default()
        },
        PrimaryWindow,
    ));
    app.world_mut().spawn(InterfaceCamera);
    let handle = NativeInterfaceHandle::new(|| {});
    let mut state = Controller::new("main".into(), None, Arc::new(AtomicBool::new(false)));
    synchronize(app.world_mut(), &handle, &services, &mut state).unwrap();
    interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
    let frame = handle.frame().unwrap();
    assert_eq!(frame.client.width, 1360.);
    assert_eq!(frame.client.height, 860.);
    let canvas = frame.canvases.first().unwrap();
    assert!(canvas.bounds.y + canvas.bounds.height <= frame.client.height);
    assert!(canvas.bounds.height > 0.);
}

#[test]
fn early_title_bar_close_is_honored_but_a_retired_document_cannot_close_its_replacement() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, _, _) = prepare(&fixture);
    let owner = fixture.owner();
    app.world_mut()
        .resource_scope(|world, mut state: Mut<Controller>| {
            close_from_window_event(world, &mut state, &fixture.bridge, &fixture.engine, None)
                .unwrap();
            assert!(state.exit_after_receipt);
            assert!(!state.close_pending);
            state.exit_after_receipt = false;
        });
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner,
            "cad_new_project",
            &json!({}),
            || Ok(()),
        )
        .unwrap();
    app.world_mut()
        .resource_scope(|world, mut state: Mut<Controller>| {
            assert!(close_from_window_event(
                world,
                &mut state,
                &fixture.bridge,
                &fixture.engine,
                Some(&owner)
            )
            .is_err());
            assert!(!state.exit_after_receipt);
            assert!(!state.close_pending);
            close_from_window_event(world, &mut state, &fixture.bridge, &fixture.engine, None)
                .unwrap();
            assert!(
                state.close_pending,
                "Even an early close must protect unsaved work"
            );
            assert!(!state.exit_after_receipt);
        });
}

#[test]
fn both_exit_routes_publish_a_valid_close_confirmation_for_dirty_work() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let mut app = native_viewport::interface_scene_fixture();
    app.insert_resource(ViewportUiAssets::default());
    app.world_mut().spawn((Window::default(), PrimaryWindow));
    app.world_mut().spawn(InterfaceCamera);
    let handle = NativeInterfaceHandle::new(|| {});
    for ui in [
        json!({"action":"file","command":"exit"}),
        json!({"action":"window","mode":"close"}),
    ] {
        let mut state = Controller::new("main".into(), None, Arc::new(AtomicBool::new(false)));
        let result = apply_control(
            app.world_mut(),
            &handle,
            &services,
            &mut state,
            &fixture.owner(),
            &json!({"expires_ms":now_ms()+30_000,"ui":ui}),
        )
        .unwrap();
        assert_eq!(result["awaiting_input"], true);
        assert!(!state.exit_after_receipt);
        synchronize(app.world_mut(), &handle, &services, &mut state).unwrap();
        interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
        let frame = handle.frame().unwrap();
        let mut registry = limo_cad_interface::SurfaceRegistry::new();
        registry
            .replace(
                frame.context,
                limo_cad_interface::SurfaceFrame {
                    client: frame.client,
                    surfaces: frame.surfaces,
                    canvases: frame.canvases,
                    modal_stack: frame.modal_stack,
                    ..Default::default()
                },
            )
            .expect("The real close frame must be inspectable, not stuck awaiting layout");
        assert_eq!(registry.frame().modal_stack, vec!["close-document"]);
    }
}

pub(super) fn prepare(fixture: &Fixture) -> (App, NativeInterfaceHandle, Entity) {
    let (mut app, handle, entity, _) = interface_shell::tests::fixture();
    let mut frame = handle.frame().unwrap();
    frame.context = fixture.owner();
    handle.present(frame).unwrap();
    app.update();
    app.insert_resource(NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    });
    let mut controller = Controller::new("main".into(), None, Arc::new(AtomicBool::new(false)));
    controller.initialized = true;
    controller.initial_owner = Some(fixture.owner());
    controller.initial_revision = fixture
        .bridge
        .engine_revision_for_window("main")
        .unwrap()
        .unwrap();
    app.insert_resource(controller);
    (app, handle, entity)
}

fn pending(fixture: &Fixture, app: &mut App, id: &str) -> std::path::PathBuf {
    fixture
        .bridge
        .publish_native_document(&fixture.engine, &fixture.owner(), "solid")
        .unwrap();
    let session = fixture
        .bridge
        .session_id_for_window("main")
        .unwrap()
        .unwrap();
    let controls = crate::session_bridge::session_root()
        .join(&session)
        .join("controls");
    fs::create_dir_all(&controls).unwrap();
    fs::write(
        controls.join(format!("{id}.request.json")),
        json!({"id":id,"expires_ms":now_ms()+30_000,"ui":{"action":"inspect"}}).to_string(),
    )
    .unwrap();
    let request = control_for_window(&fixture.bridge, "main", &fixture.engine, None).unwrap();
    assert_eq!(request["id"], id);
    app.world_mut().resource_mut::<Controller>().pending = Some(PendingControl {
        response: json!({"request_id":id,"session_id":session,"status":"applied"}),
        owner: fixture.owner(),
        presentation_deadline: now_ms() + 2_000,
        inspect: false,
        delivery: None,
        presentation_revision: None,
        delivery_only: false,
    });
    controls.join(format!("{id}.result.json"))
}

#[test]
fn visible_receipt_waits_for_scene_submission_and_hidden_or_suppressed_rendering_is_bounded() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, handle, _) = prepare(&fixture);
    let mut availability = crate::native_viewport::winit_host::NativeRenderAvailability::default();
    availability.drawable = true;
    app.insert_resource(availability);
    handle.submitted().unwrap();
    let old_revision = handle.render_receipt().unwrap().submitted_revision;
    let path = pending(&fixture, &mut app, "10-3");
    handle.invalidate_presentation();
    app.update();
    let target = handle.render_receipt().unwrap().laid_out_revision;
    assert!(target > old_revision);
    complete_control(app.world_mut());
    assert!(
        !path.exists(),
        "The old submitted scene cannot acknowledge this operation"
    );
    handle.submitted_revision(target).unwrap();
    complete_control(app.world_mut());
    let response: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(response["status"], "applied");
    assert_eq!(response["presented"], true);
    assert_eq!(response["render_status"], "submitted");

    let path = pending(&fixture, &mut app, "10-4");
    handle.invalidate_presentation();
    app.update();
    app.world_mut()
        .resource_mut::<Controller>()
        .pending
        .as_mut()
        .unwrap()
        .presentation_deadline = 0;
    complete_control(app.world_mut());
    let response: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(
        response["status"], "applied",
        "A suppressed renderer cannot undo a committed operation"
    );
    assert_eq!(response["presented"], false);
    assert_eq!(response["render_status"], "submission_timeout");

    let path = pending(&fixture, &mut app, "10-5");
    app.world_mut()
        .resource_mut::<crate::native_viewport::winit_host::NativeRenderAvailability>()
        .drawable = false;
    complete_control(app.world_mut());
    let response: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(response["presented"], false);
    assert_eq!(response["render_status"], "unavailable");
    assert!(app.world().resource::<Controller>().pending.is_none());
}

#[test]
fn inbox_watcher_keeps_admitted_controls_awake_until_durable_completion() {
    assert_admitted_control_wakes(true);
}

#[test]
fn inbox_watcher_services_early_admission_before_session_cache_initialization() {
    assert_admitted_control_wakes(false);
}

fn assert_admitted_control_wakes(initialize_cached_session: bool) {
    crate::session_bridge::tests::with_isolated_session_test(|| {
        let fixture = Fixture::new();
        let services = NativeServices {
            engine: fixture.engine.clone(),
            bridge: fixture.bridge.clone(),
        };
        let owner = fixture.owner();
        let session = fixture
            .bridge
            .session_id_for_window("main")
            .unwrap()
            .unwrap();
        let controls = crate::session_bridge::session_root()
            .join(&session)
            .join("controls");
        fs::create_dir_all(&controls).unwrap();
        let id = "704-1";
        let request = controls.join(format!("{id}.request.json"));
        fs::write(
            &request,
            json!({"id":id,"expires_ms":now_ms()+30_000,
            "ui":{"action":"inspect"}})
            .to_string(),
        )
        .unwrap();
        crate::session_bridge::control_for_window_owned(
            &fixture.bridge,
            "main",
            &fixture.engine,
            None,
            Some((&owner, id)),
        )
        .unwrap();
        let ticket = native_control::ticket_for(&fixture.bridge, &owner, &session, id).unwrap();
        if initialize_cached_session {
            native_control::validate_start(&fixture.bridge, &fixture.engine, &ticket).unwrap();
        }
        assert!(!request.exists(), "The request-file wake has been retired");

        let (send, receive) = std::sync::mpsc::channel();
        let handle = NativeInterfaceHandle::new(move || {
            let _ = send.send(());
        });
        let stop = Arc::new(AtomicBool::new(false));
        struct Watcher(Arc<AtomicBool>, Option<std::thread::JoinHandle<()>>);
        impl Drop for Watcher {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Release);
                if let Some(thread) = self.1.take() {
                    let _ = thread.join();
                }
            }
        }
        let _watcher = Watcher(
            stop.clone(),
            Some(
                start_watcher(
                    &services,
                    "main",
                    handle,
                    stop,
                    Arc::new(Mutex::new(
                        initialize_cached_session.then(|| session.clone()),
                    )),
                    app_settings::Wake::default(),
                    retention::Wake::default(),
                )
                .unwrap(),
            ),
        );
        let require_wakes = || {
            // Discard queued/one-shot wakes: a still-pending admission must
            // produce fresh scheduling even without any render callback.
            while receive.try_recv().is_ok() {}
            for _ in 0..3 {
                receive
                    .recv_timeout(Duration::from_secs(2))
                    .expect("Admitted control lost its continuing wake source");
            }
        };
        require_wakes();
        if !initialize_cached_session {
            native_control::validate_start(&fixture.bridge, &fixture.engine, &ticket).unwrap();
            require_wakes();
        }
        let result = controls.join(format!("{id}.result.json"));
        fs::create_dir(&result).unwrap();
        let mut response = json!({"status":"failed","error":"Fixture inspection unavailable","request_id":id,"session_id":session});
        assert!(matches!(
            native_control::complete(
                &fixture.bridge,
                &fixture.engine,
                &ticket,
                None,
                &mut response,
            ),
            Err(native_control::CompletionError::Publication(_))
        ));
        require_wakes();
        fs::remove_dir(&result).unwrap();
        native_control::retry_pending(&fixture.bridge, &fixture.engine, "main").unwrap();
        assert!(result.is_file());
        assert!(!native_control::has_pending(&fixture.bridge, "main").unwrap());
        // A stale disk marker must not keep the idle host awake without the
        // owning window's private admission.
        fs::write(controls.join("foreign.admitted.json"), b"{}").unwrap();
        std::thread::sleep(Duration::from_millis(60));
        while receive.try_recv().is_ok() {}
        assert!(receive.recv_timeout(Duration::from_millis(100)).is_err());
    });
}

#[test]
fn admitted_inspection_wakes_until_its_existing_deadline_without_claiming_gpu_submission() {
    crate::session_bridge::tests::with_isolated_session_test(|| {
        let fixture = Fixture::new();
        let services = NativeServices {
            engine: fixture.engine.clone(),
            bridge: fixture.bridge.clone(),
        };
        let mut app = native_viewport::interface_scene_fixture();
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        app.world_mut().spawn(InterfaceCamera);
        let wakes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = wakes.clone();
        let handle = NativeInterfaceHandle::new(move || {
            counted.fetch_add(1, Ordering::Relaxed);
        });
        let mut state = Controller::new("main".into(), None, Arc::new(AtomicBool::new(false)));
        synchronize(app.world_mut(), &handle, &services, &mut state).unwrap();
        interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
        app.insert_resource(services.clone());
        app.insert_resource(state);
        let owner = fixture.owner();
        fixture
            .bridge
            .publish_native_document(&fixture.engine, &owner, "solid")
            .unwrap();
        let session = fixture
            .bridge
            .session_id_for_window("main")
            .unwrap()
            .unwrap();
        let controls = crate::session_bridge::session_root()
            .join(&session)
            .join("controls");
        fs::create_dir_all(&controls).unwrap();
        let id = "703-1";
        let request_path = controls.join(format!("{id}.request.json"));
        let admitted_path = controls.join(format!("{id}.admitted.json"));
        let result_path = controls.join(format!("{id}.result.json"));
        fs::write(
            &request_path,
            json!({"id":id,"expires_ms":now_ms()+30_000,"ui":{"action":"inspect"}}).to_string(),
        )
        .unwrap();
        let request = crate::session_bridge::control_for_window_owned(
            &fixture.bridge,
            "main",
            &fixture.engine,
            None,
            Some((&owner, id)),
        )
        .unwrap();
        let ticket = native_control::ticket_for(&fixture.bridge, &owner, &session, id).unwrap();
        assert!(
            !request_path.exists(),
            "Admission removes the filesystem wake"
        );
        assert!(admitted_path.is_file());
        app.world_mut()
            .resource_scope(|world, mut state: Mut<Controller>| {
                start_control_with_ticket(
                    world,
                    &handle,
                    &services,
                    &mut state,
                    &owner,
                    &request,
                    Some(ticket),
                )
                .unwrap();
            });
        let presentation_deadline = app
            .world()
            .resource::<Controller>()
            .pending
            .as_ref()
            .unwrap()
            .presentation_deadline;
        app.init_resource::<crate::native_viewport::winit_host::NativeRenderAvailability>();
        app.world_mut()
            .resource_mut::<crate::native_viewport::winit_host::NativeRenderAvailability>()
            .drawable = true;
        let target = handle.render_receipt().unwrap().laid_out_revision;
        assert!(target > 0);
        assert!(handle.render_receipt().unwrap().submitted_revision < target);

        // There is no render app, queued pipeline, GPU callback or inbox watcher.
        // Discard all preparation wakes, then run later frames only when the
        // actual pending-receipt path requests one, as in the low-power host.
        wakes.store(0, Ordering::Relaxed);
        complete_control(app.world_mut());
        assert!(
            !result_path.exists(),
            "An unsubmitted frame cannot complete early"
        );
        let limit = std::time::Instant::now() + Duration::from_secs(5);
        while app.world().resource::<Controller>().pending.is_some() {
            assert!(
                std::time::Instant::now() < limit,
                "Receipt missed its finite deadline"
            );
            assert!(
                wakes.swap(0, Ordering::Relaxed) > 0,
                "No GPU submission callback exists to wake the next receipt frame"
            );
            complete_control(app.world_mut());
        }
        assert!(now_ms() >= presentation_deadline);
        let original_result = fs::read(&result_path).unwrap();
        let response: Value = serde_json::from_slice(&original_result).unwrap();
        assert_eq!(response["status"], "applied");
        assert_eq!(response["request_id"], id);
        assert_eq!(response["session_id"], session);
        assert_eq!(
            response["control_admission"]["document_id"],
            owner.document_id
        );
        assert_eq!(response["control_admission"]["epoch"], owner.epoch);
        assert_eq!(
            response["desktop_build"],
            json!(limo_cad_build_info::build_info())
        );
        assert!(response["ui"].is_object());
        assert_eq!(response["native_layout_revision"], target);
        assert!(response["native_submitted_revision"].as_u64().unwrap() < target);
        assert_eq!(response["presented"], false);
        assert_eq!(response["render_status"], "submission_timeout");
        assert!(!admitted_path.exists());
        assert!(!native_control::has_pending(&fixture.bridge, "main").unwrap());
        assert_eq!(
            wakes.swap(0, Ordering::Relaxed),
            0,
            "Completed receipts stop waking"
        );
        complete_control(app.world_mut());
        assert_eq!(wakes.load(Ordering::Relaxed), 0);
        assert!(
            fs::read(&result_path).unwrap() == original_result,
            "No receipt replay"
        );
    });
}

#[test]
fn mcp_receipt_waits_for_the_changed_native_frame_and_contains_its_actual_controls() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, handle, entity) = prepare(&fixture);
    let path = pending(&fixture, &mut app, "10-1");
    app.world_mut()
        .get_mut::<InterfaceControl>(entity)
        .unwrap()
        .label = "Updated after command".into();
    let mut frame = handle.frame().unwrap();
    frame.surfaces[0].text = Some("Changed document state".into());
    handle.present(frame).unwrap();
    complete_control(app.world_mut());
    assert!(
        !path.exists(),
        "No acknowledgement from the old laid-out frame"
    );
    app.update();
    complete_control(app.world_mut());
    let response: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(response["status"], "applied");
    assert_eq!(
        response["ui"]["surfaces"][0]["controls"][0]["label"],
        "Updated after command"
    );
    assert!(response["native_layout_revision"].as_u64().unwrap() > 0);
    assert_eq!(
        response["presented"], false,
        "Semantic layout cannot certify GPU display"
    );
    assert!(app.world().resource::<Controller>().pending.is_none());
}

#[test]
fn delayed_receipt_cannot_inspect_or_invalidate_the_replacement_documents_controls() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, handle, _) = prepare(&fixture);
    let path = pending(&fixture, &mut app, "10-2");
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "cad_new_project",
            &json!({}),
            || Ok(()),
        )
        .unwrap();
    let current = fixture.owner();
    let mut frame = handle.frame().unwrap();
    frame.context = current.clone();
    handle.present(frame).unwrap();
    app.update();
    let snapshot = handle.inspect().unwrap();
    let request = ControlRequest::Click {
        target: snapshot["surfaces"][0]["controls"][0]["id"]
            .as_str()
            .unwrap()
            .into(),
    };
    complete_control(app.world_mut());
    let response: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(response["status"], "failed");
    assert!(response.get("ui").is_none());
    assert!(
        handle.resolve(&request, &current).is_ok(),
        "Old receipt must leave fresh inspection IDs usable"
    );
}

#[test]
fn completed_tab_transition_never_certifies_the_previous_documents_frame() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, _, _) = prepare(&fixture);
    let path = pending(&fixture, &mut app, "10-4");
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "cad_new_project",
            &json!({}),
            || Ok(()),
        )
        .unwrap();
    {
        let mut controller = app.world_mut().resource_mut::<Controller>();
        let pending = controller.pending.as_mut().unwrap();
        pending.owner = fixture.owner();
        pending.presentation_deadline = 0;
    }
    complete_control(app.world_mut());
    let response: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert_eq!(
        response["status"], "applied",
        "Do not invite replay of a committed transition"
    );
    assert_eq!(response["presented"], false);
    assert_eq!(response["presentation_pending"], true);
    assert!(
        response.get("ui").is_none(),
        "Old controls must never accompany B's receipt"
    );
}

#[test]
fn queued_command_checks_live_binding_before_a_later_layout_can_publish_the_rebind() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, handle, entity) = prepare(&fixture);
    bind_command(
        app.world_mut(),
        entity,
        NativeCommand::Mutation {
            operation: "cad_set_document_name".into(),
            arguments: json!({"name":"Old command"}),
        },
    )
    .unwrap();
    app.update();
    let snapshot = handle.inspect().unwrap();
    let request = ControlRequest::Click {
        target: snapshot["surfaces"][0]["controls"][0]["id"]
            .as_str()
            .unwrap()
            .into(),
    };
    let action = handle.resolve(&request, &fixture.owner()).unwrap();
    bind_command(
        app.world_mut(),
        entity,
        NativeCommand::Mutation {
            operation: "cad_set_document_name".into(),
            arguments: json!({"name":"Rebound command"}),
        },
    )
    .unwrap();
    assert!(reduce_action(
        &fixture.engine,
        &fixture.bridge,
        app.world_mut(),
        &handle,
        &action
    )
    .unwrap_err()
    .contains("changed"));
    assert_eq!(fixture.engine.document_snapshot().name, "Untitled");
}

#[test]
fn close_guard_survives_same_tab_replacement_with_a_reset_revision() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, _, _) = prepare(&fixture);
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "cad_new_project",
            &json!({}),
            || Ok(()),
        )
        .unwrap();
    app.world_mut()
        .resource_scope(|world, mut state: Mut<Controller>| {
            request_close(world, &mut state, &fixture.bridge, &fixture.engine).unwrap();
            assert!(state.close_pending);
            assert!(!state.exit_after_receipt);
            apply_host_result(&mut state, &json!({"close_decision":"cancel"}));
            assert!(!state.close_pending);
            assert!(!state.exit_after_receipt);
        });
}

#[test]
fn blocked_kernel_keeps_native_update_and_busy_replies_responsive_without_replaying_input() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut controls, handle, button) = prepare(&fixture);
    let mut app = native_viewport::interface_scene_fixture();
    let entity = app
        .world_mut()
        .spawn(
            controls
                .world()
                .get::<InterfaceControl>(button)
                .unwrap()
                .clone(),
        )
        .id();
    app.insert_resource(
        controls
            .world_mut()
            .remove_resource::<Controller>()
            .unwrap(),
    );
    app.insert_resource(
        controls
            .world_mut()
            .remove_resource::<NativeServices>()
            .unwrap(),
    );
    app.insert_resource(handle.clone());
    native_viewport::apply_interface_model(app.world_mut(), model_snapshot(&fixture.engine))
        .unwrap();
    app.init_resource::<Messages<NativeHostInput>>();
    let services = app.world().resource::<NativeServices>().clone();
    worker::install(app.world_mut(), services.clone(), handle.clone()).unwrap();
    let receipt = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &fixture.owner())
        .unwrap();
    let pending_path = pending(&fixture, &mut app, "20-1");
    let session = fixture
        .bridge
        .session_id_for_window("main")
        .unwrap()
        .unwrap();
    app.world_mut().resource_mut::<Controller>().cached_session = Some(session.clone());
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let timed_out = Arc::new(AtomicBool::new(false));
    let timeout_flag = timed_out.clone();
    let input_owner = receipt.owner.clone();
    worker::enqueue_transaction(
        app.world_mut(),
        "cad_set_document_name".into(),
        move |services, guard| {
            services.bridge.apply_native_mutation_at(
                &services.engine,
                &receipt.owner,
                receipt.revision,
                "cad_set_document_name",
                &json!({"name":"Built without blocking the window"}),
                || {
                    guard.validate()?;
                    started_tx.send(()).unwrap();
                    if release_rx.recv_timeout(Duration::from_secs(5)).is_err() {
                        timeout_flag.store(true, Ordering::Release);
                        return Err("Native update waited on the blocked kernel".into());
                    }
                    Ok(())
                },
            )
        },
        |_, _, result| Ok(result?.value),
    )
    .unwrap();
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(
        fixture.bridge.publishers.try_lock(),
        Err(std::sync::TryLockError::WouldBlock)
    ));
    let controls = crate::session_bridge::session_root()
        .join(&session)
        .join("controls");
    fs::write(
        controls.join("20-2.request.json"),
        json!({"id":"20-2","expires_ms":now_ms()+30_000,"ui":{"action":"inspect"}}).to_string(),
    )
    .unwrap();
    let camera_before = native_viewport::interface_camera_snapshot(app.world()).1;
    for event in [
        WindowEvent::MouseButtonInput(bevy::input::mouse::MouseButtonInput {
            button: MouseButton::Middle,
            state: bevy::input::ButtonState::Pressed,
            window: Entity::PLACEHOLDER,
        }),
        WindowEvent::CursorMoved(bevy::window::CursorMoved {
            position: Vec2::new(360., 330.),
            delta: None,
            window: Entity::PLACEHOLDER,
        }),
        WindowEvent::MouseButtonInput(bevy::input::mouse::MouseButtonInput {
            button: MouseButton::Middle,
            state: bevy::input::ButtonState::Released,
            window: Entity::PLACEHOLDER,
        }),
    ] {
        let cursor = if let WindowEvent::CursorMoved(moved) = &event {
            moved.position
        } else {
            Vec2::new(300., 300.)
        };
        app.world_mut().write_message(NativeHostInput {
            ui_scale: 1.,
            context: Some(input_owner.clone()),
            cursor: Some(cursor),
            modifiers: crate::native_viewport::winit_host::Modifiers::default(),
            event,
            consumed: false,
            actions: vec![],
        });
    }
    app.world_mut().write_message(NativeHostInput {
        ui_scale: 1.,
        context: handle.frame().map(|frame| frame.context),
        cursor: None,
        modifiers: crate::native_viewport::winit_host::Modifiers::default(),
        event: WindowEvent::WindowCloseRequested(bevy::window::WindowCloseRequested {
            window: Entity::PLACEHOLDER,
        }),
        consumed: false,
        actions: vec![],
    });
    app.world_mut()
        .resource_scope(|world, mut state: Mut<Controller>| {
            update_inner(world, &handle, &services, &mut state).unwrap();
            assert!(state.close_after_worker);
            assert!(!state.exit_after_receipt);
        });
    complete_control(app.world_mut());
    let camera_after = native_viewport::interface_camera_snapshot(app.world()).1;
    assert_ne!(
        camera_after.position, camera_before.position,
        "Camera input must be processed before the kernel lock is released"
    );
    assert_ne!(camera_after.target, camera_before.target);
    assert!(
        !timed_out.load(Ordering::Acquire),
        "Rendering must not wait for the engine/publisher fence"
    );
    assert!(
        app.world()
            .get::<InterfaceControl>(entity)
            .unwrap()
            .disabled
    );
    assert!(
        !pending_path.exists(),
        "The in-flight MCP request must wait for its real result"
    );
    assert!(controls.join("20-1.request.json").exists());
    let rejected: Value =
        serde_json::from_str(&fs::read_to_string(controls.join("20-2.result.json")).unwrap())
            .unwrap();
    assert_eq!(rejected["code"], "native_busy");
    assert_eq!(rejected["mutation_applied"], false);
    assert!(!controls.join("20-2.request.json").exists());
    release_tx.send(()).unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(outcome) = worker::poll(app.world_mut(), &services) {
            outcome.value.unwrap();
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        fixture.engine.document_snapshot().name,
        "Built without blocking the window"
    );
    assert_eq!(
        native_viewport::interface_camera_snapshot(app.world()).1,
        camera_after,
        "Completion must preserve navigation performed while the model was busy"
    );
}

#[test]
fn worker_revalidates_a_queued_control_after_waiting_for_the_owner_fence() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, handle, entity) = prepare(&fixture);
    let services = app.world().resource::<NativeServices>().clone();
    worker::install(app.world_mut(), services.clone(), handle.clone()).unwrap();
    let receipt = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &fixture.owner())
        .unwrap();
    bind_command(
        app.world_mut(),
        entity,
        NativeCommand::Mutation {
            operation: "cad_set_document_name".into(),
            arguments: json!({"name":"Old target"}),
        },
    )
    .unwrap();
    app.update();
    let snapshot = handle.inspect().unwrap();
    let request = ControlRequest::Click {
        target: snapshot["surfaces"][0]["controls"][0]["id"]
            .as_str()
            .unwrap()
            .into(),
    };
    let action = handle.resolve(&request, &fixture.owner()).unwrap();
    app.world_mut()
        .insert_resource(worker::ActiveControl(action));
    let held = fixture.bridge.publishers.lock().unwrap();
    worker::enqueue_operation(
        app.world_mut(),
        receipt.owner.clone(),
        receipt.revision,
        "cad_set_document_name".into(),
        json!({"name":"Old target"}),
        |_, _, result| Ok(result?.value),
    )
    .unwrap();
    app.world_mut().remove_resource::<worker::ActiveControl>();
    bind_command(
        app.world_mut(),
        entity,
        NativeCommand::Mutation {
            operation: "cad_set_document_name".into(),
            arguments: json!({"name":"New target"}),
        },
    )
    .unwrap();
    app.update();
    drop(held);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let error = loop {
        if let Some(outcome) = worker::poll(app.world_mut(), &services) {
            break outcome.value.unwrap_err();
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(error.contains("changed"), "{error}");
    assert_eq!(fixture.engine.document_snapshot().name, "Untitled");
    assert_eq!(
        fixture
            .bridge
            .native_document_receipt(&fixture.engine, &fixture.owner())
            .unwrap(),
        receipt
    );
}

#[test]
fn native_read_only_claims_preserve_user_and_agent_saves() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    for query in [false, true] {
        let fixture = Fixture::new();
        let (mut app, handle, entity) = prepare(&fixture);
        app.add_message::<NativeHostInput>();
        let services = app.world().resource::<NativeServices>().clone();
        worker::install(app.world_mut(), services.clone(), handle.clone()).unwrap();
        let owner = fixture.owner();
        let session = fixture
            .bridge
            .session_id_for_window("main")
            .unwrap()
            .unwrap();
        let before = fixture.engine.engine_call("project_export_model", "");
        let dir = crate::session_bridge::session_root()
            .join(&session)
            .join("controls");
        fs::create_dir_all(&dir).unwrap();
        for (id, ui) in [
            ("20-1", json!({"action":"presentation","command":"status"})),
            ("20-2", json!({"action":"inspect"})),
            ("20-3", json!({"action":"capture","path":"unused.png"})),
            (
                "20-4",
                json!({"action":"click","target":"must-be-revalidated-before-dispatch"}),
            ),
        ] {
            fs::write(
                dir.join(format!("{id}.request.json")),
                json!({"id":id,"expires_ms":now_ms()+30_000,"ui":ui}).to_string(),
            )
            .unwrap();
        }
        if query {
            fs::write(
                dir.join("20-1.request.json"),
                json!({"id":"20-1","expires_ms":now_ms()+30_000,
            "sketch_query":{"method":"named_views","payload":""}})
                .to_string(),
            )
            .unwrap();
        }
        fs::write(
            dir.join("20-5.request.json"),
            json!({"id":"20-5","expires_ms":now_ms()+30_000,
        "ui":{"action":"file","command":"save","path":dir.join("deferred.limo")}})
            .to_string(),
        )
        .unwrap();
        let request: Value =
            serde_json::from_str(&fs::read_to_string(dir.join("20-1.request.json")).unwrap())
                .unwrap();
        assert!(control_poll_is_read_only(&request));
        assert!(!control_poll_is_read_only(
            &json!({"sketch_query":{"method":"solid_extrude"}})
        ));
        let action = handle
            .resolve_retained(limo_cad_interface::ControlKey(entity.to_bits()))
            .unwrap();
        handle.enqueue_action(action).unwrap();
        app.world_mut().write_message(NativeHostInput {
            ui_scale: 1.,
            context: Some(owner.clone()),
            cursor: None,
            modifiers: crate::native_viewport::winit_host::Modifiers {
                ctrl: true,
                ..default()
            },
            event: WindowEvent::KeyboardInput(bevy::input::keyboard::KeyboardInput {
                key_code: bevy::input::keyboard::KeyCode::KeyS,
                logical_key: bevy::input::keyboard::Key::Character("s".into()),
                state: bevy::input::ButtonState::Pressed,
                text: None,
                repeat: false,
                window: Entity::PLACEHOLDER,
            }),
            consumed: false,
            actions: vec![],
        });
        let held = fixture.bridge.publishers.lock().unwrap();
        worker::enqueue_control_poll(app.world_mut(), owner.clone(), "20-1".into()).unwrap();
        app.world_mut()
            .resource_scope(|world, mut state: Mut<Controller>| {
                state.cached_session = Some(session.clone());
                state.polled_control = Some(PolledControl {
                    owner: owner.clone(),
                    session,
                    id: "20-1".into(),
                    interface_only: control_poll_is_read_only(&request),
                });
                maintain_busy_window(world, &handle, &mut state).unwrap();
                assert_eq!(
                    state
                        .deferred_save
                        .as_ref()
                        .and_then(|event| event.context.as_ref()),
                    Some(&owner)
                );
            });
        assert_eq!(
            handle.take_actions().unwrap().len(),
            1,
            "The human control stays queued during the read"
        );
        let still_queued = ["20-1", "20-2", "20-3", "20-4", "20-5"].map(|id| {
            dir.join(format!("{id}.request.json")).exists()
                && !dir.join(format!("{id}.result.json")).exists()
        });
        drop(held);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let claimed = loop {
            if let Some(outcome) = worker::poll(app.world_mut(), &services) {
                break outcome.value.unwrap();
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Interface claim timed out"
            );
            std::thread::sleep(Duration::from_millis(2));
        };
        if query {
            let response: Value =
                serde_json::from_str(&fs::read_to_string(dir.join("20-1.result.json")).unwrap())
                    .unwrap();
            assert_eq!(response["status"], "applied");
        } else {
            assert_eq!(claimed["control_request"]["id"], "20-1");
        }
        assert_eq!(fixture.owner(), owner);
        assert_eq!(
            fixture.engine.engine_call("project_export_model", ""),
            before
        );
        assert!(
            still_queued.into_iter().all(|queued| queued),
            "Read-only claims must not reject Save or other clients' controls: {still_queued:?}"
        );
    }
}
