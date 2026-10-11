//! Main-window host for the native Bevy application interface.
//!
//! This uses the product's existing CAD scene and typed control reducer. Winit
//! owns the main thread, OS window, input ordering, IME and AccessKit adapter;
//! there is no second document engine or alternative public command mode.

use std::{
    collections::HashSet,
    num::NonZeroU32,
    time::{Duration, Instant},
};

use bevy::{
    ecs::message::MessageCursor,
    input::{
        keyboard::{Key, KeyCode, KeyboardInput},
        ButtonState,
    },
    prelude::*,
    window::{ExitCondition, PrimaryWindow, WindowEvent, WindowResolution},
    winit::{
        EventLoopProxyWrapper, RawWinitWindowEvent, UpdateMode, WinitSettings, WinitUserEvent,
    },
};
use limo_cad_interface::{ControlKey, DocumentContext, KeyChord};

use super::{
    interface_shell::{NativeInterfaceAction, NativeInterfaceHandle, PointerButton, PointerPhase},
    platform,
};

mod accessibility;
#[cfg(feature = "dev-native-ime-trace")]
mod ime_trace;
mod submission;
#[cfg(any(target_os = "windows", target_os = "linux"))]
mod window_icon;
pub(crate) mod window_theme;

/// Run the native desktop host. Startup prepares the always-on stdio worker
/// before entering this loop.
pub fn run() -> std::process::ExitCode {
    run_with_recipe(None)
}

/// Recipe URLs enter the same source-only queue as a warm MCP delivery.
pub fn run_with_recipe(recipe: Option<&str>) -> std::process::ExitCode {
    run_with_startup(recipe, None)
}

/// Project launches enter the same guarded File workflow as UI and MCP opens.
pub fn run_with_startup(
    recipe: Option<&str>,
    project: Option<&std::path::Path>,
) -> std::process::ExitCode {
    use crate::session_bridge::native_interface::controller::{self, NativeServices};
    use std::process::Termination;
    build(|app, handle| {
        controller::install(app, handle, NativeServices::default(), "main".into(), None);
        crate::recipe_links::install(app);
        if let Some(recipe) = recipe {
            controller::open_startup_recipe(app.world_mut(), recipe);
        }
        if let Some(project) = project {
            controller::open_startup_project(app.world_mut(), project.to_path_buf());
        }
    })
    .run()
    .report()
}

