//! Opt-in, bounded file-shortcut routing evidence. Never retain typed text.
use crate::native_viewport::winit_host::NativeHostInput;
use bevy::{
    input::keyboard::{Key, KeyCode},
    window::WindowEvent,
};
use limo_cad_interface::DocumentContext;
use serde_json::{json, Value};
use std::collections::VecDeque;
use winit::{
    event::{ElementState, WindowEvent as RawWindowEvent},
    keyboard::{Key as RawKey, KeyCode as RawKeyCode, PhysicalKey},
};

const LIMIT: usize = 16;
#[derive(Default)]
struct KeyboardIngress {
    events: u64,
    pressed: u64,
    released: u64,
    repeat: u64,
    file_candidates: u64,
    modifiers: u64,
    unidentified: u64,
}
#[derive(Default)]
pub(super) struct Trace {
    sequence: u64,
    events: VecDeque<Value>,
    keyboard_ingress: KeyboardIngress,
    raw_sequence: u64,
    raw_events: VecDeque<Value>,
}

// Winit's KeyEvent has private platform data. This borrowed projection is the
// actual raw collector input and permits its privacy policy to be tested without
// manufacturing native events or retaining text/platform payloads.
struct RawKeyboard<'a> {
    physical: &'a PhysicalKey,
    logical: &'a RawKey,
    state: ElementState,
    repeat: bool,
    is_synthetic: bool,
}

impl Trace {
    pub(super) fn opt_in() -> Option<Self> {
        (std::env::var("LIMO_CAD_FILE_SHORTCUT_DIAGNOSTICS").as_deref() == Ok("1"))
            .then(Self::default)
    }

    pub(super) fn record_raw(&mut self, event: &RawWindowEvent, owner: &DocumentContext) {
        let RawWindowEvent::KeyboardInput {
            event,
            is_synthetic,
            ..
        } = event
        else {
            return;
        };
        self.record_raw_keyboard(
            RawKeyboard {
                physical: &event.physical_key,
                logical: &event.logical_key,
                state: event.state,
                repeat: event.repeat,
                is_synthetic: *is_synthetic,
            },
            owner,
        );
    }

    fn record_raw_keyboard(&mut self, input: RawKeyboard<'_>, owner: &DocumentContext) {
        let physical_key = match input.physical {
            PhysicalKey::Code(RawKeyCode::KeyN) => Some("N"),
            PhysicalKey::Code(RawKeyCode::KeyO) => Some("O"),
            PhysicalKey::Code(RawKeyCode::KeyS) => Some("S"),
            PhysicalKey::Code(RawKeyCode::KeyW) => Some("W"),
            PhysicalKey::Code(RawKeyCode::KeyP) => Some("P"),
            _ => None,
        };
        let modifier = matches!(
            input.physical,
            PhysicalKey::Code(
                RawKeyCode::ControlLeft
                    | RawKeyCode::ControlRight
                    | RawKeyCode::ShiftLeft
                    | RawKeyCode::ShiftRight
                    | RawKeyCode::AltLeft
                    | RawKeyCode::AltRight
                    | RawKeyCode::SuperLeft
                    | RawKeyCode::SuperRight
            )
        );
        let logical_key = match input.logical {
            RawKey::Character(value) => file_key_bytes(value.as_bytes()),
            _ => None,
        };
        if physical_key.is_none() && logical_key.is_none() && !modifier {
            return;
        }
        self.raw_sequence = self.raw_sequence.saturating_add(1);
        if self.raw_events.len() == LIMIT {
            self.raw_events.pop_front();
        }
        self.raw_events.push_back(json!({
            "sequence":self.raw_sequence,"physical_file_key":physical_key,
            "logical_file_key":logical_key,"modifier":modifier,
            "pressed":input.state.is_pressed(),"repeat":input.repeat,
            "is_synthetic":input.is_synthetic,
            "owner":{"window_id":owner.window_id,"document_id":owner.document_id,"epoch":owner.epoch}
        }));
    }

