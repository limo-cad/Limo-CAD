use super::*;
use bevy::input::{keyboard::Key, ButtonState};

pub(super) struct Drag {
    owner: DocumentContext,
    start: Vec2,
    yaw: f32,
    pitch: f32,
    moved: bool,
}

pub(crate) fn input(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    event: &NativeHostInput,
) -> Result<bool, String> {
    if !world
        .get_resource::<Files>()
        .is_some_and(|files| files.scripts && files.script.preview.open)
    {
        return Ok(false);
    }
    if let WindowEvent::KeyboardInput(key) = &event.event {
        let preview_focused = handle.focused_key().is_some_and(|key| {
            world
                .get::<NativeCommandBinding>(Entity::from_bits(key.0))
                .is_some_and(|binding| {
                    binding.command
                        == NativeCommand::File(FileCommand::ScriptPreview(Action::Model))
                })
        });
        if preview_focused
            && key.state == ButtonState::Pressed
            && !event.modifiers.ctrl
            && !event.modifiers.meta
            && !event.modifiers.alt
            && !event.modifiers.shift
            && handle.frame().is_some_and(|frame| {
                frame.modal_stack.is_empty() && event.context.as_ref() == Some(&frame.context)
            })
        {
            let action = match key.logical_key {
                Key::ArrowLeft => Some(Action::Left),
                Key::ArrowRight => Some(Action::Right),
                Key::ArrowUp => Some(Action::Up),
                Key::ArrowDown => Some(Action::Down),
                Key::Home => Some(Action::Fit),
                _ => None,
            };
            if let Some(action) = action {
                command(world, handle, action, &ControlInput::Click)?;
                return Ok(true);
            }
        }
    }
    let active = world.resource_mut::<Files>().script.preview.drag.take();
    if let Some(mut drag) = active {
        let cancel = matches!(
            &event.event,
            WindowEvent::WindowFocused(bevy::window::WindowFocused { focused: false, .. })
                | WindowEvent::KeyboardFocusLost(_)
        ) || matches!(&event.event, WindowEvent::KeyboardInput(key) if key.state == ButtonState::Pressed && key.logical_key == Key::Escape);
        if cancel
            || event.context.as_ref() != Some(&drag.owner)
            || handle
                .frame()
                .is_none_or(|frame| frame.context != drag.owner || !frame.modal_stack.is_empty())
        {
            crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
            return Ok(false);
        }
        let release = matches!(&event.event, WindowEvent::MouseButtonInput(input)
            if input.button == MouseButton::Left && input.state == ButtonState::Released);
        if !release
            && !matches!(
                event.event,
                WindowEvent::CursorMoved(_) | WindowEvent::CursorLeft(_)
            )
        {
            world.resource_mut::<Files>().script.preview.drag = Some(drag);
            return Ok(false);
        }
        if let Some(cursor) = event.cursor.filter(|cursor| cursor.is_finite()) {
            let delta = cursor - drag.start;
            if !drag.moved && delta.length() >= 3. {
                drag.moved = true;
                crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
            }
            if drag.moved {
                world
                    .resource_mut::<Files>()
                    .script
                    .preview
                    .turn(drag.yaw + delta.x * 0.012, drag.pitch + delta.y * 0.012)?;
                handle.request_redraw();
            }
        }
        let moved = drag.moved;
        if !release {
            world.resource_mut::<Files>().script.preview.drag = Some(drag);
        }
        return Ok(moved);
    }
    if !matches!(&event.event, WindowEvent::MouseButtonInput(input)
        if input.button == MouseButton::Left && input.state == ButtonState::Pressed)
        || handle
            .frame()
            .is_none_or(|frame| !frame.modal_stack.is_empty())
    {
        return Ok(false);
    }
    let Some(cursor) = event.cursor.filter(|cursor| cursor.is_finite()) else {
        return Ok(false);
    };
    let Some(key) = handle.hit_key(cursor.as_dvec2().to_array()) else {
        return Ok(false);
    };
    if !world
        .get::<NativeCommandBinding>(Entity::from_bits(key.0))
        .is_some_and(|binding| {
            binding.command == NativeCommand::File(FileCommand::ScriptPreview(Action::Model))
        })
    {
        return Ok(false);
    }
    let action = handle.resolve_retained(key)?;
    if event.context.as_ref() != Some(&action.context) {
        return Ok(false);
    }
    let preview = &mut world.resource_mut::<Files>().script.preview;
    preview.playing = false;
    preview.wake = None;
    preview.drag = Some(Drag {
        owner: action.context,
        start: cursor,
        yaw: preview.yaw,
        pitch: preview.pitch,
        moved: false,
    });
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_script_preview_pointer_release_focus_loss_and_owner_change_end_the_drag() {
        let (mut app, handle, entity, _) = interface_shell::tests::fixture();
        initialize(
            app.world_mut(),
            Arc::new(Mutex::new(DocumentWorkspace::default())),
        );
        {
            let files = &mut app.world_mut().resource_mut::<Files>();
            files.scripts = true;
            files.script.preview.open = true;
        }
        bind_command(
            app.world_mut(),
            entity,
            NativeCommand::File(FileCommand::ScriptPreview(Action::Model)),
        )
        .unwrap();
        app.update();
        let frame = handle.frame().unwrap();
        let bounds = handle
            .read_surface(|_, surface| {
                surface
                    .controls
                    .iter()
                    .find(|control| control.key.0 == entity.to_bits())
                    .unwrap()
                    .bounds
            })
            .unwrap();
        let point = Vec2::new(
            (bounds.x + bounds.width / 2.) as f32,
            (bounds.y + bounds.height / 2.) as f32,
        );
        let event = |event, cursor| NativeHostInput {
            ui_scale: 1.,
            event,
            cursor: Some(cursor),
            context: Some(frame.context.clone()),
            consumed: false,
            modifiers: default(),
            actions: vec![],
        };
        let pressed = event(
            WindowEvent::MouseButtonInput(bevy::input::mouse::MouseButtonInput {
                window: Entity::PLACEHOLDER,
                button: MouseButton::Left,
                state: ButtonState::Pressed,
            }),
            point,
        );
        assert!(
            !input(app.world_mut(), &handle, &pressed).unwrap(),
            "The standard adapter must receive the original press for focus"
        );
        assert!(app
            .world()
            .resource::<Files>()
            .script
            .preview
            .drag
            .is_some());
        let moved = point + Vec2::new(20., 10.);
        assert!(input(
            app.world_mut(),
            &handle,
            &event(
                WindowEvent::CursorMoved(bevy::window::CursorMoved {
                    window: Entity::PLACEHOLDER,
                    position: moved,
                    delta: None
                }),
                moved
            )
        )
        .unwrap());
        assert!((app.world().resource::<Files>().script.preview.yaw - HOME.0 - 0.24).abs() < 1e-5);
        assert!(input(
            app.world_mut(),
            &handle,
            &event(
                WindowEvent::MouseButtonInput(bevy::input::mouse::MouseButtonInput {
                    window: Entity::PLACEHOLDER,
                    button: MouseButton::Left,
                    state: ButtonState::Released
                }),
                moved
            )
        )
        .unwrap());
        assert!(app
            .world()
            .resource::<Files>()
            .script
            .preview
            .drag
            .is_none());
        input(app.world_mut(), &handle, &pressed).unwrap();
        input(
            app.world_mut(),
            &handle,
            &event(
                WindowEvent::WindowFocused(bevy::window::WindowFocused {
                    window: Entity::PLACEHOLDER,
                    focused: false,
                }),
                point,
            ),
        )
        .unwrap();
        assert!(app
            .world()
            .resource::<Files>()
            .script
            .preview
            .drag
            .is_none());
        input(app.world_mut(), &handle, &pressed).unwrap();
        let mut stale = event(
            WindowEvent::CursorMoved(bevy::window::CursorMoved {
                window: Entity::PLACEHOLDER,
                position: moved,
                delta: None,
            }),
            moved,
        );
        stale.context.as_mut().unwrap().document_id = "replacement".into();
        input(app.world_mut(), &handle, &stale).unwrap();
        assert!(app
            .world()
            .resource::<Files>()
            .script
            .preview
            .drag
            .is_none());
    }
}