/// Unhandled model/editor/lifecycle events retain OS ordering, their cursor
/// position and document ownership at receipt. A controller must check that
/// ownership before applying an event after a project transition. IME preedit
/// and commit remain distinct original events, never synthesized key presses.
#[derive(Message, Clone, Debug)]
pub(crate) struct NativeHostInput {
    pub ui_scale: f32,
    pub context: Option<DocumentContext>,
    pub cursor: Option<Vec2>,
    pub modifiers: Modifiers,
    pub event: WindowEvent,
    pub consumed: bool,
    pub actions: Vec<NativeInterfaceAction>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Modifiers {
    pub ctrl: bool,
    pub meta: bool,
    pub alt: bool,
    pub shift: bool,
    pub alt_graph: bool,
}

#[derive(Resource, Default)]
pub(crate) struct HostInputState {
    cursor: Option<Vec2>,
    pressed: HashSet<KeyCode>,
    model_drag: HashSet<MouseButton>,
    alt_graph: bool,
    click_press: Option<ControlKey>,
    last_click: Option<(Instant, ControlKey, [f64; 2], DocumentContext)>,
}

pub(crate) fn model_pointer_active(world: &World) -> bool {
    world
        .get_resource::<HostInputState>()
        .is_some_and(|state| !state.model_drag.is_empty())
}

#[derive(Resource, Default)]
struct HostCaptureState {
    events: MessageCursor<WindowEvent>,
    raw_events: MessageCursor<RawWinitWindowEvent>,
    raw_modifiers: Modifiers,
    cursor: Option<Vec2>,
    modifiers: HostInputState,
}

#[derive(Clone, PartialEq)]
enum ModifiedInput {
    Keyboard(KeyboardInput),
    Button(MouseButton, ButtonState),
    Cursor,
    Wheel,
}

/// Winit's modifier changes precede input, but Bevy reconciles missing
/// modifier presses at the end of a batch. Preserve the original snapshots
/// for typing, selection, and camera input after focusing a window.
fn ordered_modifier_snapshots(
    state: &mut Modifiers,
    window: Entity,
    events: impl IntoIterator<Item = winit::event::WindowEvent>,
) -> Vec<(ModifiedInput, Modifiers)> {
    let mut snapshots = Vec::new();
    for event in events {
        match event {
            winit::event::WindowEvent::ModifiersChanged(modifiers) => {
                let modifiers = modifiers.state();
                state.ctrl = modifiers.control_key();
                state.meta = modifiers.super_key();
                state.alt = modifiers.alt_key();
                state.shift = modifiers.shift_key();
            }
            winit::event::WindowEvent::KeyboardInput {
                event,
                is_synthetic: false,
                ..
            } => {
                let input = bevy::winit::converters::convert_keyboard_input(&event, window);
                if input.logical_key == Key::AltGraph {
                    state.alt_graph = input.state == ButtonState::Pressed;
                }
                snapshots.push((ModifiedInput::Keyboard(input), *state));
            }
            winit::event::WindowEvent::MouseInput {
                button,
                state: pressed,
                ..
            } => snapshots.push((
                ModifiedInput::Button(
                    bevy::winit::converters::convert_mouse_button(button),
                    bevy::winit::converters::convert_element_state(pressed),
                ),
                *state,
            )),
            winit::event::WindowEvent::CursorMoved { .. } => {
                snapshots.push((ModifiedInput::Cursor, *state))
            }
            winit::event::WindowEvent::MouseWheel { .. } => {
                snapshots.push((ModifiedInput::Wheel, *state))
            }
            winit::event::WindowEvent::Focused(false) => *state = Modifiers::default(),
            _ => {}
        }
    }
    snapshots
}

fn take_ordered_modifiers(
    snapshots: &mut Vec<(ModifiedInput, Modifiers)>,
    event: &WindowEvent,
) -> Option<Modifiers> {
    let key = match event {
        WindowEvent::KeyboardInput(input) => ModifiedInput::Keyboard(input.clone()),
        WindowEvent::MouseButtonInput(input) => ModifiedInput::Button(input.button, input.state),
        WindowEvent::CursorMoved(_) => ModifiedInput::Cursor,
        WindowEvent::MouseWheel(_) => ModifiedInput::Wheel,
        _ => return None,
    };
    let index = snapshots
        .iter()
        .position(|(original, _)| *original == key)?;
    Some(snapshots.remove(index).1)
}

#[derive(Resource, Default)]
pub(crate) struct NativeRenderAvailability {
    pub drawable: bool,
    pub focused: bool,
    occluded: bool,
}

impl HostInputState {
    fn modifiers(&self) -> Modifiers {
        Modifiers {
            ctrl: self.pressed.contains(&KeyCode::ControlLeft)
                || self.pressed.contains(&KeyCode::ControlRight),
            meta: self.pressed.contains(&KeyCode::SuperLeft)
                || self.pressed.contains(&KeyCode::SuperRight),
            alt: self.pressed.contains(&KeyCode::AltLeft)
                || self.pressed.contains(&KeyCode::AltRight),
            shift: self.pressed.contains(&KeyCode::ShiftLeft)
                || self.pressed.contains(&KeyCode::ShiftRight),
            alt_graph: self.alt_graph,
        }
    }
}

/// Clear a completed/rejected atomic gesture without synthesizing a release
/// that could activate a control under its last cursor position.
pub(crate) fn cancel_native_pointer(world: &mut World, handle: &NativeInterfaceHandle) {
    if let Some(mut state) = world.get_resource_mut::<HostInputState>() {
        state.model_drag.clear();
        state.click_press = None;
        state.last_click = None;
    }
    handle.cancel_pointer();
}

/// Configure the controller before starting this one product's native runner.
/// Keeping document services outside the host also permits deterministic
/// controller tests without initializing an OS event loop or GPU.
pub(crate) fn build(configure: impl FnOnce(&mut App, NativeInterfaceHandle)) -> App {
    let mut app = App::new();
    let plugins = DefaultPlugins
        .set(WindowPlugin {
            primary_window: Some(Window {
                title: "Limo CAD".into(),
                name: Some("limo-cad".into()),
                resolution: WindowResolution::new(1360, 860),
                resize_constraints: bevy::window::WindowResizeConstraints {
                    min_width: 1200.,
                    min_height: 760.,
                    ..default()
                },
                present_mode: bevy::window::PresentMode::Fifo,
                desired_maximum_frame_latency: NonZeroU32::new(2),
                ime_enabled: false,
                ..default()
            }),
            exit_condition: ExitCondition::DontExit,
            close_when_requested: false,
            ..default()
        })
        .set(platform::cad_render_plugin());
    #[cfg(target_os = "linux")]
    let plugins = plugins.disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>();
    #[cfg(feature = "dev-native-ime-trace")]
    let plugins = if ime_trace::enabled() {
        plugins.set(ime_trace::plugin())
    } else {
        plugins.disable::<bevy::log::LogPlugin>()
    };
    app.add_plugins(plugins);
    window_theme::install(&mut app);
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    window_icon::install(&mut app);
    let wake = (**app.world().resource::<EventLoopProxyWrapper>()).clone();
    let handle = NativeInterfaceHandle::new(move || {
        let _ = wake.send_event(WinitUserEvent::WakeUp);
    });
    app.insert_resource(WinitSettings {
        focused_mode: UpdateMode::reactive_low_power(Duration::MAX),
        unfocused_mode: UpdateMode::reactive_low_power(Duration::MAX),
    });
    platform::install_native_scene(&mut app);
    app.init_resource::<HostInputState>()
        .init_resource::<HostCaptureState>()
        .init_resource::<NativeRenderAvailability>()
        .add_message::<NativeHostInput>()
        .add_systems(PreUpdate, route_window_input);
    accessibility::install(&mut app);
    submission::install(&mut app);
    configure(&mut app, handle);
    super::interface_shell::fields::install(&mut app);
    super::interface_shell::ranges::install(&mut app);
    app
}

fn key_chord(input: &KeyboardInput, modifiers: Modifiers) -> KeyChord {
    let key = match &input.logical_key {
        Key::Character(value) => value.to_string(),
        Key::Space => " ".into(),
        other => format!("{other:?}"),
    };
    KeyChord {
        key,
        ctrl: modifiers.ctrl,
        meta: modifiers.meta,
        alt: modifiers.alt,
        shift: modifiers.shift,
    }
}

fn input_window(event: &WindowEvent) -> Option<Entity> {
    match event {
        WindowEvent::CursorMoved(event) => Some(event.window),
        WindowEvent::CursorEntered(event) => Some(event.window),
        WindowEvent::CursorLeft(event) => Some(event.window),
        WindowEvent::MouseButtonInput(event) => Some(event.window),
        WindowEvent::MouseWheel(event) => Some(event.window),
        WindowEvent::KeyboardInput(event) => Some(event.window),
        WindowEvent::WindowFocused(event) => Some(event.window),
        WindowEvent::WindowCloseRequested(event) => Some(event.window),
        WindowEvent::WindowDestroyed(event) => Some(event.window),
        WindowEvent::WindowResized(event) => Some(event.window),
        WindowEvent::WindowScaleFactorChanged(event) => Some(event.window),
        WindowEvent::WindowOccluded(event) => Some(event.window),
        WindowEvent::Ime(
            bevy::window::Ime::Preedit { window, .. }
            | bevy::window::Ime::Commit { window, .. }
            | bevy::window::Ime::Enabled { window }
            | bevy::window::Ime::Disabled { window },
        ) => Some(*window),
        WindowEvent::FileDragAndDrop(
            bevy::window::FileDragAndDrop::DroppedFile { window, .. }
            | bevy::window::FileDragAndDrop::HoveredFile { window, .. }
            | bevy::window::FileDragAndDrop::HoveredFileCanceled { window },
        ) => Some(*window),
        _ => None,
    }
}

fn route_window_input(world: &mut World) {
    let Ok(window) = world
        .query_filtered::<Entity, With<PrimaryWindow>>()
        .single(world)
    else {
        return;
    };
    let handle = world.resource::<NativeInterfaceHandle>().clone();
    world.resource_scope(|world, mut state: Mut<HostCaptureState>| {
        let mut ordered_modifiers = Vec::new();
        if let Some(messages) = world.get_resource::<Messages<RawWinitWindowEvent>>() {
            let window_id = bevy::winit::WINIT_WINDOWS
                .with_borrow(|windows| windows.entity_to_winit.get(&window).copied());
            let raw = state
                .raw_events
                .read(messages)
                .filter(|event| Some(event.window_id) == window_id)
                .map(|event| event.event.clone())
                .collect::<Vec<_>>();
            handle.record_raw_file_shortcuts(&raw);
            ordered_modifiers = ordered_modifier_snapshots(&mut state.raw_modifiers, window, raw);
        }
        let events: Vec<_> = state
            .events
            .read(world.resource::<Messages<WindowEvent>>())
            .cloned()
            .collect();
        for event in &events {
            if input_window(event).is_some_and(|target| target != window) {
                continue;
            }
            if let WindowEvent::KeyboardInput(input) = event {
                if input.logical_key == Key::AltGraph {
                    state.modifiers.alt_graph = input.state == ButtonState::Pressed;
                }
                if input.state == ButtonState::Pressed {
                    state.modifiers.pressed.insert(input.key_code);
                } else {
                    state.modifiers.pressed.remove(&input.key_code);
                }
            }
            if let WindowEvent::WindowOccluded(event) = event {
                world.resource_mut::<NativeRenderAvailability>().occluded = event.occluded;
            }
            if let WindowEvent::WindowFocused(event) = event {
                world.resource_mut::<NativeRenderAvailability>().focused = event.focused;
            }
            match event {
                WindowEvent::CursorMoved(event) => state.cursor = Some(event.position),
                WindowEvent::CursorLeft(_) => state.cursor = None,
                WindowEvent::WindowFocused(event) if !event.focused => {
                    state.modifiers.pressed.clear();
                    state.modifiers.alt_graph = false;
                }
                WindowEvent::KeyboardFocusLost(_) => {
                    state.modifiers.pressed.clear();
                    state.modifiers.alt_graph = false;
                }
                _ => (),
            }
            let original_modifiers = take_ordered_modifiers(&mut ordered_modifiers, event);
            let input = NativeHostInput {
                ui_scale: handle.presented_ui_scale(),
                context: handle.presented_context(),
                cursor: state
                    .cursor
                    .map(|cursor| cursor / handle.presented_ui_scale()),
                modifiers: original_modifiers.unwrap_or_else(|| state.modifiers.modifiers()),
                event: interface_event(event.clone(), handle.presented_ui_scale()),
                consumed: false,
                actions: Vec::new(),
            };
            handle.record_file_shortcut(
                &input,
                if original_modifiers.is_some() {
                    "ingress_raw_modifiers"
                } else {
                    "ingress_reconstructed_modifiers"
                },
            );
            world.write_message(input);
        }
    });
    if let Some(window) = world.get::<Window>(window) {
        let visible = window.visible && window.physical_width() > 0 && window.physical_height() > 0;
        let mut availability = world.resource_mut::<NativeRenderAvailability>();
        availability.drawable = visible && !availability.occluded;
    }
}

/// Native input uses published UI units. Capture remains in OS logical
/// coordinates so resizing also remaps a pointer that has not moved.
fn interface_event(mut event: WindowEvent, scale: f32) -> WindowEvent {
    if let WindowEvent::CursorMoved(cursor) = &mut event {
        cursor.position /= scale;
        cursor.delta = cursor.delta.map(|delta| delta / scale);
    }
    event
}

/// Run from the controller's single ordered loop, immediately before reducing
/// this event. Editing later field buffers before earlier model actions is
/// forbidden, even when the OS delivers an entire gesture in one update.
pub(crate) fn prepare_native_input(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    input: &mut NativeHostInput,
) -> Result<(), String> {
    if input.context != handle.presented_context() {
        input.consumed = true;
        return Err("Native input belongs to a retired document".into());
    }
    world.resource_scope(|world, mut state: Mut<HostInputState>| {
        state.cursor = input.cursor;
        state.alt_graph = input.modifiers.alt_graph;
        state.pressed.clear();
        for (pressed, key) in [
            (input.modifiers.ctrl, KeyCode::ControlLeft),
            (input.modifiers.meta, KeyCode::SuperLeft),
            (input.modifiers.alt, KeyCode::AltLeft),
            (input.modifiers.shift, KeyCode::ShiftLeft),
        ] {
            if pressed {
                state.pressed.insert(key);
            }
        }
        input.consumed = super::interface_shell::fields::before_window_input(
            world,
            handle,
            &input.event,
            input.cursor,
            input.modifiers,
        )?;
        if !input.consumed {
            input.consumed = route_one(handle, &mut state, &input.event)?;
        }
        if matches!(&input.event, WindowEvent::WindowFocused(event) if event.focused) {
            super::interface_shell::fields::after_window_focus(world, handle)?;
        } else {
            super::interface_shell::fields::after_window_input(world, handle)?;
        }
        super::interface_shell::fields::after_pointer_input(
            world,
            handle,
            &input.event,
            input.cursor,
            input.modifiers,
        )?;
        input.actions = handle.take_actions()?;
        Ok(())
    })
}

fn route_one(
    handle: &NativeInterfaceHandle,
    state: &mut HostInputState,
    event: &WindowEvent,
) -> Result<bool, String> {
    match event {
        WindowEvent::CursorMoved(event) => {
            state.cursor = Some(event.position);
            if !state.model_drag.is_empty() && !handle.has_capture() {
                return Ok(false);
            }
            handle.pointer(
                PointerPhase::Move,
                event.position.as_dvec2().to_array(),
                PointerButton::Primary,
            )
        }
        WindowEvent::CursorLeft(_) => {
            let point = state
                .cursor
                .map(|point| point.as_dvec2().to_array())
                .unwrap_or([-1.0; 2]);
            state.cursor = None;
            handle.pointer(PointerPhase::Leave, point, PointerButton::Primary)
        }
        WindowEvent::MouseButtonInput(event) => {
            if event.state == ButtonState::Released && state.model_drag.remove(&event.button) {
                return Ok(false);
            }
            let button = match event.button {
                MouseButton::Left => PointerButton::Primary,
                MouseButton::Right => PointerButton::Secondary,
                _ => {
                    if event.state == ButtonState::Pressed {
                        state.model_drag.insert(event.button);
                    }
                    return Ok(false);
                }
            };
            let point = state
                .cursor
                .map(|point| point.as_dvec2().to_array())
                .unwrap_or([-1.0; 2]);
            let mut phase = if event.state == ButtonState::Pressed {
                PointerPhase::Down
            } else {
                PointerPhase::Up
            };
            if event.button == MouseButton::Left {
                let target = handle.hit_key(point);
                if event.state == ButtonState::Pressed {
                    state.click_press = target;
                } else if let Some(target) =
                    target.filter(|target| Some(*target) == state.click_press.take())
                {
                    if let Some(context) = handle.presented_context() {
                        let now = Instant::now();
                        let double =
                            state
                                .last_click
                                .as_ref()
                                .is_some_and(|(time, key, prior, owner)| {
                                    *key == target
                                        && *owner == context
                                        && now.duration_since(*time) <= Duration::from_millis(500)
                                        && (point[0] - prior[0]).abs() <= 4.
                                        && (point[1] - prior[1]).abs() <= 4.
                                });
                        if double {
                            phase = PointerPhase::DoubleClick;
                            state.last_click = None;
                        } else {
                            state.last_click = Some((now, target, point, context));
                        }
                    }
                } else {
                    state.click_press = None;
                    state.last_click = None;
                }
            } else {
                state.click_press = None;
                state.last_click = None;
            }
            let consumed = handle.pointer(phase, point, button)?;
            if !consumed && event.state == ButtonState::Pressed {
                state.model_drag.insert(event.button);
            }
            Ok(consumed)
        }
        WindowEvent::KeyboardInput(input) => {
            if input.state == ButtonState::Pressed {
                state.pressed.insert(input.key_code);
            } else {
                state.pressed.remove(&input.key_code);
                return Ok(false);
            }
            let chord = key_chord(input, state.modifiers());
            if chord.key == "Tab" && !chord.ctrl && !chord.meta && !chord.alt {
                return handle.focus_next(chord.shift);
            }
            handle.key(chord)
        }
        WindowEvent::WindowFocused(event) if !event.focused => {
            state.click_press = None;
            state.last_click = None;
            state.pressed.clear();
            state.model_drag.clear();
            state.cursor = None;
            handle.suspend_window_focus();
            Ok(false)
        }
        WindowEvent::KeyboardFocusLost(_) => {
            state.click_press = None;
            state.last_click = None;
            state.pressed.clear();
            state.model_drag.clear();
            handle.suspend_window_focus();
            Ok(false)
        }
        WindowEvent::WindowFocused(event) if event.focused => {
            handle.resume_window_focus();
            Ok(false)
        }
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_double_click_routes_once_and_cannot_cross_document_incarnations() {
        let (mut app, handle, _, _) = super::super::interface_shell::tests::fixture();
        let mut state = HostInputState {
            cursor: Some(Vec2::new(140., 140.)),
            ..default()
        };
        let window = Entity::from_bits(1);
        let click = |state: &mut HostInputState| {
            for pressed in [ButtonState::Pressed, ButtonState::Released] {
                route_one(
                    &handle,
                    state,
                    &WindowEvent::MouseButtonInput(bevy::input::mouse::MouseButtonInput {
                        button: MouseButton::Left,
                        state: pressed,
                        window,
                    }),
                )
                .unwrap();
            }
        };
        click(&mut state);
        click(&mut state);
        let actions = handle.take_actions().unwrap();
        assert_eq!(actions.len(), 2);
        assert_eq!(
            actions[0].control.input,
            limo_cad_interface::ControlInput::Click
        );
        assert_eq!(
            actions[1].control.input,
            limo_cad_interface::ControlInput::DoubleClick
        );
        click(&mut state);
        handle.take_actions().unwrap();
        let mut frame = handle.frame().unwrap();
        frame.context.epoch += 1;
        handle.present(frame).unwrap();
        app.update();
        click(&mut state);
        let actions = handle.take_actions().unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0].control.input,
            limo_cad_interface::ControlInput::Click
        );
    }

