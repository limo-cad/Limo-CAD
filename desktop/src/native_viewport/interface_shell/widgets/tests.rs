use super::*;
use crate::native_viewport::winit_host::{prepare_native_input, HostInputState, Modifiers};
use bevy::input::keyboard::{Key, KeyCode, KeyboardInput};
use bevy::input::mouse::MouseButtonInput;

fn fixture() -> (App, NativeInterfaceHandle, Entity, Entity) {
    let (mut app, handle, control, _) = super::super::tests::fixture();
    app.init_resource::<HostInputState>();
    install(&mut app);
    let window = app.world_mut().spawn_empty().id();
    (app, handle, control, window)
}

fn send(
    app: &mut App,
    handle: &NativeInterfaceHandle,
    _window: Entity,
    event: WindowEvent,
    point: Vec2,
) -> Result<Vec<NativeInterfaceAction>, String> {
    let mut input = NativeHostInput {
        ui_scale: 1.,
        context: handle.presented_context(),
        cursor: Some(point),
        modifiers: Modifiers::default(),
        event,
        consumed: false,
        actions: vec![],
    };
    prepare_native_input(app.world_mut(), handle, &mut input)?;
    Ok(input.actions)
}

fn mouse(window: Entity, state: ButtonState, button: MouseButton) -> WindowEvent {
    WindowEvent::MouseButtonInput(MouseButtonInput {
        window,
        state,
        button,
    })
}

fn key(window: Entity, code: KeyCode, logical: Key) -> WindowEvent {
    WindowEvent::KeyboardInput(KeyboardInput {
        window,
        key_code: code,
        logical_key: logical,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
    })
}

#[test]
fn admitted_mouse_gesture_uses_bevy_pressed_and_activates_once_on_release() {
    let (mut app, handle, entity, window) = fixture();
    let point = Vec2::new(140., 140.);
    assert!(send(
        &mut app,
        &handle,
        window,
        mouse(window, ButtonState::Pressed, MouseButton::Left),
        point
    )
    .unwrap()
    .is_empty());
    assert!(app.world().get::<Button>(entity).is_some());
    assert!(app.world().get::<Pressed>(entity).is_some());
    app.update();
    assert!(app.world().get::<Pressed>(entity).is_some());
    let actions = send(
        &mut app,
        &handle,
        window,
        mouse(window, ButtonState::Released, MouseButton::Left),
        point,
    )
    .unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].control.key, ControlKey(entity.to_bits()));
    assert_eq!(actions[0].control.input, ControlInput::Click);
    assert!(app.world().get::<Pressed>(entity).is_none());
    assert!(send(
        &mut app,
        &handle,
        window,
        mouse(window, ButtonState::Released, MouseButton::Left),
        point
    )
    .unwrap()
    .is_empty());
}

#[test]
fn release_outside_and_window_deactivation_cancel_without_activation() {
    let (mut app, handle, entity, window) = fixture();
    let point = Vec2::new(140., 140.);
    send(
        &mut app,
        &handle,
        window,
        mouse(window, ButtonState::Pressed, MouseButton::Left),
        point,
    )
    .unwrap();
    assert!(send(
        &mut app,
        &handle,
        window,
        mouse(window, ButtonState::Released, MouseButton::Left),
        Vec2::new(950., 750.)
    )
    .unwrap()
    .is_empty());
    assert!(app.world().get::<Pressed>(entity).is_none());

    send(
        &mut app,
        &handle,
        window,
        mouse(window, ButtonState::Pressed, MouseButton::Left),
        point,
    )
    .unwrap();
    send(
        &mut app,
        &handle,
        window,
        WindowEvent::WindowFocused(bevy::window::WindowFocused {
            window,
            focused: false,
        }),
        point,
    )
    .unwrap();
    assert!(app.world().get::<Pressed>(entity).is_none());
    assert!(handle.take_actions().unwrap().is_empty());
}

