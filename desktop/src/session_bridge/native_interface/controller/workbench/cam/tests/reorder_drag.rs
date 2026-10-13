use super::super::reorder_drag as drag;
use super::*;
use crate::native_viewport::interface_shell::{PointerButton, PointerPhase};
use crate::native_viewport::winit_host::NativeHostInput;
use bevy::{
    input::{
        keyboard::{Key, KeyCode, KeyboardInput},
        ButtonState,
    },
    ui::{ComputedStackIndex, UiGlobalTransform},
    winit::{UpdateMode, WinitSettings},
};
use std::time::Instant;

fn job_with_rows() -> CamDocumentDto {
    let mut cam = job();
    for _ in 0..4 {
        cam = duplicate(&cam, Selection::Operation(7)).unwrap().0;
    }
    duplicate(&cam, Selection::Setup(3)).unwrap().0
}
fn setup() -> (Fixture, App, NativeInterfaceHandle, NativeServices) {
    let fixture = Fixture::new();
    parse_engine_envelope(fixture.engine.engine_call(
        "cam_set_document",
        &serde_json::to_string(&job_with_rows()).unwrap(),
    ))
    .unwrap();
    let app = native_viewport::interface_scene_fixture();
    let handle = NativeInterfaceHandle::new(|| {});
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    (fixture, app, handle, services)
}
fn publish(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    fixture: &Fixture,
    tab: Tab,
    page: usize,
) {
    let owner = fixture.owner();
    let receipt = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &owner)
        .unwrap();
    let cam = fixture.engine.cam_document_snapshot();
    let selections = rows(&cam, tab);
    let old = world
        .query::<(Entity, &NativeCommandBinding)>()
        .iter(world)
        .filter(|(_, binding)| matches!(binding.command, NativeCommand::Cam(_)))
        .map(|(entity, _)| entity)
        .collect::<Vec<_>>();
    for entity in old {
        world.despawn(entity);
    }
    let draft = selections
        .first()
        .map(|(s, _)| Draft::new(&cam, *s).unwrap());
    world.insert_resource(Editor {
        owner: Some(owner.clone()),
        revision: receipt.revision,
        cam,
        tab,
        page,
        draft,
        ..default()
    });
    world.insert_resource(super::super::super::Workbench {
        owner: Some(owner.clone()),
        workspace: Workspace::Cam,
        ..default()
    });
    for (index, (selection, label)) in selections.into_iter().enumerate().skip(page * 3).take(3) {
        let mut control = InterfaceControl::button("cam/document", label);
        control.owned_keys = ["ArrowUp", "ArrowDown"]
            .map(|key| KeyChord {
                key: key.into(),
                alt: true,
                ..default()
            })
            .into();
        let entity = world
            .spawn((
                control,
                ComputedNode {
                    size: Vec2::new(228., 28.),
                    inverse_scale_factor: 1.,
                    ..default()
                },
                UiGlobalTransform::from_translation(Vec2::new(
                    124.,
                    160. + (index % 3) as f32 * 31.,
                )),
                ComputedStackIndex(index as u32 + 2),
                InheritedVisibility::VISIBLE,
            ))
            .id();
        bind_command(
            world,
            entity,
            NativeCommand::Cam(Command::Select(selection)),
        )
        .unwrap();
    }
    let bounds = InterfaceRect {
        x: 0.,
        y: 0.,
        width: 1200.,
        height: 860.,
    };
    handle
        .present(InterfaceFrame {
            context: owner,
            client: bounds,
            surface: bounds,
            canvases: vec![],
            surfaces: vec![Surface {
                name: "cam/document".into(),
                text: None,
            }],
            modal_stack: vec![],
            document_visible: true,
        })
        .unwrap();
    interface_shell::tests::publish_layout_once(world, handle.clone());
}
fn pointer(owner: &DocumentContext, x: f32, y: f32, state: Option<ButtonState>) -> NativeHostInput {
    let cursor = Vec2::new(x, y);
    NativeHostInput {
        ui_scale: 1.,
        context: Some(owner.clone()),
        cursor: Some(cursor),
        modifiers: default(),
        consumed: false,
        actions: vec![],
        event: state.map_or_else(
            || {
                WindowEvent::CursorMoved(bevy::window::CursorMoved {
                    window: Entity::PLACEHOLDER,
                    position: cursor,
                    delta: None,
                })
            },
            |state| {
                WindowEvent::MouseButtonInput(bevy::input::mouse::MouseButtonInput {
                    window: Entity::PLACEHOLDER,
                    button: MouseButton::Left,
                    state,
                })
            },
        ),
    }
}
fn key(owner: &DocumentContext, key: Key, code: KeyCode, alt: bool) -> NativeHostInput {
    let mut event = pointer(owner, 124., 160., None);
    event.modifiers.alt = alt;
    event.event = WindowEvent::KeyboardInput(KeyboardInput {
        window: Entity::PLACEHOLDER,
        key_code: code,
        logical_key: key,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
    });
    event
}
fn gesture(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    owner: &DocumentContext,
    y: f32,
    state: Option<ButtonState>,
) -> bool {
    drag::input(world, handle, services, &pointer(owner, 124., y, state)).unwrap()
}
fn drain(world: &mut World, services: &NativeServices) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(result) = worker::poll(world, services) {
            result.value.unwrap();
            return;
        }
        assert!(Instant::now() < deadline, "CAM reorder worker timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn exact_history(fixture: &Fixture, before: &Value, after: &Value) {
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &fixture.owner(), false, || Ok(()))
        .unwrap();
    assert_eq!(
        &export(fixture),
        before,
        "One Undo must restore the entire pre-drag model"
    );
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &fixture.owner(), true, || Ok(()))
        .unwrap();
    assert_eq!(
        &export(fixture),
        after,
        "One Redo must restore the exact order and keyed intent"
    );
}