    fn key(window: Entity, code: KeyCode, logical: Key, state: ButtonState) -> WindowEvent {
        WindowEvent::KeyboardInput(KeyboardInput {
            key_code: code,
            logical_key: logical,
            state,
            text: None,
            repeat: false,
            window,
        })
    }

    #[test]
    fn original_modifier_snapshots_precede_late_synthetic_super_and_remain_per_key() {
        let window = Entity::PLACEHOLDER;
        let letter = key(
            window,
            KeyCode::KeyA,
            Key::Character("a".into()),
            ButtonState::Pressed,
        );
        let WindowEvent::KeyboardInput(input) = &letter else {
            unreachable!()
        };
        let mut raw = Modifiers::default();
        ordered_modifier_snapshots(
            &mut raw,
            window,
            [winit::event::WindowEvent::ModifiersChanged(
                winit::keyboard::ModifiersState::SUPER.into(),
            )],
        );
        let mut snapshots = vec![(ModifiedInput::Keyboard(input.clone()), raw)];
        ordered_modifier_snapshots(
            &mut raw,
            window,
            [winit::event::WindowEvent::ModifiersChanged(
                winit::keyboard::ModifiersState::empty().into(),
            )],
        );
        snapshots.push((ModifiedInput::Keyboard(input.clone()), raw));
        assert!(
            take_ordered_modifiers(&mut snapshots, &letter)
                .unwrap()
                .meta
        );
        assert!(
            !take_ordered_modifiers(&mut snapshots, &letter)
                .unwrap()
                .meta
        );
        let synthetic = key(window, KeyCode::SuperLeft, Key::Super, ButtonState::Pressed);
        assert!(take_ordered_modifiers(&mut snapshots, &synthetic).is_none());
        raw.meta = true;
        ordered_modifier_snapshots(
            &mut raw,
            window,
            [winit::event::WindowEvent::Focused(false)],
        );
        assert_eq!(raw, Modifiers::default());
    }

