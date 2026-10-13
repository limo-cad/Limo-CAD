use super::*;
use crate::{
    native_viewport::interface_shell::{self, InterfaceControl},
    session_bridge::native_interface::tests::Fixture,
    six_dof_mouse::{MotionPacket, SixDofEvent, SixDofEventSink, SixDofMouseInfo},
};
use std::sync::{atomic::AtomicUsize, mpsc};

struct Fake(mpsc::Sender<SixDofEventSink>);
impl service::Backend for Fake {
    fn connect(&mut self, sink: SixDofEventSink) -> Result<SixDofMouseInfo, String> {
        self.0.send(sink).unwrap();
        Ok(SixDofMouseInfo {
            vendor_id: 1,
            product_id: 1,
            product_name: "Fake".into(),
            serial_number: None,
        })
    }
    fn disconnect(&mut self) -> Result<(), String> {
        Ok(())
    }
}
fn setup(
    fixture: &Fixture,
) -> (
    App,
    NativeInterfaceHandle,
    Arc<AtomicUsize>,
    SixDofEventSink,
) {
    let (_, handle, _, wakes) = interface_shell::tests::fixture();
    let mut frame = handle.frame().unwrap();
    frame.context = fixture.owner();
    handle.present(frame).unwrap();
    let mut app = native_viewport::interface_scene_fixture();
    native_viewport::apply_interface_model(app.world_mut(), model_snapshot(&fixture.engine))
        .unwrap();
    let revision = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &fixture.owner())
        .unwrap()
        .revision;
    app.insert_resource(NativeRenderedDocument {
        owner: fixture.owner(),
        revision,
        bodies: vec![],
    });
    let mut availability = NativeRenderAvailability::default();
    availability.focused = true;
    availability.drawable = true;
    app.insert_resource(availability);
    interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
    let (send, receive) = mpsc::channel();
    let service = service::Service::new(Fake(send)).unwrap();
    let (wake, ready) = mpsc::channel();
    service.register(
        "main".into(),
        Arc::new(move || {
            let _ = wake.send(());
        }),
    );
    service.request_at(0, true).unwrap();
    let sink = receive.recv_timeout(Duration::from_secs(3)).unwrap();
    while service.status().state != "connected" {
        ready.recv_timeout(Duration::from_secs(3)).unwrap();
    }
    app.insert_resource(Connection {
        service,
        window: "main".into(),
        observed: None,
        owner: None,
        previous: None,
        busy_pointer: false,
    });
    tick(app.world_mut(), &handle, false).unwrap();
    tick(app.world_mut(), &handle, false).unwrap();
    (app, handle, wakes, sink)
}
fn motion(sink: &SixDofEventSink) {
    sink(SixDofEvent::Motion(MotionPacket {
        translation: Some([350, 0, 0]),
        rotation: None,
    }));
}
fn camera(world: &World) -> native_viewport::ViewportCamera {
    native_viewport::interface_camera_snapshot(world).1
}

#[test]
fn native_device_motion_changes_only_camera_and_stops_scheduling_when_input_expires() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, handle, wakes, sink) = setup(&fixture);
    let before = fixture.engine.engine_call("project_export_model", "");
    let receipt = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &fixture.owner())
        .unwrap();
    let start = camera(app.world());
    motion(&sink);
    tick(app.world_mut(), &handle, false).unwrap();
    assert_ne!(camera(app.world()), start);
    let moved = camera(app.world());
    let owner = fixture.owner();
    app.world().resource::<Connection>().service.sample(
        "main",
        Some(&owner),
        Instant::now() + Duration::from_secs(1),
    );
    let count = wakes.load(Ordering::Relaxed);
    tick(app.world_mut(), &handle, false).unwrap();
    assert_eq!(camera(app.world()), moved);
    assert_eq!(
        wakes.load(Ordering::Relaxed),
        count,
        "idle device must not cause continuous redraws"
    );
    assert_eq!(
        fixture.engine.engine_call("project_export_model", ""),
        before
    );
    assert_eq!(
        fixture
            .bridge
            .native_document_receipt(&fixture.engine, &owner)
            .unwrap(),
        receipt
    );
}

