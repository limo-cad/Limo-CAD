//! The product interface, independent of its renderer and transport.
//! Register the controls actually laid out by the application; both human
//! input and MCP resolve to the same retained control and native handler.

pub mod catalog;
mod registry;

pub use registry::{ControlError, SurfaceRegistry};
use serde::{Deserialize, Serialize};

/// A retained control instance, including the renderer's reuse generation.
/// Bevy callers use Entity::to_bits(), never a row index or a display label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ControlKey(pub u64);

/// The active document incarnation in one application window. Advance epoch
/// whenever replacement/hydration changes ownership, even for the same file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentContext {
    pub window_id: String,
    pub document_id: String,
    pub epoch: u64,
}

/// Logical application-window pixels. The renderer publishes these after
/// layout, using the same coordinates for inspection and hit testing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChoiceOption {
    pub value: String,
    pub label: String,
    pub disabled: bool,
}

/// Offsets are UTF-16 code units, retaining the public script-editor contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextSelection {
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub enum Field {
    #[default]
    None,
    Text {
        value: String,
        read_only: bool,
        selection: Option<TextSelection>,
    },
    Choice {
        value: String,
        options: Vec<ChoiceOption>,
    },
    Toggle(bool),
    Range {
        value: f64,
        min: f64,
        max: f64,
        step: f64,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Control {
    pub key: ControlKey,
    /// Advance when a retained widget is bound to a different command/target.
    /// Entity generations alone do not detect a row or button being repurposed.
    pub binding: u64,
    pub surface: String,
    pub label: String,
    /// Existing accessibility roles, including text, number, tab and treeitem.
    pub role: String,
    pub bounds: Rect,
    pub visible: bool,
    pub disabled: bool,
    pub expanded: Option<bool>,
    pub selected: Option<bool>,
    pub field: Field,
    /// Portal menus retain the modal owner that opened them.
    pub modal_scope: Option<String>,
    /// Text/script editors own typing and application shortcuts; toggles do not.
    pub text_editing: bool,
    /// The actual widget handler declares its non-text keys (for example tab
    /// arrows, slider Home/End, or a tree item's Shift+F10 context menu).
    pub owned_keys: Vec<KeyChord>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Surface {
    pub name: String,
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Canvas {
    pub name: String,
    #[serde(flatten)]
    pub bounds: Rect,
}

/// One coherent, already-laid-out application frame. The viewport lives in
/// canvases under its existing name, not a second independently maintained rect.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SurfaceFrame {
    pub client: Rect,
    pub controls: Vec<Control>,
    pub surfaces: Vec<Surface>,
    pub canvases: Vec<Canvas>,
    pub focused: Option<ControlKey>,
    /// Bottom to top; only the top modal and its owned portals accept input.
    pub modal_stack: Vec<String>,
    pub document_visible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Key {
    Enter,
    Escape,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    Delete,
    Backspace,
}

/// Physical keyboard input keeps modifiers. The existing MCP key vocabulary
/// maps to unmodified chords; human-only shortcuts are not silently discarded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyChord {
    pub key: String,
    pub ctrl: bool,
    pub meta: bool,
    pub alt: bool,
    pub shift: bool,
}

impl KeyChord {
    pub fn plain(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            ..Self::default()
        }
    }
}

impl From<Key> for KeyChord {
    fn from(key: Key) -> Self {
        Self::plain(match key {
            Key::Enter => "Enter",
            Key::Escape => "Escape",
            Key::ArrowUp => "ArrowUp",
            Key::ArrowDown => "ArrowDown",
            Key::ArrowLeft => "ArrowLeft",
            Key::ArrowRight => "ArrowRight",
            Key::Home => "Home",
            Key::End => "End",
            Key::Delete => "Delete",
            Key::Backspace => "Backspace",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlInput {
    Click,
    DoubleClick,
    ContextMenu,
    SetValue(String),
    Key(KeyChord),
}

/// Existing cad_interface control request payload. Transport-only fields such
/// as pace/session/expiry remain the responsibility of the existing envelope.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ControlRequest {
    Click { target: String },
    DoubleClick { target: String },
    ContextMenu { target: String },
    SetValue { target: String, value: String },
    Key { target: String, key: Key },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedControl {
    pub key: ControlKey,
    pub input: ControlInput,
    pub(crate) stamp: ControlStamp,
}

impl ResolvedControl {
    /// The native reducer also checks its current binding component while
    /// consuming queued actions, before the next layout republishes a frame.
    pub fn binding(&self) -> u64 {
        self.stamp.binding
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ControlStamp {
    pub binding: u64,
    pub label: String,
    pub surface: String,
    pub role: String,
    pub field_kind: std::mem::Discriminant<Field>,
    pub modal_scope: Option<String>,
    pub text_editing: bool,
    pub owned_keys: Vec<KeyChord>,
}

/// Keyboard routing is separate from activation: Tab/focus movement and text
/// editing stay with the focused widget, while modal keys cannot reach CAD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardRoute {
    Widget,
    Modal,
    Model,
}
