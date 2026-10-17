use super::*;
use crate::native_viewport::winit_host::{
    prepare_native_input, HostInputState, Modifiers, NativeHostInput,
};
use bevy::{
    input::{
        keyboard::{Key, KeyboardInput},
        mouse::MouseButtonInput,
        InputPlugin,
    },
    input_focus::pointer_focus::PointerFocusPlugin,
    picking::{
        backend::{HitData, PointerHits},
        pointer::PointerLocation,
        DefaultPickingPlugins,
    },
    time::TimePlugin,
    window::{CursorMoved, PrimaryWindow},
};

// Exercise the real Bevy input, picking and focus schedules. A tiny backend
// supplies one rectangle; no test synthesizes PointerClick or FocusedInput.
fn hits(
    pointers: Query<(&PointerId, &PointerLocation)>,
    controls: Query<(Entity, &InterfaceControl)>,
    mut hits: MessageWriter<PointerHits>,
) {
    for (id, location) in &pointers {
        let Some(location) = location.location() else {
            continue;
        };
        let picks = controls
            .iter()
            .filter(|(_, control)| control.visible)
            .filter(|_| {
                (location.position - Vec2::new(140., 140.))
                    .abs()
                    .cmple(Vec2::splat(20.))
                    .all()
            })
            .map(|(entity, _)| (entity, HitData::new(entity, 0., None, None)))
            .collect();
        hits.write(PointerHits::new(*id, picks, 10.));
    }
}

fn fixture() -> (App, NativeInterfaceHandle, Entity, Entity) {
    let (mut app, handle, entity, _) = super::super::tests::fixture();
    app.add_plugins((
        TimePlugin,
        InputPlugin,
        DefaultPickingPlugins,
        PointerFocusPlugin,
    ))
    .add_message::<WindowEvent>()
    .init_resource::<HostInputState>()
    .add_systems(PreUpdate, hits.in_set(PickingSystems::Backend));
    install(&mut app);
    let window = app
        .world_mut()
        .spawn((Window::default(), PrimaryWindow))
        .observe(bevy::input_focus::tab_navigation::handle_tab_navigation)
        .id();
    app.world_mut().spawn(PointerId::Mouse);
    app.update();
    (app, handle, entity, window)
}

fn send(
    app: &mut App,
    handle: &NativeInterfaceHandle,
    event: WindowEvent,
    point: Vec2,
) -> Vec<NativeInterfaceAction> {
    if let WindowEvent::KeyboardInput(key) = &event {
        app.world_mut().write_message(key.clone());
    }
    app.world_mut().write_message(event.clone());
    app.update();
    let mut input = NativeHostInput {
        ui_scale: 1.,
        context: handle.presented_context(),
        cursor: Some(point),
        modifiers: Modifiers::default(),
        event,
        consumed: false,
        actions: vec![],
    };
    prepare_native_input(app.world_mut(), handle, &mut input).unwrap();
    assert!(
        input.actions.is_empty(),
        "the old native adapter must not activate widgets"
    );
    enqueue_actions(app.world_mut(), handle);
    handle.take_actions().unwrap()
}

fn point(_entity: Entity) -> Vec2 {
    Vec2::new(140., 140.)
}
fn move_to(app: &mut App, handle: &NativeInterfaceHandle, window: Entity, point: Vec2) {
    assert!(send(
        app,
        handle,
        WindowEvent::CursorMoved(CursorMoved {
            window,
            position: point,
            delta: None
        }),
        point
    )
    .is_empty());
}
fn mouse(window: Entity, state: ButtonState, button: MouseButton) -> WindowEvent {
    WindowEvent::MouseButtonInput(MouseButtonInput {
        window,
        state,
        button,
    })
}
fn key(window: Entity, code: KeyCode, repeat: bool) -> WindowEvent {
    WindowEvent::KeyboardInput(KeyboardInput {
        window,
        key_code: code,
        logical_key: if code == KeyCode::Tab {
            Key::Tab
        } else {
            Key::Space
        },
        state: ButtonState::Pressed,
        text: None,
        repeat,
    })
}