#[test]
fn modal_focus_close_and_replaced_document_discard_held_motion() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, handle, _, sink) = setup(&fixture);
    let original = camera(app.world());
    motion(&sink);
    app.world_mut()
        .resource_mut::<NativeRenderAvailability>()
        .focused = false;
    tick(app.world_mut(), &handle, false).unwrap();
    app.world_mut()
        .resource_mut::<NativeRenderAvailability>()
        .focused = true;
    tick(app.world_mut(), &handle, false).unwrap();
    assert_eq!(camera(app.world()), original);
    motion(&sink);
    let mut frame = handle.frame().unwrap();
    frame.surfaces.push(Surface {
        name: "test-modal".into(),
        text: None,
    });
    frame.modal_stack = vec!["test-modal".into()];
    handle.present(frame.clone()).unwrap();
    tick(app.world_mut(), &handle, false).unwrap();
    frame.modal_stack.clear();
    handle.present(frame).unwrap();
    tick(app.world_mut(), &handle, false).unwrap();
    assert_eq!(camera(app.world()), original);
    motion(&sink);
    tick(app.world_mut(), &handle, true).unwrap();
    tick(app.world_mut(), &handle, false).unwrap();
    assert_eq!(camera(app.world()), original);
    motion(&sink);
    app.world_mut()
        .resource_mut::<NativeRenderedDocument>()
        .owner
        .epoch += 1;
    tick(app.world_mut(), &handle, false).unwrap();
    assert_eq!(camera(app.world()), original);
}

#[test]
fn actual_retained_button_rejects_a_rebound_connection_action_without_opening_hardware() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, handle, entity, _) = interface_shell::tests::fixture();
    let mut frame = handle.frame().unwrap();
    frame.context = fixture.owner();
    handle.present(frame).unwrap();
    app.insert_resource(NativeRenderedDocument {
        owner: fixture.owner(),
        revision: 0,
        bodies: vec![],
    });
    let (send, receive) = mpsc::channel();
    let service = service::Service::new(Fake(send)).unwrap();
    service.register("main".into(), Arc::new(|| {}));
    app.insert_resource(Connection {
        service: service.clone(),
        window: "main".into(),
        observed: None,
        owner: None,
        previous: None,
        busy_pointer: false,
    });
    let command = Command {
        generation: 0,
        connect: true,
    };
    bind_command(app.world_mut(), entity, NativeCommand::SixDof(command)).unwrap();
    app.world_mut().entity_mut(entity).insert(ConnectionButton);
    app.update();
    let stale = handle
        .resolve_retained(limo_cad_interface::ControlKey(entity.to_bits()))
        .unwrap();
    bind_command(app.world_mut(), entity, NativeCommand::SixDof(command)).unwrap();
    app.update();
    assert!(super::super::super::reduce_action(
        &fixture.engine,
        &fixture.bridge,
        app.world_mut(),
        &handle,
        &stale
    )
    .is_err());
    assert!(receive.try_recv().is_err());
    let fresh = handle
        .resolve_retained(limo_cad_interface::ControlKey(entity.to_bits()))
        .unwrap();
    super::super::super::reduce_action(
        &fixture.engine,
        &fixture.bridge,
        app.world_mut(),
        &handle,
        &fresh,
    )
    .unwrap();
    receive.recv_timeout(Duration::from_secs(3)).unwrap();
    assert!(super::super::super::reduce_action(
        &fixture.engine,
        &fixture.bridge,
        app.world_mut(),
        &handle,
        &fresh
    )
    .is_err());
    assert!(
        receive.try_recv().is_err(),
        "one activation cannot start two driver connections"
    );
    assert!(app.world().get::<InterfaceControl>(entity).is_some());
}

fn button(world: &mut World, label: &str, x: f32) -> Entity {
    use bevy::ui::{ComputedStackIndex, UiGlobalTransform};
    world
        .spawn((
            InterfaceControl::button("Viewport", label),
            ComputedNode {
                size: Vec2::new(80., 24.),
                inverse_scale_factor: 1.,
                ..default()
            },
            UiGlobalTransform::from_translation(Vec2::new(x, 42.)),
            ComputedStackIndex(1),
            InheritedVisibility::VISIBLE,
        ))
        .id()
}

