use super::*;

fn command(state: &mut Playback, now: Instant, value: Value) -> Result<Value, String> {
    state.control(serde_json::from_value(value).unwrap(), now)
}

#[test]
fn presentation_holds_pause_step_and_finish_follow_the_existing_runner_protocol() {
    let mut state = Playback::default();
    let now = Instant::now();
    command(
        &mut state,
        now,
        json!({"command":"configure","mode":"present","speed":2.,"step_index":0,"step_count":8}),
    )
    .unwrap();
    let note = command(
        &mut state,
        now,
        json!({"command":"note","text":"A rounded corner","duration_ms":800}),
    )
    .unwrap();
    assert_eq!(note["wait_ms"], 400);
    let pause = command(
        &mut state,
        now + Duration::from_millis(100),
        json!({"command":"pause"}),
    )
    .unwrap();
    assert_eq!(pause["wait_ms"], 300);
    assert_eq!(state.status(now + Duration::from_secs(2))["wait_ms"], 300);
    assert_eq!(
        command(
            &mut state,
            now + Duration::from_secs(2),
            json!({"command":"step"})
        )
        .unwrap()["step_pending"],
        true
    );
    command(
        &mut state,
        now + Duration::from_secs(2),
        json!({"command":"dismiss"}),
    )
    .unwrap();
    assert!(state.state.paused && state.credits == 1 && !state.state.visible);
    command(
        &mut state,
        now + Duration::from_secs(2),
        json!({"command":"show"}),
    )
    .unwrap();
    command(
        &mut state,
        now + Duration::from_secs(2),
        json!({"command":"resume"}),
    )
    .unwrap();
    assert_eq!(
        state.status(now + Duration::from_millis(2300))["wait_ms"],
        0
    );
    let complete = command(
        &mut state,
        now + Duration::from_millis(2300),
        json!({"command":"finish","step_index":8,"step_count":8}),
    )
    .unwrap();
    assert_eq!(complete["finished"], true);
    assert_eq!(complete["step_pending"], false);
    assert_eq!(complete["step_index"], 8);
}

#[test]
fn presentation_rejects_invalid_requests_without_changing_state_and_resets_owners() {
    let mut state = Playback::default();
    let now = Instant::now();
    let mut owner = DocumentContext {
        window_id: "main".into(),
        document_id: "a".into(),
        epoch: 1,
    };
    state.observe(&owner, now);
    command(
        &mut state,
        now,
        json!({"command":"configure","mode":"present","step_count":8}),
    )
    .unwrap();
    let original = state.status(now);
    for invalid in [
        json!({"command":"note","duration_ms":10001}),
        json!({"command":"configure","speed":0.}),
        json!({"command":"note","step_index":9}),
        json!({"command":"note","text":"\u{7}"}),
    ] {
        assert!(command(&mut state, now, invalid).is_err());
        assert_eq!(state.status(now), original);
    }
    command(&mut state, now, json!({"command":"stop"})).unwrap();
    assert!(command(&mut state, now, json!({"command":"finish"})).is_err());
    assert!(command(&mut state, now, json!({"command":"resume"})).is_err());
    owner.epoch += 1;
    state.observe(&owner, now);
    assert!(!state.state.active && !state.state.stopped && state.credits == 0);
}

#[test]
fn paused_and_stopped_playback_survive_resident_tab_round_trips() {
    let mut world = World::new();
    let a = DocumentContext {
        window_id: "main".into(),
        document_id: "a".into(),
        epoch: 1,
    };
    let b = DocumentContext {
        document_id: "b".into(),
        ..a.clone()
    };
    assert_eq!(gate(&mut world, &a), Gate::Ready);
    command(
        &mut world.resource_mut::<Playback>(),
        Instant::now(),
        json!({"command":"pause"}),
    )
    .unwrap();
    assert_eq!(gate(&mut world, &b), Gate::Ready);
    assert_eq!(gate(&mut world, &a), Gate::Waiting);
    command(
        &mut world.resource_mut::<Playback>(),
        Instant::now(),
        json!({"command":"step"}),
    )
    .unwrap();
    assert_eq!(gate(&mut world, &b), Gate::Ready);
    assert_eq!(gate(&mut world, &a), Gate::Ready);
    assert_eq!(world.resource::<Playback>().credits, 1);
    command(
        &mut world.resource_mut::<Playback>(),
        Instant::now(),
        json!({"command":"stop"}),
    )
    .unwrap();
    assert_eq!(gate(&mut world, &b), Gate::Ready);
    assert_eq!(gate(&mut world, &a), Gate::Stopped);
    assert_eq!(gate(&mut world, &b), Gate::Ready);
    let replacement = DocumentContext {
        epoch: 2,
        ..a.clone()
    };
    assert_eq!(gate(&mut world, &replacement), Gate::Ready);
    assert!(!world.resource::<Playback>().state.active);
    assert!(!world
        .resource::<Playback>()
        .saved
        .iter()
        .any(|saved| saved.owner == a));
}

