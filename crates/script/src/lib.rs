//! Readable, versioned CAD command sequences. No embedded programming runtime.
//! The host owns the existing interface; this crate only resolves data references
//! and sequences calls, so live and headless execution use the same operations.
use serde_json::{json, Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Instant,
};
mod includes;
mod manufacturing;
mod source_navigation;
pub use includes::{
    flatten_includes, has_unresolved_includes, parse_with_includes, resolve_include_path,
    validate_include_path, MAX_INCLUDE_DEPTH,
};
pub use source_navigation::authored_chapters;

/// Shared limit for files, source text, the desktop picker and MCP.
pub const MAX_SCRIPT_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug)]
pub struct Script {
    document: Value,
    check_results: BTreeSet<String>,
}

#[derive(Clone, Copy, Debug)]
pub struct RunOptions {
    pub presentation: bool,
    pub validate: bool,
}
impl Default for RunOptions {
    fn default() -> Self {
        Self {
            presentation: false,
            validate: true,
        }
    }
}

impl Script {
    pub fn parse(source: &str) -> Result<Self, String> {
        if source.len() > MAX_SCRIPT_BYTES {
            return Err("Script exceeds 16 MiB".into());
        }
        let document: Value = serde_json::from_str(&strip_jsonc(source)?)
            .map_err(|e| format!("Invalid script JSONC: {e}"))?;
        if document
            .get("includes")
            .and_then(Value::as_array)
            .is_some_and(|items| !items.is_empty())
        {
            return Err(
                "This script includes other files. Open it from its file path so those files can be loaded.".into(),
            );
        }
        if document["version"] != 1 {
            return Err("Unsupported script version; expected 1".into());
        }
        if document["name"].as_str().is_none_or(str::is_empty) {
            return Err("Script needs a name".into());
        }
        let steps = document["steps"]
            .as_array()
            .ok_or("Script needs a steps array")?;
        if steps.is_empty() {
            return Err("Script needs at least one step".into());
        }
        if document.get("checks").is_some_and(|v| !v.is_array()) {
            return Err("checks must be an array".into());
        }
        if document
            .get("verification")
            .is_some_and(|v| v != "garden-bench")
        {
            return Err("Unknown final verification gate".into());
        }
        if document.get("starting_state").is_some_and(|v| v != "empty") {
            return Err("Version 1 scripts require starting_state empty".into());
        }
        let mut available = BTreeSet::<String>::new();
        let mut identities = BTreeSet::<String>::new();
        let mut check_results = BTreeSet::<String>::new();
        for (checking, section) in [(false, Some(steps)), (true, document["checks"].as_array())] {
            for (i, step) in section.into_iter().flatten().enumerate() {
                let id = step_id(step, checking, i)?;
                if !identities.insert(id.clone()) {
                    return Err(format!("Duplicate script step id {id}"));
                }
                let kinds = ["call", "note", "view", "let", "assert"]
                    .iter()
                    .filter(|k| step.get(**k).is_some())
                    .count();
                if kinds != 1 {
                    return Err(format!(
                        "Step {} needs exactly one call, note, view, let, or assert",
                        id
                    ));
                }
                let mut referenced = BTreeMap::new();
                references(step, &mut referenced);
                for name in referenced.keys() {
                    if !available.contains(name) {
                        let similar = similar_names(name, &available);
                        return Err(if similar.is_empty() {
                            format!("Step {id} references {name} before it is defined")
                        } else {
                            format!(
                                "Step {id} references {name} before it is defined; similar earlier results: {}",
                                similar.join(", ")
                            )
                        });
                    }
                }
                let mut declared = Vec::new();
                if step.get("call").is_some() {
                    declared.push(id.clone());
                    if let Some(bind) = step.get("bind") {
                        let bind = bind.as_object().ok_or("bind must be an object")?;
                        if bind.values().any(|value| {
                            value
                                .as_str()
                                .is_none_or(|path| !path.is_empty() && !path.starts_with('/'))
                        }) {
                            return Err(format!("Step {id} bind values must be JSON pointers"));
                        }
                        declared.extend(bind.keys().cloned());
                    }
                }
                if let Some(bind) = step.get("let") {
                    declared.extend(
                        bind.as_object()
                            .ok_or("let must be an object")?
                            .keys()
                            .cloned(),
                    );
                }
                for name in declared {
                    if name.trim().is_empty() || !available.insert(name.clone()) {
                        return Err(format!("Duplicate or empty script result name {name}"));
                    }
                    if checking {
                        check_results.insert(name);
                    }
                }
                if step.get("assert").is_some() && step.get("equals").is_none() {
                    return Err(format!("Step {id} assert needs equals"));
                }
                validate_presentation_step(step).map_err(|error| format!("Step {id}: {error}"))?;
                if let Some(call) = step.get("call") {
                    if !call["group"].is_string()
                        || !call["operation"].is_string()
                        || !call["arguments"].is_object()
                    {
                        return Err(format!(
                            "Step {} call needs group, operation and arguments",
                            id
                        ));
                    }
                    if matches!(
                        call["operation"].as_str(),
                        Some(
                            "cad_interface"
                                | "cad_attach"
                                | "cad_detach"
                                | "cad_submit"
                                | "cad_await_apply"
                        )
                    ) {
                        return Err(format!(
                            "Scripts cannot call {}: session ownership and transport belong to the host",
                            call["operation"].as_str().unwrap()
                        ));
                    }
                }
            }
        }
        if let Some(views) = document.get("views") {
            validate_named_views(views, true)?;
            let mut referenced = BTreeMap::new();
            references(views, &mut referenced);
            for name in referenced.keys() {
                if !available.contains(name) {
                    return Err(format!(
                        "View configuration references unknown result {name}"
                    ));
                }
            }
        }
        let mut exported = BTreeMap::new();
        references(&document["exports"], &mut exported);
        for name in exported.keys() {
            if !available.contains(name) {
                return Err(format!("Export references unknown result {name}"));
            }
        }
        Ok(Self {
            document,
            check_results,
        })
    }

    /// Check every declared operation before any calls run. The host supplies
    /// its existing interface catalog; the interpreter keeps no second registry.
    pub fn validate_calls(
        &self,
        mut validate: impl FnMut(&str, &str) -> Result<(), String>,
    ) -> Result<(), String> {
        for (checking, section) in [(false, "steps"), (true, "checks")] {
            for (index, step) in self.document[section]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                if let Some(call) = step.get("call") {
                    validate(
                        call["group"].as_str().unwrap(),
                        call["operation"].as_str().unwrap(),
                    )
                    .map_err(|error| {
                        format!("Step {}: {error}", step_id(step, checking, index).unwrap())
                    })?;
                }
            }
        }
        if self.document.get("views").is_some() {
            validate("document/appearance", "set_named_views")
                .map_err(|error| format!("Named views: {error}"))?;
        }
        Ok(())
    }

    /// Metadata for script browsers, using the validated source's own structure.
    pub fn metadata(&self) -> Value {
        let operations: BTreeSet<&str> = ["steps", "checks"]
            .into_iter()
            .flat_map(|section| self.document[section].as_array().into_iter().flatten())
            .filter_map(|step| step["call"]["operation"].as_str())
            .collect();
        let chapters: Vec<Value> = self.document["steps"]
            .as_array()
            .into_iter()
            .flatten()
            .enumerate()
            .filter_map(|(index, step)| {
                let text = step.get("note")?;
                let mut note = json!({"text": text, "step_index": index + 1});
                if let Some(chapter) = step.get("chapter") {
                    note["chapter"] = chapter.clone();
                }
                Some(note)
            })
            .collect();
        json!({
            "name": self.document["name"],
            "step_count": self.document["steps"].as_array().map_or(0, Vec::len),
            "check_count": self.document["checks"].as_array().map_or(0, Vec::len),
            "chapters": chapters,
            "max_source_bytes": MAX_SCRIPT_BYTES,
            "operations": operations,
        })
    }

    /// Reject unsupported omissions before the host creates any geometry.
    /// Names bound with `let` that no later step references: usually a face
    /// binding an agent forgot to use, or a leftover from an edit.
    pub fn unused_bindings(&self) -> Vec<String> {
        let mut uses = BTreeMap::new();
        references(&self.document, &mut uses);
        let mut unused = Vec::new();
        for step in self.document["steps"].as_array().into_iter().flatten() {
            if let Some(bindings) = step.get("let").and_then(Value::as_object) {
                for name in bindings.keys() {
                    if uses.get(name).copied().unwrap_or(0) == 0 {
                        unused.push(name.clone());
                    }
                }
            }
        }
        unused
    }

    pub fn validate_options(&self, options: RunOptions) -> Result<(), String> {
        if options.validate {
            return Ok(());
        }
        if !self.document["verification"].is_null() {
            return Err("This reference script requires its final verification gate".into());
        }
        let mut exported = BTreeMap::new();
        references(&self.document["exports"], &mut exported);
        references(&self.document["views"], &mut exported);
        if let Some(name) = exported
            .keys()
            .find(|name| self.check_results.contains(*name))
        {
            return Err(format!(
                "Cannot skip checks: exported or named view result {name} is produced by a check"
            ));
        }
        Ok(())
    }
}

