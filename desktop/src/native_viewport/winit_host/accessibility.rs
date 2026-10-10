//! AccessKit projection of the same live native control registry. Proxy entity
//! identities retire when their binding/owner changes, so an old OS action
//! cannot activate a replacement that reuses a retained visual control.

use super::super::interface_shell::{
    InterfaceLayout, NativeInterfaceAction, NativeInterfaceHandle,
};
use accesskit::{Action, ActionData, Node, Role, Toggled};
use bevy::{
    a11y::{AccessibilityNode, AccessibilitySystems, ActionRequest},
    input_focus::{FocusCause, InputFocus},
    prelude::*,
    window::PrimaryWindow,
    winit::{
        accessibility::{WinitActionRequestHandler, WinitActionRequestHandlers},
        EventLoopProxyWrapper, WinitUserEvent,
    },
};
use limo_cad_interface::{ControlKey, Field, Rect as ControlRect};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

#[derive(Component)]
struct AccessibleBinding(Option<NativeInterfaceAction>);

#[derive(Clone, PartialEq)]
struct AccessibleControl {
    key: ControlKey,
    label: String,
    role: String,
    bounds: ControlRect,
    disabled: bool,
    selected: Option<bool>,
    field: Field,
    action: Option<NativeInterfaceAction>,
}

#[derive(Resource, Default)]
struct AccessibleControls {
    controls: HashMap<ControlKey, (Entity, AccessibleControl)>,
    scale: Option<f64>,
}

#[derive(Resource, Default)]
struct ActionWakers(HashMap<Entity, Weak<Mutex<WinitActionRequestHandler>>>);

#[derive(Resource, Default)]
struct EditorFocusProjection(Option<InputFocus>);

pub(super) fn install(app: &mut App) {
    app.init_resource::<InputFocus>()
        .init_resource::<AccessibleControls>()
        .init_resource::<ActionWakers>()
        .init_resource::<EditorFocusProjection>()
        .add_systems(
            PostUpdate,
            publish
                .after(InterfaceLayout)
                .after(bevy::ui::UiSystems::PostLayout)
                .after(bevy::input_focus::InputFocusSystems::FocusChangeEvents)
                .before(AccessibilitySystems::Update),
        )
        .add_systems(
            PostUpdate,
            (watch_action_queues, apply_requests, restore_editor_focus)
                .chain()
                .after(AccessibilitySystems::Update),
        );
}

fn restore_editor_focus(world: &mut World) {
    if let Some(original) = world.resource_mut::<EditorFocusProjection>().0.take() {
        *world.resource_mut::<InputFocus>() = original;
    }
    let Some(handle) = world.get_resource::<NativeInterfaceHandle>() else {
        return;
    };
    let Some(key) = handle.focused_key() else {
        return;
    };
    let entity = Entity::from_bits(key.0);
    if world.get::<bevy::text::EditableText>(entity).is_none()
        || handle.resolve_retained(key).is_err()
    {
        return;
    }
    let mut focus = world.resource_mut::<InputFocus>();
    if focus.get() != Some(entity) {
        focus.set(entity, FocusCause::Navigated);
    }
}

fn watch_action_queues(
    handlers: Option<Res<WinitActionRequestHandlers>>,
    proxy: Option<Res<EventLoopProxyWrapper>>,
    mut watched: ResMut<ActionWakers>,
) {
    let (Some(handlers), Some(proxy)) = (handlers, proxy) else {
        return;
    };
    watched.0.retain(|window, _| handlers.contains_key(window));
    for (window, requests) in handlers.iter() {
        let weak = Arc::downgrade(requests);
        if watched
            .0
            .get(window)
            .is_some_and(|previous| previous.ptr_eq(&weak))
        {
            continue;
        }
        let wake = (**proxy).clone();
        match start_action_waker(requests, move || {
            wake.send_event(WinitUserEvent::WakeUp).is_ok()
        }) {
            Ok(_) => {
                watched.0.insert(*window, weak);
            }
            Err(error) => eprintln!("Native accessibility wake watcher failed: {error}"),
        }
    }
}

