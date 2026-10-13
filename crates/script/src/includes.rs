//! Compose version-1 scripts from collection fragments.
//!
//! The interpreter stays host-neutral: callers supply `load(relative_path)` to
//! read include text. Path safety (base directory, symlink escape) belongs to
//! the host loader.

use serde_json::{Map, Value};
use std::collections::BTreeSet;

use crate::{strip_jsonc, Script, MAX_SCRIPT_BYTES};

/// Nesting limit from the root document (root depth = 0).
pub const MAX_INCLUDE_DEPTH: usize = 8;

/// Expand top-level `includes` into one document, then parse as a version-1 script.
pub fn parse_with_includes(
    source: &str,
    load: impl FnMut(&str) -> Result<String, String>,
) -> Result<Script, String> {
    let flattened = flatten_includes(source, load)?;
    Script::parse(&flattened)
}

/// Return a JSON (not JSONC) document string with `includes` expanded away.
///
/// Comments from the root/fragments are not preserved; this is a resolution
/// artifact for execution and self-contained export.
pub fn flatten_includes(
    source: &str,
    mut load: impl FnMut(&str) -> Result<String, String>,
) -> Result<String, String> {
    if source.len() > MAX_SCRIPT_BYTES {
        return Err("Script exceeds 16 MiB".into());
    }
    let mut stack = Vec::new();
    let mut loaded_bytes = 0usize;
    let document = flatten_document(source, "", &mut load, 0, &mut stack, &mut loaded_bytes)?;
    let text = serde_json::to_string(&document).map_err(|e| e.to_string())?;
    if text.len() > MAX_SCRIPT_BYTES {
        return Err("Expanded script exceeds 16 MiB".into());
    }
    Ok(text)
}

/// Include paths are relative to the file that declares them.
///
/// `from_file` is empty for the root, or a root-relative forward-slash path of
/// the fragment being expanded. `..` stays rejected, so the resolved path cannot
/// climb above the root directory.
pub fn resolve_include_path(from_file: &str, include_path: &str) -> Result<String, String> {
    validate_include_path(include_path)?;
    if (from_file.contains('\\')
        || from_file
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == ".."))
        && !from_file.is_empty()
    {
        return Err(format!(
            "Include base must be a relative forward-slash path: {from_file}"
        ));
    }
    let parent = match from_file.rsplit_once('/') {
        Some((dir, _)) if !from_file.is_empty() => dir,
        _ => "",
    };
    if parent.is_empty() {
        Ok(include_path.to_owned())
    } else {
        Ok(format!("{parent}/{include_path}"))
    }
}

