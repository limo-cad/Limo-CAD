use super::*;
use bevy::input::{keyboard::Key, ButtonState};
use limo_cad_interface::Rect;
use std::time::Instant;

#[derive(Clone)]
pub(super) struct Drag {
    receipt: DocumentReceipt,
    action: NativeInterfaceAction,
    document: Arc<DocumentDto>,
    feature_id: Option<u64>,
    start: Vec2,
    cursor: Option<Vec2>,
    pub(super) target: Option<usize>,
    pub(super) moved: bool,
    last_scroll: Instant,
}

pub(crate) fn cancel_drag(world: &mut World) {
    if let Some(mut state) = world.get_resource_mut::<History>() {
        state.drag = None;
    }
    edge_updates(world, false);
}

const SCROLL_INTERVAL: Duration = Duration::from_millis(120);

#[derive(Resource, Default)]
struct EdgeCadence(Option<bevy::winit::UpdateMode>);

/// Wake only while holding a drag at an edge with more history to reveal.
/// Restoring the previous cadence keeps the idle host event driven.
fn edge_updates(world: &mut World, active: bool) {
    world.init_resource::<EdgeCadence>();
    world.resource_scope(|world, mut cadence: Mut<EdgeCadence>| {
        let Some(mut settings) = world.get_resource_mut::<bevy::winit::WinitSettings>() else {
            return;
        };
        if active && cadence.0.is_none() {
            cadence.0 = Some(settings.focused_mode);
            if let bevy::winit::UpdateMode::Reactive { wait, .. } = &mut settings.focused_mode {
                *wait = (*wait).min(SCROLL_INTERVAL);
            }
        } else if !active {
            if let Some(previous) = cadence.0.take() {
                settings.focused_mode = previous;
            }
        }
    });
}

pub(crate) fn tick(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
) -> Result<(), String> {
    tick_at(world, handle, services, Instant::now())
}

pub(super) fn tick_at(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    now: Instant,
) -> Result<(), String> {
    let active = world
        .get_resource_mut::<History>()
        .and_then(|mut state| state.drag.take());
    let Some(mut drag) = active else {
        edge_updates(world, false);
        return Ok(());
    };
    if !services
        .bridge
        .native_document_receipt(&services.engine, &drag.receipt.owner)
        .is_ok_and(|current| current == drag.receipt)
        || handle.frame().is_none_or(|frame| {
            frame.context != drag.receipt.owner || !frame.modal_stack.is_empty()
        })
    {
        edge_updates(world, false);
        crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
        handle.invalidate_presentation();
        return Err("The design changed; start the history drag again".into());
    }
    let mut direction = 0isize;
    if drag.moved {
        if let Some(cursor) = drag.cursor {
            let visible = slots(world, handle, &drag.document)?;
            drag.target = target_slot(&visible, cursor);
            let state = world.resource::<History>();
            if drag.target.is_some() {
                if visible
                    .first()
                    .is_some_and(|(_, bounds)| f64::from(cursor.x) < bounds.x + 10.)
                    && state.scroll > 0
                {
                    direction = -1;
                } else if visible.last().is_some_and(|(index, bounds)| {
                    index + 1 < drag.document.features.len()
                        && f64::from(cursor.x) > bounds.x + bounds.width - 10.
                }) {
                    direction = 1;
                }
            }
        }
    }
    if direction != 0 && now.saturating_duration_since(drag.last_scroll) >= SCROLL_INTERVAL {
        let mut state = world.resource_mut::<History>();
        state.scroll = state
            .scroll
            .saturating_add_signed(direction)
            .min(drag.document.features.len().saturating_sub(1));
        drag.last_scroll = now;
        handle.invalidate_presentation();
    }
    world.resource_mut::<History>().drag = Some(drag);
    edge_updates(world, direction != 0);
    Ok(())
}

fn slots(
    world: &World,
    handle: &NativeInterfaceHandle,
    document: &DocumentDto,
) -> Result<Vec<(usize, Rect)>, String> {
    handle.read_surface(|_, frame| {
        let mut slots = frame
            .controls
            .iter()
            .filter_map(|control| {
                let command = &world
                    .get::<NativeCommandBinding>(Entity::from_bits(control.key.0))?
                    .command;
                let NativeCommand::History(HistoryCommand::Select(id)) = command else {
                    return None;
                };
                let index = document
                    .features
                    .iter()
                    .position(|feature| feature.id.0 == *id)?;
                (control.visible && !control.disabled).then_some((index, control.bounds))
            })
            .collect::<Vec<_>>();
        slots.sort_by_key(|(index, _)| *index);
        slots
    })
}

fn target_slot(slots: &[(usize, Rect)], cursor: Vec2) -> Option<usize> {
    let (_, first) = slots.first()?;
    let (_, last) = slots.last()?;
    if !cursor.is_finite()
        || f64::from(cursor.x) < first.x - 8.
        || f64::from(cursor.x) > last.x + last.width + 8.
        || f64::from(cursor.y) < first.y - 8.
        || f64::from(cursor.y) > first.y + first.height + 8.
    {
        return None;
    }
    slots
        .iter()
        .find(|(_, bounds)| f64::from(cursor.x) < bounds.x + bounds.width / 2.)
        .map(|(index, _)| *index)
        .or_else(|| slots.last().map(|(index, _)| index + 1))
}

fn operation(
    document: &DocumentDto,
    feature_id: Option<u64>,
    target: usize,
) -> Option<(&'static str, Value)> {
    if let Some(feature_id) = feature_id {
        let index = document
            .features
            .iter()
            .position(|feature| feature.id.0 == feature_id)?;
        if target == index || target == index + 1 {
            return None;
        }
        Some((
            "solid_reorder_feature",
            json!({"feature_id": feature_id, "target_index": target}),
        ))
    } else {
        (target != document.rollback_index)
            .then(|| ("solid_set_rollback", json!({"rollback_index": target})))
    }
}

