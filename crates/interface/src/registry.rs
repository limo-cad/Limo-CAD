use crate::*;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlError {
    InvalidFrame(String),
    DocumentChanged,
    Stale,
    Changed,
    Disabled,
    ModalBlocked,
    NotEditable,
    ReadOnly,
    OptionUnavailable,
    InvalidValue,
    InspectionExhausted,
}

impl fmt::Display for ControlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFrame(message) => write!(formatter, "Invalid interface frame: {message}"),
            Self::DocumentChanged => {
                formatter.write_str("Document changed; inspect the interface again")
            }
            Self::Stale => {
                formatter.write_str("Control is stale or unavailable; inspect the interface again")
            }
            Self::Changed => formatter.write_str("Control changed; inspect the interface again"),
            Self::Disabled => formatter.write_str("Control is disabled"),
            Self::ModalBlocked => formatter.write_str("A modal dialog blocks this control"),
            Self::NotEditable => {
                formatter.write_str("Control does not accept text; use its appropriate action")
            }
            Self::ReadOnly => formatter.write_str("Field is read-only"),
            Self::OptionUnavailable => formatter.write_str("Option is unavailable"),
            Self::InvalidValue => {
                formatter.write_str("Value must be finite and within the control's range")
            }
            Self::InspectionExhausted => {
                formatter.write_str("Interface inspection identifiers are exhausted")
            }
        }
    }
}
impl std::error::Error for ControlError {}

struct Observed {
    key: ControlKey,
    stamp: ControlStamp,
}

pub struct SurfaceRegistry {
    serial: u64,
    inspection: u64,
    context: Option<DocumentContext>,
    inspected_context: Option<DocumentContext>,
    frame: SurfaceFrame,
    by_key: HashMap<ControlKey, usize>,
    observed: HashMap<String, Observed>,
}

