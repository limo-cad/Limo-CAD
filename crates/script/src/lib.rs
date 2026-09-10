//! Readable, versioned CAD command sequences. No embedded programming runtime.
//! The host owns the existing interface; this crate only resolves data references
//! and sequences calls, so live and headless execution use the same operations.
use serde_json::{json, Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Instant,
};
mod manufacturing;

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
        let document: Value = serde_json::from_str(&strip_jsonc(source)?)
            .map_err(|e| format!("Invalid script JSONC: {e}"))?;
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
                        return Err(format!("Step {id} references {name} before it is defined"));
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
            .filter_map(|step| {
                let text = step.get("note")?;
                let mut note = json!({"text": text});
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
            "operations": operations,
        })
    }

    /// Reject unsupported omissions before the host creates any geometry.
    pub fn validate_options(&self, options: RunOptions) -> Result<(), String> {
        if options.validate {
            return Ok(());
        }
        if !self.document["verification"].is_null() {
            return Err("This reference script requires its final verification gate".into());
        }
        let mut exported = BTreeMap::new();
        references(&self.document["exports"], &mut exported);
        if let Some(name) = exported
            .keys()
            .find(|name| self.check_results.contains(*name))
        {
            return Err(format!(
                "Cannot skip checks: exported result {name} is produced by a check"
            ));
        }
        Ok(())
    }
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
                // Result references are resolved and type-checked by the host;
                // malformed literal IDs fail even during a fast replay.
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
fn strip_jsonc(source: &str) -> Result<String, String> {
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
        } else if bytes[i] == b',' {
            if bytes[i + 1..]
                .iter()
                .copied()
                .find(|c| !c.is_ascii_whitespace())
                .is_some_and(|c| c == b']' || c == b'}')
            {
                bytes[i] = b' ';
            }
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
                    "first" => *found.first().ok_or("Selector matched no geometry")?,
                    "last" => *found.last().ok_or("Selector matched no geometry")?,
                    "one" => {
                        return Err(format!(
                            "Selector expected exactly one match, found {}",
                            found.len()
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

pub fn run<F>(script: &Script, mut host: F, options: RunOptions) -> Result<Value, String>
where
    F: FnMut(&str, Value) -> Result<Value, String>,
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
            // Native command results can contain a complete triangulated scene.
            // Release snapshots after their last reference instead of keeping
            // hundreds of copies alive throughout a long assembly replay.
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
    Ok(
        json!({"name":script.document["name"],"steps_completed":completed,"checks_completed":checks,"elapsed_ms":started.elapsed().as_millis(),"exports":exports}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
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
            {"view":"current","body_id":{"$ref":"part","pointer":"/id"}},
            {"call":{"group":"solid/modify","operation":"edit","arguments":{"body_id":{"$ref":"part","pointer":"/id"}}}}
        ],"checks":[{"assert":{"$ref":"part","pointer":"/id"},"equals":7}],"exports":{"part":{"$ref":"part"}}}"#).unwrap();
        let mut runs = vec![];
        for presentation in [false, true] {
            let mut calls = vec![];
            let report = run(
                &script,
                |_, args| {
                    if args["action"] == "execute" {
                        calls.push(args);
                        Ok(json!({"id":7}))
                    } else {
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