#[test]
fn insertion_slots_remove_the_source_once_and_ignore_both_unchanged_slots() {
    let ids = [7, 8, 9, 10, 11];
    assert_eq!(drag::ordered(&ids, 7, 5), Some(vec![8, 9, 10, 11, 7]));
    assert_eq!(drag::ordered(&ids, 11, 0), Some(vec![11, 7, 8, 9, 10]));
    for slot in [2, 3, 6] {
        assert_eq!(drag::ordered(&ids, 9, slot), None);
    }
    assert_eq!(drag::ordered(&ids, 999, 0), None);
}

#[test]
fn cam_pointer_drop_crosses_pages_commits_once_and_keeps_exact_history() {
    use ButtonState::{Pressed, Released};
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let (fixture, mut app, handle, services) = setup();
    let mode = UpdateMode::reactive_low_power(Duration::MAX);
    app.insert_resource(WinitSettings {
        focused_mode: mode,
        unfocused_mode: mode,
    });
    publish(app.world_mut(), &handle, &fixture, Tab::Toolpaths, 0);
    worker::install(app.world_mut(), services.clone(), handle.clone()).unwrap();
    let owner = fixture.owner();
    let receipt = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &owner)
        .unwrap();
    app.insert_resource(NativeRenderedDocument {
        owner: owner.clone(),
        revision: receipt.revision,
        bodies: vec![],
    });
    let mut availability = crate::native_viewport::winit_host::NativeRenderAvailability::default();
    availability.focused = true;
    availability.drawable = true;
    app.insert_resource(availability);
    use crate::session_bridge::native_interface::controller::six_dof;
    assert!(six_dof::eligible(app.world(), &handle, false).is_some());
    let original = fixture.engine.cam_document_snapshot();
    let before = export(&fixture);
    assert!(!gesture(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        160.,
        Some(Pressed)
    ));
    handle
        .pointer(PointerPhase::Down, [124., 160.], PointerButton::Primary)
        .unwrap();
    assert!(!gesture(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        164.,
        None
    ));
    assert!(handle.has_capture());
    assert!(gesture(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        234.,
        None
    ));
    assert!(
        !handle.has_capture(),
        "Drag threshold must cancel ordinary row activation"
    );
    assert!(
        six_dof::eligible(app.world(), &handle, false).is_none(),
        "CAM drag ownership must still block device motion and Fit after releasing ordinary capture"
    );
    assert!(!worker::busy(app.world()));
    assert_eq!(export(&fixture), before);
    drag::tick_at(
        app.world_mut(),
        &handle,
        &services,
        Instant::now() + Duration::from_secs(1),
    )
    .unwrap();
    assert_eq!(app.world().resource::<Editor>().page, 1);
    assert!(
        matches!(app.world().resource::<WinitSettings>().focused_mode,UpdateMode::Reactive {wait,..} if wait<=Duration::from_millis(120))
    );
    publish(app.world_mut(), &handle, &fixture, Tab::Toolpaths, 1);
    assert!(gesture(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        201.,
        None
    ));
    drag::tick_at(
        app.world_mut(),
        &handle,
        &services,
        Instant::now() + Duration::from_secs(2),
    )
    .unwrap();
    assert_eq!(
        app.world().resource::<Editor>().page,
        1,
        "Paging must not escape the operation's setup"
    );
    assert_eq!(export(&fixture), before);
    assert!(gesture(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        201.,
        Some(Released)
    ));
    assert!(worker::busy(app.world()));
    drain(app.world_mut(), &services);
    let mut expected = original.clone();
    expected.setups[0].operations.rotate_left(1);
    assert_eq!(fixture.engine.cam_document_snapshot(), expected);
    let after = export(&fixture);
    exact_history(&fixture, &before, &after);
    assert_eq!(app.world().resource::<WinitSettings>().focused_mode, mode);
    assert!(
        !gesture(
            app.world_mut(),
            &handle,
            &services,
            &owner,
            201.,
            Some(Released)
        ),
        "Duplicate Up must not submit again"
    );
    assert!(!worker::busy(app.world()));
}