    #[test]
    fn held_modifiers_apply_to_first_click_cursor_and_wheel_before_synthetic_keys() {
        use winit::event::{
            DeviceId, ElementState, MouseScrollDelta, TouchPhase, WindowEvent as Raw,
        };
        let window = Entity::PLACEHOLDER;
        let mut raw = Modifiers::default();
        let mut snapshots = ordered_modifier_snapshots(
            &mut raw,
            window,
            [
                Raw::ModifiersChanged(
                    (winit::keyboard::ModifiersState::SHIFT
                        | winit::keyboard::ModifiersState::CONTROL)
                        .into(),
                ),
                Raw::MouseInput {
                    device_id: DeviceId::dummy(),
                    state: ElementState::Pressed,
                    button: winit::event::MouseButton::Left,
                },
                Raw::CursorMoved {
                    device_id: DeviceId::dummy(),
                    position: winit::dpi::PhysicalPosition::new(30., 40.),
                },
                Raw::MouseWheel {
                    device_id: DeviceId::dummy(),
                    delta: MouseScrollDelta::LineDelta(0., 1.),
                    phase: TouchPhase::Moved,
                },
                Raw::ModifiersChanged(winit::keyboard::ModifiersState::empty().into()),
                Raw::MouseInput {
                    device_id: DeviceId::dummy(),
                    state: ElementState::Released,
                    button: winit::event::MouseButton::Left,
                },
            ],
        );
        let button = |state| {
            WindowEvent::MouseButtonInput(bevy::input::mouse::MouseButtonInput {
                button: MouseButton::Left,
                state,
                window,
            })
        };
        let cursor = WindowEvent::CursorMoved(bevy::window::CursorMoved {
            position: Vec2::new(30., 40.),
            delta: None,
            window,
        });
        let wheel = WindowEvent::MouseWheel(bevy::input::mouse::MouseWheel {
            unit: bevy::input::mouse::MouseScrollUnit::Line,
            x: 0.,
            y: 1.,
            window,
            phase: bevy::input::touch::TouchPhase::Moved,
        });
        for event in [button(ButtonState::Pressed), cursor, wheel] {
            let modifiers = take_ordered_modifiers(&mut snapshots, &event).unwrap();
            assert!(modifiers.shift && modifiers.ctrl);
        }
        assert_eq!(
            take_ordered_modifiers(&mut snapshots, &button(ButtonState::Released)),
            Some(Modifiers::default())
        );
        assert!(snapshots.is_empty());
    }

