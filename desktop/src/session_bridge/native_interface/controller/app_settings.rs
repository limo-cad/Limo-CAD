//! Application preferences shared with the release shell; never document data.
use super::*;
use crate::app_preferences::{
    self as preferences, Effective, Locale, Observer, Preferences, Store, ThemePreference,
};
use crate::native_viewport::{localization, ui};
use chrome::{rect, Widgets};
use limo_cad_interface::{ControlInput, Field};
use std::sync::atomic::AtomicU64;
use std::time::Instant;

mod panel;
#[cfg(test)]
mod tests;

#[derive(Resource, Clone, Default)]
pub(super) struct Wake(Arc<AtomicU64>);
impl Wake {
    pub(super) fn changed(&self) {
        self.0.fetch_add(1, Ordering::Release);
    }
    fn generation(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }
}
pub(super) fn install(world: &mut World) -> Wake {
    world.init_resource::<Wake>();
    world.resource::<Wake>().clone()
}

/// Effective live preference includes pending choices when persistence failed.
pub(super) fn six_dof_speed(world: &World) -> f32 {
    world
        .get_resource::<Settings>()
        .map_or(preferences::DEFAULT_SIX_DOF_SPEED, |settings| {
            settings.effective().six_dof_speed
        }) as f32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Theme(ThemePreference),
    Language(Locale),
    Speed,
    ResetSpeed,
    InterfaceSize(u8),
    GpuStock(bool),
    Scroll(i32),
    Retry,
    Close,
}

#[derive(Resource)]
struct Settings {
    store: Result<Store, String>,
    observer: Option<Observer>,
    saved: Preferences,
    pending: Preferences,
    save_error: Option<String>,
    wake: u64,
    detected: Locale,
    error: Option<String>,
    widgets: Widgets,
    scroll: f32,
    content: Option<InterfaceRect>,
    scroll_max: f32,
}
impl Settings {
    fn new(store: Result<Store, String>, detected: Locale) -> Self {
        let observer = store.as_ref().ok().map(|s| Observer::new(s.clone()));
        Self {
            error: store.as_ref().err().cloned().or_else(|| {
                preferences::locale::validate_dictionaries()
                    .err()
                    .map(str::to_owned)
            }),
            store,
            observer,
            saved: Preferences::default(),
            pending: Preferences::default(),
            save_error: None,
            wake: 0,
            detected,
            widgets: Widgets::default(),
            scroll: 0.,
            content: None,
            scroll_max: 0.,
        }
    }
    fn effective(&self) -> Effective {
        let mut live = self.saved.clone();
        merge(&mut live, &self.pending);
        live.effective(self.detected)
    }
    fn poll(&mut self, force: bool) {
        if let Some(result) = self
            .observer
            .as_mut()
            .and_then(|o| o.poll(Instant::now(), force))
        {
            match result {
                Ok(saved) => {
                    self.saved = saved;
                    self.error = self.save_error.clone();
                }
                Err(error) => self.error = Some(error),
            }
        }
    }
    fn patch(&mut self, patch: Preferences) -> Result<(), String> {
        merge(&mut self.pending, &patch);
        let result = self
            .store
            .as_ref()
            .map_err(Clone::clone)
            .and_then(|store| store.patch(self.pending.clone()));
        match result {
            Ok(saved) => {
                self.saved = saved;
                self.pending = Preferences::default();
                self.error = None;
                self.save_error = None;
                Ok(())
            }
            Err(error) => {
                self.error = Some(error.clone());
                self.save_error = Some(error.clone());
                Err(error)
            }
        }
    }
}

fn merge(target: &mut Preferences, patch: &Preferences) {
    if let Some(theme) = patch.theme {
        target.theme = Some(theme);
    }
    if let Some(locale) = patch.locale {
        target.locale = Some(locale);
    }
    if let Some(speed) = patch.six_dof_speed {
        target.six_dof_speed = Some(speed);
    }
    if let Some(enabled) = patch.gpu_stock_removal {
        target.gpu_stock_removal = Some(enabled);
    }
    if let Some(scale) = patch.ui_scale {
        target.ui_scale = Some(scale);
    }
}

fn initialize(world: &mut World) {
    if !world.contains_resource::<Settings>() {
        world.insert_resource(Settings::new(
            Store::native(),
            crate::native_viewport::system_locale::detected_locale(),
        ));
    }
}

fn system_dark(world: &mut World) -> bool {
    world
        .get_resource::<native_viewport::winit_host::window_theme::SystemTheme>()
        .is_some_and(|theme| theme.is_dark())
}