fn store_named_views<F>(
    script: &Script,
    bindings: &Bindings,
    host: &mut F,
    steps_completed: usize,
) -> Result<(), String>
where
    F: FnMut(&str, Value, RunProgress) -> Result<Value, String>,
{
    let Some(views) = script.document.get("views") else {
        return Ok(());
    };
    let mut resolved = resolve(views, bindings)?;
    for view in resolved.as_array_mut().into_iter().flatten() {
        if let Some(ids) = view
            .get_mut("visible_body_ids")
            .and_then(Value::as_array_mut)
        {
            let mut seen = BTreeSet::new();
            ids.retain(|id| id.as_u64().is_none_or(|id| seen.insert(id)));
        }
    }
    validate_named_views(&resolved, false)?;
    let mut response = host(
        "cad_interface",
        json!({
            "action": "execute",
            "group": "document/appearance",
            "operation": "set_named_views",
            "arguments": { "views": resolved }
        }),
        RunProgress {
            steps_completed,
            step_count: script.document["steps"].as_array().map_or(0, Vec::len),
        },
    )?;
    if let Some(text) = response.as_str() {
        if let Ok(parsed) = serde_json::from_str(text) {
            response = parsed;
        }
    }
    if response["status"] == "failed" {
        return Err(format!("{response}"));
    }
    Ok(())
}

fn is_result_expression(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        object
            .keys()
            .any(|key| matches!(key.as_str(), "$ref" | "$select" | "$count" | "$project"))
    })
}

fn numeric_vec3(value: Option<&Value>) -> Option<[f64; 3]> {
    let coords = value?.as_array()?;
    if coords.len() != 3 {
        return None;
    }
    let mut vector = [0.0; 3];
    for (index, component) in coords.iter().enumerate() {
        vector[index] = component.as_f64()?;
    }
    Some(vector)
}

fn validate_vec3(value: &Value, label: &str) -> Result<(), String> {
    if is_result_expression(value) {
        return Ok(());
    }
    let coords = value
        .as_array()
        .filter(|coords| coords.len() == 3)
        .ok_or_else(|| format!("{label} must be a 3-number vector"))?;
    if !coords
        .iter()
        .all(|component| component.as_f64().is_some_and(|number| number.is_finite()))
    {
        return Err(format!("{label} must be a 3-number vector"));
    }
    if coords.iter().any(|component| {
        component
            .as_f64()
            .is_some_and(|number| number.abs() > 1.0e6)
    }) {
        return Err(format!("{label} must stay within 1000000 mm"));
    }
    Ok(())
}

fn validate_body_id(value: &Value, label: &str) -> Result<(), String> {
    if is_result_expression(value) {
        return Ok(());
    }
    if value.as_u64().is_some_and(|id| id >= 1) {
        return Ok(());
    }
    Err(format!("{label} must be a body id or result reference"))
}

fn validate_view_fields(object: &Map<String, Value>, allowed: &[&str]) -> Result<(), String> {
    if let Some(field) = object
        .keys()
        .find(|field| !allowed.contains(&field.as_str()))
    {
        return Err(format!("Unknown named view field '{field}'"));
    }
    Ok(())
}

fn validate_named_views(views: &Value, allow_expressions: bool) -> Result<(), String> {
    let views = views
        .as_array()
        .ok_or("views must be an array of named view configurations")?;
    let mut names = BTreeSet::new();
    for view in views {
        let view = view.as_object().ok_or("A named view must be an object")?;
        validate_view_fields(
            view,
            &["name", "camera", "visible_body_ids", "part_offsets"],
        )?;
        let name = view
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| {
                !name.is_empty()
                    && *name == name.trim()
                    && name.chars().count() <= 200
                    && !name.chars().any(char::is_control)
            })
            .ok_or("A named view needs a unique printable name")?;
        if !names.insert(name.to_owned()) {
            return Err(format!("Duplicate named view '{name}'"));
        }
        let camera = view
            .get("camera")
            .and_then(Value::as_object)
            .ok_or_else(|| format!("Named view '{name}' needs a camera"))?;
        validate_view_fields(camera, &["position", "target", "up"])?;
        for axis in ["position", "target", "up"] {
            let value = camera
                .get(axis)
                .ok_or_else(|| format!("Named view '{name}' camera needs {axis}"))?;
            if !allow_expressions && is_result_expression(value) {
                return Err(format!(
                    "Named view '{name}' camera {axis} must resolve to a vector"
                ));
            }
            validate_vec3(value, &format!("Named view '{name}' camera {axis}"))?;
        }
        if let Some(up) = camera.get("up").and_then(Value::as_array) {
            let up: Vec<f64> = up.iter().filter_map(Value::as_f64).collect();
            if up.len() == 3 && up[0] * up[0] + up[1] * up[1] + up[2] * up[2] <= 1e-24 {
                return Err(format!("Named view '{name}' needs a non-zero camera up"));
            }
        }
        if let (Some(position), Some(target)) = (
            numeric_vec3(camera.get("position")),
            numeric_vec3(camera.get("target")),
        ) {
            let direction = [
                position[0] - target[0],
                position[1] - target[1],
                position[2] - target[2],
            ];
            let direction_length = direction
                .iter()
                .map(|component| component * component)
                .sum::<f64>();
            if direction_length <= 1e-12 {
                return Err(format!(
                    "Named view '{name}' camera position and target must differ"
                ));
            }
            if let Some(up) = numeric_vec3(camera.get("up")) {
                let up_length = up
                    .iter()
                    .map(|component| component * component)
                    .sum::<f64>();
                let cross = [
                    direction[1] * up[2] - direction[2] * up[1],
                    direction[2] * up[0] - direction[0] * up[2],
                    direction[0] * up[1] - direction[1] * up[0],
                ];
                let cross_length = cross
                    .iter()
                    .map(|component| component * component)
                    .sum::<f64>();
                if up_length > 1e-24 && cross_length <= direction_length * up_length * 1e-12 {
                    return Err(format!(
                        "Named view '{name}' camera up must not be parallel to the view direction"
                    ));
                }
            }
        }
        let visible = view
            .get("visible_body_ids")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("Named view '{name}' needs visible_body_ids"))?;
        let mut visible_ids = BTreeSet::new();
        for id in visible {
            if !allow_expressions && is_result_expression(id) {
                return Err(format!(
                    "Named view '{name}' visible body must resolve to an id"
                ));
            }
            validate_body_id(id, &format!("Named view '{name}' visible body"))?;
            if let Some(id) = id.as_u64() {
                if !visible_ids.insert(id) {
                    return Err(format!("Named view '{name}' has a duplicate visible body"));
                }
            }
        }
        if let Some(offsets) = view.get("part_offsets") {
            let offsets = offsets
                .as_array()
                .ok_or_else(|| format!("Named view '{name}' part_offsets must be an array"))?;
            let mut offset_ids = BTreeSet::new();
            for offset in offsets {
                let offset = offset
                    .as_object()
                    .ok_or("A part offset must be an object")?;
                validate_view_fields(offset, &["body_id", "translation"])?;
                let body = offset
                    .get("body_id")
                    .ok_or("A part offset needs a body_id")?;
                if !allow_expressions && is_result_expression(body) {
                    return Err("A part offset body must resolve to an id".into());
                }
                validate_body_id(body, &format!("Named view '{name}' part offset"))?;
                if let Some(id) = body.as_u64() {
                    if !offset_ids.insert(id) {
                        return Err(format!("Named view '{name}' has a duplicate part offset"));
                    }
                }
                let translation = offset
                    .get("translation")
                    .ok_or("A part offset needs a translation")?;
                if !allow_expressions && is_result_expression(translation) {
                    return Err("A part offset translation must resolve to a vector".into());
                }
                validate_vec3(translation, &format!("Named view '{name}' part offset"))?;
            }
        }
    }
    Ok(())
}

