//! Opt-in, bounded file-shortcut routing evidence. Never retain typed text.
use crate::native_viewport::winit_host::NativeHostInput;
use bevy::{
    input::keyboard::{Key, KeyCode},
    window::WindowEvent,
};
use serde_json::{json, Value};
use std::collections::VecDeque;

const LIMIT: usize = 16;
#[derive(Default)]
pub(super) struct Trace {
    sequence: u64,
    events: VecDeque<Value>,
}
impl Trace {
    pub(super) fn opt_in() -> Option<Self> {
        (std::env::var("LIMO_CAD_FILE_SHORTCUT_DIAGNOSTICS").as_deref() == Ok("1"))
            .then(Self::default)
    }
    pub(super) fn record(&mut self, event: &NativeHostInput, decision: &'static str) {
        let WindowEvent::KeyboardInput(input) = &event.event else {
            return;
        };
        if !input.state.is_pressed() || input.repeat {
            return;
        }
        let key = match input.key_code {
            KeyCode::KeyN => "N",
            KeyCode::KeyO => "O",
            KeyCode::KeyS => "S",
            KeyCode::KeyW => "W",
            KeyCode::KeyP => "P",
            _ => return,
        };
        let logical = match &input.logical_key {
            Key::Character(value) if value.eq_ignore_ascii_case(key) => "matching_letter",
            Key::Character(value) if value.chars().all(char::is_control) => "control_scalar",
            Key::Unidentified(_) => "unidentified",
            _ => "other",
        };
        self.sequence = self.sequence.saturating_add(1);
        if self.events.len() == LIMIT {
            self.events.pop_front();
        }
        self.events.push_back(json!({
            "sequence":self.sequence,"key":key,"logical_category":logical,
            "ctrl":event.modifiers.ctrl,"shift":event.modifiers.shift,
            "meta":event.modifiers.meta,"alt":event.modifiers.alt,
            "alt_graph":event.modifiers.alt_graph,"decision":decision,
            "owner":event.context.as_ref().map(|owner| json!({
                "window_id":owner.window_id,"document_id":owner.document_id,"epoch":owner.epoch
            }))
        }));
    }
    pub(super) fn snapshot(&self) -> Value {
        json!({"event_limit":LIMIT,"events":self.events,"sequence":self.sequence,
            "source":"native file-shortcut ingress and routing; no typed text retained"})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        input::{keyboard::KeyboardInput, ButtonState},
        prelude::Entity,
    };
    #[test]
    fn file_shortcut_trace_is_bounded_and_never_retains_text() {
        let mut trace = Trace::default();
        let mut input = NativeHostInput {
            ui_scale: 1.,
            context: None,
            cursor: None,
            modifiers: Default::default(),
            consumed: false,
            actions: vec![],
            event: WindowEvent::KeyboardInput(KeyboardInput {
                key_code: KeyCode::KeyS,
                logical_key: Key::Character("private value".into()),
                state: ButtonState::Pressed,
                text: Some("private text".into()),
                repeat: false,
                window: Entity::PLACEHOLDER,
            }),
        };
        for _ in 0..32 {
            trace.record(&input, "ingress");
        }
        let value = trace.snapshot();
        assert_eq!(value["events"].as_array().unwrap().len(), LIMIT);
        assert_eq!(value["events"][0]["sequence"], 17);
        assert!(!value.to_string().contains("private"));
        if let WindowEvent::KeyboardInput(key) = &mut input.event {
            key.key_code = KeyCode::KeyA;
        }
        trace.record(&input, "ingress");
        assert_eq!(trace.snapshot()["sequence"], 32);
    }
}