#[test]
fn cam_drag_noop_escape_cross_setup_occlusion_dirty_and_stale_do_not_mutate() {
    use ButtonState::{Pressed, Released};
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let (fixture, mut app, handle, services) = setup();
    let owner = fixture.owner();
    let before = export(&fixture);
    publish(app.world_mut(), &handle, &fixture, Tab::Toolpaths, 0);
    for end in [162., 173.] {
        gesture(
            app.world_mut(),
            &handle,
            &services,
            &owner,
            160.,
            Some(Pressed),
        );
        gesture(app.world_mut(), &handle, &services, &owner, end, None);
        gesture(
            app.world_mut(),
            &handle,
            &services,
            &owner,
            end,
            Some(Released),
        );
        assert_eq!(export(&fixture), before);
    }
    gesture(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        160.,
        Some(Pressed),
    );
    gesture(app.world_mut(), &handle, &services, &owner, 201., None);
    assert!(drag::input(
        app.world_mut(),
        &handle,
        &services,
        &key(&owner, Key::Escape, KeyCode::Escape, false)
    )
    .unwrap());
    gesture(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        201.,
        Some(Released),
    );
    assert_eq!(export(&fixture), before);
    publish(app.world_mut(), &handle, &fixture, Tab::Toolpaths, 1);
    gesture(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        160.,
        Some(Pressed),
    );
    gesture(app.world_mut(), &handle, &services, &owner, 230., None);
    gesture(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        230.,
        Some(Released),
    );
    assert_eq!(export(&fixture), before);
    let overlay = app
        .world_mut()
        .spawn((
            interface_shell::InterfaceOccluder,
            ComputedNode {
                size: Vec2::new(228., 28.),
                inverse_scale_factor: 1.,
                ..default()
            },
            UiGlobalTransform::from_translation(Vec2::new(124., 191.)),
            ComputedStackIndex(100),
            InheritedVisibility::VISIBLE,
        ))
        .id();
    interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
    gesture(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        160.,
        Some(Pressed),
    );
    gesture(app.world_mut(), &handle, &services, &owner, 201., None);
    gesture(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        201.,
        Some(Released),
    );
    assert_eq!(export(&fixture), before);
    app.world_mut().despawn(overlay);
    publish(app.world_mut(), &handle, &fixture, Tab::Toolpaths, 0);
    gesture(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        160.,
        Some(Pressed),
    );
    set(
        app.world_mut()
            .resource_mut::<Editor>()
            .draft
            .as_mut()
            .unwrap(),
        "/name",
        "Unapplied",
    );
    assert!(drag::input(
        app.world_mut(),
        &handle,
        &services,
        &pointer(&owner, 124., 230., None)
    )
    .unwrap_err()
    .contains("Apply or reset"));
    assert_eq!(export(&fixture), before);
    publish(app.world_mut(), &handle, &fixture, Tab::Toolpaths, 0);
    gesture(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        160.,
        Some(Pressed),
    );
    fixture.rename(&owner, "External revision").unwrap();
    let revised = export(&fixture);
    assert!(drag::input(
        app.world_mut(),
        &handle,
        &services,
        &pointer(&owner, 124., 230., Some(Released))
    )
    .is_err());
    assert_eq!(export(&fixture), revised);
}

