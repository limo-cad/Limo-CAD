use super::super::tests::Fixture;
use super::*;
use crate::session_bridge::parse_engine_envelope;
use std::fs;

mod playback_priority;

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