    pub(super) fn record(&mut self, event: &NativeHostInput, decision: &'static str) {
        let WindowEvent::KeyboardInput(input) = &event.event else {
            return;
        };
        let physical_key = physical_file_key(input.key_code);
        let logical_key = logical_file_key(&input.logical_key);
        let key = physical_key.or(logical_key);
        let physical_category = match input.key_code {
            _ if physical_key.is_some() => "file_letter",
            KeyCode::ControlLeft
            | KeyCode::ControlRight
            | KeyCode::ShiftLeft
            | KeyCode::ShiftRight
            | KeyCode::AltLeft
            | KeyCode::AltRight
            | KeyCode::SuperLeft
            | KeyCode::SuperRight => "modifier",
            KeyCode::Unidentified(_) => "unidentified",
            _ => "other",
        };
        // Routing can record the same event again. Count keyboard arrival only
        // at the two host ingress sites, even if no file candidate is recognized.
        if matches!(
            decision,
            "ingress_raw_modifiers" | "ingress_reconstructed_modifiers"
        ) {
            let ingress = &mut self.keyboard_ingress;
            ingress.events = ingress.events.saturating_add(1);
            if input.state.is_pressed() {
                ingress.pressed = ingress.pressed.saturating_add(1);
            } else {
                ingress.released = ingress.released.saturating_add(1);
            }
            if input.repeat {
                ingress.repeat = ingress.repeat.saturating_add(1);
            }
            if key.is_some() {
                ingress.file_candidates = ingress.file_candidates.saturating_add(1);
            }
            if physical_category == "modifier" {
                ingress.modifiers = ingress.modifiers.saturating_add(1);
            }
            if physical_category == "unidentified" {
                ingress.unidentified = ingress.unidentified.saturating_add(1);
            }
        }
        let Some(key) = key else {
            return;
        };
        let logical = match &input.logical_key {
            Key::Character(value) if value.eq_ignore_ascii_case(key) => "matching_letter",
            Key::Character(value) if value.len() == 1 && value.as_bytes()[0].is_ascii_control() => {
                "control_scalar"
            }
            Key::Character(_) if logical_key.is_some() => "different_file_letter",
            Key::Unidentified(_) => "unidentified",
            _ => "other",
        };
        self.sequence = self.sequence.saturating_add(1);
        if self.events.len() == LIMIT {
            self.events.pop_front();
        }
        self.events.push_back(json!({
            "sequence":self.sequence,"key":key,"logical_category":logical,
            "physical_file_key":physical_key,"physical_category":physical_category,
            "logical_file_key":logical_key,"pressed":input.state.is_pressed(),
            "repeat":input.repeat,
            "ctrl":event.modifiers.ctrl,"shift":event.modifiers.shift,
            "meta":event.modifiers.meta,"alt":event.modifiers.alt,
            "alt_graph":event.modifiers.alt_graph,"decision":decision,
            "owner":event.context.as_ref().map(|owner| json!({
                "window_id":owner.window_id,"document_id":owner.document_id,"epoch":owner.epoch
            }))
        }));
    }
    pub(super) fn snapshot(&self) -> Value {
        let ingress = &self.keyboard_ingress;
        json!({"event_limit":LIMIT,"events":self.events,"sequence":self.sequence,
            "raw_winit":{"event_limit":LIMIT,"events":self.raw_events,"sequence":self.raw_sequence,
                "source":"Consumed primary-window RawWinitWindowEvent stream; owner is the presented frame at batch capture. This is downstream of Win32/Winit event assembly, not native delivery or future-ownership proof."},
            "keyboard_ingress":{"events":ingress.events,"pressed":ingress.pressed,
                "released":ingress.released,"repeat":ingress.repeat,
                "file_candidates":ingress.file_candidates,"modifiers":ingress.modifiers,
                "unidentified":ingress.unidentified},
            "source":"native file-shortcut ingress and routing; no typed text retained"})
    }
}

fn physical_file_key(key: KeyCode) -> Option<&'static str> {
    match key {
        KeyCode::KeyN => Some("N"),
        KeyCode::KeyO => Some("O"),
        KeyCode::KeyS => Some("S"),
        KeyCode::KeyW => Some("W"),
        KeyCode::KeyP => Some("P"),
        _ => None,
    }
}

fn logical_file_key(key: &Key) -> Option<&'static str> {
    let Key::Character(value) = key else {
        return None;
    };
    // Only these file letters and their ASCII Ctrl scalars leave this helper.
    // Never retain arbitrary logical keys, typed text or native key codes.
    file_key_bytes(value.as_bytes())
}