fn step_id(step: &Value, checking: bool, index: usize) -> Result<String, String> {
    match step.get("id") {
        None => Ok(format!(
            "{}{}",
            if checking { "check-" } else { "step-" },
            index + 1
        )),
        Some(value) => value
            .as_str()
            .filter(|name| !name.trim().is_empty())
            .map(str::to_owned)
            .ok_or_else(|| "Step id must be a nonempty string".into()),
    }
}

fn validate_presentation_step(step: &Value) -> Result<(), String> {
    if step.get("note").is_none() && step.get("view").is_none() {
        return Ok(());
    }
    let is_note = step.get("note").is_some();
    let allowed: &[&str] = if is_note {
        &["id", "note", "chapter", "duration_ms"]
    } else {
        &[
            "id",
            "view",
            "fit",
            "target",
            "body_id",
            "component_id",
            "duration_ms",
            "orbit_degrees",
        ]
    };
    for key in step
        .as_object()
        .ok_or("Presentation step must be an object")?
        .keys()
    {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("Unknown presentation field {key}"));
        }
    }
    if let Some(duration) = step.get("duration_ms") {
        if !duration.as_u64().is_some_and(|value| value <= 10_000) {
            return Err("duration_ms must be an integer from 0 to 10000".into());
        }
    }
    if is_note {
        for (key, limit) in [("note", 4000), ("chapter", 200)] {
            if let Some(value) = step.get(key) {
                let text = value
                    .as_str()
                    .ok_or_else(|| format!("{key} must be text"))?;
                if text.chars().count() > limit
                    || text
                        .chars()
                        .any(|c| c.is_control() && c != '\n' && c != '\t')
                {
                    return Err(format!(
                        "{key} must contain at most {limit} printable characters"
                    ));
                }
            }
        }
    } else {
        if let Some(angle) = step.get("orbit_degrees") {
            if !angle
                .as_f64()
                .is_some_and(|angle| angle.is_finite() && (-360.0..=360.0).contains(&angle))
                || step["view"] != "current"
            {
                return Err(
                    "orbit_degrees requires current view and a finite angle from -360 to 360"
                        .into(),
                );
            }
        }
        if !matches!(
            step["view"].as_str(),
            Some("current" | "isometric" | "top" | "bottom" | "front" | "back" | "left" | "right")
        ) {
            return Err("Unknown camera view".into());
        }
        if step.get("fit").is_some_and(|value| !value.is_boolean()) {
            return Err("fit must be a boolean".into());
        }
        if step
            .get("target")
            .is_some_and(|value| value != "active_sketch")
        {
            return Err("target must be active_sketch".into());
        }
        for key in ["body_id", "component_id"] {
            if let Some(value) = step.get(key) {
                if value.as_u64().is_none()
                    && !value.as_object().is_some_and(|object| {
                        object
                            .keys()
                            .any(|key| matches!(key.as_str(), "$ref" | "$select" | "$count"))
                    })
                {
                    return Err(format!("{key} must be an unsigned ID or result reference"));
                }
            }
        }
        if ["target", "body_id", "component_id"]
            .iter()
            .filter(|key| step.get(**key).is_some())
            .count()
            > 1
        {
            return Err("Camera view accepts only one focal target".into());
        }
    }
    Ok(())
}

/// Comments become spaces, preserving serde's original line/column diagnostics.
/// JSON strings are never changed. Trailing commas are accepted for easy edits.
pub(crate) fn strip_jsonc(source: &str) -> Result<String, String> {
    let mut bytes = source.as_bytes().to_vec();
    let (mut i, mut string, mut escape) = (0, false, false);
    while i < bytes.len() {
        if string {
            if escape {
                escape = false;
            } else if bytes[i] == b'\\' {
                escape = true;
            } else if bytes[i] == b'"' {
                string = false;
            }
            i += 1;
            continue;
        }
        if bytes[i] == b'"' {
            string = true;
            i += 1;
            continue;
        }
        if bytes.get(i..i + 2) == Some(b"//") {
            while i < bytes.len() && bytes[i] != b'\n' {
                bytes[i] = b' ';
                i += 1;
            }
        } else if bytes.get(i..i + 2) == Some(b"/*") {
            bytes[i] = b' ';
            bytes[i + 1] = b' ';
            i += 2;
            let mut closed = false;
            while i < bytes.len() {
                if bytes.get(i..i + 2) == Some(b"*/") {
                    bytes[i] = b' ';
                    bytes[i + 1] = b' ';
                    i += 2;
                    closed = true;
                    break;
                }
                if bytes[i] != b'\n' && bytes[i] != b'\r' {
                    bytes[i] = b' ';
                }
                i += 1;
            }
            if !closed {
                return Err("Unterminated JSONC block comment".into());
            }
        } else {
            i += 1;
        }
    }
    string = false;
    escape = false;
    for i in 0..bytes.len() {
        if string {
            if escape {
                escape = false;
            } else if bytes[i] == b'\\' {
                escape = true;
            } else if bytes[i] == b'"' {
                string = false;
            }
        } else if bytes[i] == b'"' {
            string = true;
        } else if bytes[i] == b','
            && bytes[i + 1..]
                .iter()
                .copied()
                .find(|c| !c.is_ascii_whitespace())
                .is_some_and(|c| c == b']' || c == b'}')
        {
            bytes[i] = b' ';
        }
    }
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

type Bindings = BTreeMap<String, Value>;
fn references(value: &Value, counts: &mut BTreeMap<String, usize>) {
    match value {
        Value::Object(object) => {
            if let Some(name) = object.get("$ref").and_then(Value::as_str) {
                *counts.entry(name.to_owned()).or_default() += 1;
            }
            for value in object.values() {
                references(value, counts);
            }
        }
        Value::Array(values) => {
            for value in values {
                references(value, counts);
            }
        }
        _ => {}
    }
}
/// Compact description of selector candidates: the fields an agent needs to tell
/// geometry apart, never whole meshes.
fn candidate_digest(found: &[&Value]) -> String {
    const FIELDS: [&str; 9] = [
        "/id",
        "/kind",
        "/name",
        "/plane/origin",
        "/plane/normal",
        "/cylinder/origin",
        "/cylinder/radius",
        "/signature/area",
        "/position",
    ];
    let digests: Vec<String> = found
        .iter()
        .take(4)
        .map(|value| {
            let mut fields = serde_json::Map::new();
            for path in FIELDS {
                if let Some(field) = value.pointer(path) {
                    fields.insert(path.trim_start_matches('/').to_owned(), field.clone());
                }
            }
            if fields.is_empty() {
                let text = value.to_string();
                text.chars().take(120).collect()
            } else {
                Value::Object(fields).to_string()
            }
        })
        .collect();
    let mut text = digests.join(", ");
    if found.len() > 4 {
        text.push_str(&format!(", and {} more", found.len() - 4));
    }
    text
}

/// Explain an empty selection: what each where test asked for, and which values
/// the entries actually carry at that pointer, so the next where test is one edit.
fn selector_no_match(select: &Value, array: &[Value], bindings: &Bindings) -> String {
    let Some(where_tests) = select.get("where") else {
        return format!("Selector matched no geometry among {} entries", array.len());
    };
    let resolved = resolve(where_tests, bindings).unwrap_or_else(|_| where_tests.clone());
    let Some(tests) = resolved.as_object() else {
        return format!("Selector matched no geometry among {} entries", array.len());
    };
    let mut wanted = Vec::new();
    let mut present = Vec::new();
    for (path, expected) in tests {
        wanted.push(format!("{path} = {expected}"));
        let mut values: Vec<String> = Vec::new();
        for entry in array {
            if let Some(value) = entry.pointer(path) {
                let text = value.to_string();
                if !values.contains(&text) {
                    values.push(text);
                }
            }
            if values.len() >= 6 {
                break;
            }
        }
        present.push(format!("{path}: [{}]", values.join(", ")));
    }
    format!(
        "Selector matched no geometry among {} entries. Where tests: {}. Values present at those pointers: {}.",
        array.len(),
        wanted.join(", "),
        present.join("; ")
    )
}

/// Earlier result names an agent probably meant: a shared prefix or an edit
/// distance of at most two.
fn similar_names(name: &str, available: &BTreeSet<String>) -> Vec<String> {
    let mut similar: Vec<String> = available
        .iter()
        .filter(|candidate| {
            let prefix = name.len().min(candidate.len()).min(6);
            (prefix >= 4 && candidate[..prefix] == name[..prefix])
                || candidate.starts_with(name)
                || name.starts_with(candidate.as_str())
                || edit_distance(name, candidate) <= 2
        })
        .cloned()
        .collect();
    similar.truncate(3);
    similar
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut previous: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut current = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            current.push(
                (previous[j] + cost)
                    .min(previous[j + 1] + 1)
                    .min(current[j] + 1),
            );
        }
        previous = current;
    }
    previous[b.len()]
}