impl Default for SurfaceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl SurfaceRegistry {
    pub fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let serial = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .expect("Interface registry identifiers exhausted");
        Self {
            serial,
            inspection: 0,
            context: None,
            inspected_context: None,
            frame: SurfaceFrame::default(),
            by_key: HashMap::new(),
            observed: HashMap::new(),
        }
    }

    /// Replace atomically after coherent layout. A rejected frame cannot erase
    /// the last good frame or make its controls point at another instance.
    pub fn replace(
        &mut self,
        context: DocumentContext,
        frame: SurfaceFrame,
    ) -> Result<(), ControlError> {
        validate(&context, &frame)?;
        let by_key = frame
            .controls
            .iter()
            .enumerate()
            .map(|(index, control)| (control.key, index))
            .collect();
        if self.context.as_ref() != Some(&context) {
            self.observed.clear();
            self.inspected_context = None;
        }
        self.context = Some(context);
        self.frame = frame;
        self.by_key = by_key;
        Ok(())
    }

    pub fn frame(&self) -> &SurfaceFrame {
        &self.frame
    }
    pub fn context(&self) -> Option<&DocumentContext> {
        self.context.as_ref()
    }

    /// Inspection IDs deliberately differ from retained renderer keys and
    /// expire on each successful inspection, including across registry/window instances.
    pub fn inspect(&mut self) -> Result<Value, ControlError> {
        let context = self.context.clone().ok_or(ControlError::DocumentChanged)?;
        self.inspection = self
            .inspection
            .checked_add(1)
            .ok_or(ControlError::InspectionExhausted)?;
        let mut observed = HashMap::new();
        let mut controls = Vec::new();
        let mut focused = Value::Null;
        for control in self
            .frame
            .controls
            .iter()
            .filter(|control| available(control))
        {
            let id = format!(
                "control-{}-{}-{}",
                self.serial,
                self.inspection,
                observed.len() + 1
            );
            let label = normalized_label(&control.label);
            let mut value = json!({"id":id,"surface":control.surface,"label":label,
                "role":control.role,"disabled":control.disabled,"bounds":control.bounds});
            if let Some(expanded) = control.expanded {
                value["expanded"] = json!(expanded);
            }
            if let Some(selected) = control.selected {
                value["selected"] = json!(selected);
            }
            match &control.field {
                Field::None => (),
                Field::Toggle(checked) => value["value"] = json!(checked),
                Field::Range {
                    value: current,
                    min,
                    max,
                    step,
                } => {
                    value["value"] = json!(current);
                    value["min"] = json!(min);
                    value["max"] = json!(max);
                    value["step"] = json!(step);
                }
                Field::Text {
                    value: text,
                    selection,
                    read_only,
                } => {
                    value["read_only"] = json!(read_only);
                    if let Some(selection) = selection {
                        value["selection"] = json!(selection);
                    }
                    text_value(&mut value, text, *selection);
                }
                Field::Choice {
                    value: text,
                    options,
                } => {
                    text_value(&mut value, text, None);
                    value["options"] = json!(options);
                }
            }
            if self.frame.focused == Some(control.key) {
                focused = json!(id);
            }
            observed.insert(
                id,
                Observed {
                    key: control.key,
                    stamp: stamp(control),
                },
            );
            controls.push(value);
        }
        let mut names = Vec::new();
        for control in &controls {
            let name = control["surface"].as_str().unwrap();
            if !names.contains(&name) {
                names.push(name);
            }
        }
        for surface in &self.frame.surfaces {
            if !names.contains(&surface.name.as_str()) {
                names.push(surface.name.as_str());
            }
        }
        let surfaces: Vec<_> = names.into_iter().map(|name| {
            let mut value = json!({"name":name,"controls":controls.iter().filter(|control| control["surface"] == name).collect::<Vec<_>>()});
            if let Some(text) = self.frame.surfaces.iter().find(|surface| surface.name == name).and_then(|surface| surface.text.as_deref()) {
                let (excerpt, _, length) = excerpt(text, 0, 4096);
                value["text"] = json!(excerpt);
                if length > 4096 { value["text_truncated"] = json!(true); }
            }
            value
        }).collect();
        let result = json!({"surfaces":surfaces,"canvases":self.frame.canvases.iter().filter(|canvas| canvas.bounds.width > 0.0 && canvas.bounds.height > 0.0).collect::<Vec<_>>(),"client":self.frame.client,
            "document_visible":self.frame.document_visible,"focused_control":focused,
            "unlabeled_controls":controls.iter().filter(|control|control["label"] == "").map(|control|json!({"id":control["id"],"surface":control["surface"],"role":control["role"]})).collect::<Vec<_>>()});
        self.inspected_context = Some(context);
        self.observed = observed;
        Ok(result)
    }

    pub fn resolve(
        &self,
        request: &ControlRequest,
        context: &DocumentContext,
    ) -> Result<ResolvedControl, ControlError> {
        self.check_context(context)?;
        if self.inspected_context.as_ref() != Some(context) {
            return Err(ControlError::DocumentChanged);
        }
        let (target, input) = match request {
            ControlRequest::Click { target } => (target, ControlInput::Click),
            ControlRequest::DoubleClick { target } => (target, ControlInput::DoubleClick),
            ControlRequest::ContextMenu { target } => (target, ControlInput::ContextMenu),
            ControlRequest::SetValue { target, value } => {
                (target, ControlInput::SetValue(value.clone()))
            }
            ControlRequest::Key { target, key } => (target, ControlInput::Key((*key).into())),
        };
        let observed = self.observed.get(target).ok_or(ControlError::Stale)?;
        let control = self.control(observed.key)?;
        if observed.stamp != stamp(control) {
            return Err(ControlError::Changed);
        }
        self.resolve_key(observed.key, input, context)
    }

    /// Human input and MCP share validation and action identity. Dispatchers
    /// must call validate_resolved at consumption, not only resolve at enqueue.
    /// The real handler then owns focus/blur, edit/commit, and key defaults.
    pub fn resolve_key(
        &self,
        key: ControlKey,
        input: ControlInput,
        context: &DocumentContext,
    ) -> Result<ResolvedControl, ControlError> {
        self.check_context(context)?;
        let control = self.control(key)?;
        if !available(control) {
            return Err(ControlError::Stale);
        }
        if control.disabled {
            return Err(ControlError::Disabled);
        }
        if !self.in_active_modal(control) {
            return Err(ControlError::ModalBlocked);
        }
        let input = if let (
            Field::Range {
                value,
                min,
                max,
                step,
            },
            ControlInput::Key(key),
        ) = (&control.field, &input)
        {
            if !key.ctrl && !key.meta && !key.alt {
                let delta = step * if key.shift { 10. } else { 1. };
                let next = match key.key.as_str() {
                    "Home" => Some(*min),
                    "End" => Some(*max),
                    "ArrowUp" | "ArrowRight" => Some((value + delta).clamp(*min, *max)),
                    "ArrowDown" | "ArrowLeft" => Some((value - delta).clamp(*min, *max)),
                    _ => None,
                };
                next.map(|v| ControlInput::SetValue(v.to_string()))
                    .unwrap_or(input)
            } else {
                input
            }
        } else {
            input
        };
        if let ControlInput::SetValue(value) = &input {
            match &control.field {
                Field::Text {
                    read_only: true, ..
                } => return Err(ControlError::ReadOnly),
                Field::Text { .. } => (),
                Field::Range { min, max, .. } => {
                    let value = value
                        .parse::<f64>()
                        .map_err(|_| ControlError::InvalidValue)?;
                    if !value.is_finite() || value < *min || value > *max {
                        return Err(ControlError::InvalidValue);
                    }
                }
                Field::Choice { options, .. }
                    if options
                        .iter()
                        .any(|option| option.value == *value && !option.disabled) => {}
                Field::Choice { .. } => return Err(ControlError::OptionUnavailable),
                _ => return Err(ControlError::NotEditable),
            }
        }
        Ok(ResolvedControl {
            key,
            input,
            stamp: stamp(control),
        })
    }

    /// Recheck a queued action against its original binding, not a fresh resolve
    /// that would authorize a replacement handler attached to the same entity.
    pub fn validate_resolved(
        &self,
        resolved: &ResolvedControl,
        context: &DocumentContext,
    ) -> Result<(), ControlError> {
        self.check_context(context)?;
        if stamp(self.control(resolved.key)?) != resolved.stamp {
            return Err(ControlError::Changed);
        }
        self.resolve_key(resolved.key, resolved.input.clone(), context)
            .map(|_| ())
    }

    /// Route document shortcuts before a capture-phase CAD handler sees them.
    /// A modal owns Escape even when focus restoration has not completed yet.
    pub fn keyboard_route(
        &self,
        context: &DocumentContext,
        chord: &KeyChord,
    ) -> Result<KeyboardRoute, ControlError> {
        self.check_context(context)?;
        if !self.frame.modal_stack.is_empty() {
            return Ok(KeyboardRoute::Modal);
        }
        let focused = self
            .frame
            .focused
            .and_then(|key| self.control(key).ok())
            .filter(|control| available(control));
        let ordinary = !chord.ctrl && !chord.meta && !chord.alt;
        let key = chord.key.as_str();
        if (ordinary && key == "Tab")
            || focused.is_some_and(|control| {
                control.text_editing
                    || matches!(control.field, Field::Text { .. })
                    || control.owned_keys.contains(chord)
                    || (ordinary && matches!(key, "Enter" | " " | "Space"))
                    || (ordinary
                        && matches!(control.field, Field::Choice { .. })
                        && matches!(key, "ArrowUp" | "ArrowDown" | "Home" | "End"))
                    || (ordinary
                        && matches!(control.field, Field::Range { .. })
                        && matches!(
                            key,
                            "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight" | "Home" | "End"
                        ))
            })
        {
            Ok(KeyboardRoute::Widget)
        } else {
            Ok(KeyboardRoute::Model)
        }
    }

    /// Focus traversal follows rendered order, remains within the active modal,
    /// and skips hidden/disabled controls. It does not synthesize an activation.
    pub fn focus_target(&self, reverse: bool) -> Option<ControlKey> {
        let controls: Vec<_> = self
            .frame
            .controls
            .iter()
            .filter(|control| {
                available(control) && !control.disabled && self.in_active_modal(control)
            })
            .map(|control| control.key)
            .collect();
        if controls.is_empty() {
            return None;
        }
        let index = self
            .frame
            .focused
            .and_then(|key| controls.iter().position(|candidate| *candidate == key));
        Some(
            controls[match index {
                Some(index) if reverse => (index + controls.len() - 1) % controls.len(),
                Some(index) => (index + 1) % controls.len(),
                None if reverse => controls.len() - 1,
                None => 0,
            }],
        )
    }

    fn control(&self, key: ControlKey) -> Result<&Control, ControlError> {
        self.by_key
            .get(&key)
            .map(|index| &self.frame.controls[*index])
            .ok_or(ControlError::Stale)
    }
    fn check_context(&self, context: &DocumentContext) -> Result<(), ControlError> {
        if self.context.as_ref() == Some(context) {
            Ok(())
        } else {
            Err(ControlError::DocumentChanged)
        }
    }
    fn in_active_modal(&self, control: &Control) -> bool {
        self.frame
            .modal_stack
            .last()
            .is_none_or(|modal| control.modal_scope.as_ref() == Some(modal))
    }
}