fn file_key_bytes(value: &[u8]) -> Option<&'static str> {
    match value {
        [b'n' | b'N' | 0x0e] => Some("N"),
        [b'o' | b'O' | 0x0f] => Some("O"),
        [b's' | b'S' | 0x13] => Some("S"),
        [b'w' | b'W' | 0x17] => Some("W"),
        [b'p' | b'P' | 0x10] => Some("P"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        input::{
            keyboard::{KeyboardInput, NativeKey, NativeKeyCode},
            ButtonState,
        },
        prelude::Entity,
    };

    fn raw(
        trace: &mut Trace,
        physical: &PhysicalKey,
        logical: &RawKey,
        state: ElementState,
        repeat: bool,
        is_synthetic: bool,
    ) {
        trace.record_raw_keyboard(
            RawKeyboard {
                physical,
                logical,
                state,
                repeat,
                is_synthetic,
            },
            &DocumentContext {
                window_id: "main".into(),
                document_id: "document-17".into(),
                epoch: 9,
            },
        );
    }

    #[test]
    fn raw_file_shortcut_trace_is_bounded_and_retains_only_whitelisted_keys() {
        use winit::keyboard::{NativeKey as RawNativeKey, NativeKeyCode as RawNativeKeyCode};
        let mut trace = Trace::default();
        let physical = PhysicalKey::Code(RawKeyCode::KeyS);
        let private = RawKey::Character("private text".into());
        for _ in 0..32 {
            raw(
                &mut trace,
                &physical,
                &private,
                ElementState::Pressed,
                false,
                false,
            );
        }
        let value = trace.snapshot();
        let events = value["raw_winit"]["events"].as_array().unwrap();
        assert_eq!(events.len(), LIMIT);
        assert_eq!(events[0]["sequence"], 17);
        assert_eq!(events[0]["physical_file_key"], "S");
        assert_eq!(events[0]["logical_file_key"], Value::Null);
        assert_eq!(events[0]["owner"]["document_id"], "document-17");
        assert_eq!(events[0]["owner"]["epoch"], 9);
        assert!(!value.to_string().contains("private"));
        let unidentified = PhysicalKey::Unidentified(RawNativeKeyCode::Windows(65535));
        for logical in [
            private,
            RawKey::Unidentified(RawNativeKey::Web("private native key".into())),
        ] {
            raw(
                &mut trace,
                &unidentified,
                &logical,
                ElementState::Pressed,
                false,
                false,
            );
        }
        assert_eq!(trace.snapshot()["raw_winit"]["sequence"], 32);
        for (code, letter, scalar, expected) in [
            (RawKeyCode::KeyN, "n", "\u{0e}", "N"),
            (RawKeyCode::KeyO, "O", "\u{0f}", "O"),
            (RawKeyCode::KeyS, "s", "\u{13}", "S"),
            (RawKeyCode::KeyW, "W", "\u{17}", "W"),
            (RawKeyCode::KeyP, "p", "\u{10}", "P"),
        ] {
            raw(
                &mut trace,
                &PhysicalKey::Code(code),
                &RawKey::Character(letter.into()),
                ElementState::Pressed,
                false,
                false,
            );
            assert_eq!(
                trace.snapshot()["raw_winit"]["events"]
                    .as_array()
                    .unwrap()
                    .last()
                    .unwrap()["physical_file_key"],
                expected
            );
            raw(
                &mut trace,
                &unidentified,
                &RawKey::Character(scalar.into()),
                ElementState::Released,
                false,
                false,
            );
            let value = trace.snapshot();
            let last = value["raw_winit"]["events"]
                .as_array()
                .unwrap()
                .last()
                .unwrap();
            assert_eq!(last["physical_file_key"], Value::Null);
            assert_eq!(last["logical_file_key"], expected);
            assert!(!value.to_string().contains("private"));
            assert!(!value.to_string().contains("65535"));
        }
    }

    #[test]
    fn raw_file_shortcut_trace_keeps_synthetic_state_without_counting_converted_ingress() {
        let mut trace = Trace::default();
        let physical = PhysicalKey::Code(RawKeyCode::KeyS);
        let logical = RawKey::Character("s".into());
        raw(
            &mut trace,
            &physical,
            &logical,
            ElementState::Pressed,
            false,
            true,
        );
        raw(
            &mut trace,
            &physical,
            &logical,
            ElementState::Released,
            true,
            false,
        );
        raw(
            &mut trace,
            &PhysicalKey::Code(RawKeyCode::ControlLeft),
            &RawKey::Named(winit::keyboard::NamedKey::Control),
            ElementState::Pressed,
            false,
            false,
        );
        trace.record_raw(
            &RawWindowEvent::RedrawRequested,
            &DocumentContext {
                window_id: "main".into(),
                document_id: "document-17".into(),
                epoch: 9,
            },
        );
        let before = trace.snapshot();
        assert_eq!(before["raw_winit"]["sequence"], 3);
        assert_eq!(before["raw_winit"]["events"][0]["is_synthetic"], true);
        assert_eq!(before["raw_winit"]["events"][0]["pressed"], true);
        assert_eq!(before["raw_winit"]["events"][1]["pressed"], false);
        assert_eq!(before["raw_winit"]["events"][1]["repeat"], true);
        assert_eq!(before["raw_winit"]["events"][1]["is_synthetic"], false);
        assert_eq!(before["raw_winit"]["events"][2]["modifier"], true);
        assert_eq!(
            before["raw_winit"]["events"][2]["physical_file_key"],
            Value::Null
        );
        assert_eq!(before["keyboard_ingress"]["events"], 0);
        assert_eq!(before["sequence"], 0);
        let mut converted = input(KeyCode::KeyS, Key::Character("s".into()));
        if let WindowEvent::KeyboardInput(key) = &mut converted.event {
            key.state = ButtonState::Released;
            key.repeat = true;
        }
        trace.record(&converted, "ingress_raw_modifiers");
        trace.record(&converted, "not_command_chord");
        let after = trace.snapshot();
        assert_eq!(after["raw_winit"], before["raw_winit"]);
        assert_eq!(after["keyboard_ingress"]["events"], 1);
        assert_eq!(after["keyboard_ingress"]["released"], 1);
        assert_eq!(after["keyboard_ingress"]["repeat"], 1);
        assert_eq!(after["sequence"], 2);
    }

    fn input(key_code: KeyCode, logical_key: Key) -> NativeHostInput {
        NativeHostInput {
            ui_scale: 1.,
            context: None,
            cursor: None,
            modifiers: Default::default(),
            consumed: false,
            actions: vec![],
            event: WindowEvent::KeyboardInput(KeyboardInput {
                key_code,
                logical_key,
                state: ButtonState::Pressed,
                text: Some("private text".into()),
                repeat: false,
                window: Entity::PLACEHOLDER,
            }),
        }
    }
    #[test]
    fn file_shortcut_trace_is_bounded_and_never_retains_text() {
        let mut trace = Trace::default();
        let mut input = input(KeyCode::KeyS, Key::Character("private value".into()));
        for _ in 0..32 {
            trace.record(&input, "ingress_raw_modifiers");
        }
        let value = trace.snapshot();
        assert_eq!(value["events"].as_array().unwrap().len(), LIMIT);
        assert_eq!(value["events"][0]["sequence"], 17);
        assert_eq!(value["keyboard_ingress"]["events"], 32);
        assert_eq!(value["events"][0]["logical_category"], "other");
        assert_eq!(value["events"][0]["logical_file_key"], Value::Null);
        assert!(!value.to_string().contains("private"));
        if let WindowEvent::KeyboardInput(key) = &mut input.event {
            key.key_code = KeyCode::KeyA;
        }
        trace.record(&input, "ingress_reconstructed_modifiers");
        assert_eq!(trace.snapshot()["sequence"], 32);
        assert_eq!(trace.snapshot()["keyboard_ingress"]["events"], 33);
    }
    #[test]
    fn file_shortcut_trace_keeps_repeats_and_releases_without_double_counting() {
        let mut trace = Trace::default();
        let mut input = input(KeyCode::KeyS, Key::Character("s".into()));
        if let WindowEvent::KeyboardInput(key) = &mut input.event {
            key.repeat = true;
        }
        trace.record(&input, "ingress_raw_modifiers");
        trace.record(&input, "not_a_file_shortcut");
        if let WindowEvent::KeyboardInput(key) = &mut input.event {
            key.state = ButtonState::Released;
            key.repeat = false;
        }
        trace.record(&input, "ingress_raw_modifiers");
        let value = trace.snapshot();
        assert_eq!(value["keyboard_ingress"]["events"], 2);
        assert_eq!(value["keyboard_ingress"]["pressed"], 1);
        assert_eq!(value["keyboard_ingress"]["released"], 1);
        assert_eq!(value["keyboard_ingress"]["repeat"], 1);
        assert_eq!(value["keyboard_ingress"]["file_candidates"], 2);
        assert_eq!(value["sequence"], 3);
        assert_eq!(value["events"][0]["pressed"], true);
        assert_eq!(value["events"][0]["repeat"], true);
        assert_eq!(value["events"][2]["pressed"], false);
        assert_eq!(value["events"][2]["repeat"], false);
    }
    #[test]
    fn file_shortcut_trace_keeps_only_allowed_logical_candidates() {
        for (letter, scalar, expected) in [
            ("N", "\u{0e}", "N"),
            ("o", "\u{0f}", "O"),
            ("S", "\u{13}", "S"),
            ("w", "\u{17}", "W"),
            ("p", "\u{10}", "P"),
        ] {
            for (logical, category) in [(letter, "matching_letter"), (scalar, "control_scalar")] {
                let mut trace = Trace::default();
                let input = input(
                    KeyCode::Unidentified(NativeKeyCode::Windows(65535)),
                    Key::Character(logical.into()),
                );
                trace.record(&input, "ingress_raw_modifiers");
                let value = trace.snapshot();
                assert_eq!(value["events"][0]["key"], expected);
                assert_eq!(value["events"][0]["physical_file_key"], Value::Null);
                assert_eq!(value["events"][0]["physical_category"], "unidentified");
                assert_eq!(value["events"][0]["logical_file_key"], expected);
                assert_eq!(value["events"][0]["logical_category"], category);
                assert!(!value.to_string().contains("private"));
                assert!(!value.to_string().contains("65535"));
            }
        }
        let mut trace = Trace::default();
        trace.record(
            &input(KeyCode::KeyA, Key::Character("s".into())),
            "ingress_raw_modifiers",
        );
        trace.record(
            &input(KeyCode::KeyN, Key::Character("s".into())),
            "ingress_raw_modifiers",
        );
        let value = trace.snapshot();
        assert_eq!(value["events"][0]["physical_category"], "other");
        assert_eq!(value["events"][0]["logical_file_key"], "S");
        assert_eq!(value["events"][1]["physical_file_key"], "N");
        assert_eq!(value["events"][1]["logical_file_key"], "S");
        assert_eq!(
            value["events"][1]["logical_category"],
            "different_file_letter"
        );
    }
    #[test]
    fn file_shortcut_trace_counts_unmatched_keyboard_ingress_without_retaining_keys() {
        let mut trace = Trace::default();
        let unknown = input(
            KeyCode::Unidentified(NativeKeyCode::Windows(65535)),
            Key::Unidentified(NativeKey::Web("private key".into())),
        );
        trace.record(&unknown, "ingress_raw_modifiers");
        trace.record(&unknown, "not_a_file_shortcut");
        trace.record(
            &input(KeyCode::ControlLeft, Key::Control),
            "ingress_reconstructed_modifiers",
        );
        trace.record(
            &input(KeyCode::KeyA, Key::Character("private value".into())),
            "ingress_raw_modifiers",
        );
        let value = trace.snapshot();
        assert_eq!(value["keyboard_ingress"]["events"], 3);
        assert_eq!(value["keyboard_ingress"]["unidentified"], 1);
        assert_eq!(value["keyboard_ingress"]["modifiers"], 1);
        assert_eq!(value["keyboard_ingress"]["file_candidates"], 0);
        assert_eq!(value["sequence"], 0);
        assert!(value["events"].as_array().unwrap().is_empty());
        assert!(!value.to_string().contains("private"));
        assert!(!value.to_string().contains("65535"));
    }
}