fn flatten_document(
    source: &str,
    from_file: &str,
    load: &mut impl FnMut(&str) -> Result<String, String>,
    depth: usize,
    stack: &mut Vec<String>,
    loaded_bytes: &mut usize,
) -> Result<Value, String> {
    *loaded_bytes = loaded_bytes.saturating_add(source.len());
    if *loaded_bytes > MAX_SCRIPT_BYTES {
        return Err("Expanded script exceeds 16 MiB".into());
    }
    let document: Value = serde_json::from_str(&strip_jsonc(source)?)
        .map_err(|e| format!("Invalid script JSONC: {e}"))?;
    let object = document
        .as_object()
        .ok_or("Script document must be a JSON object")?;
    let includes = match object.get("includes") {
        None => return Ok(document),
        Some(Value::Null) => return Ok(document),
        Some(Value::Array(items)) if items.is_empty() => {
            let mut clean = document.clone();
            clean.as_object_mut().unwrap().remove("includes");
            return Ok(clean);
        }
        Some(Value::Array(_)) => object.get("includes").unwrap(),
        Some(_) => return Err("includes must be an array of paths or {path} objects".into()),
    };

    if depth > MAX_INCLUDE_DEPTH {
        return Err(format!(
            "Include nesting exceeds maximum depth {MAX_INCLUDE_DEPTH}"
        ));
    }

    let mut merged_steps = Vec::new();
    let mut merged_checks = Vec::new();

    for (index, entry) in includes.as_array().unwrap().iter().enumerate() {
        let requested = include_path(entry, index)?;
        let path = resolve_include_path(from_file, &requested)?;
        if stack.iter().any(|seen| seen == &path) {
            return Err(format!("Include cycle detected at {path}"));
        }
        stack.push(path.clone());
        let fragment_source = load(&path).map_err(|e| format!("Include {path}: {e}"))?;
        if fragment_source.len() > MAX_SCRIPT_BYTES {
            stack.pop();
            return Err(format!("Include {path} exceeds 16 MiB"));
        }
        let fragment = flatten_document(
            &fragment_source,
            &path,
            load,
            depth + 1,
            stack,
            loaded_bytes,
        )?;
        stack.pop();
        append_fragment(&path, &fragment, &mut merged_steps, &mut merged_checks)?;
    }

    let root_steps = object
        .get("steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let root_checks = object
        .get("checks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    merged_steps.extend(root_steps);
    merged_checks.extend(root_checks);

    let mut out = Map::new();
    for (key, value) in object {
        if key == "includes" || key == "steps" || key == "checks" {
            continue;
        }
        out.insert(key.clone(), value.clone());
    }
    out.insert("steps".into(), Value::Array(merged_steps));
    if !merged_checks.is_empty() || object.get("checks").is_some() {
        out.insert("checks".into(), Value::Array(merged_checks));
    }
    Ok(Value::Object(out))
}

fn include_path(entry: &Value, index: usize) -> Result<String, String> {
    match entry {
        Value::String(path) => Ok(path.clone()),
        Value::Object(object) => object
            .get("path")
            .and_then(Value::as_str)
            .filter(|path| !path.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| format!("includes[{index}] object needs a nonempty path")),
        _ => Err(format!(
            "includes[{index}] must be a relative path string or {{path}} object"
        )),
    }
}

/// Shared path grammar for hosts and tests. Hosts still enforce base-dir containment.
pub fn validate_include_path(path: &str) -> Result<(), String> {
    if path.is_empty() {
        return Err("Include path must be nonempty".into());
    }
    if path.starts_with('/') || path.starts_with('\\') {
        return Err(format!("Include path must be relative: {path}"));
    }
    if path.chars().nth(1) == Some(':') {
        return Err(format!("Include path must be relative: {path}"));
    }
    if path.contains('\\') {
        return Err(format!(
            "Include path must use forward slashes only: {path}"
        ));
    }
    let lower = path.to_ascii_lowercase();
    if !(lower.ends_with(".limo.jsonc")
        || lower.ends_with(".nbcad.jsonc")
        || lower.ends_with(".collection.jsonc"))
    {
        return Err(format!(
            "Include path must end with .limo.jsonc or .collection.jsonc: {path}"
        ));
    }
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(format!(
                "Include path must not contain empty, '.', or '..' segments: {path}"
            ));
        }
    }
    Ok(())
}

fn append_fragment(
    path: &str,
    fragment: &Value,
    steps: &mut Vec<Value>,
    checks: &mut Vec<Value>,
) -> Result<(), String> {
    let object = fragment
        .as_object()
        .ok_or_else(|| format!("Include {path} must be a JSON object"))?;
    let lower = path.to_ascii_lowercase();
    let full_script = lower.ends_with(".limo.jsonc") || lower.ends_with(".nbcad.jsonc");
    let allowed: BTreeSet<&str> = ["$schema", "name", "steps", "checks", "includes"]
        .into_iter()
        .collect();

    let ignored: BTreeSet<&str> = [
        "$schema",
        "version",
        "starting_state",
        "verification",
        "exports",
    ]
    .into_iter()
    .collect();
    for key in object.keys() {
        if allowed.contains(key.as_str()) {
            continue;
        }
        if full_script && ignored.contains(key.as_str()) {
            continue;
        }
        return Err(format!(
            "Include {path} has unsupported field {key}; collections may only supply name, steps, checks, and includes"
        ));
    }
    if full_script {
        if object.get("version").is_some_and(|version| version != 1) {
            return Err(format!("Include {path} must be version 1"));
        }
        if object
            .get("starting_state")
            .is_some_and(|state| state != "empty")
        {
            return Err(format!("Include {path} starting_state must be empty"));
        }
    }
    let fragment_steps = object
        .get("steps")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("Include {path} needs a steps array"))?;
    if fragment_steps.is_empty() {
        return Err(format!("Include {path} needs at least one step"));
    }
    steps.extend(fragment_steps.iter().cloned());
    if let Some(fragment_checks) = object.get("checks") {
        let array = fragment_checks
            .as_array()
            .ok_or_else(|| format!("Include {path} checks must be an array"))?;
        checks.extend(array.iter().cloned());
    }
    Ok(())
}