#[test]
fn cam_focused_alt_arrows_share_validated_order_and_leave_text_fields_alone() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let (fixture, mut app, handle, services) = setup();
    let owner = fixture.owner();
    let before = export(&fixture);
    publish(app.world_mut(), &handle, &fixture, Tab::Setups, 0);
    handle
        .pointer(PointerPhase::Down, [124., 160.], PointerButton::Primary)
        .unwrap();
    handle.cancel_pointer();
    assert!(drag::input(
        app.world_mut(),
        &handle,
        &services,
        &key(&owner, Key::ArrowDown, KeyCode::ArrowDown, true)
    )
    .unwrap());
    let after = export(&fixture);
    assert_ne!(after, before);
    let mut expected = job_with_rows();
    expected.setups.swap(0, 1);
    assert_eq!(fixture.engine.cam_document_snapshot(), expected);
    exact_history(&fixture, &before, &after);
    publish(app.world_mut(), &handle, &fixture, Tab::Tools, 0);
    handle
        .pointer(PointerPhase::Down, [124., 160.], PointerButton::Primary)
        .unwrap();
    handle.cancel_pointer();
    assert!(!drag::input(
        app.world_mut(),
        &handle,
        &services,
        &key(&fixture.owner(), Key::ArrowUp, KeyCode::ArrowUp, true)
    )
    .unwrap());
    assert_eq!(export(&fixture), after);
}

#[test]
fn cam_drag_cancels_on_focus_loss_same_frame_modal_and_document_replacement() {
    use ButtonState::{Pressed, Released};
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let (fixture, mut app, handle, services) = setup();
    let owner = fixture.owner();
    let before = export(&fixture);
    publish(app.world_mut(), &handle, &fixture, Tab::Toolpaths, 0);
    gesture(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        160.,
        Some(Pressed),
    );
    gesture(app.world_mut(), &handle, &services, &owner, 201., None);
    assert!(
        drag::active(app.world()),
        "Other input devices must see ownership after the drag threshold"
    );
    let mut lost = pointer(&owner, 124., 201., None);
    lost.event = WindowEvent::WindowFocused(bevy::window::WindowFocused {
        window: Entity::PLACEHOLDER,
        focused: false,
    });
    assert!(!drag::input(app.world_mut(), &handle, &services, &lost).unwrap());
    assert!(!drag::active(app.world()));
    gesture(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        201.,
        Some(Released),
    );
    assert_eq!(export(&fixture), before);
    gesture(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        160.,
        Some(Pressed),
    );
    app.world_mut()
        .resource_mut::<super::super::super::Workbench>()
        .menu = Some("workspace".into());
    assert!(handle.frame().unwrap().modal_stack.is_empty());
    assert!(drag::input(
        app.world_mut(),
        &handle,
        &services,
        &pointer(&owner, 124., 201., None)
    )
    .is_err());
    assert!(!drag::active(app.world()));
    assert_eq!(export(&fixture), before);
    publish(app.world_mut(), &handle, &fixture, Tab::Toolpaths, 0);
    gesture(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        160.,
        Some(Pressed),
    );
    parse_engine_envelope(fixture.bridge.with_project_session_transition(
        "main",
        &fixture.engine,
        || fixture.engine.create_project_session("cam-other"),
    ))
    .unwrap();
    let replacement = export(&fixture);
    assert!(drag::input(
        app.world_mut(),
        &handle,
        &services,
        &pointer(&owner, 124., 201., Some(Released))
    )
    .is_err());
    assert!(!drag::active(app.world()));
    assert_eq!(export(&fixture), replacement);
}