fn start_action_waker(
    requests: &Arc<Mutex<WinitActionRequestHandler>>,
    wake: impl Fn() -> bool + Send + 'static,
) -> std::io::Result<std::thread::JoinHandle<()>> {
    let requests = Arc::downgrade(requests);
    std::thread::Builder::new()
        .name("native-a11y-wake".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_millis(40));
            let Some(requests) = requests.upgrade() else {
                break;
            };
            let Ok(pending) = requests.lock().map(|queue| !queue.is_empty()) else {
                break;
            };
            if pending && !wake() {
                break;
            }
        })
}

fn publish(world: &mut World) {
    let Some(handle) = world.get_resource::<NativeInterfaceHandle>().cloned() else {
        return;
    };
    let scale = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(world)
        .map(|window| f64::from(window.scale_factor()))
        .unwrap_or(1.0)
        * f64::from(handle.presented_ui_scale());
    let snapshot = handle.read_surface(|_, frame| {
        let modal = frame.modal_stack.last();
        (
            frame.focused,
            frame
                .controls
                .iter()
                .filter(|control| {
                    control.visible
                        && modal.is_none_or(|scope| control.modal_scope.as_ref() == Some(scope))
                })
                .map(|control| AccessibleControl {
                    key: control.key,
                    label: control.label.clone(),
                    role: control.role.clone(),
                    bounds: control.bounds,
                    disabled: control.disabled,
                    selected: control.selected,
                    field: control.field.clone(),
                    action: None,
                })
                .collect::<Vec<_>>(),
        )
    });
    let (focused, mut controls) = snapshot.unwrap_or_default();
    for control in &mut controls {
        if !control.disabled {
            control.action = handle.resolve_retained(control.key).ok();
        }
    }
    let scale_changed = world.resource::<AccessibleControls>().scale != Some(scale);
    let mut previous = std::mem::take(&mut world.resource_mut::<AccessibleControls>().controls);
    let mut current = HashMap::new();
    let mut next_focus = None;
    for control in controls {
        let old = previous.remove(&control.key);
        let retained = old
            .as_ref()
            .filter(|(_, old)| old.action == control.action && old.role == control.role);
        let unchanged = !scale_changed && retained.is_some_and(|(_, old)| old == &control);
        let entity = if let Some((entity, _)) = retained {
            *entity
        } else {
            if let Some((entity, _)) = old {
                world.despawn(entity);
            }
            world.spawn_empty().id()
        };
        if focused == Some(control.key) && control.action.is_some() {
            next_focus = Some(entity);
        }
        if unchanged {
            current.insert(control.key, (entity, control));
            continue;
        }
        let mut node = Node::new(match control.role.as_str() {
            "tab" => Role::Tab,
            "treeitem" => Role::TreeItem,
            "menuitem" => Role::MenuItem,
            "checkbox" => Role::CheckBox,
            "radio" => Role::RadioButton,
            "slider" => Role::Slider,
            "textbox" => Role::TextInput,
            "multiline_textbox" => Role::MultilineTextInput,
            _ => Role::Button,
        });
        node.set_label(control.label.clone());
        node.set_bounds(accesskit::Rect::new(
            control.bounds.x * scale,
            control.bounds.y * scale,
            (control.bounds.x + control.bounds.width) * scale,
            (control.bounds.y + control.bounds.height) * scale,
        ));
        if control.disabled {
            node.set_disabled();
        }
        if matches!(control.role.as_str(), "radio" | "checkbox") {
            let checked = match control.field {
                Field::Toggle(value) => Some(value),
                _ => control.selected,
            };
            if let Some(checked) = checked {
                node.set_toggled(if checked {
                    Toggled::True
                } else {
                    Toggled::False
                });
            }
        } else if let Some(selected) = control.selected {
            node.set_selected(selected);
        }
        if let Field::Text {
            value, read_only, ..
        } = &control.field
        {
            node.set_value(value.clone());
            if *read_only {
                node.set_read_only();
            } else if !control.disabled {
                node.add_action(Action::SetValue);
            }
        }
        if let Field::Range {
            value,
            min,
            max,
            step,
        } = control.field
        {
            node.set_numeric_value(value);
            node.set_min_numeric_value(min);
            node.set_max_numeric_value(max);
            node.set_numeric_value_step(step);
            if !control.disabled {
                node.add_action(Action::SetValue);
                node.add_action(Action::Increment);
                node.add_action(Action::Decrement);
            }
        }
        if control.action.is_some() {
            node.add_action(Action::Click);
            node.add_action(Action::Focus);
        }
        let changed = world
            .get::<AccessibilityNode>(entity)
            .is_none_or(|existing| existing.0 != node);
        if changed {
            world.entity_mut(entity).insert(AccessibilityNode(node));
        }
        world
            .entity_mut(entity)
            .insert(AccessibleBinding(control.action.clone()));
        current.insert(control.key, (entity, control));
    }
    for (_, (entity, _)) in previous {
        world.despawn(entity);
    }
    let mut published = world.resource_mut::<AccessibleControls>();
    published.controls = current;
    published.scale = Some(scale);
    if next_focus.is_none()
        && world
            .resource::<InputFocus>()
            .get()
            .is_some_and(|entity| world.get::<bevy::ui_widgets::TextInput>(entity).is_some())
    {
        return;
    }
    let actual_focus = world.resource::<InputFocus>().get();
    if actual_focus.is_some_and(|entity| {
        focused == Some(ControlKey(entity.to_bits()))
            && world.get::<bevy::text::EditableText>(entity).is_some()
    }) && next_focus.is_some()
        && next_focus != actual_focus
    {
        let original = world.resource::<InputFocus>().clone();
        world.resource_mut::<EditorFocusProjection>().0 = Some(original);
    }
    let mut focus = world.resource_mut::<InputFocus>();
    if focus.get() != next_focus {
        if let Some(entity) = next_focus {
            focus.set(entity, FocusCause::Navigated);
        } else {
            focus.clear();
        }
    }
}