fn pointer<'a>(value: &'a Value, path: &str) -> Result<&'a Value, String> {
    if path.is_empty() {
        Ok(value)
    } else {
        value
            .pointer(path)
            .ok_or_else(|| format!("Missing result path {path}"))
    }
}
fn equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            if a.is_u64() && b.is_u64() {
                a.as_u64() == b.as_u64()
            } else {
                a.as_f64()
                    .zip(b.as_f64())
                    .is_some_and(|(a, b)| (a - b).abs() <= 1e-6)
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| equal(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len() && a.iter().all(|(k, a)| b.get(k).is_some_and(|b| equal(a, b)))
        }
        _ => a == b,
    }
}
fn matches(value: &Value, predicate: &Value, bindings: &Bindings) -> Result<bool, String> {
    let object = predicate
        .as_object()
        .ok_or("Selector where must be an object")?;
    for (path, expected) in object {
        let yes = match path.as_str() {
            "$and" | "$or" => {
                let terms = expected.as_array().ok_or("and/or requires an array")?;
                let results = terms
                    .iter()
                    .map(|p| matches(value, p, bindings))
                    .collect::<Result<Vec<_>, _>>()?;
                if path == "$and" {
                    results.into_iter().all(|b| b)
                } else {
                    results.into_iter().any(|b| b)
                }
            }
            "$every" => {
                let array = pointer(
                    value,
                    expected["path"].as_str().ok_or("every needs a path")?,
                )?
                .as_array()
                .ok_or("every path must be an array")?;
                !array.is_empty()
                    && array
                        .iter()
                        .map(|v| matches(v, &expected["where"], bindings))
                        .collect::<Result<Vec<_>, _>>()?
                        .into_iter()
                        .all(|b| b)
            }
            _ => value.pointer(path).is_some_and(|actual| {
                resolve(expected, bindings).is_ok_and(|expected| equal(actual, &expected))
            }),
        };
        if !yes {
            return Ok(false);
        }
    }
    Ok(true)
}

fn resolve(expression: &Value, bindings: &Bindings) -> Result<Value, String> {
    match expression {
        Value::Array(values) => values.iter().map(|v| resolve(v, bindings)).collect(),
        Value::Object(object) => {
            if let Some(name) = object.get("$ref") {
                let name = name.as_str().ok_or("ref must name a prior result")?;
                let value = bindings
                    .get(name)
                    .ok_or_else(|| format!("Unknown result reference {name}"))?;
                return Ok(pointer(
                    value,
                    object.get("pointer").and_then(Value::as_str).unwrap_or(""),
                )?
                .clone());
            }
            if let Some(select) = object.get("$select") {
                let source = resolve(&select["from"], bindings)?;
                let array = pointer(&source, select["path"].as_str().unwrap_or(""))?
                    .as_array()
                    .ok_or("select needs an array")?;
                let mut found = Vec::new();
                for value in array {
                    if select
                        .get("where")
                        .map(|p| matches(value, p, bindings))
                        .transpose()?
                        .unwrap_or(true)
                    {
                        found.push(value);
                    }
                }
                let path = select["pointer"].as_str().unwrap_or("");
                let take = select["take"].as_str().unwrap_or("one");
                if take == "all" {
                    return found
                        .into_iter()
                        .map(|v| pointer(v, path).cloned())
                        .collect();
                }
                let value = match take {
                    "one" if found.len() == 1 => found[0],
                    "first" => *found
                        .first()
                        .ok_or_else(|| selector_no_match(select, array, bindings))?,
                    "last" => *found
                        .last()
                        .ok_or_else(|| selector_no_match(select, array, bindings))?,
                    "one" if found.is_empty() => {
                        return Err(selector_no_match(select, array, bindings))
                    }
                    "one" => {
                        return Err(format!(
                            "Selector expected exactly one match, found {}. Candidates: {}. Add a where test on a field that separates them (for a face, for example /plane/origin/2).",
                            found.len(),
                            candidate_digest(&found)
                        ))
                    }
                    _ => return Err(format!("Unknown selector take {take}")),
                };
                return Ok(pointer(value, path)?.clone());
            }
            if let Some(count) = object.get("$count") {
                let value = resolve(count, bindings)?;
                return Ok(json!(value
                    .as_array()
                    .ok_or("count requires an array")?
                    .len()));
            }
            if let Some(project) = object.get("$project") {
                let point = resolve(&project["point"], bindings)?;
                let basis = resolve(&project["basis"], bindings)?;
                let vector = |value: &Value| -> Result<[f64; 3], String> {
                    if let Some(a) = value.as_array() {
                        if a.len() == 3 {
                            return Ok([
                                a[0].as_f64().ok_or("Coordinate must be numeric")?,
                                a[1].as_f64().ok_or("Coordinate must be numeric")?,
                                a[2].as_f64().ok_or("Coordinate must be numeric")?,
                            ]);
                        }
                    }
                    Ok([
                        value["x"].as_f64().ok_or("Coordinate x missing")?,
                        value["y"].as_f64().ok_or("Coordinate y missing")?,
                        value["z"].as_f64().ok_or("Coordinate z missing")?,
                    ])
                };
                let point = vector(&point)?;
                let origin = vector(&basis["origin"])?;
                let u = vector(&basis["u"])?;
                let v = vector(&basis["v"])?;
                let dot = |axis: [f64; 3]| {
                    (0..3)
                        .map(|i| (point[i] - origin[i]) * axis[i])
                        .sum::<f64>()
                };
                return Ok(json!({"x":dot(u),"y":dot(v)}));
            }
            object
                .iter()
                .map(|(k, v)| Ok((k.clone(), resolve(v, bindings)?)))
                .collect::<Result<Map<_, _>, String>>()
                .map(Value::Object)
        }
        _ => Ok(expression.clone()),
    }
}