fn polling_controller(
    world: &mut World,
    fixture: &Fixture,
) -> crate::session_bridge::native_interface::controller::Controller {
    use crate::session_bridge::native_interface::controller::{Controller, PolledControl};
    let owner = fixture.owner();
    let receipt = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &owner)
        .unwrap();
    let session = fixture
        .bridge
        .session_id_for_window(&owner.window_id)
        .unwrap()
        .unwrap();
    let mut state = Controller::new(
        owner.window_id.clone(),
        None,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    );
    state.synchronized = Some((owner.clone(), receipt.revision));
    worker::enqueue_control_poll(world, owner.clone(), "987-1".into()).unwrap();
    state.polled_control = Some(PolledControl {
        owner,
        session,
        id: "987-1".into(),
        interface_only: true,
    });
    state
}

#[test]
fn cam_drag_release_during_interface_poll_replays_once_after_idle() {
    use crate::session_bridge::native_interface::controller;
    use ButtonState::{Pressed, Released};
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let (fixture, mut app, handle, services) = setup();
    let owner = fixture.owner();
    let before = export(&fixture);
    let original = fixture.engine.cam_document_snapshot();
    publish(app.world_mut(), &handle, &fixture, Tab::Toolpaths, 0);
    app.init_resource::<Messages<NativeHostInput>>();
    worker::install(app.world_mut(), services.clone(), handle.clone()).unwrap();
    gesture(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        160.,
        Some(Pressed),
    );
    let mut state = polling_controller(app.world_mut(), &fixture);
    controller::maintain_busy_window(app.world_mut(), &handle, &mut state).unwrap();
    assert!(drag::active(app.world()));
    for step in 0..256 {
        app.world_mut()
            .write_message(pointer(&owner, 124., 170. + step as f32 * 0.12, None));
    }
    app.world_mut()
        .write_message(pointer(&owner, 124., 201., Some(Released)));
    controller::maintain_busy_window(app.world_mut(), &handle, &mut state).unwrap();
    assert!(worker::busy(app.world()));
    assert!(drag::active(app.world()));
    assert_eq!(export(&fixture), before);
    assert_eq!(state.deferred_pointer.as_ref().unwrap().events.len(), 2);
    drain(app.world_mut(), &services);
    state.polled_control = None;
    let deferred =
        controller::take_deferred_pointer_input(app.world_mut(), &handle, &services, &mut state)
            .unwrap();
    for event in deferred {
        drag::input(app.world_mut(), &handle, &services, &event).unwrap();
    }
    assert!(!drag::active(app.world()));
    assert!(worker::busy(app.world()));
    drain(app.world_mut(), &services);
    let mut expected = original;
    expected.setups[0].operations.swap(0, 1);
    assert_eq!(fixture.engine.cam_document_snapshot(), expected);
    let after = export(&fixture);
    exact_history(&fixture, &before, &after);
    assert!(controller::take_deferred_pointer_input(
        app.world_mut(),
        &handle,
        &services,
        &mut state,
    )
    .unwrap()
    .is_empty());
    assert!(!worker::busy(app.world()));
    assert_eq!(export(&fixture), after);
}

