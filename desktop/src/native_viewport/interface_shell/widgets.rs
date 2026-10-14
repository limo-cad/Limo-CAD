//! WIP: headless Bevy button/checkbox behavior behind the native input fence.
//!
//! The ordered host admits input against presented geometry and its original
//! document/binding. Only that admitted input can reach the existing reducer
//! through a widget observer. Ordinary picking/focus events have no admission
//! and cannot enqueue a command. Styling, tab order, menus and text stay native.

use super::*;
use bevy::{
    input::ButtonState,
    input_focus::FocusedInput,
    picking::{
        backend::HitData,
        events::{Pointer, PointerClick, PointerPress, PointerRelease},
        pointer::{Location, PointerButton as BevyPointerButton, PointerId},
        PickingSettings,
    },
    ui::{Checkable, Checked, InteractionDisabled, Pressed},
    ui_widgets::{Activate, Button, ButtonPlugin, Checkbox, CheckboxPlugin, ValueChange},
    window::{WindowEvent, WindowRef},
};
use std::time::Duration;

use super::super::winit_host::NativeHostInput;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum WidgetKind {
    Button,
    Checkbox,
}

#[derive(Resource, Default)]
struct WidgetInput {
    pressed: Option<NativeInterfaceAction>,
    admitted: Option<NativeInterfaceAction>,
    emitted: bool,
}

pub(super) fn install(app: &mut App) {
    // DefaultPlugins already includes these; headless fixtures install only
    // the two behavior plugins rather than starting an OS window or renderer.
    if !app.is_plugin_added::<ButtonPlugin>() {
        app.add_plugins(ButtonPlugin);
    }
    if !app.is_plugin_added::<CheckboxPlugin>() {
        app.add_plugins(CheckboxPlugin);
    }
    app.init_resource::<WidgetInput>()
        .add_observer(on_activate)
        .add_observer(on_toggle)
        .add_systems(
            Update,
            synchronize
                .after(InterfaceReduction)
                .before(update_controls),
        );
    // Native input is already ordered and hit-tested by the guarded adapter.
    // A second picking pipeline would observe the same OS gesture twice.
    if let Some(mut settings) = app.world_mut().get_resource_mut::<PickingSettings>() {
        settings.is_input_enabled = false;
    }
}

fn kind(control: &InterfaceControl) -> Option<WidgetKind> {
    if control.text_editing {
        return None;
    }
    match (&control.field, control.role.as_str()) {
        (Field::Toggle(_), "checkbox" | "button") => Some(WidgetKind::Checkbox),
        (Field::None, "button" | "menuitem") => Some(WidgetKind::Button),
        _ => None,
    }
}

fn synchronize(world: &mut World) {
    let controls = world
        .query_filtered::<(Entity, &InterfaceControl), With<InterfaceButtonStyle>>()
        .iter(world)
        .map(|(entity, control)| (entity, control.clone()))
        .collect::<Vec<_>>();
    for (entity, control) in controls {
        synchronize_control(world, entity, &control);
    }
    let Some(handle) = world.get_resource::<NativeInterfaceHandle>() else {
        return;
    };
    let still_captured = handle.shared.lock().ok().and_then(|shared| {
        shared
            .capture
            .as_ref()
            .map(|capture| NativeInterfaceAction {
                context: capture.context.clone(),
                control: capture.resolved.clone(),
            })
    });
    if world.resource::<WidgetInput>().pressed != still_captured {
        cancel(world);
    }
}

fn synchronize_control(world: &mut World, entity: Entity, control: &InterfaceControl) {
    let next = kind(control);
    let previous = world.get::<WidgetKind>(entity).copied();
    let mut target = world.entity_mut(entity);
    if next != previous {
        target.remove::<(
            WidgetKind,
            Button,
            Checkbox,
            Checkable,
            Checked,
            Pressed,
            InteractionDisabled,
        )>();
        match next {
            Some(WidgetKind::Button) => {
                target.insert((WidgetKind::Button, Button));
            }
            Some(WidgetKind::Checkbox) => {
                target.insert((WidgetKind::Checkbox, Checkbox));
            }
            None => return,
        }
        // The native host owns its stamped accessibility projection. Widget
        // requirements must not add a second unnamed control to that tree.
        target.remove::<bevy::a11y::AccessibilityNode>();
    }
    if next.is_none() {
        return;
    }
    if control.disabled || !control.visible {
        target.insert(InteractionDisabled);
        target.remove::<Pressed>();
    } else {
        target.remove::<InteractionDisabled>();
    }
    if matches!(control.field, Field::Toggle(true)) {
        target.insert(Checked);
    } else {
        target.remove::<Checked>();
    }
}

fn on_activate(event: On<Activate>, mut input: ResMut<WidgetInput>) {
    if input
        .admitted
        .as_ref()
        .is_some_and(|action| action.control.key == ControlKey(event.entity.to_bits()))
    {
        input.emitted = true;
    }
}

fn on_toggle(
    event: On<ValueChange<bool>>,
    controls: Query<&InterfaceControl>,
    mut input: ResMut<WidgetInput>,
) {
    let admitted = input
        .admitted
        .as_ref()
        .is_some_and(|action| action.control.key == ControlKey(event.source.to_bits()));
    if admitted
        && event.is_final
        && controls.get(event.source).is_ok_and(
            |control| matches!(control.field, Field::Toggle(checked) if event.value != checked),
        )
    {
        input.emitted = true;
    }
    // Controlled state: the existing native reducer applies the original
    // click/key intent. Some toggles represent commands (e.g. Fix/Unfix), not
    // an independently writable boolean. Project Checked after reduction.
}