fn apply_requests(
    mut requests: MessageReader<ActionRequest>,
    bindings: Query<&AccessibleBinding>,
    handle: Res<NativeInterfaceHandle>,
) {
    for request in requests.read() {
        if request.target_tree != accesskit::TreeId::ROOT {
            continue;
        }
        let Some(entity) = Entity::try_from_bits(request.target_node.0) else {
            continue;
        };
        let Ok(AccessibleBinding(Some(action))) = bindings.get(entity) else {
            continue;
        };
        let edit = match (&request.action, &request.data) {
            (Action::SetValue, Some(ActionData::Value(value))) => Some(
                limo_cad_interface::ControlInput::SetValue(value.to_string()),
            ),
            (Action::SetValue, Some(ActionData::NumericValue(value))) => Some(
                limo_cad_interface::ControlInput::SetValue(value.to_string()),
            ),
            (Action::Increment, _) => Some(limo_cad_interface::ControlInput::Key(
                limo_cad_interface::KeyChord::plain("ArrowRight"),
            )),
            (Action::Decrement, _) => Some(limo_cad_interface::ControlInput::Key(
                limo_cad_interface::KeyChord::plain("ArrowLeft"),
            )),
            _ => None,
        };
        if let Some(edit) = edit {
            if let Err(error) = handle.assistive_edit(action, edit) {
                eprintln!("Native accessibility edit rejected: {error}");
            }
            continue;
        }
        let activate = match request.action {
            Action::Click => true,
            Action::Focus => false,
            _ => continue,
        };
        if let Err(error) = handle.assistive_action(action, activate) {
            eprintln!("Native accessibility action rejected: {error}");
        }
    }
}

#[cfg(test)]
mod widget_focus_tests {
    use super::*;

