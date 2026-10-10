//! Bounded, opt-in evidence of real native IME delivery. This observer does not
//! manufacture events or participate in editing, focus, or input-source choice.
use super::{InterfaceControl, NativeInterfaceAction, NativeInterfaceHandle};
use bevy::{
    prelude::*,
    text::EditableText,
    window::{Ime, WindowEvent},
};
use serde_json::{json, Value};

const MAX_EVENTS: usize = 256;
const MAX_VALUE_BYTES: usize = 4096;

fn enabled_for(platform: &str, read: impl Fn(&str) -> Option<String>) -> bool {
    let matches = |key, expected| read(key).as_deref() == Some(expected);
    let platform = platform == "macos"
        && matches("LIMO_CAD_NATIVE_IME_TEST", "macos-japanese")
        && matches("RUNNER_OS", "macOS")
        || platform == "windows"
            && matches("LIMO_CAD_NATIVE_IME_TEST", "windows-japanese")
            && matches("RUNNER_OS", "Windows");
    platform
        && matches("GITHUB_ACTIONS", "true")
        && matches("RUNNER_ENVIRONMENT", "github-hosted")
        && matches("GITHUB_REPOSITORY_ID", limo_cad_build_info::repository_id())
}

#[cfg(target_os = "windows")]
mod windows;

pub(super) fn owner_snapshot(owner: &limo_cad_interface::DocumentContext) -> Value {
    json!({"window_id":owner.window_id, "document_id":owner.document_id, "epoch":owner.epoch})
}

pub(super) struct Trace {
    events: Vec<Value>,
    keyboard: Vec<Value>,
    configuration: Value,
    overflow: bool,
}

impl Trace {
    #[cfg(test)]
    pub(super) fn for_test() -> Self {
        Self {
            events: Vec::new(),
            keyboard: Vec::new(),
            configuration: Value::Null,
            overflow: false,
        }
    }

    pub(super) fn opt_in() -> Option<Self> {
        enabled_for(std::env::consts::OS, |key| std::env::var(key).ok()).then(|| Self {
            events: Vec::new(),
            keyboard: Vec::new(),
            configuration: Value::Null,
            overflow: false,
        })
    }

    fn push(&mut self, mut event: Value) {
        if self.events.len() >= MAX_EVENTS
            || event["value"]
                .as_str()
                .is_some_and(|value| value.len() > MAX_VALUE_BYTES)
        {
            self.overflow = true;
            return;
        }
        event["sequence"] = json!(self.events.len() + 1);
        self.events.push(event);
    }

    pub(super) fn snapshot(&self) -> Value {
        json!({"source":"received Bevy WindowEvent::Ime", "overflow":self.overflow,
            "event_limit":MAX_EVENTS, "events":self.events, "current":self.events.last(),
            "configuration":self.configuration, "keyboard":self.keyboard})
    }
}

pub(super) fn enabled(handle: &NativeInterfaceHandle) -> bool {
    handle
        .shared
        .lock()
        .is_ok_and(|shared| shared.ime_diagnostics.is_some())
}

fn configuration(world: &World, handle: &NativeInterfaceHandle, window: Entity) -> Value {
    let key = handle.focused_key();
    let field = key.map(|key| Entity::from_bits(key.0));
    let components = field.map(|entity| {
        json!({
            "native_text_field":world.get::<super::fields::NativeTextField>(entity).is_some(),
            "editable_text":world.get::<EditableText>(entity).is_some(),
            "computed_node":world.get::<ComputedNode>(entity).is_some(),
            "ui_transform":world.get::<bevy::ui::UiGlobalTransform>(entity).is_some(),
            "render_target":world.get::<bevy::ui::ComputedUiRenderTargetInfo>(entity).is_some(),
        })
    });
    let window_state = world.get::<Window>(window).map(|window| {
        json!({
        "ime_enabled":window.ime_enabled, "ime_position":window.ime_position.to_array(),
        "focused":window.focused})
    });
    json!({"sample":"current state after the preceding frame's Winit propagation",
        "window_entity":window.to_bits(), "window":window_state,
        "control_key":key.map(|key|key.0),
        "binding":field.and_then(|entity|world.get::<InterfaceControl>(entity).map(|control|control.binding)),
        "context":handle.frame().map(|frame|owner_snapshot(&frame.context)),
        "native_field_components":components, "appkit":appkit_input_context(window),
        "win32":windows_input_context(window)})
}