#[test]
fn real_picking_activates_once_and_owns_pressed_focus_and_hover() {
    let (mut app, handle, entity, window) = fixture();
    let p = point(entity);
    move_to(&mut app, &handle, window, p);
    assert!(send(
        &mut app,
        &handle,
        mouse(window, ButtonState::Pressed, MouseButton::Left),
        p
    )
    .is_empty());
    assert!(app.world().get::<Pressed>(entity).is_some());
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(entity));
    assert!(app.world().get::<Hovered>(entity).unwrap().0);
    let actions = send(
        &mut app,
        &handle,
        mouse(window, ButtonState::Released, MouseButton::Left),
        p,
    );
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].control.key, ControlKey(entity.to_bits()));
    assert!(app.world().get::<Pressed>(entity).is_none());
    assert!(send(
        &mut app,
        &handle,
        mouse(window, ButtonState::Released, MouseButton::Left),
        p
    )
    .is_empty());
}

#[test]
fn standard_checkbox_space_and_tab_preserve_controlled_state_and_ignore_repeat() {
    let (mut app, handle, entity, window) = fixture();
    {
        let mut control = app.world_mut().get_mut::<InterfaceControl>(entity).unwrap();
        control.field = Field::Toggle(false);
        control.role = "checkbox".into();
    }
    app.update();
    let p = point(entity);
    assert!(send(&mut app, &handle, key(window, KeyCode::Tab, false), p).is_empty());
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(entity));
    assert_eq!(
        send(&mut app, &handle, key(window, KeyCode::Space, false), p).len(),
        1
    );
    assert!(
        app.world().get::<Checked>(entity).is_none(),
        "only accepted CAD state checks the widget"
    );
    app.world_mut()
        .get_mut::<InterfaceControl>(entity)
        .unwrap()
        .field = Field::Toggle(true);
    app.update();
    assert!(app.world().get::<Checked>(entity).is_some());
    assert!(send(&mut app, &handle, key(window, KeyCode::Space, true), p).is_empty());
    assert_eq!(
        send(&mut app, &handle, key(window, KeyCode::Space, false), p).len(),
        1
    );
}

#[test]
fn release_outside_cancels_and_secondary_click_never_activates() {
    let (mut app, handle, entity, window) = fixture();
    let p = point(entity);
    move_to(&mut app, &handle, window, p);
    send(
        &mut app,
        &handle,
        mouse(window, ButtonState::Pressed, MouseButton::Left),
        p,
    );
    let outside = Vec2::new(950., 750.);
    move_to(&mut app, &handle, window, outside);
    assert!(send(
        &mut app,
        &handle,
        mouse(window, ButtonState::Released, MouseButton::Left),
        outside
    )
    .is_empty());
    assert!(app.world().get::<Pressed>(entity).is_none());
    move_to(&mut app, &handle, window, p);
    // Secondary clicks remain the native context-menu adapter's responsibility.
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut()
            .write_message(mouse(window, state, MouseButton::Right));
        app.update();
    }
    assert!(app.world().resource::<WidgetActions>().0.is_empty());
}

#[test]
fn disabled_modal_covered_and_rebound_widgets_do_not_activate() {
    let (mut app, handle, entity, window) = fixture();
    let p = point(entity);
    move_to(&mut app, &handle, window, p);
    send(
        &mut app,
        &handle,
        mouse(window, ButtonState::Pressed, MouseButton::Left),
        p,
    );
    app.world_mut()
        .get_mut::<InterfaceControl>(entity)
        .unwrap()
        .binding += 1;
    app.update();
    assert!(send(
        &mut app,
        &handle,
        mouse(window, ButtonState::Released, MouseButton::Left),
        p
    )
    .is_empty());
    app.world_mut()
        .get_mut::<InterfaceControl>(entity)
        .unwrap()
        .disabled = true;
    app.update();
    assert!(send(&mut app, &handle, key(window, KeyCode::Space, false), p).is_empty());
    app.world_mut()
        .get_mut::<InterfaceControl>(entity)
        .unwrap()
        .disabled = false;
    let mut frame = handle.frame().unwrap();
    frame.modal_stack.push("another-dialog".into());
    handle.present(frame).unwrap();
    app.update();
    assert!(app.world().get::<InteractionDisabled>(entity).is_some());
    assert!(app.world().get::<TabIndex>(entity).is_none());
}

