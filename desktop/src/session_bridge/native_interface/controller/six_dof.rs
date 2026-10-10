//! Explicit, process-owned 3D mouse connection and per-window camera routing.
//! Driver work belongs to the sleeping connection worker; no model mutation is
//! needed for navigation, including while an OCC operation owns the engine.
use super::*;
use crate::native_viewport::winit_host::{self, NativeRenderAvailability};
use std::time::Instant;

mod mailbox;
mod service;
#[cfg(test)]
pub(crate) use mailbox::device_axes;
pub(crate) use mailbox::Motion;
pub(super) use service::Status;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Command {
    pub generation: u64,
    pub connect: bool,
}

#[derive(Resource)]
struct Connection {
    service: Arc<service::Service>,
    window: String,
    observed: Option<(u64, &'static str)>,
    owner: Option<DocumentContext>,
    previous: Option<Instant>,
    busy_pointer: bool,
}
impl Drop for Connection {
    fn drop(&mut self) {
        self.service.unregister(&self.window);
    }
}

#[derive(Component)]
pub(super) struct ConnectionButton;
#[derive(Component)]
pub(super) struct ConnectionDot;

pub(super) fn install(
    world: &mut World,
    window: &str,
    handle: &NativeInterfaceHandle,
) -> Result<(), String> {
    let service = service::Service::process()?;
    let wake = handle.clone();
    service.register(window.into(), Arc::new(move || wake.request_redraw()));
    world.insert_resource(Connection {
        service,
        window: window.into(),
        observed: None,
        owner: None,
        previous: None,
        busy_pointer: false,
    });
    Ok(())
}

pub(super) fn status(world: &World) -> Status {
    world.get_resource::<Connection>().map_or_else(
        || Status {
            state: "unavailable",
            message: "3D mouse worker unavailable".into(),
            ..default()
        },
        |connection| connection.service.status(),
    )
}
pub(super) fn disabled(status: &Status) -> bool {
    matches!(status.state, "connecting" | "disconnecting" | "unavailable")
}
pub(super) fn command(status: &Status) -> Command {
    Command {
        generation: status.generation,
        connect: status.state != "connected",
    }
}
pub(super) fn color(world: &World, status: &Status) -> Color {
    match status.state {
        "connected" => Color::srgb_u8(88, 173, 114),
        "connecting" | "disconnecting" => Color::srgb_u8(218, 162, 68),
        "error" => Color::srgb_u8(225, 91, 100),
        _ => crate::native_viewport::ui::theme(world).mute,
    }
}

pub(crate) fn reduce(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    action: &NativeInterfaceAction,
    command: Command,
) -> Result<Value, String> {
    if !super::super::is_activation(&action.control.input) {
        return Err("Activate the 3D mouse connection button".into());
    }
    handle.validate_action(action)?;
    let current = world
        .get_resource::<NativeRenderedDocument>()
        .map(|rendered| &rendered.owner);
    if current != Some(&action.context) {
        return Err("The rendered document changed; inspect again".into());
    }
    let connection = world
        .get_resource::<Connection>()
        .ok_or("3D mouse worker unavailable")?;
    let status = connection
        .service
        .request_at(command.generation, command.connect)?;
    handle.invalidate_presentation();
    handle.request_redraw();
    Ok(json!({"six_dof":status}))
}

pub(super) fn eligible(
    world: &World,
    handle: &NativeInterfaceHandle,
    closing: bool,
) -> Option<workspace::DocumentReceipt> {
    let available = world.get_resource::<NativeRenderAvailability>()?;
    if closing
        || !available.focused
        || !available.drawable
        || workbench::workspace(world) == workbench::Workspace::Drawing
        || files::modal(world)
            .or_else(|| workbench::modal(world))
            .or_else(|| history::modal(world))
            .or_else(|| crate::native_editor::panel::modal(world))
            .or_else(|| crate::native_editor::support::modal(world))
            .is_some()
        || handle.has_capture()
        || workbench::cam::reorder_drag::active(world)
        || workbench::cam::geometry_pick::active(world)
        || winit_host::model_pointer_active(world)
        || view::pointer_active(world)
    {
        return None;
    }
    let rendered = world.get_resource::<NativeRenderedDocument>()?;
    let current = handle
        .read_surface(|owner, frame| owner == &rendered.owner && frame.modal_stack.is_empty())
        .unwrap_or(false);
    current.then(|| workspace::DocumentReceipt {
        owner: rendered.owner.clone(),
        revision: rendered.revision,
    })
}

pub(super) fn tick(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    closing: bool,
) -> Result<(), String> {
    let Some(mut connection) = world.remove_resource::<Connection>() else {
        return Ok(());
    };
    let now = Instant::now();
    let status = connection.service.status();
    let changed = connection.observed != Some((status.generation, status.state));
    connection.observed = Some((status.generation, status.state));
    let rendered = if changed {
        None
    } else {
        eligible(world, handle, closing)
    };
    let owner = rendered.as_ref().map(|r| &r.owner);
    if connection.owner.as_ref() != owner {
        connection.previous = None;
        connection.owner = owner.cloned();
    }
    let (motion, fit) = connection.service.sample(&connection.window, owner, now);
    let result = (|| {
        if let Some(rendered) = rendered {
            if motion.active() {
                let dt = connection.previous.map_or(1. / 60., |last| {
                    now.saturating_duration_since(last).as_secs_f32()
                });
                connection.previous = Some(now);
                view::six_dof::apply(
                    world,
                    &rendered.owner,
                    motion,
                    dt,
                    app_settings::six_dof_speed(world),
                )?;
                handle.request_redraw();
            } else {
                connection.previous = None;
            }
            if fit {
                view::request(
                    world,
                    &rendered.owner,
                    rendered.revision,
                    &json!({"view":"current", "fit":true, "duration_ms":if worker::busy(world) {0} else {300},
                        "expires_ms":crate::session_bridge::now_ms()+5000}),
                )?;
                handle.request_redraw();
            }
        }
        Ok(())
    })();
    let refreshed = if changed {
        let refreshed = refresh_controls(world, &status);
        handle.invalidate_presentation();
        handle.request_redraw();
        refreshed
    } else {
        Ok(())
    };
    world.insert_resource(connection);
    result.and(refreshed)
}

fn refresh_controls(world: &mut World, status: &Status) -> Result<(), String> {
    let entities = world
        .query_filtered::<Entity, With<ConnectionButton>>()
        .iter(world)
        .collect::<Vec<_>>();
    for entity in entities {
        let next = NativeCommand::SixDof(command(status));
        if world
            .get::<NativeCommandBinding>(entity)
            .is_none_or(|binding| binding.command != next)
        {
            bind_command(world, entity, next)?;
        }
        if let Some(mut control) = world.get_mut::<InterfaceControl>(entity) {
            control.label = status.message.clone();
            control.disabled = disabled(status);
            control.selected = Some(status.state == "connected");
        }
    }
    let color = color(world, status);
    for mut background in world
        .query_filtered::<&mut BackgroundColor, With<ConnectionDot>>()
        .iter_mut(world)
    {
        if background.0 != color {
            background.0 = color;
        }
    }
    Ok(())
}

/// Same stamped pointer adapter as the existing playback controls. It handles
/// only this button and never acquires the busy model/engine lock.
pub(super) fn busy_input(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    input: &NativeHostInput,
) -> Result<bool, String> {
    use crate::native_viewport::interface_shell::{PointerButton, PointerPhase};
    use bevy::input::ButtonState;
    let Some(connection) = world.get_resource::<Connection>() else {
        return Ok(false);
    };
    if !handle
        .read_surface(|owner, frame| {
            input.context.as_ref() == Some(owner) && frame.modal_stack.is_empty()
        })
        .unwrap_or(false)
    {
        return Ok(false);
    }
    let captured = connection.busy_pointer && handle.has_capture();
    if connection.busy_pointer && !captured {
        world.resource_mut::<Connection>().busy_pointer = false;
    }
    if matches!(
        input.event,
        WindowEvent::WindowFocused(bevy::window::WindowFocused { focused: false, .. })
            | WindowEvent::CursorLeft(_)
    ) {
        if captured {
            handle.cancel_pointer();
        }
        world.resource_mut::<Connection>().busy_pointer = false;
        return Ok(false);
    }
    let Some(cursor) = input.cursor else {
        return Ok(false);
    };
    let point = cursor.as_dvec2().to_array();
    let phase = match &input.event {
        WindowEvent::MouseButtonInput(button) if button.button == MouseButton::Left => {
            if button.state == ButtonState::Pressed {
                let Some(key) = handle.hit_key(point) else {
                    return Ok(false);
                };
                if world
                    .get::<ConnectionButton>(Entity::from_bits(key.0))
                    .is_none()
                {
                    return Ok(false);
                }
                world.resource_mut::<Connection>().busy_pointer = true;
                PointerPhase::Down
            } else if captured {
                world.resource_mut::<Connection>().busy_pointer = false;
                PointerPhase::Up
            } else {
                return Ok(false);
            }
        }
        WindowEvent::CursorMoved(_) if captured => PointerPhase::Move,
        _ => return Ok(false),
    };
    handle.pointer(phase, point, PointerButton::Primary)?;
    for action in handle.take_actions()? {
        let entity = Entity::from_bits(action.control.key.0);
        let Some(binding) = world.get::<NativeCommandBinding>(entity) else {
            continue;
        };
        let NativeCommand::SixDof(command) = binding.command else {
            continue;
        };
        let Some(control) = world.get::<InterfaceControl>(entity) else {
            continue;
        };
        if !control.disabled
            && control.visible
            && control.binding == binding.generation
            && control.binding == action.control.binding()
        {
            reduce(world, handle, &action, command)?;
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests;