/// First runs after the preceding frame's Last/Winit propagation. This exclusive
/// system reads the native context on the UI thread even when no Ime event arrives. It is
/// a no-op outside the explicit disposable-runner diagnostic opt-in.
pub(super) fn observe_configuration(world: &mut World) {
    let Some(handle) = world.get_resource::<NativeInterfaceHandle>().cloned() else {
        return;
    };
    if !enabled(&handle) {
        return;
    }
    let Ok(window) = world
        .query_filtered::<Entity, With<bevy::window::PrimaryWindow>>()
        .single(world)
    else {
        return;
    };
    let value = configuration(world, &handle, window);
    if let Ok(mut shared) = handle.shared.lock() {
        if let Some(trace) = &mut shared.ime_diagnostics {
            trace.configuration = value;
        }
    };
}

pub(super) fn received_keyboard(
    world: &World,
    handle: &NativeInterfaceHandle,
    action: &NativeInterfaceAction,
    event: &WindowEvent,
) {
    let WindowEvent::KeyboardInput(key) = event else {
        return;
    };
    if !key.state.is_pressed() || !enabled(handle) {
        return;
    }
    let mut sample = configuration(world, handle, key.window);
    sample["sample"] = json!("before routing original WindowEvent::KeyboardInput");
    sample["key_code"] = json!(format!("{:?}", key.key_code));
    sample["context"] = owner_snapshot(&action.context);
    if let Ok(mut shared) = handle.shared.lock() {
        if let Some(trace) = &mut shared.ime_diagnostics {
            if trace.keyboard.len() >= MAX_EVENTS {
                trace.overflow = true;
            } else {
                trace.keyboard.push(sample);
            }
        }
    }
}

pub(super) fn received(
    world: &World,
    handle: &NativeInterfaceHandle,
    action: &NativeInterfaceAction,
    event: &WindowEvent,
) {
    let WindowEvent::Ime(ime) = event else { return };
    let Ok(mut shared) = handle.shared.lock() else {
        return;
    };
    let Some(trace) = &mut shared.ime_diagnostics else {
        return;
    };
    let (kind, window, value, cursor) = match ime {
        Ime::Preedit {
            window,
            value,
            cursor,
        } => ("preedit", *window, value.as_str(), *cursor),
        Ime::Commit { window, value } => ("commit", *window, value.as_str(), None),
        Ime::Enabled { window } => ("enabled", *window, "", None),
        Ime::Disabled { window } => ("disabled", *window, "", None),
    };
    let entity = Entity::from_bits(action.control.key.0);
    let Some(editor) = world.get::<EditableText>(entity) else {
        return;
    };
    trace.push(
        json!({"kind":kind, "window_entity":window.to_bits(), "value":value,
        "cursor":cursor, "context":owner_snapshot(&action.context),
        "control_key":action.control.key.0, "binding":action.control.binding(),
        "control_label":world.get::<InterfaceControl>(entity).map(|control| &control.label),
        "composing":editor.is_composing(), "appkit":appkit_input_context(window),
        "win32":windows_input_context(window)}),
    );
}

#[cfg(target_os = "windows")]
fn windows_input_context(entity: Entity) -> Value {
    windows::input_context(entity)
}

#[cfg(not(target_os = "windows"))]
fn windows_input_context(_entity: Entity) -> Value {
    Value::Null
}

#[cfg(not(target_os = "macos"))]
fn appkit_input_context(_entity: Entity) -> Value {
    Value::Null
}