#[test]
fn queued_activation_cannot_cross_document_replacement() {
    let (mut app, handle, entity, window) = fixture();
    let p = point(entity);
    send(&mut app, &handle, key(window, KeyCode::Tab, false), p);
    app.world_mut()
        .write_message(match key(window, KeyCode::Space, false) {
            WindowEvent::KeyboardInput(input) => input,
            _ => unreachable!(),
        });
    app.update();
    assert_eq!(app.world().resource::<WidgetActions>().0.len(), 1);
    let mut frame = handle.frame().unwrap();
    frame.context.document_id = "replacement".into();
    handle.present(frame).unwrap();
    app.update();
    enqueue_actions(app.world_mut(), &handle);
    assert!(handle.take_actions().unwrap().is_empty());
}

// Use actual Bevy camera propagation and layout: the input-only fixture above
// deliberately supplies geometry and cannot detect a detached UI root.
#[test]
fn focus_group_preserves_real_layout_and_camera_across_frames() {
    use bevy::{
        app::{HierarchyPropagatePlugin, PropagateSet},
        camera::{ComputedCameraValues, RenderTargetInfo},
        ui::{
            ui_layout_system, ui_surface::UiSurface, update::propagate_ui_target_cameras,
            ComputedUiRenderTargetInfo, ComputedUiTargetCamera, UiScale,
        },
    };
    let mut app = App::new();
    let handle = NativeInterfaceHandle::new(|| {});
    app.insert_resource(handle.clone())
        .init_resource::<UiScale>()
        .init_resource::<UiSurface>()
        .init_resource::<bevy::text::FontCx>()
        .init_resource::<bevy::text::RemSize>()
        .add_plugins((
            HierarchyPropagatePlugin::<ComputedUiTargetCamera>::new(PostUpdate),
            HierarchyPropagatePlugin::<ComputedUiRenderTargetInfo>::new(PostUpdate),
        ))
        .add_systems(
            PostUpdate,
            propagate_ui_target_cameras.before(ui_layout_system),
        )
        .configure_sets(
            PostUpdate,
            (
                PropagateSet::<ComputedUiTargetCamera>::default(),
                PropagateSet::<ComputedUiRenderTargetInfo>::default(),
            )
                .after(propagate_ui_target_cameras)
                .before(ui_layout_system),
        )
        .add_systems(
            PostUpdate,
            (ui_layout_system, publish_layout.in_set(InterfaceLayout)).chain(),
        );
    // The original regression only occurred with a real primary window.
    app.world_mut().spawn((Window::default(), PrimaryWindow));
    let camera = app
        .world_mut()
        .spawn((
            InterfaceCamera,
            Camera2d,
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: UVec2::new(1000, 800),
                        scale_factor: 1.,
                    }),
                    ..default()
                },
                ..default()
            },
        ))
        .id();
    let entity = app
        .world_mut()
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(120.),
                top: px(120.),
                width: px(80.),
                height: px(24.),
                ..default()
            },
            UiTargetCamera(camera),
            InterfaceControl::button("Viewport", "Select"),
            InheritedVisibility::VISIBLE,
        ))
        .id();
    handle
        .present(super::super::tests::frame("document-a", 1))
        .unwrap();
    app.update();
    let initial = handle
        .read_surface(|_, frame| frame.controls.clone())
        .unwrap();
    assert_eq!(initial.len(), 1);
    assert!(initial[0].visible);
    install(&mut app);
    // Grouping happens after layout; later frames must still see the same roots.
    for _ in 0..4 {
        app.update();
        use bevy::ecs::system::RunSystemOnce;
        let roots = app
            .world_mut()
            .run_system_once(|roots: bevy::ui::experimental::UiRootNodes| {
                roots.iter().collect::<Vec<_>>()
            })
            .unwrap();
        assert!(
            roots.contains(&entity),
            "control must remain a Bevy layout/render root"
        );
        assert_eq!(
            app.world().get::<ComputedNode>(entity).unwrap().size,
            Vec2::new(80., 24.)
        );
        assert_eq!(
            app.world()
                .get::<ComputedUiTargetCamera>(entity)
                .unwrap()
                .get(),
            Some(camera)
        );
        let current = handle
            .read_surface(|_, frame| frame.controls.clone())
            .unwrap();
        assert_eq!(current, initial);
    }
}