#[test]
fn physical_pause_and_stop_during_modeling_change_only_owned_playback() {
    use bevy::ui::{ComputedStackIndex, UiGlobalTransform};
    let mut app = native_viewport::interface_scene_fixture();
    let world = app.world_mut();
    let handle = NativeInterfaceHandle::new(|| {});
    let owner = DocumentContext {
        window_id: "main".into(),
        document_id: "a".into(),
        epoch: 1,
    };
    observe(world, &owner);
    for (index, command) in [Command::Pause, Command::Stop].into_iter().enumerate() {
        let entity = world
            .spawn((
                InterfaceControl::button("document/presentation", format!("{command:?}")),
                ComputedNode {
                    size: Vec2::new(80., 24.),
                    inverse_scale_factor: 1.,
                    ..default()
                },
                UiGlobalTransform::from_translation(Vec2::new(140. + index as f32 * 90., 140.)),
                ComputedStackIndex(index as u32 + 1),
                InheritedVisibility::VISIBLE,
            ))
            .id();
        bind_command(world, entity, NativeCommand::Presentation(command)).unwrap();
    }
    let bounds = InterfaceRect {
        x: 0.,
        y: 0.,
        width: 800.,
        height: 600.,
    };
    handle
        .present(InterfaceFrame {
            context: owner.clone(),
            client: bounds,
            surface: bounds,
            canvases: vec![],
            surfaces: vec![Surface {
                name: "document/presentation".into(),
                text: None,
            }],
            modal_stack: vec![],
            document_visible: true,
        })
        .unwrap();
    interface_shell::tests::publish_layout_once(world, handle.clone());
    let click = |world: &mut World, context: &DocumentContext, x: f32| {
        for state in [ButtonState::Pressed, ButtonState::Released] {
            busy_input(
                world,
                &handle,
                &NativeHostInput {
                    ui_scale: 1.,
                    context: Some(context.clone()),
                    cursor: Some(Vec2::new(x, 140.)),
                    modifiers: default(),
                    event: WindowEvent::MouseButtonInput(bevy::input::mouse::MouseButtonInput {
                        button: MouseButton::Left,
                        state,
                        window: Entity::PLACEHOLDER,
                    }),
                    consumed: false,
                    actions: vec![],
                },
            )
            .unwrap();
        }
    };
    click(
        world,
        &DocumentContext {
            epoch: 2,
            ..owner.clone()
        },
        230.,
    );
    assert_eq!(gate(world, &owner), Gate::Ready);
    busy_input(
        world,
        &handle,
        &NativeHostInput {
            ui_scale: 1.,
            context: Some(owner.clone()),
            cursor: Some(Vec2::new(140., 140.)),
            modifiers: default(),
            event: WindowEvent::MouseButtonInput(bevy::input::mouse::MouseButtonInput {
                button: MouseButton::Left,
                state: ButtonState::Pressed,
                window: Entity::PLACEHOLDER,
            }),
            consumed: false,
            actions: vec![],
        },
    )
    .unwrap();
    handle
        .pointer(
            interface_shell::PointerPhase::Up,
            [140., 140.],
            interface_shell::PointerButton::Primary,
        )
        .unwrap();
    handle.take_actions().unwrap();
    assert!(
        !busy_input(
            world,
            &handle,
            &NativeHostInput {
                ui_scale: 1.,
                context: Some(owner.clone()),
                cursor: Some(Vec2::new(500., 300.)),
                modifiers: default(),
                event: WindowEvent::CursorMoved(bevy::window::CursorMoved {
                    window: Entity::PLACEHOLDER,
                    position: Vec2::new(500., 300.),
                    delta: None,
                }),
                consumed: false,
                actions: vec![],
            }
        )
        .unwrap(),
        "A completed busy click must not swallow later camera navigation"
    );
    assert!(!world.resource::<Playback>().busy_pointer);
    click(world, &owner, 140.);
    assert_eq!(gate(world, &owner), Gate::Waiting);
    applied(
        world,
        &owner,
        &json!({"applied":true,"name":"solid_extrude"}),
    );
    assert_eq!(gate(world, &owner), Gate::Waiting);
    click(world, &owner, 230.);
    applied(
        world,
        &owner,
        &json!({"applied":true,"name":"solid_extrude"}),
    );
    assert_eq!(gate(world, &owner), Gate::Stopped);
}