fn available(control: &Control) -> bool {
    control.visible && control.bounds.width > 0.0 && control.bounds.height > 0.0
}

fn validate(context: &DocumentContext, frame: &SurfaceFrame) -> Result<(), ControlError> {
    let invalid = |message: &str| ControlError::InvalidFrame(message.into());
    if context.window_id.is_empty() || context.document_id.is_empty() {
        return Err(invalid("Document/window identity is required"));
    }
    let valid_rect = |rect: Rect| {
        [rect.x, rect.y, rect.width, rect.height]
            .into_iter()
            .all(f64::is_finite)
            && rect.width >= 0.0
            && rect.height >= 0.0
    };
    if !valid_rect(frame.client) {
        return Err(invalid("Client bounds are invalid"));
    }
    let mut keys = HashSet::new();
    for control in &frame.controls {
        if !keys.insert(control.key) {
            return Err(invalid("Control instance keys must be unique"));
        }
        if control.surface.is_empty() || control.role.is_empty() || !valid_rect(control.bounds) {
            return Err(invalid("Control surface, role or bounds are invalid"));
        }
        if let Field::Text {
            value,
            selection: Some(selection),
            ..
        } = &control.field
        {
            if selection.start > selection.end || selection.end > value.encode_utf16().count() {
                return Err(invalid("Text selection lies outside its UTF-16 value"));
            }
        }
        if let Field::Choice { options, .. } = &control.field {
            let mut values = HashSet::new();
            if options.iter().any(|option| !values.insert(&option.value)) {
                return Err(invalid("Choice option values must be unique"));
            }
        }
        if let Field::Range {
            value,
            min,
            max,
            step,
        } = control.field
        {
            if [value, min, max, step, max - min]
                .iter()
                .any(|v| !v.is_finite())
                || min >= max
                || value < min
                || value > max
                || step <= 0.
            {
                return Err(invalid(
                    "Numeric ranges need finite bounds, an in-range value and a positive step",
                ));
            }
        }
    }
    let mut surfaces = HashSet::new();
    if frame
        .surfaces
        .iter()
        .any(|surface| surface.name.is_empty() || !surfaces.insert(&surface.name))
    {
        return Err(invalid("Surface names must be nonempty and unique"));
    }
    let mut modals = HashSet::new();
    if frame
        .modal_stack
        .iter()
        .any(|modal| !surfaces.contains(modal) || !modals.insert(modal))
    {
        return Err(invalid(
            "Modal stack must reference distinct rendered surfaces",
        ));
    }
    if frame.controls.iter().any(|control| {
        control
            .modal_scope
            .as_ref()
            .is_some_and(|scope| !modals.contains(scope))
    }) {
        return Err(invalid("Control modal owner is not rendered"));
    }
    let mut canvases = HashSet::new();
    if frame.canvases.iter().any(|canvas| {
        canvas.name.is_empty() || !canvases.insert(&canvas.name) || !valid_rect(canvas.bounds)
    }) {
        return Err(invalid("Canvas names/bounds are invalid"));
    }
    if frame.focused.is_some_and(|key| !keys.contains(&key)) {
        return Err(invalid("Focused control is not part of this frame"));
    }
    Ok(())
}

