//! Existing MCP canvas gestures are native input, not alternate sketch commands.
//! All picks pass through the same focus, ownership and editor handlers as Winit.

use super::*;
use crate::native_viewport::winit_host::{cancel_native_pointer, prepare_native_input, Modifiers};
use crate::session_bridge::native_interface::controller::{
    cancel_canvas_navigation, navigate_canvas_input, reduce_control_input, workbench,
};
use bevy::{
    input::mouse::MouseButtonInput,
    window::{CursorMoved, PrimaryWindow, WindowFocused, WindowScaleFactorChanged},
};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Gesture {
    Move,
    Click,
    DoubleClick,
    Drag,
}

#[derive(Clone, Copy, Default, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum PointerButton {
    #[default]
    Left,
    Middle,
}

impl From<PointerButton> for MouseButton {
    fn from(button: PointerButton) -> Self {
        match button {
            PointerButton::Left => Self::Left,
            PointerButton::Middle => Self::Middle,
        }
    }
}

#[derive(Deserialize)]
struct Request {
    gesture: Gesture,
    #[serde(default)]
    button: PointerButton,
    #[serde(default)]
    canvas: Option<String>,
    point: Option<[f64; 2]>,
    world: Option<[f64; 3]>,
    to: Option<[f64; 2]>,
    #[serde(default)]
    shift: bool,
    /// `false` leaves the button down so a later gesture can cancel the preview.
    #[serde(default)]
    release: Option<bool>,
    /// Include the displayed instance poses. Refitting the camera would cancel
    /// the drag, so the fixture reads them from this gesture instead.
    #[serde(default)]
    poses: bool,
    /// Inject the host event `observe()` already treats as a drag cancellation,
    /// after the pointer has moved and before release.
    #[serde(default)]
    lifecycle: Option<Lifecycle>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Lifecycle {
    Unfocus,
    Scale,
}

fn lifecycle_event(window: Entity, lifecycle: Lifecycle) -> WindowEvent {
    match lifecycle {
        Lifecycle::Unfocus => WindowEvent::WindowFocused(WindowFocused {
            window,
            focused: false,
        }),
        Lifecycle::Scale => WindowEvent::WindowScaleFactorChanged(WindowScaleFactorChanged {
            window,
            scale_factor: 2.,
        }),
    }
}

fn inside(bounds: InterfaceRect, point: [f64; 2]) -> bool {
    point.iter().all(|v| v.is_finite())
        && point[0] >= bounds.x
        && point[0] < bounds.x + bounds.width
        && point[1] >= bounds.y
        && point[1] < bounds.y + bounds.height
}

pub(crate) fn drive(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    owner: &DocumentContext,
    request: &Value,
) -> Result<Value, String> {
    if worker::busy(world) {
        return Err("Wait for the current modeling operation to finish".into());
    }
    let request: Request =
        serde_json::from_value(request.clone()).map_err(|e| format!("Canvas gesture: {e}"))?;
    let (drawing, bounds) = handle.read_surface(|context, frame| {
        if context != owner {
            return Err("Canvas gesture belongs to a retired document".to_owned());
        }
        if !frame.modal_stack.is_empty() {
            return Err("Close the current dialog before using the canvas".to_owned());
        }
        let canvas_name = request.canvas.as_deref().unwrap_or_else(|| {
            if frame.canvases.iter().any(|canvas| canvas.name == "drawing") {
                "drawing"
            } else {
                "viewport"
            }
        });
        if !matches!(canvas_name, "viewport" | "drawing") {
            return Err(format!("Unknown native canvas: {canvas_name}"));
        }
        frame
            .canvases
            .iter()
            .find(|c| c.name == canvas_name)
            .map(|c| (canvas_name == "drawing", c.bounds))
            .ok_or_else(|| format!("The {canvas_name} canvas is unavailable"))
    })??;
    services
        .bridge
        .with_native_document_owner(&services.engine, owner, || Ok(()))?;
    let point = match (request.point, request.world) {
        (Some(point), None) => point,
        (None, Some(_)) if drawing => {
            return Err("Drawing gestures require client point coordinates from inspect".into());
        }
        (None, Some(point)) => {
            let point = native_viewport::interface_world_point(world, &owner.document_id, point)?
                .ok_or("The requested world point is behind the camera")?;
            [
                bounds.x + f64::from(point[0]),
                bounds.y + f64::from(point[1]),
            ]
        }
        _ => return Err("Specify exactly one of point or world for a canvas gesture".into()),
    };
    let end = if request.gesture == Gesture::Drag {
        request.to.ok_or("Drag requires an end point")?
    } else {
        point
    };
    for point in [point, end] {
        if !inside(bounds, point) {
            return Err("Point is outside the canvas".into());
        }
        if handle.owns_pointer(point)
            && !(drawing
                && (handle.canvas_owns_pointer("drawing", point)
                    || workbench::drawing_canvas_control(world, handle, point)))
        {
            return Err("A native control covers that canvas point".into());
        }
    }
    if !drawing {
        initialize(world);
    }
    if request.gesture == Gesture::DoubleClick
        && (drawing
            || request.button != PointerButton::Left
            || world.resource::<Editor>().draft.tool != Some(CreateTool::Spline))
    {
        return Err(if drawing {
            "Use click to select a drawing annotation"
        } else {
            "Double-click completes an active sketch spline"
        }
        .into());
    }
    let mut windows = world.query_filtered::<Entity, With<PrimaryWindow>>();
    let window = windows
        .single(world)
        .map_err(|_| "Native window is unavailable")?;
    let cursor = Vec2::new(point[0] as f32, point[1] as f32);
    let mut events = vec![WindowEvent::CursorMoved(CursorMoved {
        window,
        position: cursor,
        delta: None,
    })];
    if request.gesture != Gesture::Move {
        events.push(WindowEvent::MouseButtonInput(MouseButtonInput {
            button: request.button.into(),
            state: ButtonState::Pressed,
            window,
        }));
        if request.gesture == Gesture::Drag {
            events.push(WindowEvent::CursorMoved(CursorMoved {
                window,
                position: Vec2::new(end[0] as f32, end[1] as f32),
                delta: Some(Vec2::new(
                    (end[0] - point[0]) as f32,
                    (end[1] - point[1]) as f32,
                )),
            }));
            if let Some(lifecycle) = request.lifecycle {
                events.push(lifecycle_event(window, lifecycle));
            }
        }
        if request.release.unwrap_or(true) {
            events.push(WindowEvent::MouseButtonInput(MouseButtonInput {
                button: request.button.into(),
                state: ButtonState::Released,
                window,
            }));
        }
    } else if let Some(lifecycle) = request.lifecycle {
        events.push(lifecycle_event(window, lifecycle));
    }
    let result = (|| {
        let mut result = json!({"handled":false});
        let mut cursor = cursor;
        for event in events {
            if let WindowEvent::CursorMoved(moved) = &event {
                cursor = moved.position;
            }
            let mut input = NativeHostInput {
                ui_scale: 1.,
                context: Some(owner.clone()),
                cursor: Some(cursor),
                modifiers: Modifiers {
                    shift: request.shift,
                    ..default()
                },
                event,
                consumed: false,
                actions: vec![],
            };
            if navigate_canvas_input(world, handle, &input)? {
                result = json!({"handled":true,"navigation":true});
                continue;
            }
            prepare_native_input(world, handle, &mut input)?;
            for action in std::mem::take(&mut input.actions) {
                let value = reduce_control_input(
                    &services.engine,
                    &services.bridge,
                    world,
                    handle,
                    &action,
                )?;
                if worker::busy(world) {
                    return Ok(value);
                }
                result = value;
                result["handled"] = json!(true);
            }
            if drawing {
                if workbench::drawing_author_input(world, handle, services, &input)? {
                    result = json!({"handled":true,"drawing":true});
                }
                if worker::busy(world) {
                    return Ok(result);
                }
            } else if !input.consumed {
                let value = process_one(world, handle, services, &input)?;
                if value["handled"] == true || value["mutation_pending"] == true {
                    result = value;
                }
                if worker::busy(world) {
                    return Ok(result);
                }
            }
        }
        if request.gesture == Gesture::DoubleClick {
            return execute(
                world,
                &services.engine,
                &services.bridge,
                owner,
                EditorCommand::Complete,
                || Ok(()),
            );
        }
        if result["handled"] != true {
            return Err("No active native canvas interaction handled this gesture".into());
        }
        Ok(result)
    })();
    if drawing {
        workbench::cancel_drawing_author_input(world);
    } else {
        world.resource_mut::<Editor>().press = None;
    }
    cancel_canvas_navigation(world);
    if result.is_err() {
        mechanism::cancel(world);
    }
    cancel_native_pointer(world, handle);
    let report_poses = request.poses || request.lifecycle.is_some();
    result.and_then(|value| {
        if drawing {
            return Ok(value);
        }
        let mut value = mechanism::tick(world, handle, services, owner)?.unwrap_or(value);
        if report_poses {
            let (_, _, view, _) = native_viewport::interface_view(world);
            value["instance_body_poses"] = serde_json::to_value(view.instance_body_poses.as_ref())
                .map_err(|error| error.to_string())?;
        }
        Ok(value)
    })
}
