//! Pointer navigation only reads the rendered camera and the current UI owner.
//! In particular, it must not acquire the document/publisher locks held by OCC.
use super::*;
use crate::native_viewport::{
    interface_shell::NativeInterfaceHandle,
    winit_host::{cancel_native_pointer, NativeHostInput},
};
use crate::session_bridge::native_interface::controller::workbench::{self, NavigationTool};
use bevy::{
    input::{mouse::MouseScrollUnit, ButtonState},
    prelude::{MouseButton, Quat, Resource, Vec2},
    window::{Window, WindowEvent},
};
use limo_cad_interface::Rect;

#[derive(Clone, Copy)]
enum Mode {
    Pan,
    Orbit,
    Zoom,
    ZoomWindow,
}

struct Drag {
    owner: DocumentContext,
    button: MouseButton,
    mode: Mode,
    cursor: Vec2,
    start: Vec2,
}

#[derive(Resource, Default)]
struct Navigation {
    drag: Option<Drag>,
}

pub(in super::super) fn cancel_pointer(world: &mut World) {
    if let Some(mut state) = world.get_resource_mut::<Navigation>() {
        state.drag = None;
    }
    world.insert_resource(workbench::NavigationRectangle(None));
}

pub(in super::super) fn pointer_active(world: &World) -> bool {
    world
        .get_resource::<Navigation>()
        .is_some_and(|state| state.drag.is_some())
}

fn inside(bounds: Rect, cursor: Vec2) -> bool {
    cursor.is_finite()
        && f64::from(cursor.x) >= bounds.x
        && f64::from(cursor.x) < bounds.x + bounds.width
        && f64::from(cursor.y) >= bounds.y
        && f64::from(cursor.y) < bounds.y + bounds.height
}

/// Returns true when navigation owns this event; selection and feature picks
/// continue through the editor only for unhandled input. A drag keeps the mode
/// chosen on press until release, including when Shift changes mid-gesture.
pub(in super::super) fn navigate(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    input: &NativeHostInput,
) -> Result<bool, String> {
    let mut state = world.remove_resource::<Navigation>().unwrap_or_default();
    let result = navigate_inner(world, handle, input, &mut state);
    world.insert_resource(state);
    result
}

