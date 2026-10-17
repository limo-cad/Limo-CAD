//! Standard Bevy picking, focus and control behavior, with CAD commands
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
    ui_widgets::{
        Activate, ActivateOnPress, Button, ButtonPlugin, Checkbox, CheckboxPlugin, MenuButton,
        MenuItem, MenuPlugin, RadioButton, RadioGroupPlugin, Slider, SliderDragState,
        SliderOrientation, SliderPlugin, SliderPrecision, SliderRange, SliderStep, SliderValue,
        Tab, TabPlugin, TrackClick, ValueChange,
    },
    window::WindowEvent,
};

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum WidgetKind {
    Button,
    Checkbox,
    Radio,
    Slider,
    Tab,
    MenuButton,
    MenuItem,
}

mod choices;
mod selection;
pub(crate) use selection::{choice_group, preserves_parent, radio_group, tab_group};

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
    super::dialogs::install(app);
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
    if !app.is_plugin_added::<RadioGroupPlugin>() {
        app.add_plugins(RadioGroupPlugin);
    }
    if !app.is_plugin_added::<SliderPlugin>() {
        app.add_plugins(SliderPlugin);
    }
    if !app.is_plugin_added::<TabPlugin>() {
        app.add_plugins(TabPlugin);
    }
    if !app.is_plugin_added::<MenuPlugin>() {
        app.add_plugins(MenuPlugin);
    }
    choices::install(app);
    app.init_resource::<WidgetActions>()
        .insert_resource(UiPickingSettings {
            require_markers: true,
        })
        .add_observer(on_activate)
        .add_observer(on_toggle)
        .add_observer(on_range)
        .add_observer(on_tab)
        .add_observer(on_tab_double_click)
        .add_systems(First, primary_pointer.in_set(PickingSystems::PostInput))
        .configure_sets(
            PreUpdate,
            InputFocusSystems::Dispatch.after(PickingSystems::Last),
        )
        .add_systems(PreUpdate, publish_focus.after(InputFocusSystems::Dispatch))
        .add_systems(
            PostUpdate,
            (choices::publish_focus, synchronize)
                .chain()
                .after(bevy::ui_widgets::MenuFocusSystem)
                .in_set(WidgetLayout)
                .after(InterfaceLayout)
                .before(bevy::a11y::AccessibilitySystems::Update),
        );
}