/// Completed authored steps before the current host call. A call is not counted
/// until its response, expectations and bindings have all succeeded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunProgress {
    pub steps_completed: usize,
    pub step_count: usize,
}

pub fn run<F>(script: &Script, mut host: F, options: RunOptions) -> Result<Value, String>
where
    F: FnMut(&str, Value) -> Result<Value, String>,
{
    run_with_progress(script, |name, args, _| host(name, args), options)
}

/// Run the same interpreter with progress carried on existing host calls;
/// observing progress never adds calls or changes authored execution order.
pub fn run_with_progress<F>(
    script: &Script,
    mut host: F,
    options: RunOptions,
) -> Result<Value, String>
where
    F: FnMut(&str, Value, RunProgress) -> Result<Value, String>,
{
    script.validate_options(options)?;
    let started = Instant::now();
    let mut bindings = Bindings::new();
    let mut completed = 0;
    let mut checks = 0;
    let mut uses = BTreeMap::new();
    references(&script.document, &mut uses);
    for (checking, steps) in [
        (false, script.document["steps"].as_array()),
        (true, script.document["checks"].as_array()),
    ] {
        if checking && !options.validate {
            continue;
        }
        for (index, step) in steps.into_iter().flatten().enumerate() {
            let id = step_id(step, checking, index)?;
            let progress = RunProgress {
                steps_completed: completed,
                step_count: script.document["steps"].as_array().unwrap().len(),
            };
            let result = (|| -> Result<(), String> {
                if let Some(values) = step.get("let") {
                    for (name, expression) in values.as_object().ok_or("let requires an object")? {
                        let value = resolve(expression, &bindings)?;
                        if bindings.insert(name.clone(), value).is_some() {
                            return Err(format!("Duplicate binding {name}"));
                        }
                    }
                } else if let Some(expression) = step.get("assert") {
                    let actual = resolve(expression, &bindings)?;
                    let expected = resolve(&step["equals"], &bindings)?;
                    if !equal(&actual, &expected) {
                        return Err(format!(
                            "Assertion failed: actual {actual}, expected {expected}"
                        ));
                    }
                } else if let Some(call) = step.get("call") {
                    let arguments = resolve(&call["arguments"], &bindings)?;
                    let mut result = host(
                        "cad_interface",
                        json!({"action":"execute","group":call["group"],"operation":call["operation"],"arguments":arguments}),
                        progress,
                    )?;
                    if let Some(text) = result.as_str() {
                        if let Ok(parsed) = serde_json::from_str(text) {
                            result = parsed;
                        }
                    }
                    if result["status"] == "failed" {
                        return Err(format!("Operation rejected: {result}"));
                    }
                    if result
                        .pointer("/scene/errors")
                        .and_then(Value::as_array)
                        .is_some_and(|errors| !errors.is_empty())
                    {
                        return Err(format!(
                            "Geometry operation failed: {}",
                            result["scene"]["errors"]
                        ));
                    }
                    if let Some(expect) = step.get("expect") {
                        if !matches(&result, expect, &bindings)? {
                            return Err(format!("Result did not satisfy {expect}"));
                        }
                    }
                    if let Some(bind) = step.get("bind") {
                        for (name, path) in bind.as_object().ok_or("bind requires an object")? {
                            let value = pointer(
                                &result,
                                path.as_str().ok_or("bind value must be a JSON pointer")?,
                            )?
                            .clone();
                            if bindings.insert(name.clone(), value).is_some() {
                                return Err(format!("Duplicate binding {name}"));
                            }
                        }
                    }
                    if bindings.insert(id.clone(), result).is_some() {
                        return Err(format!("Duplicate step id {id}"));
                    }
                } else if options.presentation {
                    let mut request = step.as_object().ok_or("Step must be an object")?.clone();
                    request.remove("id");
                    if let Some(note) = request.remove("note") {
                        request.insert("action".into(), json!("presentation"));
                        request.insert("command".into(), json!("note"));
                        request.insert("text".into(), note);
                        request.insert("step_index".into(), json!(index + 1));
                        request.insert("step_count".into(), json!(steps.unwrap().len()));
                    } else {
                        request.insert("action".into(), json!("view"));
                    }
                    let response = host(
                        "cad_interface",
                        resolve(&Value::Object(request), &bindings)?,
                        progress,
                    )?;
                    if response["status"] == "failed" {
                        return Err(format!("Presentation failed: {response}"));
                    }
                }
                Ok(())
            })();
            result.map_err(|error| {
                format!(
                    "Script '{}' stopped at {id} ({} completed): {error}",
                    script.document["name"],
                    completed + checks
                )
            })?;
            if checking {
                checks += 1;
            } else {
                completed += 1;
            }

            let mut consumed = BTreeMap::new();
            references(step, &mut consumed);
            for (name, count) in consumed {
                if let Some(remaining) = uses.get_mut(&name) {
                    *remaining = remaining.saturating_sub(count);
                }
            }
            bindings.retain(|name, _| uses.get(name).copied().unwrap_or(0) > 0);
        }
    }
    let exports = script
        .document
        .get("exports")
        .map(|e| resolve(e, &bindings))
        .transpose()?
        .unwrap_or(json!({}));
    if options.validate && script.document["verification"] == "garden-bench" {
        manufacturing::bench(&exports)?;
    }
    store_named_views(script, &bindings, &mut host, completed).map_err(|error| {
        format!(
            "Script '{}' could not store named views: {error}",
            script.document["name"]
        )
    })?;
    Ok(
        json!({"name":script.document["name"],"steps_completed":completed,"checks_completed":checks,"elapsed_ms":started.elapsed().as_millis(),"exports":exports}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn garden_bench_notches_select_geometry_not_profile_order() {
        let script = Script::parse(include_str!(
            "../../../examples/scripts/garden-bench.limo.jsonc"
        ))
        .unwrap();
        for (part, area, outside_area) in [
            ("front", 3933.0, 345.0),
            ("second", 483.0, 69.0),
            ("rear", 4623.0, 345.0),
        ] {
            for side in [2, 3] {
                let step = script.document["steps"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|step| step["id"] == format!("seat_{part}_extrude_{side}"))
                    .unwrap();
                let args = &step["call"]["arguments"];
                let selection = &args["profile_indices"];
                let binding = format!("seat_{part}_profiles_{side}");

                for selected_id in [17, 91] {
                    let intended = json!({"index":selected_id,"area":area});
                    let mut profiles = vec![
                        json!({"index":0,"area":102000.0 - area}),
                        intended.clone(),
                        json!({"index":3,"area":outside_area}),
                    ];
                    if selected_id == 91 {
                        profiles.rotate_left(1);
                    }
                    let catalog = |profiles: Vec<Value>| {
                        json!([
                            {"sketch_name":"Unrelated sketch","profiles":[intended.clone()]},
                            {"sketch_name":args["sketch_name"],"profiles":profiles}
                        ])
                    };
                    let mut bindings =
                        Bindings::from([(binding.clone(), catalog(profiles.clone()))]);
                    assert_eq!(
                        resolve(selection, &bindings).unwrap(),
                        json!([selected_id]),
                        "{part} notch {side} must select its in-stock area"
                    );

                    profiles.push(intended.clone());
                    bindings.insert(binding.clone(), catalog(profiles));
                    assert!(
                        resolve(selection, &bindings).is_err(),
                        "an ambiguous notch must fail before cutting"
                    );
                    bindings.insert(binding.clone(), catalog(vec![]));
                    assert!(
                        resolve(selection, &bindings).is_err(),
                        "a missing notch must not fall back to the remainder"
                    );
                }
            }
        }
    }

    #[test]
    fn source_limit_and_chapter_positions_are_shared_by_all_hosts() {
        let source = r#"{"version":1,"name":"Chapters","steps":[{"let":{"size":12}},{"chapter":"Edit","note":"Change the dimension"}]}"#;
        let padded = format!("{}{}", " ".repeat(2 * 1024 * 1024 + 1), source);
        let metadata = Script::parse(&padded).unwrap().metadata();
        assert_eq!(metadata["chapters"][0]["step_index"], 2);
        assert_eq!(metadata["max_source_bytes"], MAX_SCRIPT_BYTES);
        let boundary = format!("{}{}", source, " ".repeat(MAX_SCRIPT_BYTES - source.len()));
        assert!(Script::parse(&boundary).is_ok());
        assert!(Script::parse(&format!("{boundary} "))
            .unwrap_err()
            .contains("16 MiB"));
        assert!(Script::parse(&" ".repeat(MAX_SCRIPT_BYTES + 1))
            .unwrap_err()
            .contains("16 MiB"));
    }

    #[test]
    fn host_progress_counts_completed_steps_without_extra_calls_or_early_success() {
        let script = Script::parse(r#"{"version":1,"name":"Progress","steps":[
            {"note":"Skipped in fast mode"},{"let":{"size":7}},
            {"call":{"group":"g","operation":"make","arguments":{"size":{"$ref":"size"}}},"expect":{"/id":7}},
            {"assert":{"$ref":"size"},"equals":7},{"view":"isometric"},
            {"call":{"group":"g","operation":"draw","arguments":{}}}
        ],"checks":[{"call":{"group":"g","operation":"inspect","arguments":{}},"expect":{"/id":7}}]}"#).unwrap();
        let mut observed = Vec::new();
        let report = run_with_progress(
            &script,
            |name, args, progress| {
                observed.push((name.to_string(), args, progress));
                Ok(json!({"id":7}))
            },
            RunOptions::default(),
        )
        .unwrap();
        assert_eq!(
            observed.iter().map(|(_, _, p)| *p).collect::<Vec<_>>(),
            [2, 5, 6].map(|steps_completed| RunProgress {
                steps_completed,
                step_count: 6
            })
        );
        assert_eq!(report["steps_completed"], 6);
        assert_eq!(report["checks_completed"], 1);
        let mut plain_calls = Vec::new();
        let plain = run(
            &script,
            |name, args| {
                plain_calls.push((name.to_string(), args));
                Ok(json!({"id":7}))
            },
            RunOptions::default(),
        )
        .unwrap();
        assert_eq!(
            plain_calls,
            observed
                .into_iter()
                .map(|(name, args, _)| (name, args))
                .collect::<Vec<_>>()
        );
        assert_eq!(plain["exports"], report["exports"]);
        let mut failed_progress = Vec::new();
        let error = run_with_progress(
            &script,
            |_, _, progress| {
                failed_progress.push(progress);
                Ok(json!({"id":8}))
            },
            RunOptions::default(),
        )
        .unwrap_err();
        assert!(error.contains("2 completed") && error.contains("Result did not satisfy"));
        assert_eq!(
            failed_progress,
            [RunProgress {
                steps_completed: 2,
                step_count: 6
            }]
        );
    }

    #[test]
    fn generated_result_ids_and_duplicate_names_are_checked_before_execution() {
        let script = Script::parse(r#"{"version":1,"name":"implicit references","steps":[
            {"call":{"group":"solid/create","operation":"make","arguments":{}}},
            {"assert":{"$ref":"step-1","pointer":"/id"},"equals":7}
        ],"checks":[{"call":{"group":"inspect","operation":"read","arguments":{}}}],"exports":{"final":{"$ref":"check-1"}}}"#).unwrap();
        let result = run(&script, |_, _| Ok(json!({"id":7})), RunOptions::default()).unwrap();
        assert_eq!(result["exports"]["final"]["id"], 7);
        for source in [
            r#"{"version":1,"name":"duplicate implicit","steps":[{"call":{"group":"g","operation":"first","arguments":{}}},{"id":"step-1","call":{"group":"g","operation":"later","arguments":{}}}]}"#,
            r#"{"version":1,"name":"duplicate notes","steps":[{"id":"intro","note":"First"},{"id":"intro","note":"Second"}]}"#,
            r#"{"version":1,"name":"duplicate result","steps":[{"id":"first","call":{"group":"g","operation":"first","arguments":{}},"bind":{"first":"/id"}}]}"#,
            r#"{"version":1,"name":"forward reference","steps":[{"assert":{"$ref":"future"},"equals":1},{"let":{"future":1}}]}"#,
        ] {
            assert!(Script::parse(source).is_err(), "{source}");
        }
    }

    #[test]
    fn dependent_exports_cannot_skip_checks_after_building_the_model() {
        let script = Script::parse(r#"{"version":1,"name":"required exports","steps":[{"call":{"group":"g","operation":"make","arguments":{}}}],"checks":[{"id":"final","call":{"group":"g","operation":"inspect","arguments":{}}}],"exports":{"model":{"$ref":"final"}}}"#).unwrap();
        let mut calls = 0;
        let error = run(
            &script,
            |_, _| {
                calls += 1;
                Ok(json!({}))
            },
            RunOptions {
                presentation: false,
                validate: false,
            },
        )
        .unwrap_err();
        assert!(error.contains("Cannot skip checks"));
        assert_eq!(calls, 0);
        let gated = Script::parse(r#"{"version":1,"name":"required gate","verification":"garden-bench","steps":[{"note":"Start"}]}"#).unwrap();
        assert!(gated
            .validate_options(RunOptions {
                presentation: false,
                validate: false
            })
            .unwrap_err()
            .contains("requires"));
    }

    #[test]
    fn malformed_presentations_fail_preflight_even_for_fast_mode() {
        for step in [
            json!({"note":42}),
            json!({"note":"x","duration_ms":-1}),
            json!({"note":"x","duraton_ms":50}),
            json!({"note":"x","chapter":"x".repeat(201)}),
            json!({"view":"sideways"}),
            json!({"view":"front","fit":"yes"}),
            json!({"view":"current","body_id":-1}),
            json!({"view":"current","body_id":1,"component_id":2}),
            json!({"view":"current","orbit_degrees":361}),
            json!({"view":"current","orbit_degrees":-361}),
            json!({"view":"current","orbit_degrees":"120"}),
            json!({"view":"isometric","orbit_degrees":120}),
            json!({"assert":null}),
        ] {
            let source = json!({"version":1,"name":"invalid late presentation","steps":[{"call":{"group":"g","operation":"make","arguments":{}}},step]}).to_string();
            assert!(Script::parse(&source).is_err(), "{source}");
        }
    }

    #[test]
    fn fast_and_present_modes_execute_identical_modeling_and_checks() {
        let script = Script::parse(r#"{"version":1,"name":"two modes","steps":[
            {"note":"First body","chapter":"Create","duration_ms":300},
            {"id":"part","call":{"group":"solid/create","operation":"make","arguments":{}}},
            {"view":"current","body_id":{"$ref":"part","pointer":"/id"},"orbit_degrees":360,"duration_ms":3000},
            {"call":{"group":"solid/modify","operation":"edit","arguments":{"body_id":{"$ref":"part","pointer":"/id"}}}}
        ],"checks":[{"assert":{"$ref":"part","pointer":"/id"},"equals":7}],"exports":{"part":{"$ref":"part"}}}"#).unwrap();
        let mut runs = vec![];
        for presentation in [false, true] {
            let mut calls = vec![];
            let mut orbits = vec![];
            let report = run(
                &script,
                |_, args| {
                    if args["action"] == "execute" {
                        calls.push(args);
                        Ok(json!({"id":7}))
                    } else {
                        if args.get("orbit_degrees").is_some() {
                            orbits.push(args);
                        }
                        Ok(json!({"status":"applied"}))
                    }
                },
                RunOptions {
                    presentation,
                    validate: true,
                },
            )
            .unwrap();
            assert_eq!(report["exports"]["part"]["id"], 7);
            assert_eq!(orbits.len(), usize::from(presentation));
            if presentation {
                assert_eq!(
                    orbits[0],
                    json!({"action":"view","view":"current","body_id":7,"orbit_degrees":360,"duration_ms":3000})
                );
            }
            runs.push(calls);
        }
        assert_eq!(runs[0], runs[1]);
    }

    #[test]
    fn comments_preserve_strings_and_accept_trailing_commas() {
        let text =
            "{\n// comment\n\"url\":\"https://example/*literal*/\",/* multi\nline */\"a\":[1,],}";
        let clean = strip_jsonc(text).unwrap();
        assert_eq!(clean.lines().count(), text.lines().count());
        let value: Value = serde_json::from_str(&clean).unwrap();
        assert_eq!(value["url"], "https://example/*literal*/");
        assert_eq!(value["a"], json!([1]));
        assert!(strip_jsonc("{/*never closes").is_err());
    }
    #[test]
    fn resolves_geometry_from_new_results_and_stops_on_ambiguity() {
        let bindings = BTreeMap::from([(
            "shape".into(),
            json!({"edges":[{"id":900,"points":[{"z":5.0},{"z":5.0000001}]},{"id":901,"points":[{"z":0}]}]}),
        )]);
        let select = json!({"$select":{"from":{"$ref":"shape"},"path":"/edges","where":{"$every":{"path":"/points","where":{"/z":5.0}}},"pointer":"/id"}});
        assert_eq!(resolve(&select, &bindings).unwrap(), 900);
        assert!(resolve(
            &json!({"$select":{"from":{"$ref":"shape"},"path":"/edges"}}),
            &bindings
        )
        .is_err());
        assert!(resolve(&json!({"$ref":"missing"}), &bindings).is_err());
    }
    #[test]
    fn replay_uses_response_references_and_fast_skips_only_presentation() {
        let script=Script::parse(r#"{"version":1,"name":"test","steps":[{"note":"A","chapter":"start"},{"id":"made","call":{"group":"solid/create","operation":"make","arguments":{}}},{"call":{"group":"solid/modify","operation":"edit","arguments":{"body_id":{"$ref":"made","pointer":"/id"}}}}],"checks":[{"assert":{"$ref":"made","pointer":"/id"},"equals":123}]}"#).unwrap();
        let mut calls = vec![];
        let result = run(
            &script,
            |_, args| {
                calls.push(args.clone());
                Ok(json!({"id":123}))
            },
            RunOptions::default(),
        )
        .unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[1]["arguments"]["body_id"], 123);
        assert_eq!(result["checks_completed"], 1);
    }

    #[test]
    fn malformed_named_views_fail_before_modeling() {
        let view = json!({"name":"review", "camera":{"position":[10,0,0],"target":[0,0,0],"up":[0,0,1]},"visible_body_ids":[1]});
        let source = |view: Value| {
            json!({"version":1,"name":"review","steps":[{"call":{"group":"solid/create","operation":"make","arguments":{}}}],"views":[view]}).to_string()
        };
        let mut invalid = vec![];
        let mut value = view.clone();
        value["name"] = json!(" review ");
        invalid.push(value);
        let mut value = view.clone();
        value["visible_body_ids"] = json!([1, 1]);
        invalid.push(value);
        let mut value = view.clone();
        value["extra"] = json!(true);
        invalid.push(value);
        let mut value = view.clone();
        value["camera"]["extra"] = json!(true);
        invalid.push(value);
        let mut value = view.clone();
        value["part_offsets"] = json!([
            {"body_id":1,"translation":[0,1,0]}, {"body_id":1,"translation":[0,2,0]}
        ]);
        invalid.push(value);
        let mut value = view.clone();
        value["part_offsets"] = json!([
            {"body_id":1,"translation":[0,1,0],"extra":true}
        ]);
        invalid.push(value);
        for value in invalid {
            assert!(Script::parse(&source(value)).is_err());
        }
        let script = Script::parse(&source(view)).unwrap();
        assert!(script
            .validate_calls(|_, operation| if operation == "set_named_views" {
                Err("unsupported operation".into())
            } else {
                Ok(())
            })
            .unwrap_err()
            .contains("Named views"));
    }

    #[test]
    fn named_views_cannot_depend_on_skipped_checks() {
        let script = Script::parse(&json!({"version":1,"name":"review","steps":[{"note":"x"}],
            "checks":[{"id":"checked","call":{"group":"solid/query","operation":"body","arguments":{}}}],
            "views":[{"name":"review","camera":{"position":[10,0,0],"target":[0,0,0],"up":[0,0,1]},"visible_body_ids":[{"$ref":"checked","pointer":"/id"}]}]
        }).to_string()).unwrap();
        let mut calls = 0;
        let error = run(
            &script,
            |_, _| {
                calls += 1;
                Ok(json!({"id":1}))
            },
            RunOptions {
                presentation: false,
                validate: false,
            },
        )
        .unwrap_err();
        assert!(error.contains("Cannot skip checks"));
        assert_eq!(calls, 0);
    }

    #[test]
    fn resolved_named_views_are_validated_and_host_failure_is_propagated() {
        let script = Script::parse(&json!({"version":1,"name":"review",
            "steps":[{"id":"body","call":{"group":"solid/create","operation":"make","arguments":{}}}],
            "views":[{"name":"review","camera":{"position":[10,0,0],"target":[0,0,0],"up":[0,0,1]},"visible_body_ids":[{"$ref":"body","pointer":"/id"}]}]
        }).to_string()).unwrap();
        let mut calls = 0;
        let error = run(
            &script,
            |_, _| {
                calls += 1;
                Ok(json!({"id":0}))
            },
            RunOptions::default(),
        )
        .unwrap_err();
        assert!(error.contains("body id"));
        assert_eq!(calls, 1);
        let error = run(
            &script,
            |_, args| {
                if args["operation"] == "set_named_views" {
                    Ok(json!(r#"{"status":"failed","error":"rejected views"}"#))
                } else {
                    Ok(json!({"id":1}))
                }
            },
            RunOptions::default(),
        )
        .unwrap_err();
        assert!(error.contains("rejected views"));
    }

    #[test]
    fn named_view_body_aliases_are_deduplicated_after_resolution() {
        let script = Script::parse(&json!({"version":1,"name":"review",
            "steps":[{"id":"body","call":{"group":"solid/create","operation":"make","arguments":{}}}],
            "views":[{"name":"review","camera":{"position":[10,0,0],"target":[0,0,0],"up":[0,0,1]},
                "visible_body_ids":[{"$ref":"body","pointer":"/id"},{"$ref":"body","pointer":"/id"}]}]
        }).to_string()).unwrap();
        run(
            &script,
            |_, args| {
                if args["operation"] == "set_named_views" {
                    assert_eq!(
                        args["arguments"]["views"][0]["visible_body_ids"],
                        json!([1])
                    );
                }
                Ok(json!({"id":1}))
            },
            RunOptions::default(),
        )
        .unwrap();
    }

    #[test]
    fn replay_stores_named_views_from_resolved_body_ids() {
        let source = r#"{"version":1,"name":"Detent review","steps":[
            {"id":"clip","call":{"group":"solid/create","operation":"make","arguments":{}}},
            {"note":"Presentation is skipped in fast mode"}
        ],"views":[{
            "name":"detent",
            "camera":{"position":[80,-40,30],"target":[0,0,8],"up":[0,0,1]},
            "visible_body_ids":[{"$ref":"clip","pointer":"/id"}],
            "part_offsets":[{"body_id":{"$ref":"clip","pointer":"/id"},"translation":[0,14,0]}]
        }]}"#;
        let script = Script::parse(source).unwrap();
        let mut calls = vec![];
        run(
            &script,
            |_, args| {
                calls.push(args);
                Ok(json!({"id": 4}))
            },
            RunOptions::default(),
        )
        .unwrap();
        assert_eq!(calls.len(), 2, "fast replay still stores views");
        assert_eq!(calls[1]["action"], "execute");
        assert_eq!(calls[1]["operation"], "set_named_views");
        assert_eq!(calls[1]["arguments"]["views"][0]["name"], "detent");
        assert_eq!(
            calls[1]["arguments"]["views"][0]["visible_body_ids"],
            json!([4])
        );
        assert_eq!(
            calls[1]["arguments"]["views"][0]["part_offsets"][0]["translation"],
            json!([0, 14, 0])
        );
        assert!(Script::parse(
            r#"{"version":1,"name":"bad","steps":[{"note":"x"}],"views":[{"name":"","camera":{},"visible_body_ids":[]}]}"#
        )
        .is_err());
        assert!(Script::parse(
            r#"{"version":1,"name":"collapsed","steps":[{"note":"x"}],"views":[{"name":"bad","camera":{"position":[0,0,0],"target":[0,0,0],"up":[0,0,1]},"visible_body_ids":[]}]}"#
        )
        .unwrap_err()
        .contains("position and target"));
        assert!(Script::parse(
            r#"{"version":1,"name":"far","steps":[{"note":"x"}],"views":[{"name":"bad","camera":{"position":[1e20,0,0],"target":[0,0,0],"up":[0,0,1]},"visible_body_ids":[]}]}"#
        )
        .unwrap_err()
        .contains("1000000"));
    }
    #[test]
    fn failures_do_not_execute_following_steps() {
        let script=Script::parse(r#"{"version":1,"name":"test","steps":[{"call":{"group":"solid/create","operation":"bad","arguments":{}}},{"note":"must not happen"}]}"#).unwrap();
        let mut calls = 0;
        let error = run(
            &script,
            |_, _| {
                calls += 1;
                Ok(json!({"scene":{"errors":["invalid solid"]}}))
            },
            RunOptions {
                presentation: true,
                validate: true,
            },
        )
        .unwrap_err();
        assert_eq!(calls, 1);
        assert!(error.contains("step-1"));
    }
    #[test]
    fn preflight_rejects_unsupported_version_and_nested_transport() {
        assert!(Script::parse(r#"{"version":2,"name":"bad","steps":[{"note":"x"}]}"#).is_err());
        assert!(Script::parse(r#"{"version":1,"name":"bad","steps":[{"call":{"group":"file","operation":"cad_interface","arguments":{}}}]}"#).is_err());
    }

    #[test]
    fn ownership_changes_fail_before_any_model_command_in_steps_or_checks() {
        for operation in [
            "cad_attach",
            "cad_detach",
            "cad_interface",
            "cad_submit",
            "cad_await_apply",
        ] {
            for section in ["steps", "checks"] {
                let mut source = json!({"version":1,"name":"retain the selected document",
                    "steps":[{"call":{"group":"sketch/draw","operation":"sketch_begin","arguments":{}}}],
                    "checks":[]});
                source[section].as_array_mut().unwrap().push(json!({"call":{
                    "group":"document/session","operation":operation,"arguments":{}
                }}));
                let mut calls = 0;
                let result = Script::parse(&source.to_string()).and_then(|script| {
                    run(
                        &script,
                        |_, _| {
                            calls += 1;
                            Ok(json!({}))
                        },
                        RunOptions::default(),
                    )
                });
                assert!(
                    result.unwrap_err().contains(operation),
                    "{operation} in {section}"
                );
                assert_eq!(
                    calls, 0,
                    "A late ownership change must fail preflight before any modeling"
                );
            }
        }
    }
}

#[cfg(test)]
mod diagnostics_tests {
    use super::*;

    fn faces_host() -> impl FnMut(&str, Value) -> Result<Value, String> {
        |_, _| {
            Ok(json!({"scene":{"bodies":[{"id":1,"faces":[
                {"id":10,"plane":{"origin":[0.0,0.0,19.0],"normal":[0.0,0.0,1.0]},"signature":{"area":100.0}},
                {"id":11,"plane":{"origin":[5.0,5.0,8.0],"normal":[0.0,0.0,1.0]},"signature":{"area":12.0}},
                {"id":12,"plane":{"origin":[0.0,0.0,0.0],"normal":[0.0,0.0,-1.0]},"signature":{"area":100.0}}
            ]}]}}))
        }
    }

    fn select_script(where_tests: &str) -> Script {
        Script::parse(&format!(
            r#"{{"version":1,"name":"select","steps":[
                {{"id":"build","call":{{"group":"g","operation":"make","arguments":{{}}}}}},
                {{"let":{{"top":{{"$select":{{"from":{{"$select":{{"from":{{"$ref":"build"}},"path":"/scene/bodies","take":"first"}}}},"path":"/faces","where":{where_tests},"take":"one","pointer":"/id"}}}}}}}}
            ]}}"#
        ))
        .unwrap()
    }

    #[test]
    fn ambiguous_selector_lists_candidates_with_distinguishing_fields() {
        let error = run(
            &select_script(r#"{"/plane/normal/2":1}"#),
            faces_host(),
            RunOptions::default(),
        )
        .unwrap_err();
        assert!(error.contains("found 2"), "{error}");
        assert!(error.contains("\"plane/origin\":[0.0,0.0,19.0]"), "{error}");
        assert!(error.contains("\"plane/origin\":[5.0,5.0,8.0]"), "{error}");
        assert!(error.contains("/plane/origin/2"), "{error}");
    }

    #[test]
    fn empty_selector_reports_the_values_actually_present() {
        let error = run(
            &select_script(r#"{"/plane/normal/2":1,"/plane/origin/2":11.5}"#),
            faces_host(),
            RunOptions::default(),
        )
        .unwrap_err();
        assert!(
            error.contains("matched no geometry among 3 entries"),
            "{error}"
        );
        assert!(error.contains("/plane/origin/2 = 11.5"), "{error}");
        assert!(
            error.contains("/plane/origin/2: [19.0, 8.0, 0.0]"),
            "{error}"
        );
    }

    #[test]
    fn unknown_reference_suggests_similar_earlier_results() {
        let error = Script::parse(
            r#"{"version":1,"name":"typo","steps":[
                {"id":"plate_rect_2","call":{"group":"g","operation":"make","arguments":{}}},
                {"id":"plate_fix","call":{"group":"g","operation":"fix","arguments":{"entity":{"$ref":"plate_rect","pointer":"/id"}}}}
            ]}"#,
        )
        .unwrap_err();
        assert!(error.contains("plate_fix references plate_rect"), "{error}");
        assert!(
            error.contains("similar earlier results: plate_rect_2"),
            "{error}"
        );
    }

    #[test]
    fn unused_let_bindings_are_listed() {
        let script = Script::parse(
            r#"{"version":1,"name":"unused","steps":[
                {"id":"build","call":{"group":"g","operation":"make","arguments":{}}},
                {"let":{"used":{"$ref":"build","pointer":"/id"},"forgotten":{"$ref":"build","pointer":"/id"}}},
                {"call":{"group":"g","operation":"edit","arguments":{"body":{"$ref":"used"}}}}
            ]}"#,
        )
        .unwrap();
        assert_eq!(script.unused_bindings(), vec!["forgotten".to_string()]);
    }
}