/// Called during idle reduction, before painting. No engine/OCCT lock is used.
pub(super) fn refresh(world: &mut World, force: bool) {
    initialize(world);
    let wake = world.get_resource::<Wake>().map_or(0, Wake::generation);
    let mut settings = world.remove_resource::<Settings>().unwrap();
    let notified = settings.wake != wake;
    settings.wake = wake;
    settings.poll(force || notified);
    let effective = settings.effective();
    native_viewport::apply_interface_gpu_stock_preference(world, effective.gpu_stock_removal);
    let resolved = effective.theme.resolve(system_dark(world));
    let changed_locale = localization::set_locale(world, effective.locale);
    let palette = preferences::palette::viewport_palette(resolved);
    let changed = world
        .get_resource::<ui::Appearance>()
        .is_none_or(|current| current.palette != *palette);
    if changed || changed_locale {
        let revision = ui::appearance_revision(world).wrapping_add(1);
        world.insert_resource(ui::Appearance {
            palette: *palette,
            theme: ViewportUiTheme::from_palette(palette),
            revision,
        });
        if changed {
            native_viewport::apply_interface_palette(world, *palette);
            let theme = ui::theme(world);
            interface_shell::refresh_theme(world, theme);
        }
    }
    world.insert_resource(settings);
}

/// Publish the scale with the complete controller layout. A worker may start
/// after Settings reduction in the same input batch; its busy frame must keep
/// the previous scale until viewport and UI bounds can change together.
pub(super) fn apply_scale(world: &mut World) {
    let scale = world
        .get_resource::<Settings>()
        .map_or(preferences::DEFAULT_UI_SCALE, |settings| {
            settings.effective().ui_scale
        }) as f32;
    if world
        .get_resource::<bevy::ui::UiScale>()
        .is_some_and(|current| current.0 == scale)
    {
        return;
    }
    world.insert_resource(bevy::ui::UiScale(scale));
    cancel_pointer_input(world);
}

/// Existing inbox watcher owns idle wakeups. Compare bounded preference reads
/// there, so unchanged settings do not keep the GPU or UI layout running.
pub(super) fn watch() -> Option<Observer> {
    Store::native().ok().map(Observer::new)
}

pub(super) fn caption(world: &World) -> Option<String> {
    let settings = world.get_resource::<Settings>()?;
    Some(json!({
        "preferences_path":settings.store.as_ref().ok().map(|store|store.path().to_string_lossy()),
        "explicit":settings.saved,
        "pending":settings.pending,
        "effective":settings.effective(),
        "error":settings.error,
        "open":files::settings_open(world),
    }).to_string())
}

pub(crate) fn reduce(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    engine: &AppState,
    bridge: &SessionBridgeState,
    action: &NativeInterfaceAction,
    command: Command,
) -> Result<Value, String> {
    bridge
        .with_native_document_owner(engine, &action.context, || handle.validate_action(action))?;
    if !files::settings_open(world) {
        return Err("Settings is closed".into());
    }
    if let Command::Scroll(direction) = command {
        if !super::super::is_activation(&action.control.input) {
            return Err("Activate the Settings scroll control".into());
        }
        let mut settings = world.resource_mut::<Settings>();
        let page = settings
            .content
            .map_or(200., |body| body.height as f32 * 0.8);
        settings.scroll =
            (settings.scroll + direction as f32 * page).clamp(0., settings.scroll_max);
        return Ok(json!({"scroll":settings.scroll}));
    }
    let patch = match command {
        Command::Speed => {
            let ControlInput::SetValue(value) = &action.control.input else {
                if super::super::is_activation(&action.control.input) {
                    return Ok(json!({"focused":true}));
                }
                return Err("Choose a navigation speed".into());
            };
            let value: f64 = value
                .parse()
                .map_err(|_| "Navigation speed must be a number")?;
            if !value.is_finite()
                || !(preferences::MIN_SIX_DOF_SPEED..=preferences::MAX_SIX_DOF_SPEED)
                    .contains(&value)
            {
                return Err("Navigation speed must be between 0.25 and 3".into());
            }
            Preferences {
                six_dof_speed: Some(value),
                ..default()
            }
        }
        _ => {
            if !super::super::is_activation(&action.control.input) {
                return Err("Activate the Settings control".into());
            }
            match command {
                Command::Theme(theme) => Preferences {
                    theme: Some(theme),
                    ..default()
                },
                Command::Language(locale) => Preferences {
                    locale: Some(locale),
                    ..default()
                },
                Command::ResetSpeed => Preferences {
                    six_dof_speed: Some(preferences::DEFAULT_SIX_DOF_SPEED),
                    ..default()
                },
                Command::InterfaceSize(index) => Preferences {
                    ui_scale: Some(
                        *preferences::UI_SCALE_OPTIONS
                            .get(index as usize)
                            .ok_or("Choose an available interface size")?,
                    ),
                    ..default()
                },
                Command::GpuStock(enabled) => Preferences {
                    gpu_stock_removal: Some(enabled),
                    ..default()
                },
                Command::Retry => Preferences::default(),
                Command::Close => {
                    files::close_settings(world);
                    return Ok(json!({"closed":true}));
                }
                Command::Speed | Command::Scroll(_) => unreachable!(),
            }
        }
    };
    Ok(patch_preferences(world, patch))
}

fn patch_preferences(world: &mut World, patch: Preferences) -> Value {
    initialize(world);
    let mut settings = world.remove_resource::<Settings>().unwrap();
    let result = settings.patch(patch);
    world.insert_resource(settings);
    refresh(world, true);
    json!({"preferences": world.resource::<Settings>().effective(),"persisted":result.is_ok(),"error":result.err()})
}