    #[test]
    fn publishing_guarded_accessibility_nodes_preserves_standard_widget_text_focus() {
        let (mut app, handle, _, _) = super::super::super::interface_shell::tests::fixture();
        app.init_resource::<InputFocus>()
            .init_resource::<AccessibleControls>();
        let field = app.world_mut().spawn(bevy::ui_widgets::TextInput).id();
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(field, FocusCause::Pressed);
        publish(app.world_mut());
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(field));
        assert!(handle.focused_key().is_none());
    }

    #[test]
    fn native_editor_focus_is_real_during_layout_and_render_and_guarded_during_accesskit() {
        let (mut app, handle, field, _) = super::super::super::interface_shell::tests::fixture();
        app.add_message::<ActionRequest>();
        install(&mut app);
        app.world_mut()
            .entity_mut(field)
            .insert(bevy::text::EditableText::new("12 mm"));
        let key = ControlKey(field.to_bits());
        handle
            .prepare_activation(&handle.resolve_retained(key).unwrap())
            .unwrap();
        app.world_mut()
            .resource_mut::<InputFocus>()
            .set(field, FocusCause::Pressed);
        let losses = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = losses.clone();
        app.add_observer(move |event: On<bevy::input_focus::FocusLost>| {
            if event.entity == field {
                counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        });
        app.add_systems(
            PostUpdate,
            bevy::input_focus::process_recorded_focus_changes
                .in_set(bevy::input_focus::InputFocusSystems::FocusChangeEvents),
        );
        app.add_systems(
            PostUpdate,
            (move |focus: Res<InputFocus>| {
                assert_eq!(
                    focus.get(),
                    Some(field),
                    "Text layout needs the editable entity"
                );
            })
            .in_set(bevy::ui::UiSystems::PostLayout),
        );
        app.add_systems(
            PostUpdate,
            (move |focus: Res<InputFocus>, controls: Res<AccessibleControls>| {
                let proxy = controls.controls[&key].0;
                assert_ne!(proxy, field, "OS actions keep their generational proxy");
                assert_eq!(
                    focus.get(),
                    Some(proxy),
                    "AccessKit must publish the proxy NodeId"
                );
            })
            .in_set(AccessibilitySystems::Update),
        );
        for _ in 0..2 {
            app.update();
            assert_eq!(
                app.world().resource::<InputFocus>().get(),
                Some(field),
                "Render extraction needs the editable entity after AccessKit publication"
            );
        }
        assert_eq!(
            losses.load(std::sync::atomic::Ordering::Relaxed),
            0,
            "AccessKit proxy publication must not blur the editor"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_viewport::interface_shell::{tests::fixture, InterfaceControl};

    #[test]
    fn assistive_input_wakes_an_idle_host_without_consuming_or_polling_frames() {
        let requests = Arc::new(Mutex::new(WinitActionRequestHandler::default()));
        let (send, receive) = std::sync::mpsc::channel();
        let watcher = start_action_waker(&requests, move || send.send(()).is_ok()).unwrap();
        assert!(matches!(
            receive.recv_timeout(Duration::from_millis(120)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        let request = accesskit::ActionRequest {
            action: Action::Click,
            target_tree: accesskit::TreeId::ROOT,
            target_node: accesskit::NodeId(17),
            data: None,
        };
        requests.lock().unwrap().push_back(request);
        receive
            .recv_timeout(Duration::from_secs(2))
            .expect("Idle assistive input must wake Winit");
        assert_eq!(
            requests.lock().unwrap().pop_front().unwrap().target_node,
            accesskit::NodeId(17)
        );
        drop(requests);
        watcher.join().unwrap();
    }

    #[test]
    fn native_field_states_are_exposed_to_assistive_technology() {
        let (mut app, _, control, _) = fixture();
        app.add_message::<ActionRequest>();
        install(&mut app);
        let key = ControlKey(control.to_bits());
        let read = |app: &App| {
            let proxy = app.world().resource::<AccessibleControls>().controls[&key].0;
            app.world()
                .get::<AccessibilityNode>(proxy)
                .unwrap()
                .0
                .clone()
        };
        {
            let mut widget = app
                .world_mut()
                .get_mut::<InterfaceControl>(control)
                .unwrap();
            widget.role = "radio".into();
            widget.selected = Some(true);
        }
        app.update();
        assert_eq!(read(&app).role(), Role::RadioButton);
        assert_eq!(read(&app).toggled(), Some(Toggled::True));
        app.world_mut()
            .get_mut::<InterfaceControl>(control)
            .unwrap()
            .selected = Some(false);
        app.update();
        assert_eq!(read(&app).toggled(), Some(Toggled::False));
        {
            let mut widget = app
                .world_mut()
                .get_mut::<InterfaceControl>(control)
                .unwrap();
            widget.role = "checkbox".into();
            widget.selected = None;
            widget.field = Field::Toggle(true);
        }
        app.update();
        assert_eq!(read(&app).role(), Role::CheckBox);
        assert_eq!(read(&app).toggled(), Some(Toggled::True));
        {
            let mut widget = app
                .world_mut()
                .get_mut::<InterfaceControl>(control)
                .unwrap();
            widget.role = "textbox".into();
            widget.field = Field::Text {
                value: "12 mm".into(),
                read_only: true,
                selection: None,
            };
        }
        app.update();
        assert_eq!(read(&app).value(), Some("12 mm"));
        assert!(read(&app).is_read_only());
        assert!(!read(&app).supports_action(Action::SetValue));
        assert_eq!(read(&app).toggled(), None);
        {
            let mut widget = app
                .world_mut()
                .get_mut::<InterfaceControl>(control)
                .unwrap();
            widget.field = Field::Text {
                value: "12 mm".into(),
                read_only: false,
                selection: None,
            };
        }
        app.update();
        assert!(read(&app).supports_action(Action::SetValue));
        {
            let mut widget = app
                .world_mut()
                .get_mut::<InterfaceControl>(control)
                .unwrap();
            widget.role = "slider".into();
            widget.field = Field::Range {
                value: 2.,
                min: 0.,
                max: 5.,
                step: 0.1,
            };
        }
        app.update();
        assert_eq!(read(&app).role(), Role::Slider);
        assert_eq!(read(&app).numeric_value(), Some(2.));
        assert_eq!(read(&app).min_numeric_value(), Some(0.));
        assert_eq!(read(&app).max_numeric_value(), Some(5.));
        assert!(read(&app).supports_action(Action::SetValue));
    }

    #[test]
    fn assistive_actions_use_live_controls_and_old_proxy_ids_retire_on_rebind() {
        let (mut app, handle, control, _) = fixture();
        app.add_message::<ActionRequest>();
        install(&mut app);
        app.update();
        let key = ControlKey(control.to_bits());
        let proxy = app.world().resource::<AccessibleControls>().controls[&key].0;
        let send = |app: &mut App, target: Entity| {
            app.world_mut()
                .write_message(ActionRequest(accesskit::ActionRequest {
                    action: Action::Click,
                    target_tree: accesskit::TreeId::ROOT,
                    target_node: accesskit::NodeId(target.to_bits()),
                    data: None,
                }));
        };
        send(&mut app, proxy);
        app.update();
        let action = handle.take_actions().unwrap().pop().unwrap();
        assert_eq!(action.control.key, key);
        assert_eq!(action.control.binding(), 1);
        send(&mut app, proxy);
        app.world_mut()
            .get_mut::<InterfaceControl>(control)
            .unwrap()
            .binding = 2;
        app.update();
        assert!(handle.take_actions().unwrap().is_empty());
        let replacement = app.world().resource::<AccessibleControls>().controls[&key].0;
        assert_ne!(proxy, replacement);
        assert!(app.world().get_entity(proxy).is_err());
        send(&mut app, replacement);
        app.update();
        assert_eq!(handle.take_actions().unwrap()[0].control.binding(), 2);
        let retained = app.world().resource::<AccessibleControls>().controls[&key].0;
        app.update();
        assert_eq!(
            app.world().resource::<AccessibleControls>().controls[&key].0,
            retained
        );
    }
}
