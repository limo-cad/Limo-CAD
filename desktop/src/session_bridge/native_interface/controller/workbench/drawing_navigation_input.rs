//! Ordered native paper gestures. This path only reads the published owner and
//! cached paper, and therefore remains available during an OCC worker operation.
use super::*;
use crate::native_viewport::winit_host::NativeHostInput;
use bevy::{
    input::{mouse::MouseScrollUnit, ButtonState},
    window::WindowEvent,
};
use drawing_navigation::{Wheel, WheelUnit};

pub(super) fn navigate(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    input: &NativeHostInput,
) -> Result<bool, String> {
    let Some(mut state) = world.remove_resource::<Workbench>() else {
        return Ok(false);
    };
    let result = inner(world, handle, input, &mut state);
    world.insert_resource(state);
    result
}
fn inner(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    input: &NativeHostInput,
    state: &mut Workbench,
) -> Result<bool, String> {
    let Some(view) = &mut state.paper_view else {
        return Ok(false);
    };
    if state.workspace != Workspace::Drawing || state.sketch {
        view.navigation.cancel();
        return Ok(false);
    }
    let cancel = matches!(&input.event,WindowEvent::WindowFocused(e) if !e.focused)
        || matches!(
            &input.event,
            WindowEvent::KeyboardFocusLost(_)
                | WindowEvent::CursorLeft(_)
                | WindowEvent::WindowCloseRequested(_)
                | WindowEvent::WindowDestroyed(_)
                | WindowEvent::WindowResized(_)
                | WindowEvent::WindowScaleFactorChanged(_)
                | WindowEvent::WindowBackendScaleFactorChanged(_)
        );
    let escape = matches!(&input.event,WindowEvent::KeyboardInput(key) if key.state==ButtonState::Pressed && key.logical_key==bevy::input::keyboard::Key::Escape);
    if cancel || escape {
        return Ok(view.navigation.cancel() && escape);
    }
    if matches!(&input.event,WindowEvent::MouseButtonInput(button) if button.button==MouseButton::Middle && button.state==ButtonState::Released)
    {
        return Ok(view.navigation.cancel());
    }
    let Some((_, sheet, _)) = &state.paper_key else {
        return Ok(false);
    };
    let valid = handle.read_surface(|owner, frame| {
        input.context.as_ref() == Some(owner)
            && frame.modal_stack.is_empty()
            && view.navigation.matches(owner, sheet.id)
    })?;
    if !valid || input.consumed || handle.has_capture() {
        view.navigation.cancel();
        return Ok(false);
    }
    let Some(cursor) = input
        .cursor
        .filter(|p| p.is_finite())
        .map(|p| p.as_dvec2().to_array())
    else {
        view.navigation.cancel();
        return Ok(false);
    };
    let owner = input.context.as_ref().unwrap();
    let control = handle.hit_key(cursor).is_some_and(|key| {
        !matches!(
            world
                .get::<NativeCommandBinding>(Entity::from_bits(key.0))
                .map(|b| &b.command),
            Some(NativeCommand::Drawing(drawing_editor::Command::Annotation(
                _,
                drawing_authoring::Command::Select(_)
                    | drawing_authoring::Command::Anchor(_)
                    | drawing_authoring::Command::Chamfer(_)
                    | drawing_authoring::Command::CloudEdge(_, _)
            )))
        )
    });
    let handled = match &input.event {
        WindowEvent::MouseButtonInput(button)
            if !control
                && button.button == MouseButton::Middle
                && button.state == ButtonState::Pressed =>
        {
            view.navigation.begin_pan(owner, sheet.id, cursor)
        }
        WindowEvent::CursorMoved(_) if view.navigation.is_panning() => {
            view.navigation.pan_to(owner, sheet.id, cursor)
        }
        WindowEvent::MouseWheel(wheel) if !control => view.navigation.wheel(
            owner,
            sheet.id,
            cursor,
            Wheel {
                delta: [wheel.x as f64, wheel.y as f64],
                unit: if wheel.unit == MouseScrollUnit::Line {
                    WheelUnit::Line
                } else {
                    WheelUnit::Pixel
                },
                window_scale: world
                    .get::<Window>(wheel.window)
                    .map_or(1., |w| w.scale_factor() as f64)
                    * f64::from(handle.presented_ui_scale()),
                ctrl: input.modifiers.ctrl,
                alt: input.modifiers.alt,
                macos: cfg!(target_os = "macos"),
                now_ms: world
                    .get_resource::<Time<Real>>()
                    .map_or(0., |t| t.elapsed_secs_f64() * 1000.),
            },
        ),
        WindowEvent::PinchGesture(pinch) if !control => {
            view.navigation
                .pinch(owner, sheet.id, cursor, pinch.0 as f64)
        }
        _ => false,
    };
    if handled {
        drawing_paper::repaint(world, state)?;
        handle.invalidate_presentation();
    }
    Ok(handled)
}

pub(super) fn execute(world: &mut World, command: &Command) -> Result<Value, String> {
    drawing_authoring::cancel_input(world);
    let mut state = world
        .remove_resource::<Workbench>()
        .ok_or("Open the drawing workspace")?;
    let result = (|| {
        if state.workspace != Workspace::Drawing {
            return Err("Open the drawing workspace".into());
        }
        let view = state
            .paper_view
            .as_mut()
            .ok_or("Create a drawing sheet first")?;
        match command {
            Command::DrawingFit => view.navigation.fit(),
            Command::DrawingZoom(step) => {
                view.navigation
                    .zoom_at(view.navigation.zoom + f64::from(*step) * 0.1, None);
            }
            _ => return Err("Choose a drawing navigation control".into()),
        }
        drawing_paper::repaint(world, &mut state)?;
        world
            .resource::<NativeInterfaceHandle>()
            .invalidate_presentation();
        Ok(json!({"handled":true}))
    })();
    world.insert_resource(state);
    result
}
