//! Standard Bevy picking, focus and button/checkbox behavior, with CAD commands
//! as controlled state. No synthetic pointer events or second click recognizer.

use super::*;
use bevy::{
    input::{keyboard::KeyCode, ButtonState},
    input_focus::{
        tab_navigation::{NavAction, TabGroup, TabIndex, TabNavigation},
        FocusCause, InputDispatchPlugin, InputFocus, InputFocusPlugin, InputFocusSystems,
        InputFocusVisible,
    },
    picking::{
        events::PointerState,
        hover::Hovered,
        pointer::{PointerAction, PointerButton as BevyPointerButton, PointerId, PointerInput},
        Pickable, PickingSystems,
    },
    ui::{
        experimental::GhostNode,
        picking_backend::{UiPickingCamera, UiPickingSettings},
        Checkable, Checked, InteractionDisabled, Pressed,
    },
    ui_widgets::{Activate, Button, ButtonPlugin, Checkbox, CheckboxPlugin},
    window::WindowEvent,
};

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum WidgetKind {
    Button,
    Checkbox,
}

/// Published command ownership, not input state. Retained entities can be
/// rebound; a queued activation must never operate on the replacement target.
#[derive(Component, Clone, PartialEq, Eq)]
struct WidgetAction(NativeInterfaceAction);

#[derive(Component)]
struct WidgetFocusRoot;

#[derive(Resource, Default)]
struct WidgetActions(Vec<NativeInterfaceAction>);

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct WidgetLayout;