/// UI size accelerators share the Settings persistence and deferred layout path.
/// Text editors, modals and stale document events keep their input ownership.
pub(super) fn shortcut(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    event: &NativeHostInput,
) -> Result<Option<Value>, String> {
    use bevy::input::{
        keyboard::{Key, KeyCode},
        ButtonState,
    };
    let Some(frame) = handle.frame() else {
        return Ok(None);
    };
    if event.consumed
        || !frame.document_visible
        || !frame.modal_stack.is_empty()
        || event.context.as_ref() != Some(&frame.context)
        || event.modifiers.alt
        || event.modifiers.alt_graph
        || !(event.modifiers.ctrl || event.modifiers.meta)
        || handle.focused_key().is_some_and(|key| {
            world
                .get::<InterfaceControl>(Entity::from_bits(key.0))
                .is_none_or(|control| {
                    control.text_editing || matches!(control.field, Field::Text { .. })
                })
        })
    {
        return Ok(None);
    }
    let WindowEvent::KeyboardInput(key) = &event.event else {
        return Ok(None);
    };
    if key.state != ButtonState::Pressed || key.repeat {
        return Ok(None);
    }
    let character = match &key.logical_key {
        Key::Character(value) => value.as_str(),
        _ => "",
    };
    let direction = match (character, key.key_code) {
        ("=" | "+", _) | (_, KeyCode::NumpadAdd) => Some(true),
        ("-" | "_", _) | (_, KeyCode::NumpadSubtract) => Some(false),
        ("0", _) | (_, KeyCode::Numpad0 | KeyCode::Digit0) if !event.modifiers.shift => None,
        _ => return Ok(None),
    };
    services
        .bridge
        .with_native_document_owner(&services.engine, &frame.context, || Ok(()))?;
    initialize(world);
    let current = world.resource::<Settings>().effective().ui_scale;
    let scale = direction.map_or(preferences::DEFAULT_UI_SCALE, |increase| {
        preferences::step_ui_scale(current, increase)
    });
    Ok(Some(patch_preferences(
        world,
        Preferences {
            ui_scale: Some(scale),
            ..default()
        },
    )))
}

pub(super) fn synchronize(
    world: &mut World,
    camera: Entity,
    services: &NativeServices,
    width: f32,
    height: f32,
) -> Result<(), String> {
    initialize(world);
    let mut settings = world.remove_resource::<Settings>().unwrap();
    settings.widgets.begin();
    let result = if files::settings_open(world) {
        panel::paint(world, camera, services, &mut settings, width, height)
    } else {
        settings.content = None;
        settings.scroll = 0.;
        Ok(())
    };
    settings.widgets.finish(world);
    world.insert_resource(settings);
    result
}

/// Scroll the existing settings body; paper/model navigation cannot consume a
/// wheel belonging to this modal. Physical pixels convert through actual DPI.
pub(super) fn input(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    event: &NativeHostInput,
) -> bool {
    if !files::settings_open(world) {
        return false;
    }
    let Some(frame) = handle.frame() else {
        return false;
    };
    if event.context.as_ref() != Some(&frame.context)
        || frame.modal_stack.last().map(String::as_str) != Some("app-settings")
    {
        return false;
    }
    let amount = match &event.event {
        WindowEvent::MouseWheel(wheel) => {
            let scale = world
                .get::<Window>(wheel.window)
                .map_or(1., |w| w.resolution.scale_factor());
            match wheel.unit {
                bevy::input::mouse::MouseScrollUnit::Line => wheel.y * 28.,
                bevy::input::mouse::MouseScrollUnit::Pixel => {
                    wheel.y / (scale * handle.presented_ui_scale())
                }
            }
        }
        WindowEvent::KeyboardInput(key)
            if key.state == bevy::input::ButtonState::Pressed
                && !event.modifiers.ctrl
                && !event.modifiers.meta
                && !event.modifiers.alt =>
        {
            let page = world
                .get_resource::<Settings>()
                .and_then(|s| s.content)
                .map_or(200., |r| r.height as f32 * 0.8);
            match key.logical_key {
                bevy::input::keyboard::Key::PageDown => -page,
                bevy::input::keyboard::Key::PageUp => page,
                _ => return false,
            }
        }
        _ => return false,
    };
    let Some(mut settings) = world.get_resource_mut::<Settings>() else {
        return true;
    };
    let keyboard = matches!(event.event, WindowEvent::KeyboardInput(_));
    if amount.is_finite()
        && (keyboard
            || settings.content.is_some_and(|r| {
                event.cursor.is_some_and(|p| {
                    p.x as f64 >= r.x
                        && p.x as f64 <= r.x + r.width
                        && p.y as f64 >= r.y
                        && p.y as f64 <= r.y + r.height
                })
            }))
    {
        settings.scroll = (settings.scroll - amount).clamp(0., settings.scroll_max);
    }
    true
}