#[test]
fn busy_modeling_preserves_presentation_and_script_camera_controls_until_idle() {
    use crate::session_bridge::{
        atomic_write, pending_control_requests, reject_busy_controls, session_root,
    };
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = crate::session_bridge::native_interface::tests::Fixture::new();
    let session = fixture
        .bridge
        .session_id_for_window("main")
        .unwrap()
        .unwrap();
    let dir = session_root().join(&session).join("controls");
    std::fs::create_dir_all(&dir).unwrap();
    for (id, ui) in [
        ("123-1", json!({"action":"presentation","command":"pause"})),
        ("123-2", json!({"action":"presentation","command":"status"})),
        ("123-3", json!({"action":"presentation","command":"stop"})),
        ("123-4", json!({"action":"click","target":"new-sketch"})),
    ] {
        atomic_write(
            &dir.join(format!("{id}.request.json")),
            &json!({"id":id,"expires_ms":now_ms()+3000,"ui":ui}).to_string(),
        )
        .unwrap();
    }
    for (id, view) in [("123-5", "isometric"), ("123-6", "not-a-view")] {
        atomic_write(
            &dir.join(format!("{id}.request.json")),
            &json!({
                "id":id,"expires_ms":now_ms()+3000,"view":view,"fit":true,"duration_ms":350,
            })
            .to_string(),
        )
        .unwrap();
    }
    reject_busy_controls(&session, None).unwrap();
    let remaining = pending_control_requests(&dir);
    assert_eq!(remaining.len(), 4);
    assert!(remaining
        .iter()
        .all(|(_, request)| request["ui"]["action"] == "presentation"
            || request["view"] == "isometric"));
    assert!(dir.join("123-4.result.json").exists());
    assert!(!dir.join("123-3.result.json").exists());
    assert!(!dir.join("123-5.result.json").exists());
    assert!(dir.join("123-6.result.json").exists());
}

#[test]
fn single_step_is_consumed_only_by_successful_owned_inbox_apply_and_status_is_top_level() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = crate::session_bridge::native_interface::tests::Fixture::new();
    let owner = fixture.owner();
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let handle = NativeInterfaceHandle::new(|| {});
    let mut world = World::new();
    let mut controller = Controller::new("main".into(), None, Arc::new(AtomicBool::new(false)));
    start_control(&mut world, &handle, &services, &mut controller, &owner,
        &json!({"id":"presentation-test","session_id":"presentation-test","expires_ms":now_ms()+3000,
            "ui":{"action":"presentation","command":"configure","mode":"present","step_count":5}})).unwrap();
    let response = &controller.pending.as_ref().unwrap().response;
    assert_eq!(response["status"], "applied");
    assert_eq!(
        response["presentation"]["active"], true,
        "Runner status must be on the response root"
    );
    let value = request(
        &mut world,
        &handle,
        &services,
        &owner,
        &json!({"command":"configure","mode":"present","step_count":5}),
    )
    .unwrap();
    assert_eq!(value["presentation"]["active"], true);
    request(
        &mut world,
        &handle,
        &services,
        &owner,
        &json!({"command":"pause"}),
    )
    .unwrap();
    assert_eq!(gate(&mut world, &owner), Gate::Waiting);
    request(
        &mut world,
        &handle,
        &services,
        &owner,
        &json!({"command":"step"}),
    )
    .unwrap();
    assert_eq!(gate(&mut world, &owner), Gate::Ready);
    applied(&mut world, &owner, &json!({"applied":false}));
    assert_eq!(gate(&mut world, &owner), Gate::Ready);
    applied(
        &mut world,
        &owner,
        &json!({"applied":true,"name":"solid_extrude","script_progress":{"steps_completed":3,"step_count":5}}),
    );
    assert_eq!(gate(&mut world, &owner), Gate::Waiting);
    assert_eq!(world.resource::<Playback>().state.step_index, 3);
    request(
        &mut world,
        &handle,
        &services,
        &owner,
        &json!({"command":"stop"}),
    )
    .unwrap();
    assert_eq!(gate(&mut world, &owner), Gate::Stopped);
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
    let replacement = fixture.owner();
    assert_ne!(replacement, owner);
    assert_eq!(gate(&mut world, &replacement), Gate::Ready);
    assert!(request(
        &mut world,
        &handle,
        &services,
        &owner,
        &json!({"command":"stop"})
    )
    .is_err());
    assert!(!world.resource::<Playback>().state.stopped);
}

#[test]
fn presentation_pace_and_camera_duration_use_the_same_configuration() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = crate::session_bridge::native_interface::tests::Fixture::new();
    let owner = fixture.owner();
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let handle = NativeInterfaceHandle::new(|| {});
    let mut world = World::new();
    pace(&mut world, &services, &owner, &json!(500)).unwrap();
    applied(
        &mut world,
        &owner,
        &json!({"applied":true,"name":"sketch_begin"}),
    );
    assert_eq!(gate(&mut world, &owner), Gate::Waiting);
    request(
        &mut world,
        &handle,
        &services,
        &owner,
        &json!({"command":"configure","mode":"fast"}),
    )
    .unwrap();
    assert_eq!(gate(&mut world, &owner), Gate::Ready);
    assert_eq!(motion_duration(&world, &owner, 500), 0);
    assert_eq!(world.resource::<Playback>().pace_ms, 0);
    request(
        &mut world,
        &handle,
        &services,
        &owner,
        &json!({"command":"configure","mode":"present","speed":2.}),
    )
    .unwrap();
    assert_eq!(motion_duration(&world, &owner, 500), 250);
    request(
        &mut world,
        &handle,
        &services,
        &owner,
        &json!({"command":"finish"}),
    )
    .unwrap();
    assert_eq!(motion_duration(&world, &owner, 500), 500);
}