pub(crate) fn cancel(world: &mut World) {
    let Some(mut input) = world.get_resource_mut::<WidgetInput>() else {
        return;
    };
    let pressed = input.pressed.take();
    input.admitted = None;
    input.emitted = false;
    if let Some(action) = pressed {
        if let Ok(mut target) = world.get_entity_mut(Entity::from_bits(action.control.key.0)) {
            target.remove::<Pressed>();
        }
    }
}

fn pointer(input: &NativeHostInput) -> Pointer {
    let window = super::super::winit_host::input_window(&input.event)
        .expect("native pointer event has a window");
    Pointer::new(
        PointerId::Mouse,
        Location {
            target: bevy::camera::RenderTarget::Window(WindowRef::Entity(window))
                .normalize(Some(window))
                .expect("explicit native window"),
            position: input.cursor.unwrap_or(Vec2::ZERO),
        },
    )
}

fn hit(world: &World, entity: Entity) -> HitData {
    HitData::new(
        world
            .get::<UiTargetCamera>(entity)
            .map_or(entity, |camera| camera.0),
        0.,
        None,
        None,
    )
}

pub(crate) fn adapt_input(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    input: &mut NativeHostInput,
) -> Result<(), String> {
    if !world.contains_resource::<WidgetInput>() {
        return Ok(());
    }
    if matches!(&input.event, WindowEvent::MouseButtonInput(event)
        if event.button == MouseButton::Left && event.state == ButtonState::Pressed)
    {
        cancel(world);
        let capture = handle
            .shared
            .lock()
            .map_err(|_| "Native interface lock poisoned")?
            .capture
            .clone();
        if let Some(capture) = capture {
            let entity = Entity::from_bits(capture.resolved.key.0);
            if let Some(control) = world.get::<InterfaceControl>(entity).cloned() {
                synchronize_control(world, entity, &control);
                if kind(&control).is_some() {
                    let action = NativeInterfaceAction {
                        context: capture.context,
                        control: capture.resolved,
                    };
                    handle.validate_dispatch_action(&action)?;
                    world.resource_mut::<WidgetInput>().pressed = Some(action);
                    world.trigger(PointerPress {
                        entity,
                        pointer: pointer(input),
                        button: BevyPointerButton::Primary,
                        hit: hit(world, entity),
                        count: 1,
                    });
                    world.flush();
                }
            }
        }
    }

    let result = adapt_actions(world, handle, input);
    if matches!(&input.event,
        WindowEvent::MouseButtonInput(event) if event.state == ButtonState::Released)
        || matches!(&input.event, WindowEvent::WindowFocused(event) if !event.focused)
        || matches!(&input.event, WindowEvent::KeyboardFocusLost(_))
        || result.is_err()
    {
        cancel(world);
    }
    result
}

fn adapt_actions(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    input: &mut NativeHostInput,
) -> Result<(), String> {
    for action in std::mem::take(&mut input.actions) {
        let entity = Entity::from_bits(action.control.key.0);
        let control = world.get::<InterfaceControl>(entity).cloned();
        let activation = match &action.control.input {
            ControlInput::Click => matches!(&input.event, WindowEvent::MouseButtonInput(event)
                if event.button == MouseButton::Left && event.state == ButtonState::Released),
            ControlInput::Key(key) => {
                !key.ctrl
                    && !key.meta
                    && !key.alt
                    && !key.shift
                    && matches!(key.key.as_str(), "Enter" | " " | "Space")
            }
            _ => false,
        };
        let Some(control) = control.filter(|control| activation && kind(control).is_some()) else {
            input.actions.push(action);
            continue;
        };
        handle.validate_dispatch_action(&action)?;
        if control.binding != action.control.binding() || control.disabled || !control.visible {
            return Err("Widget control changed before activation".into());
        }
        synchronize_control(world, entity, &control);
        {
            let mut dispatch = world.resource_mut::<WidgetInput>();
            dispatch.admitted = Some(action.clone());
            dispatch.emitted = false;
        }
        match &input.event {
            WindowEvent::MouseButtonInput(_) => {
                if world.resource::<WidgetInput>().pressed.as_ref() != Some(&action) {
                    cancel(world);
                    return Err("Widget press belongs to an earlier control".into());
                }
                world.trigger(PointerClick {
                    entity,
                    pointer: pointer(input),
                    button: BevyPointerButton::Primary,
                    hit: hit(world, entity),
                    duration: Duration::ZERO,
                    count: 1,
                });
                world.flush();
                world.trigger(PointerRelease {
                    entity,
                    pointer: pointer(input),
                    button: BevyPointerButton::Primary,
                    hit: hit(world, entity),
                });
            }
            WindowEvent::KeyboardInput(event) => {
                world.trigger(FocusedInput::new(entity, event.clone(), event.window));
            }
            _ => {}
        }
        world.flush();
        let emitted = {
            let mut dispatch = world.resource_mut::<WidgetInput>();
            dispatch.admitted = None;
            std::mem::take(&mut dispatch.emitted)
        };
        if emitted {
            handle.validate_dispatch_action(&action)?;
            input.actions.push(action);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