fn navigate_inner(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    input: &NativeHostInput,
    state: &mut Navigation,
) -> Result<bool, String> {
    let escape = matches!(&input.event, WindowEvent::KeyboardInput(key)
        if key.state == ButtonState::Pressed && key.key_code == bevy::input::keyboard::KeyCode::Escape);
    let lost = matches!(&input.event,
        WindowEvent::WindowFocused(event) if !event.focused)
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
    if escape || lost {
        world.insert_resource(workbench::NavigationRectangle(None));
        let dragging = state.drag.take().is_some();
        let navigation_cancelled = escape
            && !input.consumed
            && handle.read_surface(|owner, frame| {
                input.context.as_ref() == Some(owner) && frame.modal_stack.is_empty()
            })?
            && workbench::navigation(world) != NavigationTool::Select;
        if navigation_cancelled {
            workbench::execute(
                world,
                &workbench::Command::Navigation(NavigationTool::Select),
            )?;
        }
        if dragging {
            cancel_native_pointer(world, handle);
        }
        return Ok((escape && dragging) || navigation_cancelled);
    }
    if !matches!(
        input.event,
        WindowEvent::MouseButtonInput(_)
            | WindowEvent::CursorMoved(_)
            | WindowEvent::MouseWheel(_)
            | WindowEvent::PinchGesture(_)
    ) {
        return Ok(false);
    }
    if let WindowEvent::MouseButtonInput(button) = &input.event {
        if button.state == ButtonState::Released {
            let captured = state
                .drag
                .as_ref()
                .is_some_and(|d| d.button == button.button);
            if captured {
                let drag = state.drag.take().unwrap();
                world.insert_resource(workbench::NavigationRectangle(None));
                if matches!(drag.mode, Mode::ZoomWindow)
                    && input.context.as_ref() == Some(&drag.owner)
                {
                    let bounds = handle.read_surface(|owner, frame| {
                        (owner == &drag.owner && frame.modal_stack.is_empty())
                            .then(|| {
                                frame
                                    .canvases
                                    .iter()
                                    .find(|c| c.name == "viewport")
                                    .map(|c| c.bounds)
                            })
                            .flatten()
                    })?;
                    if let Some(bounds) = bounds {
                        let size = (drag.cursor - drag.start).abs();
                        if size.min_element() >= 8. {
                            let center = (drag.start + drag.cursor) * 0.5;
                            let canvas_center = Vec2::new(
                                (bounds.x + bounds.width * 0.5) as f32,
                                (bounds.y + bounds.height * 0.5) as f32,
                            );
                            move_camera(
                                world,
                                input,
                                bounds,
                                Mode::Pan,
                                canvas_center - center,
                                None,
                            )?;
                            let scale = (size.x / bounds.width as f32)
                                .max(size.y / bounds.height as f32)
                                .clamp(0.01, 1.);
                            move_camera(
                                world,
                                input,
                                bounds,
                                Mode::Pan,
                                Vec2::ZERO,
                                Some(scale.ln()),
                            )?;
                        }
                    }
                }
            }
            return Ok(captured);
        }
    }
    let bounds = handle.read_surface(|owner, frame| {
        (input.context.as_ref() == Some(owner) && frame.modal_stack.is_empty())
            .then(|| {
                frame
                    .canvases
                    .iter()
                    .find(|c| c.name == "viewport")
                    .map(|c| c.bounds)
            })
            .flatten()
    })?;
    let Some(bounds) = bounds.filter(|_| !input.consumed && !handle.has_capture()) else {
        state.drag = None;
        world.insert_resource(workbench::NavigationRectangle(None));
        return Ok(false);
    };
    if state
        .drag
        .as_ref()
        .is_some_and(|drag| Some(&drag.owner) != input.context.as_ref())
    {
        state.drag = None;
        world.insert_resource(workbench::NavigationRectangle(None));
    }
    let Some(cursor) = input.cursor.filter(|p| p.is_finite()) else {
        state.drag = None;
        world.insert_resource(workbench::NavigationRectangle(None));
        return Ok(false);
    };
    let point = cursor.as_dvec2().to_array();
    let blocked = if matches!(
        input.event,
        WindowEvent::MouseWheel(_) | WindowEvent::PinchGesture(_)
    ) {
        handle.blocks_camera_gesture(point)
    } else {
        handle.owns_pointer(point)
    };
    let on_canvas = inside(bounds, cursor) && !blocked;
    let on_dial = handle
        .hit_key(cursor.as_dvec2().to_array())
        .is_some_and(|key| Some(key) == workbench::dial_key(world))
        && workbench::dial(world).is_some_and(|dial| {
            let center = Vec2::new(
                (dial.x + dial.width / 2.) as f32,
                (dial.y + dial.height / 2.) as f32,
            );
            cursor.distance(center) < dial.width as f32 / 2.
        });
    match &input.event {
        WindowEvent::MouseButtonInput(button) if (on_canvas || on_dial) && state.drag.is_none() => {
            let mode = match button.button {
                MouseButton::Left if on_dial => Mode::Orbit,
                MouseButton::Left if on_canvas => match workbench::navigation(world) {
                    NavigationTool::Select => return Ok(false),
                    NavigationTool::Orbit => Mode::Orbit,
                    NavigationTool::Pan => Mode::Pan,
                    NavigationTool::Zoom => Mode::Zoom,
                    NavigationTool::ZoomWindow => Mode::ZoomWindow,
                },
                MouseButton::Middle if !input.modifiers.shift => Mode::Pan,
                MouseButton::Middle | MouseButton::Right => Mode::Orbit,
                _ => return Ok(false),
            };
            state.drag = Some(Drag {
                owner: input.context.clone().expect("validated canvas owner"),
                button: button.button,
                mode,
                cursor,
                start: cursor,
            });
            super::cancel(world, "Camera transition interrupted by pointer navigation");
            Ok(true)
        }
        WindowEvent::CursorMoved(_) => {
            let Some(drag) = &mut state.drag else {
                return Ok(false);
            };
            let delta = cursor - drag.cursor;
            drag.cursor = if matches!(drag.mode, Mode::ZoomWindow) {
                cursor.clamp(
                    Vec2::new(bounds.x as f32, bounds.y as f32),
                    Vec2::new(
                        (bounds.x + bounds.width) as f32,
                        (bounds.y + bounds.height) as f32,
                    ),
                )
            } else {
                cursor
            };
            if matches!(drag.mode, Mode::ZoomWindow) {
                let min = drag.start.min(drag.cursor);
                let size = (drag.cursor - drag.start).abs();
                world.insert_resource(workbench::NavigationRectangle(Some(Rect {
                    x: min.x as f64,
                    y: min.y as f64,
                    width: size.x as f64,
                    height: size.y as f64,
                })));
            } else {
                move_camera(
                    world,
                    input,
                    bounds,
                    drag.mode,
                    delta,
                    matches!(drag.mode, Mode::Zoom).then_some(delta.y * 0.01),
                )?;
            }
            Ok(true)
        }
        WindowEvent::MouseWheel(wheel) if on_canvas && state.drag.is_none() => {
            let factor = if wheel.unit == MouseScrollUnit::Line {
                16.
            } else {
                world
                    .get::<Window>(wheel.window)
                    .map_or(1., |w| 1. / w.scale_factor())
                    / handle.presented_ui_scale()
            };
            let delta = Vec2::new(wheel.x, wheel.y) * factor;
            if !delta.is_finite() {
                return Ok(false);
            }
            let delta = delta.clamp(Vec2::splat(-240.), Vec2::splat(240.));
            if input.modifiers.shift {
                move_camera(world, input, bounds, Mode::Orbit, delta * 0.6, None)?;
            } else if input.modifiers.ctrl || wheel.unit == MouseScrollUnit::Line {
                move_camera(
                    world,
                    input,
                    bounds,
                    Mode::Pan,
                    Vec2::ZERO,
                    Some(-delta.y * 0.002),
                )?;
            } else {
                move_camera(world, input, bounds, Mode::Pan, delta, None)?;
            }
            Ok(true)
        }
        WindowEvent::PinchGesture(pinch)
            if on_canvas && state.drag.is_none() && pinch.0.is_finite() =>
        {
            move_camera(
                world,
                input,
                bounds,
                Mode::Pan,
                Vec2::ZERO,
                Some(-pinch.0.clamp(-1., 1.)),
            )?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn move_camera(
    world: &mut World,
    input: &NativeHostInput,
    bounds: Rect,
    mode: Mode,
    delta: Vec2,
    zoom: Option<f32>,
) -> Result<(), String> {
    if delta == Vec2::ZERO && zoom.is_none_or(|amount| amount == 0.) {
        return Ok(());
    }
    let (session, mut camera) = native_viewport::interface_camera_snapshot(world);
    if input
        .context
        .as_ref()
        .is_none_or(|owner| owner.document_id != session)
    {
        return Err("The rendered document is not current".into());
    }
    let (target, distance, _) = super::motion::pose(camera)?;
    let offset = Vec3::from_array(camera.position) - target;
    let up = Vec3::from_array(camera.up).normalize();
    let height = (bounds.height as f32).max(1.);
    let limit = Vec2::new((bounds.width as f32).max(1.), height);
    let delta = delta.clamp(-limit, limit);
    if let Some(amount) = zoom {
        let radius = (distance * amount.exp()).clamp(1e-4, 1e9);
        camera.position = (target + offset / distance * radius).to_array();
    } else {
        match mode {
            Mode::Pan => {
                let right = (-offset).cross(up).normalize();
                let screen_up = right.cross(-offset).normalize();
                let scale =
                    2. * distance * (camera.vertical_fov_degrees.to_radians() * 0.5).tan() / height;
                let shift = (-right * delta.x + screen_up * delta.y) * scale;
                camera.position = (Vec3::from_array(camera.position) + shift).to_array();
                camera.target = (target + shift).to_array();
            }
            Mode::Zoom | Mode::ZoomWindow => {}
            Mode::Orbit => {
                let to_y = Quat::from_rotation_arc(up, Vec3::Y);
                let local = to_y * (offset / distance);
                let theta = local.x.atan2(local.z) - std::f32::consts::TAU * delta.x / height;
                let phi = (local.y.clamp(-1., 1.).acos()
                    - std::f32::consts::TAU * delta.y / height)
                    .clamp(1e-3, std::f32::consts::PI - 1e-3);
                let direction =
                    Vec3::new(phi.sin() * theta.sin(), phi.cos(), phi.sin() * theta.cos());
                camera.position = (target + to_y.inverse() * direction * distance).to_array();
            }
        }
    }
    native_viewport::apply_interface_view(world, &session, Some(camera), None)?;
    super::cancel(world, "Camera transition interrupted by pointer navigation");
    Ok(())
}

#[cfg(test)]
mod tests;