    #[test]
    fn modifier_state_is_ordered_and_focus_loss_cancels_it() {
        let handle = NativeInterfaceHandle::new(|| {});
        let mut state = HostInputState::default();
        let window = Entity::from_bits(1);
        route_one(
            &handle,
            &mut state,
            &key(
                window,
                KeyCode::ControlLeft,
                Key::Control,
                ButtonState::Pressed,
            ),
        )
        .unwrap();
        route_one(
            &handle,
            &mut state,
            &key(
                window,
                KeyCode::ControlRight,
                Key::Control,
                ButtonState::Pressed,
            ),
        )
        .unwrap();
        route_one(
            &handle,
            &mut state,
            &key(
                window,
                KeyCode::ControlLeft,
                Key::Control,
                ButtonState::Released,
            ),
        )
        .unwrap();
        assert!(state.modifiers().ctrl);
        let input = KeyboardInput {
            key_code: KeyCode::KeyZ,
            logical_key: Key::Character("z".into()),
            state: ButtonState::Pressed,
            text: Some("z".into()),
            repeat: false,
            window,
        };
        assert_eq!(
            key_chord(&input, state.modifiers()),
            KeyChord {
                key: "z".into(),
                ctrl: true,
                meta: false,
                alt: false,
                shift: false
            }
        );
        route_one(
            &handle,
            &mut state,
            &WindowEvent::WindowFocused(bevy::window::WindowFocused {
                window,
                focused: false,
            }),
        )
        .unwrap();
        assert_eq!(state.modifiers(), Modifiers::default());
    }

