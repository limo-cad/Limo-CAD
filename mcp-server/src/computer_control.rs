//! Computer input is a separate OS surface, never an engine or script operation.
use serde_json::{json, Value};

#[cfg(any(test, all(windows, feature = "native-computer-control")))]
mod native_text;

#[cfg(all(windows, feature = "native-computer-control"))]
mod capture;
#[cfg(all(windows, feature = "native-computer-control"))]
pub(crate) use capture::run_worker_if_requested;
#[cfg(all(windows, feature = "native-computer-control"))]
mod windows;
#[cfg(all(windows, feature = "native-computer-control"))]
pub(crate) use windows::ComputerControl;

#[cfg(not(all(windows, feature = "native-computer-control")))]
#[derive(Default)]
pub(crate) struct ComputerControl {}

#[cfg(not(all(windows, feature = "native-computer-control")))]
impl ComputerControl {
    pub(crate) fn call(&mut self, _: &Value, _: Option<&str>) -> Result<Value, String> {
        Err(json!({
            "code": if cfg!(feature = "native-computer-control") {
                "computer_control_unsupported_platform"
            } else {
                "computer_control_disabled"
            },
            "message": description(),
            "available": false,
            "required_feature": "native-computer-control",
            "supported_platform": "windows",
        })
        .to_string())
    }
}

pub(super) fn description() -> &'static str {
    if !cfg!(feature = "native-computer-control") {
        return "Native computer control is disabled in this build. Enable the native-computer-control Cargo feature on Windows to compile OS mouse and keyboard input. No actions are available while disabled.";
    }
    if !cfg!(windows) {
        return "Native computer control is unavailable on this platform. The native-computer-control Cargo feature currently supports Windows only; no actions are available here.";
    }
    "Windows computer control implemented in Rust with real OS mouse and keyboard input. action=observe returns the presented interface, exact active desktop owner, physical client bounds and a short-lived one-shot observation token. A CAD-owned native modal dialog becomes the observed target and includes a real PNG capture, native control geometry and focused editable child. Its client_to_image_offset maps physical client points to the attached window image. A minimized or unpresented main window returns focus_only=true without qualified controls/coordinates. All other actions require that token; focus restores/activates only that window, then observe again before input. move positions only the pointer at a physical client-pixel point, without mouse-button or keyboard input. click/double_click/drag/wheel take physical client-pixel points from capture and optional Ctrl/Shift modifiers. A drag takes one endpoint or a bounded waypoint path with dwell and optional Escape cancellation; all points are qualified before input. key accepts Ctrl/Shift chords and named keys; text types printable BMP Unicode into focused Bevy fields, or printable Unicode including non-BMP into focused native-dialog controls. Unsupported Bevy text is rejected in full before input. Input rejects changed documents, geometry, layouts, replaced processes, held keys, foreign foreground windows and occluded pointer targets. GUI and MCP must run the same clean build and executable path. An input_sent receipt confirms OS insertion only: observe/capture afterward to verify the visible result. No external helper, scripts, arbitrary applications or direct model commands."
}

pub(super) fn schema() -> Value {
    let point = json!({"type":"array","items":{"type":"integer"},"minItems":2,"maxItems":2});
    json!({"type":"object","description":description(),
        "x-limo-cad-availability":{
            "feature":"native-computer-control",
            "feature_enabled":cfg!(feature = "native-computer-control"),
            "platform_supported":cfg!(windows),
            "available":cfg!(all(windows, feature = "native-computer-control")),
            "text_support":{
                "bevy_window":"printable_bmp",
                "native_dialog":"printable_unicode",
                "unsupported_bevy_text":"rejected_before_input",
            },
        },
        "additionalProperties":false,"required":["action"],"properties":{
        "action":{"type":"string","enum":["observe","focus","move","click","double_click","drag","wheel","key","text"]},
        "session_id":{"type":"string","description":"Explicit current active desktop session, otherwise use the attached session."},
        "observation":{"type":"string","description":"One-shot token from observe, valid for 60 seconds; observe again after every action."},
        "point":point,"to":point,
        "modifiers":{"type":"array","items":{"type":"string","enum":["Ctrl","Shift"]},"maxItems":2,"uniqueItems":true,"description":"Held during one pointer gesture; released on completion or failure."},
        "path":{"type":"array","minItems":1,"maxItems":8,"items":{"type":"object","additionalProperties":false,"required":["point"],"properties":{"point":point,"hold_ms":{"type":"integer","minimum":0,"maximum":800,"default":0}}},"description":"Drag-only physical-client waypoints; mutually exclusive with to. Total dwell cannot exceed 1600ms. The complete path is qualified before input."},
        "cancel":{"type":"boolean","default":false,"description":"Drag only: release modifiers and send Escape while the button is held, then release it."},
        "button":{"type":"string","enum":["left","middle","right"],"default":"left"},
        "delta":{"type":"integer","minimum":-1200,"maximum":1200,"multipleOf":120,"description":"Wheel delta in whole Windows mouse notches: multiples of 120, positive scrolls up."},
        "key":{"type":"string","description":"Enter, Escape, Tab, Backspace, Delete, arrows, Home/End/PageUp/PageDown, F1–F12 or an ASCII letter/digit, optionally prefixed Ctrl+ or Shift+."},
        "text":{"type":"string","maxLength":512,"description":"1-512 printable Unicode characters for the observed focused editable text control. Bevy fields support BMP characters (U+0000-U+FFFF, excluding controls); any non-BMP character rejects the complete request before input. Native dialogs also support non-BMP Unicode."}
    }})
}