fn normalized_label(label: &str) -> String {
    excerpt(
        &label.split_whitespace().collect::<Vec<_>>().join(" "),
        0,
        240,
    )
    .0
}

fn stamp(control: &Control) -> ControlStamp {
    ControlStamp {
        binding: control.binding,
        label: normalized_label(&control.label),
        surface: control.surface.clone(),
        role: control.role.clone(),
        field_kind: std::mem::discriminant(&control.field),
        modal_scope: control.modal_scope.clone(),
        text_editing: control.text_editing,
        owned_keys: control.owned_keys.clone(),
    }
}

fn text_value(output: &mut Value, text: &str, selection: Option<TextSelection>) {
    let length = text.encode_utf16().count();
    if length <= 4096 {
        output["value"] = json!(text);
        return;
    }
    let start = selection
        .map_or(0, |selection| selection.start.saturating_sub(512))
        .min(length - 4096);
    let (value, start, _) = excerpt(text, start, 4096);
    output["value"] = json!(value);
    output["value_truncated"] = json!(true);
    output["value_length"] = json!(length);
    output["value_start"] = json!(start);
}

/// UTF-16 offsets without slicing a Unicode scalar in half. Shift a bisected
/// start forward, as the existing editor contract does, so EOF remains visible.
fn excerpt(text: &str, requested_start: usize, limit: usize) -> (String, usize, usize) {
    let mut value = String::new();
    let mut offset = 0;
    let mut start = None;
    let mut used = 0;
    for character in text.chars() {
        let units = character.len_utf16();
        if offset >= requested_start && used + units <= limit {
            start.get_or_insert(offset);
            value.push(character);
            used += units;
        } else if start.is_some() {
            break;
        }
        offset += units;
    }
    (value, start.unwrap_or(offset), text.encode_utf16().count())
}

#[cfg(test)]
mod tests;