    #[test]
    fn ime_and_close_are_delivered_to_the_controller_without_synthetic_keys() {
        let mut app = App::new();
        app.add_message::<WindowEvent>()
            .add_message::<NativeHostInput>()
            .insert_resource(NativeInterfaceHandle::new(|| {}))
            .init_resource::<HostInputState>()
            .init_resource::<HostCaptureState>()
            .init_resource::<NativeRenderAvailability>()
            .add_systems(Update, route_window_input);
        let window = app.world_mut().spawn(PrimaryWindow).id();
        let foreign = app.world_mut().spawn_empty().id();
        let expected = [
            WindowEvent::Ime(bevy::window::Ime::Preedit {
                window,
                value: "日本".into(),
                cursor: Some((0, 6)),
            }),
            WindowEvent::Ime(bevy::window::Ime::Commit {
                window,
                value: "日本語".into(),
            }),
            WindowEvent::WindowCloseRequested(bevy::window::WindowCloseRequested { window }),
        ];
        for event in &expected {
            app.world_mut().write_message(event.clone());
        }
        app.world_mut()
            .write_message(WindowEvent::Ime(bevy::window::Ime::Commit {
                window: foreign,
                value: "foreign".into(),
            }));
        app.update();
        let events: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<NativeHostInput>>()
            .drain()
            .collect();
        assert_eq!(
            events
                .iter()
                .map(|input| input.event.clone())
                .collect::<Vec<_>>(),
            expected
        );
    }