#[test]
fn deferred_cam_drop_cannot_cross_focus_loss_revision_owner_or_model_work() {
    use crate::session_bridge::native_interface::controller;
    use ButtonState::{Pressed, Released};
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    for invalidation in [
        "focus", "revision", "owner", "model", "io", "solver", "command",
    ] {
        let (fixture, mut app, handle, services) = setup();
        let owner = fixture.owner();
        publish(app.world_mut(), &handle, &fixture, Tab::Toolpaths, 0);
        app.init_resource::<Messages<NativeHostInput>>();
        worker::install(app.world_mut(), services.clone(), handle.clone()).unwrap();
        gesture(
            app.world_mut(),
            &handle,
            &services,
            &owner,
            160.,
            Some(Pressed),
        );
        gesture(app.world_mut(), &handle, &services, &owner, 201., None);
        let mut state = polling_controller(app.world_mut(), &fixture);
        app.world_mut()
            .write_message(pointer(&owner, 124., 201., Some(Released)));
        controller::maintain_busy_window(app.world_mut(), &handle, &mut state).unwrap();
        assert!(state.deferred_pointer.is_some(), "{invalidation}");
        if invalidation == "focus" {
            let mut event = pointer(&owner, 124., 201., None);
            event.event = WindowEvent::WindowFocused(bevy::window::WindowFocused {
                window: Entity::PLACEHOLDER,
                focused: false,
            });
            app.world_mut().write_message(event);
            controller::maintain_busy_window(app.world_mut(), &handle, &mut state).unwrap();
        }
        if invalidation == "solver" {
            state.polled_control.as_mut().unwrap().interface_only = false;
            controller::maintain_busy_window(app.world_mut(), &handle, &mut state).unwrap();
        }
        drain(app.world_mut(), &services);
        state.polled_control = None;
        match invalidation {
            "command" => {
                let has_primary = {
                    let world = app.world_mut();
                    world
                        .query_filtered::<&Window, With<bevy::window::PrimaryWindow>>()
                        .iter(world)
                        .next()
                        .is_some()
                };
                if !has_primary {
                    app.world_mut()
                        .spawn((Window::default(), bevy::window::PrimaryWindow));
                }
                controller::start_control(
                    app.world_mut(),
                    &handle,
                    &services,
                    &mut state,
                    &owner,
                    &json!({"id":"987-2","session_id":"row-drag-test","expires_ms":crate::session_bridge::now_ms()+30_000,
                        "ui":{"action":"window","mode":"inspect"}}),
                )
                .unwrap();
                assert_eq!(
                    state.pending.as_ref().unwrap().response["status"],
                    "applied"
                );
            }
            "revision" => {
                fixture.rename(&owner, "Newer model revision").unwrap();
            }
            "owner" => {
                parse_engine_envelope(fixture.bridge.with_project_session_transition(
                    "main",
                    &fixture.engine,
                    || fixture.engine.create_project_session("deferred-other"),
                ))
                .unwrap();
            }
            "model" => {
                let receipt = fixture
                    .bridge
                    .native_document_receipt(&fixture.engine, &owner)
                    .unwrap();
                worker::enqueue_operation(
                    app.world_mut(),
                    owner.clone(),
                    receipt.revision,
                    "cad_set_document_name".into(),
                    json!({"name":"A later explicit mutation"}),
                    |_, _, result| Ok(result?.value),
                )
                .unwrap();
                controller::maintain_busy_window(app.world_mut(), &handle, &mut state).unwrap();
                drain(app.world_mut(), &services);
            }
            "io" => {
                let receipt = fixture
                    .bridge
                    .native_document_receipt(&fixture.engine, &owner)
                    .unwrap();
                worker::enqueue_document_io(
                    app.world_mut(),
                    "save without rebuilding the scene".into(),
                    move |_, guard| {
                        guard.validate()?;
                        Ok(NativeMutationResult {
                            context: receipt.owner,
                            engine_revision: receipt.revision,
                            value: Value::Null,
                        })
                    },
                    |_, _, result| Ok(result?.value),
                )
                .unwrap();
                controller::maintain_busy_window(app.world_mut(), &handle, &mut state).unwrap();
                drain(app.world_mut(), &services);
            }
            _ => {}
        }
        let current = export(&fixture);
        let result = controller::take_deferred_pointer_input(
            app.world_mut(),
            &handle,
            &services,
            &mut state,
        );
        assert!(
            result.as_ref().map_or(true, |events| events.is_empty()),
            "{invalidation} replayed a drop stamped for older input"
        );
        assert!(state.deferred_pointer.is_none(), "{invalidation}");
        assert!(!drag::active(app.world()), "{invalidation}");
        assert!(!worker::busy(app.world()), "{invalidation}");
        assert_eq!(export(&fixture), current, "{invalidation}");
    }
}