#[cfg(target_os = "macos")]
fn appkit_input_context(entity: Entity) -> Value {
    use objc2::{msg_send, runtime::AnyObject, MainThreadMarker};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    if MainThreadMarker::new().is_none() {
        return json!({"error":"not on AppKit main thread"});
    }
    bevy::winit::WINIT_WINDOWS.with_borrow(|windows| {
        let Some(window) = windows.get_window(entity) else {
            return Value::Null;
        };
        let Ok(raw) = window.window_handle() else {
            return Value::Null;
        };
        let RawWindowHandle::AppKit(raw) = raw.as_raw() else {
            return Value::Null;
        };

        unsafe {
            let view = raw.ns_view.as_ptr().cast::<AnyObject>();
            let context: *mut AnyObject = msg_send![view, inputContext];
            let native_window: *mut AnyObject = msg_send![view, window];
            if native_window.is_null() {
                return json!({"error":"view has no window"});
            }
            let source: *mut AnyObject = if context.is_null() {
                std::ptr::null_mut()
            } else {
                msg_send![context, selectedKeyboardInputSource]
            };
            let source_id = if source.is_null() {
                None
            } else {
                let utf8: *const std::ffi::c_char = msg_send![source, UTF8String];
                (!utf8.is_null()).then(|| {
                    std::ffi::CStr::from_ptr(utf8)
                        .to_string_lossy()
                        .into_owned()
                })
            };
            let first: *mut AnyObject = msg_send![native_window, firstResponder];
            let key: bool = msg_send![native_window, isKeyWindow];
            let number: isize = msg_send![native_window, windowNumber];
            let scale: f64 = msg_send![native_window, backingScaleFactor];
            json!({"source_id":source_id, "input_context_present":!context.is_null(),
                "window_number":number, "key_window":key,
                "first_responder_is_view":first == view, "backing_scale":scale})
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_require_the_owned_repository_and_explicit_hosted_platform() {
        for (platform, runner, opt_in) in [
            ("windows", "Windows", "windows-japanese"),
            ("macos", "macOS", "macos-japanese"),
        ] {
            let environment = |key: &str| {
                Some(match key {
                    "LIMO_CAD_NATIVE_IME_TEST" => opt_in.into(),
                    "RUNNER_OS" => runner.into(),
                    "GITHUB_ACTIONS" => "true".into(),
                    "RUNNER_ENVIRONMENT" => "github-hosted".into(),
                    "GITHUB_REPOSITORY_ID" => limo_cad_build_info::repository_id().into(),
                    _ => return None,
                })
            };
            assert!(enabled_for(platform, environment));
            assert!(!enabled_for("linux", environment));
            for key in [
                "LIMO_CAD_NATIVE_IME_TEST",
                "RUNNER_OS",
                "GITHUB_ACTIONS",
                "RUNNER_ENVIRONMENT",
                "GITHUB_REPOSITORY_ID",
            ] {
                assert!(
                    !enabled_for(platform, |name| {
                        if name == key {
                            None
                        } else {
                            environment(name)
                        }
                    }),
                    "{platform}: missing {key}"
                );
            }
            for repository in ["1313334316", "01313334315"] {
                assert!(!enabled_for(platform, |key| {
                    if key == "GITHUB_REPOSITORY_ID" {
                        Some(repository.into())
                    } else {
                        environment(key)
                    }
                }));
            }
        }
    }

    #[test]
    fn received_ime_trace_is_bounded_and_keeps_order_without_truncating_text() {
        let mut trace = Trace {
            events: Vec::new(),
            keyboard: Vec::new(),
            configuration: Value::Null,
            overflow: false,
        };
        trace.push(json!({"kind":"preedit", "value":"はる", "composing":true}));
        trace.push(json!({"kind":"commit", "value":"はる", "composing":false}));
        assert_eq!(trace.snapshot()["events"][1]["sequence"], 2);
        trace.push(json!({"value":"x".repeat(MAX_VALUE_BYTES + 1)}));
        assert!(trace.overflow);
        assert_eq!(trace.events.len(), 2);
        for _ in 0..MAX_EVENTS {
            trace.push(json!({"value":""}));
        }
        assert_eq!(trace.events.len(), MAX_EVENTS);
        assert_eq!(trace.events[0]["value"], "はる");
    }
}