#[test]
fn newly_opened_native_modal_blocks_motion_before_its_semantic_frame_is_published() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, handle, _, sink) = setup(&fixture);
    app.insert_resource(NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    });
    files::initialize(
        app.world_mut(),
        Arc::new(Mutex::new(workspace::DocumentWorkspace::default())),
    );
    let opener = button(app.world_mut(), "File", 60.);
    bind_command(
        app.world_mut(),
        opener,
        NativeCommand::File(files::FileCommand::Menu),
    )
    .unwrap();
    interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
    let action = handle
        .resolve_retained(limo_cad_interface::ControlKey(opener.to_bits()))
        .unwrap();
    let original = camera(app.world());
    motion(&sink);
    super::super::super::reduce_action(
        &fixture.engine,
        &fixture.bridge,
        app.world_mut(),
        &handle,
        &action,
    )
    .unwrap();
    assert_eq!(files::modal(app.world()), Some("file-menu"));
    assert!(
        handle.frame().unwrap().modal_stack.is_empty(),
        "Exercise the same-frame publication gap"
    );
    tick(app.world_mut(), &handle, false).unwrap();
    assert_eq!(camera(app.world()), original);
    super::super::super::reduce_action(
        &fixture.engine,
        &fixture.bridge,
        app.world_mut(),
        &handle,
        &action,
    )
    .unwrap();
    assert!(files::modal(app.world()).is_none());
    tick(app.world_mut(), &handle, false).unwrap();
    assert_eq!(camera(app.world()), original);
    motion(&sink);
    tick(app.world_mut(), &handle, false).unwrap();
    assert_ne!(camera(app.world()), original);
}

#[test]
fn busy_pointer_released_by_idle_adapter_cannot_consume_another_controls_later_capture() {
    use crate::native_viewport::{
        interface_shell::{PointerButton, PointerPhase},
        winit_host::Modifiers,
    };
    use bevy::input::{mouse::MouseButtonInput, ButtonState};
    let (mut app, handle, device, _) = interface_shell::tests::fixture();
    let (send, _receive) = mpsc::channel();
    let service = service::Service::new(Fake(send)).unwrap();
    service.register("main".into(), Arc::new(|| {}));
    app.insert_resource(Connection {
        service,
        window: "main".into(),
        observed: None,
        owner: None,
        previous: None,
        busy_pointer: false,
    });
    app.world_mut().entity_mut(device).insert(ConnectionButton);
    let other = button(app.world_mut(), "Other active control", 260.);
    app.update();
    let event = |x: f32, state| NativeHostInput {
        ui_scale: 1.,
        context: handle.frame().map(|frame| frame.context),
        cursor: Some(Vec2::new(x, 140.)),
        modifiers: Modifiers::default(),
        consumed: false,
        actions: vec![],
        event: WindowEvent::MouseButtonInput(MouseButtonInput {
            button: MouseButton::Left,
            state,
            window: Entity::PLACEHOLDER,
        }),
    };
    assert!(busy_input(app.world_mut(), &handle, &event(140., ButtonState::Pressed)).unwrap());
    assert!(handle.has_capture());
    handle
        .pointer(PointerPhase::Up, [140., 140.], PointerButton::Primary)
        .unwrap();
    handle.take_actions().unwrap();
    assert!(!handle.has_capture());
    assert!(!busy_input(app.world_mut(), &handle, &event(340., ButtonState::Pressed)).unwrap());
    handle
        .pointer(PointerPhase::Down, [340., 140.], PointerButton::Primary)
        .unwrap();
    assert!(handle.has_capture());
    assert!(!busy_input(
        app.world_mut(),
        &handle,
        &event(340., ButtonState::Released)
    )
    .unwrap());
    handle
        .pointer(PointerPhase::Up, [340., 140.], PointerButton::Primary)
        .unwrap();
    let actions = handle.take_actions().unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].control.key.0, other.to_bits());
}