/// A drag keeps the original control and document receipt. Previewing an
/// insertion slot never replays history; only a completed drop enters the worker.
pub(crate) fn pointer(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    event: &NativeHostInput,
) -> Result<bool, String> {
    let active = world
        .get_resource_mut::<History>()
        .and_then(|mut state| state.drag.take());
    if let Some(mut drag) = active {
        let cancelled = matches!(
            &event.event,
            WindowEvent::WindowFocused(bevy::window::WindowFocused { focused: false, .. })
                | WindowEvent::KeyboardFocusLost(_)
        ) || matches!(&event.event, WindowEvent::KeyboardInput(input) if input.state == ButtonState::Pressed && input.logical_key == Key::Escape);
        if cancelled {
            edge_updates(world, false);
            crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
            handle.invalidate_presentation();
            return Ok(matches!(event.event, WindowEvent::KeyboardInput(_)));
        }
        if event.context.as_ref() != Some(&drag.receipt.owner)
            || !services
                .bridge
                .native_document_receipt(&services.engine, &drag.receipt.owner)
                .is_ok_and(|current| current == drag.receipt)
            || handle
                .frame()
                .is_none_or(|frame| !frame.modal_stack.is_empty())
        {
            edge_updates(world, false);
            crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
            return Err("The design changed; start the history drag again".into());
        }
        let release = matches!(&event.event, WindowEvent::MouseButtonInput(input) if input.button == MouseButton::Left && input.state == ButtonState::Released);
        let motion = matches!(
            event.event,
            WindowEvent::CursorMoved(_) | WindowEvent::CursorLeft(_)
        );
        if !motion && !release {
            world.resource_mut::<History>().drag = Some(drag);
            return Ok(false);
        }
        if let Some(cursor) = event.cursor {
            drag.cursor = Some(cursor);
            if !drag.moved && cursor.distance(drag.start) >= 4. {
                if let Err(error) = handle.validate_action(&drag.action) {
                    edge_updates(world, false);
                    crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
                    return Err(error);
                }
                drag.moved = true;
                crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
            }
            if drag.moved {
                idle(world)?;
                let visible = slots(world, handle, &drag.document)?;
                drag.target = target_slot(&visible, cursor);
            }
        } else {
            drag.cursor = None;
            drag.target = None;
        }
        if release {
            edge_updates(world, false);
            if !drag.moved {
                return Ok(false);
            }
            crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
            handle.invalidate_presentation();
            if let Some(target) = drag.target {
                if let Some((name, args)) = operation(&drag.document, drag.feature_id, target) {
                    mutation(world, drag.receipt, name, args)?;
                }
            }
            return Ok(true);
        }
        let moved = drag.moved;
        world.resource_mut::<History>().drag = Some(drag);
        if moved {
            handle.invalidate_presentation();
        }
        return Ok(moved);
    }
    if !matches!(&event.event, WindowEvent::MouseButtonInput(input) if input.button == MouseButton::Left && input.state == ButtonState::Pressed)
        || idle(world).is_err()
        || handle
            .frame()
            .is_none_or(|frame| !frame.modal_stack.is_empty())
    {
        return Ok(false);
    }
    let Some(cursor) = event.cursor else {
        return Ok(false);
    };
    let Some(key) = handle.hit_key(cursor.as_dvec2().to_array()) else {
        return Ok(false);
    };
    let Some(command) = world.get::<NativeCommandBinding>(Entity::from_bits(key.0)) else {
        return Ok(false);
    };
    let feature_id = match command.command {
        NativeCommand::History(HistoryCommand::Select(id)) => Some(id),
        NativeCommand::History(HistoryCommand::RollbackMarker) => None,
        _ => return Ok(false),
    };
    let action = handle.resolve_retained(key)?;
    if event.context.as_ref() != Some(&action.context) {
        return Ok(false);
    }
    let document = services.engine.document_snapshot();
    if feature_id.is_some() && document.rollback_index != document.features.len() {
        return Ok(false);
    }
    let receipt = services
        .bridge
        .native_document_receipt(&services.engine, &action.context)?;
    world.init_resource::<History>();
    world.resource_mut::<History>().drag = Some(Drag {
        receipt,
        action,
        document: Arc::new(document),
        feature_id,
        start: cursor,
        cursor: Some(cursor),
        target: None,
        moved: false,
        last_scroll: Instant::now(),
    });
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drop_slots_follow_visible_control_centers_and_reject_escape_on_both_axes() {
        let rows = [
            (
                3,
                Rect {
                    x: 338.,
                    y: 822.,
                    width: 60.,
                    height: 28.,
                },
            ),
            (
                4,
                Rect {
                    x: 404.,
                    y: 822.,
                    width: 100.,
                    height: 28.,
                },
            ),
        ];
        assert_eq!(target_slot(&rows, Vec2::new(340., 830.)), Some(3));
        assert_eq!(target_slot(&rows, Vec2::new(390., 830.)), Some(4));
        assert_eq!(target_slot(&rows, Vec2::new(500., 830.)), Some(5));
        assert_eq!(target_slot(&rows, Vec2::new(390., 700.)), None);
        assert_eq!(target_slot(&rows, Vec2::new(100., 830.)), None);
        assert_eq!(target_slot(&rows, Vec2::new(900., 830.)), None);
        assert_eq!(target_slot(&rows, Vec2::new(332., 830.)), Some(3));
        assert_eq!(target_slot(&rows, Vec2::new(510., 830.)), Some(5));
        assert_eq!(target_slot(&rows, Vec2::new(f32::NAN, 830.)), None);
    }
}