    #[test]
    fn crossing_chrome_during_a_model_drag_preserves_model_input_and_leave_clears_hover() {
        use crate::native_viewport::interface_shell::tests::fixture;
        let (_app, handle, _, wakes) = fixture();
        let window = Entity::from_bits(1);
        let mut state = HostInputState::default();
        let move_to = |x, y| {
            WindowEvent::CursorMoved(bevy::window::CursorMoved {
                window,
                position: Vec2::new(x, y),
                delta: None,
            })
        };
        assert!(!route_one(&handle, &mut state, &move_to(800.0, 600.0)).unwrap());
        assert!(!route_one(
            &handle,
            &mut state,
            &WindowEvent::MouseButtonInput(bevy::input::mouse::MouseButtonInput {
                window,
                button: MouseButton::Left,
                state: ButtonState::Pressed
            })
        )
        .unwrap());
        assert!(!route_one(&handle, &mut state, &move_to(140.0, 140.0)).unwrap());
        assert!(!route_one(
            &handle,
            &mut state,
            &WindowEvent::MouseButtonInput(bevy::input::mouse::MouseButtonInput {
                window,
                button: MouseButton::Left,
                state: ButtonState::Released
            })
        )
        .unwrap());
        assert!(handle.take_actions().unwrap().is_empty());
        assert!(route_one(&handle, &mut state, &move_to(140.0, 140.0)).unwrap());
        let before = wakes.load(std::sync::atomic::Ordering::Relaxed);
        assert!(route_one(
            &handle,
            &mut state,
            &WindowEvent::CursorLeft(bevy::window::CursorLeft { window })
        )
        .unwrap());
        assert!(wakes.load(std::sync::atomic::Ordering::Relaxed) > before);
        assert_eq!(state.cursor, None);
    }
}