pub(super) fn install(app: &mut App) {
    if !app.is_plugin_added::<bevy::input::InputPlugin>() {
        app.add_plugins(bevy::input::InputPlugin);
    }
    if !app.is_plugin_added::<bevy::time::TimePlugin>() {
        app.add_plugins(bevy::time::TimePlugin);
    }
    if !app.is_plugin_added::<bevy::picking::PickingPlugin>() {
        app.add_plugins(bevy::picking::DefaultPickingPlugins);
    }
    if !app.is_plugin_added::<bevy::input_focus::pointer_focus::PointerFocusPlugin>() {
        app.add_plugins(bevy::input_focus::pointer_focus::PointerFocusPlugin);
    }
    app.add_message::<WindowEvent>();
    if !app.is_plugin_added::<InputFocusPlugin>() {
        app.add_plugins(InputFocusPlugin);
    }
    if !app.is_plugin_added::<InputDispatchPlugin>() {
        app.add_plugins(InputDispatchPlugin);
    }
    if !app.is_plugin_added::<ButtonPlugin>() {
        app.add_plugins(ButtonPlugin);
    }
    if !app.is_plugin_added::<CheckboxPlugin>() {
        app.add_plugins(CheckboxPlugin);
    }
    app.init_resource::<WidgetActions>()
        .insert_resource(UiPickingSettings {
            require_markers: true,
        })
        .add_observer(on_activate)
        .add_observer(on_toggle)
        .add_systems(First, primary_pointer.in_set(PickingSystems::PostInput))
        .configure_sets(
            PreUpdate,
            InputFocusSystems::Dispatch.after(PickingSystems::Last),
        )
        .add_systems(PreUpdate, publish_focus.after(InputFocusSystems::Dispatch))
        .add_systems(
            PostUpdate,
            synchronize
                .in_set(WidgetLayout)
                .after(InterfaceLayout)
                .before(bevy::a11y::AccessibilitySystems::Update),
        );
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

/// Secondary buttons belong to CAD camera/context-menu gestures. Bevy's
/// headless widgets accept every pointer button, so select the primary stream
/// at the input boundary; leave hit testing and gesture recognition to Bevy.
fn primary_pointer(mut inputs: bevy::ecs::message::MessageMutator<PointerInput>) {
    for input in inputs.read() {
        if matches!(input.action, PointerAction::Press(button) | PointerAction::Release(button)
            if button != BevyPointerButton::Primary)
        {
            input.action = PointerAction::Move { delta: Vec2::ZERO };
        }
    }
}

fn publish_focus(focus: Res<InputFocus>, handle: Res<NativeInterfaceHandle>) {
    let Ok(mut shared) = handle.shared.lock() else {
        return;
    };
    let focused = focus
        .get()
        .map(|entity| ControlKey(entity.to_bits()))
        .filter(|key| {
            shared
                .registry
                .frame()
                .controls
                .iter()
                .any(|c| c.key == *key)
        });
    if focused != shared.focused && set_focus(&mut shared, focused).is_ok() {
        shared.revision = shared.revision.wrapping_add(1);
    }
}

/// Use Bevy's navigation algorithm with the host's per-event modifier snapshot.
/// The automatic plugin reads final batch key state, which loses Shift on macOS
/// modifier-only events and on batches containing the key release.
fn navigate_tab(
    In(backwards): In<bool>,
    navigation: TabNavigation,
    mut focus: ResMut<InputFocus>,
    mut visible: ResMut<InputFocusVisible>,
) {
    if let Ok(entity) = navigation.navigate(
        &focus,
        if backwards {
            NavAction::Previous
        } else {
            NavAction::Next
        },
    ) {
        focus.set(entity, FocusCause::Navigated);
        visible.0 = true;
    }
}

fn synchronize(world: &mut World) {
    let handle = world.resource::<NativeInterfaceHandle>().clone();
    let Ok((context, frame, excluded)) = handle.shared.lock().map(|shared| {
        (
            shared.registry.context().cloned(),
            shared.registry.frame().clone(),
            shared.tab_excluded.clone(),
        )
    }) else {
        return;
    };
    let Some(context) = context else {
        return;
    };
    // A non-UI parent (including Window) removes children from Bevy's UI-root
    // traversal. GhostNode supplies a focus group without changing their layout.
    let focus_root = world
        .query_filtered::<Entity, With<WidgetFocusRoot>>()
        .single(world)
        .unwrap_or_else(|_| {
            world
                .spawn((WidgetFocusRoot, GhostNode, TabGroup::default()))
                .id()
        });
    let cameras = world
        .query_filtered::<Entity, With<InterfaceCamera>>()
        .iter(world)
        .collect::<Vec<_>>();
    for camera in cameras {
        world.entity_mut(camera).insert(UiPickingCamera);
    }
    // Only actual controls and panel backgrounds participate. Labels and glyphs
    // are decorative, so a checkbox square and its row have the same target.
    let blockers = world
        .query_filtered::<Entity, With<InterfaceOccluder>>()
        .iter(world)
        .collect::<Vec<_>>();
    for entity in blockers {
        world.entity_mut(entity).insert(Pickable::default());
    }
    let mut retired = false;
    for (index, published) in frame.controls.iter().enumerate() {
        let entity = Entity::from_bits(published.key.0);
        let Some(control) = world.get::<InterfaceControl>(entity).cloned() else {
            continue;
        };
        let next = kind(&control);
        let previous = world.get::<WidgetKind>(entity).copied();
        let action = handle
            .resolve_input(published.key, ControlInput::Click, &context)
            .ok();
        let eligible = action.is_some();
        let old_action = world.get::<WidgetAction>(entity).map(|a| &a.0);
        if old_action.is_some() && old_action != action.as_ref() {
            retired = true;
        }
        let mut root = entity;
        while let Some(parent) = world.get::<ChildOf>(root) {
            root = parent.parent();
        }
        if root != focus_root {
            world.entity_mut(root).insert(ChildOf(focus_root));
        }
        let mut target = world.entity_mut(entity);
        if eligible {
            target.insert(TabIndex(if excluded.contains(&published.key) {
                -1
            } else {
                index as i32
            }));
        } else {
            target.remove::<TabIndex>();
        }
        target.insert(if target.contains::<InterfacePointerPassthrough>() {
            Pickable::IGNORE
        } else {
            Pickable::default()
        });
        if next != previous {
            target.remove::<(
                WidgetKind,
                Button,
                Checkbox,
                Checkable,
                Checked,
                Pressed,
                InteractionDisabled,
                WidgetAction,
            )>();
            match next {
                Some(WidgetKind::Button) => {
                    target.insert((WidgetKind::Button, Button, Hovered::default()));
                }
                Some(WidgetKind::Checkbox) => {
                    target.insert((WidgetKind::Checkbox, Checkbox, Hovered::default()));
                }
                None => {}
            }
        }
        if next.is_none() {
            continue;
        }
        if let Some(action) = action {
            target.insert(WidgetAction(action));
            target.remove::<InteractionDisabled>();
        } else {
            target.remove::<WidgetAction>();
            target.insert(InteractionDisabled);
        }
        if matches!(control.field, Field::Toggle(true)) {
            target.insert(Checked);
        } else {
            target.remove::<Checked>();
        }
    }
    if retired {
        cancel(world);
    }
    // Application-driven changes (modal open/close, restored document, field
    // editing and accessibility) use the same Bevy focus resource as pointers.
    let next = handle.focused_key().map(|key| Entity::from_bits(key.0));
    let mut focus = world.resource_mut::<InputFocus>();
    if focus.get() != next {
        if let Some(entity) = next {
            focus.set(entity, FocusCause::Navigated);
        } else {
            focus.clear();
        }
    }
}

fn on_activate(
    event: On<Activate>,
    bindings: Query<&WidgetAction>,
    mut pending: ResMut<WidgetActions>,
) {
    if let Ok(action) = bindings.get(event.entity) {
        pending.0.push(action.0.clone());
    }
}

fn on_toggle(
    event: On<bevy::ui_widgets::ValueChange<bool>>,
    bindings: Query<(&WidgetAction, &InterfaceControl)>,
    mut pending: ResMut<WidgetActions>,
) {
    if let Ok((action, control)) = bindings.get(event.source) {
        if event.is_final && matches!(control.field, Field::Toggle(value) if value != event.value) {
            pending.0.push(action.0.clone());
        }
    }
}

/// Reduce after native field edits have been committed. The existing reducer
/// validates the published owner/binding and projects accepted checkbox state.
pub(crate) fn enqueue_actions(world: &mut World, handle: &NativeInterfaceHandle) {
    for action in std::mem::take(&mut world.resource_mut::<WidgetActions>().0) {
        if let Err(error) = handle.enqueue_action(action) {
            eprintln!("Widget action rejected: {error}");
        }
    }
}

pub(crate) fn is_widget(world: &World, entity: Entity) -> bool {
    world.get::<WidgetKind>(entity).is_some()
}

pub(crate) fn cancel(world: &mut World) {
    if let Some(mut state) = world.get_resource_mut::<PointerState>() {
        state.clear(PointerId::Mouse);
    }
    let pressed = world
        .query_filtered::<Entity, (With<WidgetKind>, With<Pressed>)>()
        .iter(world)
        .collect::<Vec<_>>();
    for entity in pressed {
        world.entity_mut(entity).remove::<Pressed>();
    }
}

/// Partition UI and canvas input without recognizing a second widget gesture.
/// The remaining native adapter owns text/ranges, secondary clicks and CAD keys.
pub(crate) fn owns_input(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    state: &mut super::super::winit_host::HostInputState,
    input: &super::super::winit_host::NativeHostInput,
) -> bool {
    let hit = input
        .cursor
        .and_then(|point| handle.hit_key(point.as_dvec2().to_array()))
        .is_some_and(|key| is_widget(world, Entity::from_bits(key.0)));
    match &input.event {
        WindowEvent::MouseButtonInput(event) if event.button == MouseButton::Left => {
            if event.state == ButtonState::Pressed {
                state.widget_drag = hit;
                hit
            } else {
                std::mem::take(&mut state.widget_drag) || hit
            }
        }
        WindowEvent::CursorMoved(_) => hit || state.widget_drag,
        WindowEvent::KeyboardInput(event) => {
            if event.key_code == KeyCode::Tab {
                if input.modifiers.ctrl || input.modifiers.meta || input.modifiers.alt {
                    return false;
                }
                if event.state == ButtonState::Pressed && !event.repeat {
                    if let Err(error) =
                        world.run_system_cached_with(navigate_tab, input.modifiers.shift)
                    {
                        eprintln!("Widget Tab navigation failed: {error}");
                    }
                    if let Err(error) = world.run_system_cached(publish_focus) {
                        eprintln!("Widget focus publication failed: {error}");
                    }
                }
                true
            } else {
                matches!(event.key_code, KeyCode::Enter | KeyCode::Space)
                    && handle
                        .focused_key()
                        .is_some_and(|key| is_widget(world, Entity::from_bits(key.0)))
            }
        }
        WindowEvent::WindowFocused(event) if !event.focused => {
            state.widget_drag = false;
            cancel(world);
            false
        }
        WindowEvent::KeyboardFocusLost(_) => {
            state.widget_drag = false;
            cancel(world);
            false
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests;