/// True when the document declares a nonempty includes array (before flatten).
pub fn has_unresolved_includes(source: &str) -> Result<bool, String> {
    let document: Value = serde_json::from_str(&strip_jsonc(source)?)
        .map_err(|e| format!("Invalid script JSONC: {e}"))?;
    Ok(document
        .get("includes")
        .and_then(Value::as_array)
        .is_some_and(|items| !items.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{run, RunOptions};
    use serde_json::json;
    use std::collections::BTreeMap;

    fn load_map<'a>(
        files: BTreeMap<&'a str, &'a str>,
    ) -> impl FnMut(&str) -> Result<String, String> + use<'a> {
        move |path: &str| {
            files
                .get(path)
                .map(|text| (*text).to_owned())
                .ok_or_else(|| format!("missing {path}"))
        }
    }

    #[test]
    fn flattens_collection_steps_before_root_and_preserves_root_name() {
        let root = r#"{
          "version":1,"name":"Root assembly","includes":["parts/a.collection.jsonc"],
          "steps":[{"id":"root_note","note":"Assemble"}]
        }"#;
        let mut files = BTreeMap::new();
        files.insert(
            "parts/a.collection.jsonc",
            r#"{"name":"Part A","steps":[{"id":"a_let","let":{"size":3}}]}"#,
        );
        let flat = flatten_includes(root, load_map(files)).unwrap();
        let value: Value = serde_json::from_str(&flat).unwrap();
        assert_eq!(value["name"], "Root assembly");
        assert!(value.get("includes").is_none());
        assert_eq!(value["steps"][0]["id"], "a_let");
        assert_eq!(value["steps"][1]["id"], "root_note");
        let script = Script::parse(&flat).unwrap();
        assert_eq!(script.metadata()["step_count"], 2);
    }

    #[test]
    fn rejects_cycles_and_parent_segments() {
        let root = r#"{"version":1,"name":"Cyclic","includes":["a.collection.jsonc"],"steps":[{"note":"x"}]}"#;
        let mut files = BTreeMap::new();
        files.insert(
            "a.collection.jsonc",
            r#"{"steps":[{"note":"a"}],"includes":["b.collection.jsonc"]}"#,
        );
        files.insert(
            "b.collection.jsonc",
            r#"{"steps":[{"note":"b"}],"includes":["a.collection.jsonc"]}"#,
        );
        assert!(flatten_includes(root, load_map(files))
            .unwrap_err()
            .contains("cycle"));
        assert!(validate_include_path("../x.collection.jsonc").is_err());
        assert!(validate_include_path("/abs.collection.jsonc").is_err());
        assert!(validate_include_path("C:/x.collection.jsonc").is_err());
    }

    #[test]
    fn fast_mode_skips_included_presentation_like_monolithic_scripts() {
        let root = r#"{
          "version":1,"name":"Fast compose","includes":["part.collection.jsonc"],
          "steps":[{"id":"finish","call":{"group":"g","operation":"done","arguments":{}}}]
        }"#;
        let mut files = BTreeMap::new();
        files.insert(
            "part.collection.jsonc",
            r#"{"steps":[
              {"note":"Teaching only","chapter":"Intro","duration_ms":500},
              {"id":"make","call":{"group":"g","operation":"make","arguments":{}}}
            ]}"#,
        );
        let script = parse_with_includes(root, load_map(files)).unwrap();
        let mut calls = Vec::new();
        run(
            &script,
            |_, args| {
                calls.push(args);
                Ok(json!({"id":1}))
            },
            RunOptions {
                presentation: false,
                validate: true,
            },
        )
        .unwrap();
        assert_eq!(calls.len(), 2);
        assert!(calls.iter().all(|args| args["action"] == "execute"));
    }

    #[test]
    fn duplicate_ids_across_include_and_root_fail_preflight() {
        let root = r#"{
          "version":1,"name":"Dup","includes":["part.collection.jsonc"],
          "steps":[{"id":"shared","note":"root"}]
        }"#;
        let mut files = BTreeMap::new();
        files.insert(
            "part.collection.jsonc",
            r#"{"steps":[{"id":"shared","let":{"x":1}}]}"#,
        );
        assert!(parse_with_includes(root, load_map(files))
            .unwrap_err()
            .contains("Duplicate"));
    }

    #[test]
    fn included_full_script_contributes_steps_and_ignores_root_fields() {
        let root = r#"{
          "version":1,"name":"Assembly",
          "includes":["part.limo.jsonc"],
          "steps":[{"id":"mate","note":"mate"}]
        }"#;
        let mut files = BTreeMap::new();
        files.insert(
            "part.limo.jsonc",
            r#"{
              "$schema":"./limo-cad-script.schema.json",
              "version":1,
              "name":"Part",
              "starting_state":"empty",
              "verification":"garden-bench",
              "exports":{"made":{"$ref":"make"}},
              "steps":[{"id":"make","let":{"size":2}}]
            }"#,
        );
        let flat = flatten_includes(root, load_map(files)).unwrap();
        let value: Value = serde_json::from_str(&flat).unwrap();
        assert_eq!(value["name"], "Assembly");
        assert!(value.get("exports").is_none() || value["exports"].get("made").is_none());
        assert_eq!(value["steps"][0]["id"], "make");
        assert_eq!(value["steps"][1]["id"], "mate");
        assert!(value.get("verification").is_none());
    }

    #[test]
    fn nested_include_paths_resolve_against_the_including_file() {
        let root = r#"{"version":1,"name":"Nested","includes":["collections/a.collection.jsonc"],"steps":[{"id":"root","note":"root"}]}"#;
        let mut files = BTreeMap::new();
        files.insert(
            "collections/a.collection.jsonc",
            r#"{"steps":[{"id":"a","note":"a"}],"includes":["b.collection.jsonc"]}"#,
        );
        files.insert(
            "collections/b.collection.jsonc",
            r#"{"steps":[{"id":"nested","note":"nested"}]}"#,
        );
        files.insert(
            "b.collection.jsonc",
            r#"{"steps":[{"id":"wrong-root-level","note":"wrong"}]}"#,
        );
        let mut requested = Vec::new();
        let flat = flatten_includes(root, |path| {
            requested.push(path.to_owned());
            files
                .get(path)
                .map(|text| (*text).to_owned())
                .ok_or_else(|| format!("missing {path}"))
        })
        .unwrap();
        assert_eq!(
            requested,
            vec![
                "collections/a.collection.jsonc",
                "collections/b.collection.jsonc"
            ]
        );
        let value: Value = serde_json::from_str(&flat).unwrap();
        assert_eq!(value["steps"][0]["id"], "nested");
        assert_eq!(value["steps"][1]["id"], "a");
        assert_eq!(value["steps"][2]["id"], "root");
    }

    #[test]
    fn expanded_size_is_rejected_before_later_fragments_load() {
        let root = r#"{"version":1,"name":"Big","includes":["a.collection.jsonc","b.collection.jsonc","c.collection.jsonc"],"steps":[{"note":"end"}]}"#;
        let chunk = "x".repeat(8 * 1024 * 1024);
        let fragment = format!(r#"{{"name":"{chunk}","steps":[{{"note":"n"}}]}}"#);
        let mut loads = 0;
        let error = flatten_includes(root, |path| {
            loads += 1;
            assert_ne!(path, "c.collection.jsonc");
            Ok(fragment.clone())
        })
        .unwrap_err();
        assert!(error.contains("16 MiB"), "{error}");
        assert!(loads < 3, "loaded {loads} fragments before rejecting");
    }
}