#[test]
fn checkbox_proposal_preserves_command_intent_and_projects_only_accepted_state() {
    let (mut app, handle, entity, window) = fixture();
    {
        let mut control = app.world_mut().get_mut::<InterfaceControl>(entity).unwrap();
        control.role = "checkbox".into();
        control.field = Field::Toggle(false);
    }
    app.update();
    let point = Vec2::new(140., 140.);
    send(
        &mut app,
        &handle,
        window,
        mouse(window, ButtonState::Pressed, MouseButton::Left),
        point,
    )
    .unwrap();
    let actions = send(
        &mut app,
        &handle,
        window,
        mouse(window, ButtonState::Released, MouseButton::Left),
        point,
    )
    .unwrap();
    assert_eq!(actions.len(), 1);
    assert!(app.world().get::<Checkbox>(entity).is_some());
    assert!(app.world().get::<Button>(entity).is_none());
    assert!(app.world().get::<Checked>(entity).is_none());
    assert_eq!(actions[0].control.input, ControlInput::Click);
    app.world_mut()
        .get_mut::<InterfaceControl>(entity)
        .unwrap()
        .field = Field::Toggle(true);
    let actions = send(
        &mut app,
        &handle,
        window,
        key(window, KeyCode::Space, Key::Space),
        point,
    )
    .unwrap();
    assert_eq!(actions.len(), 1);
    assert!(app.world().get::<Checked>(entity).is_some());
}

#[test]
fn unadmitted_widget_events_cannot_dispatch_to_a_current_or_replacement_document() {
    let (mut app, handle, entity, window) = fixture();
    let point = Vec2::new(140., 140.);
    send(
        &mut app,
        &handle,
        window,
        mouse(window, ButtonState::Pressed, MouseButton::Left),
        point,
    )
    .unwrap();
    app.world_mut().trigger(Activate { entity });
    app.world_mut().trigger(ValueChange::<bool> {
        source: entity,
        value: true,
        is_final: true,
    });
    app.world_mut().flush();
    assert!(handle.take_actions().unwrap().is_empty());
    assert!(!app.world().resource::<WidgetInput>().emitted);
    handle
        .present(super::super::tests::frame("replacement", 2))
        .unwrap();
    app.update();
    assert!(app.world().get::<Pressed>(entity).is_none());
    assert!(send(
        &mut app,
        &handle,
        window,
        mouse(window, ButtonState::Released, MouseButton::Left),
        point
    )
    .unwrap()
    .is_empty());
}

#[test]
fn rebound_control_does_not_receive_the_original_press() {
    let (mut app, handle, entity, window) = fixture();
    let point = Vec2::new(140., 140.);
    send(
        &mut app,
        &handle,
        window,
        mouse(window, ButtonState::Pressed, MouseButton::Left),
        point,
    )
    .unwrap();
    app.world_mut()
        .get_mut::<InterfaceControl>(entity)
        .unwrap()
        .binding += 1;
    app.update();
    assert!(send(
        &mut app,
        &handle,
        window,
        mouse(window, ButtonState::Released, MouseButton::Left),
        point
    )
    .is_err());
    assert!(handle.take_actions().unwrap().is_empty());
}

#[test]
fn keyboard_uses_bevy_activation_while_context_menu_keeps_its_native_route() {
    let (mut app, handle, entity, window) = fixture();
    let point = Vec2::new(140., 140.);
    send(
        &mut app,
        &handle,
        window,
        mouse(window, ButtonState::Pressed, MouseButton::Right),
        point,
    )
    .unwrap();
    let actions = send(
        &mut app,
        &handle,
        window,
        mouse(window, ButtonState::Released, MouseButton::Right),
        point,
    )
    .unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].control.input, ControlInput::ContextMenu);
    let actions = send(
        &mut app,
        &handle,
        window,
        key(window, KeyCode::Enter, Key::Enter),
        point,
    )
    .unwrap();
    assert_eq!(actions.len(), 1);
    assert!(app.world().get::<Button>(entity).is_some());
    assert_eq!(actions[0].control.key, ControlKey(entity.to_bits()));
}

#[test]
fn disabling_a_pressed_widget_cancels_its_bevy_state() {
    let (mut app, handle, entity, window) = fixture();
    let point = Vec2::new(140., 140.);
    send(
        &mut app,
        &handle,
        window,
        mouse(window, ButtonState::Pressed, MouseButton::Left),
        point,
    )
    .unwrap();
    let snapshot = {
        let mut control = app.world_mut().get_mut::<InterfaceControl>(entity).unwrap();
        control.disabled = true;
        control.clone()
    };
    synchronize_control(app.world_mut(), entity, &snapshot);
    app.update();
    assert!(app.world().get::<InteractionDisabled>(entity).is_some());
    assert!(app.world().get::<Pressed>(entity).is_none());
    assert!(send(
        &mut app,
        &handle,
        window,
        mouse(window, ButtonState::Released, MouseButton::Left),
        point
    )
    .unwrap()
    .is_empty());
    assert!(handle.take_actions().unwrap().is_empty());
}
