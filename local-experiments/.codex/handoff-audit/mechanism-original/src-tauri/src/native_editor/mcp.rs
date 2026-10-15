//! Existing MCP canvas gestures are native input, not alternate sketch commands.
//! All picks pass through the same focus, ownership and editor handlers as Winit.

use super::*;
use crate::native_viewport::winit_host::{cancel_native_pointer, prepare_native_input, Modifiers};
use crate::session_bridge::native_interface::controller::reduce_control_input;
use bevy::{
    input::mouse::MouseButtonInput,
    window::{CursorMoved, PrimaryWindow},
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

#[derive(Deserialize)]
struct Request {
    gesture: Gesture,
    #[serde(default)]
    canvas: Option<String>,
    point: Option<[f64; 2]>,
    world: Option<[f64; 3]>,
    to: Option<[f64; 2]>,
    #[serde(default)]
    shift: bool,
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
    let canvas_name = request.canvas.as_deref().unwrap_or("viewport");
    if canvas_name != "viewport" {
        return Err("The native drawing canvas is not available yet".into());
    }
    let bounds = handle.read_surface(|context, frame| {
        if context != owner {
            return Err("Canvas gesture belongs to a retired document".to_owned());
        }
        if !frame.modal_stack.is_empty() {
            return Err("Close the current dialog before using the canvas".to_owned());
        }
        frame
            .canvases
            .iter()
            .find(|c| c.name == canvas_name)
            .map(|c| c.bounds)
            .ok_or_else(|| "The model canvas is unavailable".to_owned())
    })??;
    services
        .bridge
        .with_native_document_owner(&services.engine, owner, || Ok(()))?;
    let point = match (request.point, request.world) {
        (Some(point), None) => point,
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
        if handle.owns_pointer(point) {
            return Err("A native control covers that canvas point".into());
        }
    }
    initialize(world);
    if request.gesture == Gesture::DoubleClick
        && world.resource::<Editor>().draft.tool != Some(CreateTool::Spline)
    {
        return Err("Double-click editing is not migrated for this native tool yet".into());
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
            button: MouseButton::Left,
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
        }
        events.push(WindowEvent::MouseButtonInput(MouseButtonInput {
            button: MouseButton::Left,
            state: ButtonState::Released,
            window,
        }));
    }
    let result = (|| {
        let mut result = json!({"handled":false});
        let mut cursor = cursor;
        for event in events {
            if let WindowEvent::CursorMoved(moved) = &event {
                cursor = moved.position;
            }
            let mut input = NativeHostInput {
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
            }
            if !input.consumed {
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
    // Each MCP call is an atomic gesture. Errors, focus changes and worker
    // enqueue must never leave a synthetic primary button held across calls.
    world.resource_mut::<Editor>().press = None;
    if result.is_err() { mechanism::cancel(world); }
    cancel_native_pointer(world, handle);
    result.and_then(|value| Ok(mechanism::tick(world,handle,services,owner)?.unwrap_or(value)))
}