fn kind(world: &World, entity: Entity, control: &InterfaceControl) -> Option<WidgetKind> {
    if control.text_editing {
        return None;
    }
    match (&control.field, control.role.as_str()) {
        (Field::Toggle(_), "checkbox" | "button") => Some(WidgetKind::Checkbox),
        (Field::None, "button" | "menuitem") => Some(WidgetKind::Button),
        (Field::None, "radio") => Some(WidgetKind::Radio),
        (Field::None, "tab")
            if world.get::<ChildOf>(entity).is_some_and(|parent| {
                world
                    .get::<bevy::ui_widgets::TabList>(parent.parent())
                    .is_some()
            }) =>
        {
            Some(WidgetKind::Tab)
        }
        (Field::None, "option") => Some(WidgetKind::MenuItem),
        (Field::Range { .. }, "slider") if range_steps(&control.field).is_some() => {
            Some(WidgetKind::Slider)
        }
        (Field::Choice { .. }, "combobox") if control.expanded.is_some() => {
            Some(WidgetKind::MenuButton)
        }
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
        let next = kind(world, entity, &control);
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
        if eligible && next != Some(WidgetKind::Tab) {
            target.insert(TabIndex(if excluded.contains(&published.key) {
                -1
            } else {
                index as i32
            }));
        } else if next != Some(WidgetKind::Tab) {
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
            target.remove::<(
                RadioButton,
                Slider,
                SliderValue,
                SliderRange,
                SliderStep,
                SliderPrecision,
                SliderDragState,
                Tab,
                bevy::ui::Selected,
                bevy::ui::Selectable,
                MenuButton,
                MenuItem,
                ActivateOnPress,
            )>();
            match next {
                Some(WidgetKind::Button) => {
                    target.insert((WidgetKind::Button, Button, Hovered::default()));
                }
                Some(WidgetKind::Checkbox) => {
                    target.insert((WidgetKind::Checkbox, Checkbox, Hovered::default()));
                }
                Some(WidgetKind::Radio) => {
                    target.insert((WidgetKind::Radio, RadioButton, Hovered::default()));
                }
                Some(WidgetKind::Slider) => {
                    target.insert((
                        WidgetKind::Slider,
                        Slider {
                            track_click: TrackClick::Snap,
                            orientation: SliderOrientation::Horizontal,
                        },
                        SliderStep(1.),
                        SliderPrecision(0),
                        Hovered::default(),
                    ));
                }
                Some(WidgetKind::Tab) => {
                    target.insert((WidgetKind::Tab, Tab, Hovered::default()));
                }
                Some(WidgetKind::MenuButton) => {
                    target.insert((WidgetKind::MenuButton, MenuButton, Hovered::default()));
                }
                Some(WidgetKind::MenuItem) => {
                    target.insert((WidgetKind::MenuItem, MenuItem, Hovered::default()));
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
        if matches!(control.field, Field::Toggle(true))
            || (next == Some(WidgetKind::Radio) && control.selected == Some(true))
        {
            target.insert(Checked);
        } else {
            target.remove::<Checked>();
        }
        if let (Some(WidgetKind::Slider), Some(steps)) = (next, range_steps(&control.field)) {
            let Field::Range {
                value, min, step, ..
            } = control.field
            else {
                unreachable!()
            };
            let range = SliderRange::new(0., steps);
            let value = SliderValue(((value - min) / step) as f32);
            if target.get::<SliderRange>() != Some(&range) {
                target.insert(range);
            }
            if target.get::<SliderValue>() != Some(&value) {
                target.insert(value);
            }
        }
    }
    selection::synchronize(world);
    choices::activate_ready(world);
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
    bindings: Query<(&WidgetAction, &WidgetKind)>,
    parents: Query<&ChildOf>,
    popups: Query<&choices::ChoicePopup>,
    mut menus: ResMut<choices::MenuRequests>,
    mut pending: ResMut<WidgetActions>,
) {
    if let Ok((action, kind)) = bindings.get(event.entity) {
        if *kind == WidgetKind::MenuButton {
            return;
        }
        if let Some(popup) = parents
            .iter_ancestors(event.entity)
            .find_map(|e| popups.get(e).ok())
        {
            menus.selected.insert(popup.0);
        }
        pending.0.push(action.0.clone());
    }
}

fn on_toggle(
    event: On<bevy::ui_widgets::ValueChange<bool>>,
    bindings: Query<(&WidgetAction, &InterfaceControl, &WidgetKind)>,
    mut pending: ResMut<WidgetActions>,
) {
    if let Ok((action, control, kind)) = bindings.get(event.source) {
        let changed = match kind {
            WidgetKind::Radio => event.value && control.selected != Some(true),
            WidgetKind::Checkbox => {
                matches!(control.field, Field::Toggle(value) if value != event.value)
            }
            _ => false,
        };
        if event.is_final && changed {
            pending.0.push(action.0.clone());
        }
    }
}

/// Represent the slider as step indices. CAD values stay f64, including decimal
/// steps and non-zero origins; no dimensional value is narrowed to f32.
fn range_steps(field: &Field) -> Option<f32> {
    let Field::Range {
        value,
        min,
        max,
        step,
    } = *field
    else {
        return None;
    };
    let steps = (max - min) / step;
    (value.is_finite()
        && min.is_finite()
        && max.is_finite()
        && step.is_finite()
        && step > 0.
        && steps > 0.
        && steps <= 1_048_576.)
        .then_some(steps as f32)
}

fn on_range(
    event: On<ValueChange<f32>>,
    bindings: Query<(&WidgetAction, &InterfaceControl), With<Slider>>,
    mut pending: ResMut<WidgetActions>,
) {
    let Ok((action, control)) = bindings.get(event.source) else {
        return;
    };
    let Some(steps) = range_steps(&control.field) else {
        return;
    };
    let Field::Range { min, max, step, .. } = control.field else {
        return;
    };
    if !event.value.is_finite() {
        return;
    }
    let value = if event.value >= steps {
        max
    } else {
        (min + f64::from(event.value.round()) * step).clamp(min, max)
    };
    let mut action = action.0.clone();
    action.control.input = ControlInput::SetValue(value.to_string());
    if pending.0.last().is_some_and(|last| {
        last.context == action.context
            && last.control.key == action.control.key
            && last.control.binding() == action.control.binding()
            && matches!(last.control.input, ControlInput::SetValue(_))
    }) {
        pending.0.pop();
    }
    pending.0.push(action);
}

fn on_tab(
    event: On<ValueChange<Option<Entity>>>,
    bindings: Query<&WidgetAction, With<Tab>>,
    mut pending: ResMut<WidgetActions>,
) {
    if let Some(entity) = event.value {
        if let Ok(action) = bindings.get(entity) {
            pending.0.push(action.0.clone());
        }
    }
}

// Document-tab renaming is an application action. Bevy supplies the click count.
fn on_tab_double_click(
    event: On<bevy::picking::events::PointerClick>,
    bindings: Query<(&WidgetAction, &InterfaceControl), With<Tab>>,
    mut pending: ResMut<WidgetActions>,
) {
    if event.button != BevyPointerButton::Primary || event.count != 2 {
        return;
    }
    if let Ok((action, control)) = bindings.get(event.entity) {
        if control.surface == "document/session" && control.selected == Some(true) {
            let mut action = action.0.clone();
            action.control.input = ControlInput::DoubleClick;
            pending.0.push(action);
        }
    }
}

/// Reduce after native field edits have been committed. The existing reducer
/// validates the published owner/binding and projects accepted checkbox state.
pub(crate) fn enqueue_actions(world: &mut World, handle: &NativeInterfaceHandle) {
    choices::enqueue(world);
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
    if let Some(mut captures) = world.get_resource_mut::<bevy::picking::hover::PointerCaptureMap>()
    {
        captures.release(PointerId::Mouse);
    }
    for mut drag in world.query::<&mut SliderDragState>().iter_mut(world) {
        drag.dragging = false;
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
/// The remaining native adapter owns text, unmigrated choices, secondary clicks and CAD keys.
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
                handle
                    .focused_key()
                    .and_then(|key| world.get::<WidgetKind>(Entity::from_bits(key.0)))
                    .is_some_and(|kind| match event.key_code {
                        KeyCode::Enter | KeyCode::Space => *kind != WidgetKind::Slider,
                        KeyCode::ArrowLeft | KeyCode::ArrowRight | KeyCode::Home | KeyCode::End => {
                            matches!(
                                kind,
                                WidgetKind::Slider
                                    | WidgetKind::Radio
                                    | WidgetKind::Tab
                                    | WidgetKind::MenuButton
                                    | WidgetKind::MenuItem
                            )
                        }
                        KeyCode::ArrowUp | KeyCode::ArrowDown => matches!(
                            kind,
                            WidgetKind::Radio | WidgetKind::MenuButton | WidgetKind::MenuItem
                        ),
                        KeyCode::Escape => {
                            *kind == WidgetKind::MenuItem
                                || (*kind == WidgetKind::MenuButton
                                    && handle
                                        .focused_key()
                                        .and_then(|key| {
                                            world.get::<InterfaceControl>(Entity::from_bits(key.0))
                                        })
                                        .is_some_and(|control| control.expanded == Some(true)))
                        }
                        _ => false,
                    })
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
