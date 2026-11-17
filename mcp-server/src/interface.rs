use serde_json::Value;

use limo_cad_script::MAX_SCRIPT_BYTES;

/// Keep native file exchange discoverable without enlarging the already broad
/// interface JSON macro past Rust's default expansion depth.
pub fn with_file_options(mut schema: Value) -> Value {
    schema["properties"]["command"]["enum"]
        .as_array_mut()
        .unwrap()
        .extend(
            [
                "new",
                "close",
                "import_step",
                "export_step",
                "export_3mf",
                "export_stl",
                "export_drawing_svg",
                "export_drawing_dxf",
                "export_drawing_pdf",
                "export_profile_dxf",
                "print_drawing",
                "print_status",
            ]
            .map(Value::from),
        );
    schema["properties"]["selected_only"] = serde_json::json!({"type":"boolean","description":"For file exports, export only selected bodies or occurrences."});
    schema["properties"]["scope"] = serde_json::json!({"type":"string","enum":["assembly","definition"],"description":"Required for 3MF/STL file export: placed assembly occurrences or one mesh per selected definition."});
    schema["properties"]["named_view"] = serde_json::json!({"type":"string","description":"For assembly exports only: a saved presentation or print view. Empty selects assembled placement; omitted uses the current recalled view. Not accepted by action view; use recall_named_view with name to recall a saved camera, visibility and display offsets."});
    schema["properties"]["print_bed"] = crate::print_bed_schema();
    schema["properties"]["allow_layout_issues"] = serde_json::json!({"type":"boolean","default":false,"description":"Deliberately proceed after reviewing print-layout diagnostics. Timeline, ownership and unsaved-layout guards still apply."});
    schema["properties"]["feature_id"] = serde_json::json!({"type":"integer","minimum":0,"description":"For export_profile_dxf: sketch feature ID from sketch_profiles."});
    schema["properties"]["profile_index"] = serde_json::json!({"type":"integer","minimum":0,"maximum":4294967295u64,"description":"For export_profile_dxf: zero-based even-depth material-region index from sketch_profiles. Its immediate hole wires are included at 1:1 in local sketch-plane millimetres."});
    schema
}

/// Authored text plus the expanded document the interpreter runs.
#[derive(Debug)]
pub struct LoadedScript {
    pub authored: String,
    pub expanded: String,
}

/// Load an authored text script, never executable code or a model snapshot.
pub fn script_source(arguments: &Value) -> Result<String, String> {
    load_script(arguments).map(|loaded| loaded.expanded)
}

/// Read exact authored text without parsing or expanding includes. The native
/// repair editor uses the same file admission and byte bounds as normal loading.
pub fn read_authored_script(file: &std::path::Path) -> Result<String, String> {
    let path = file.to_str().ok_or("script path must be valid Unicode")?;
    if !file.is_absolute() || !path.to_lowercase().ends_with(".limo.jsonc") {
        return Err("script path must be an absolute .limo.jsonc file path".into());
    }
    read_script_file(file, &format!("script {path}"))
}

fn read_script_file(path: &std::path::Path, label: &str) -> Result<String, String> {
    let metadata = std::fs::metadata(path).map_err(|e| format!("read {label}: {e}"))?;
    if !metadata.is_file() || metadata.len() > MAX_SCRIPT_BYTES as u64 {
        return Err(format!(
            "{label} must be a regular file no larger than 16 MiB"
        ));
    }
    let file = std::fs::File::open(path).map_err(|e| format!("read {label}: {e}"))?;
    let metadata = file.metadata().map_err(|e| format!("read {label}: {e}"))?;
    if !metadata.is_file() || metadata.len() > MAX_SCRIPT_BYTES as u64 {
        return Err(format!(
            "{label} must be a regular file no larger than 16 MiB"
        ));
    }
    read_text_bounded(file, MAX_SCRIPT_BYTES).map_err(|e| format!("read {label}: {e}"))
}

fn read_text_bounded(reader: impl std::io::Read, limit: usize) -> Result<String, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("script exceeds 16 MiB".into());
    }
    String::from_utf8(bytes).map_err(|e| format!("script is not valid UTF-8: {e}"))
}

pub fn load_script(arguments: &Value) -> Result<LoadedScript, String> {
    let recipe = arguments.get("recipe");
    let source = arguments.get("source");
    let path = arguments.get("path");
    let include_base = arguments.get("include_base");
    let present = [recipe, source, path]
        .into_iter()
        .filter(|v| v.is_some())
        .count();
    if present != 1 {
        return Err("script requires exactly one of recipe, source or path".into());
    }
    if include_base.is_some() && source.is_none() {
        return Err("include_base is only valid with inline script source".into());
    }
    if let Some(recipe) = recipe {
        let id = recipe.as_str().ok_or("recipe must be an ID string")?;
        let recipe = limo_cad_recipes::find(id)?;
        let text: String = recipe.source.into();
        if limo_cad_script::has_unresolved_includes(&text)? {
            return Err(format!(
                "bundled recipe {id} has unresolved includes; flatten at catalog build time"
            ));
        }
        return Ok(LoadedScript {
            expanded: text.clone(),
            authored: text,
        });
    }
    let (source, base_dir): (String, Option<std::path::PathBuf>) = match (source, path) {
        (Some(source), None) => {
            let text = source
                .as_str()
                .ok_or("script source must be text")?
                .to_owned();
            let base = match include_base {
                Some(value) => Some(include_base_dir(value)?),
                None => None,
            };
            (text, base)
        }
        (None, Some(path)) => {
            let path = path.as_str().ok_or("script path must be a string")?;
            let file = std::path::Path::new(path);
            let text = read_authored_script(file)?;
            let base = file
                .parent()
                .ok_or("script path has no parent directory")?
                .to_path_buf();
            (text, Some(base))
        }
        _ => return Err("script requires exactly one of source or path".into()),
    };
    if source.len() > MAX_SCRIPT_BYTES {
        return Err("script exceeds 16 MiB".into());
    }
    let expanded = expand_includes_if_needed(&source, base_dir.as_deref())?;
    Ok(LoadedScript {
        authored: source,
        expanded,
    })
}

fn include_base_dir(value: &Value) -> Result<std::path::PathBuf, String> {
    let dir = value
        .as_str()
        .ok_or("include_base must be an absolute directory")?;
    let path = std::path::Path::new(dir);
    if !path.is_absolute() {
        return Err("include_base must be an absolute directory".into());
    }
    let metadata = std::fs::metadata(path).map_err(|e| format!("include_base {dir}: {e}"))?;
    if !metadata.is_dir() {
        return Err("include_base must be an absolute directory".into());
    }
    Ok(path.to_path_buf())
}

fn expand_includes_if_needed(
    source: &str,
    base_dir: Option<&std::path::Path>,
) -> Result<String, String> {
    if !limo_cad_script::has_unresolved_includes(source)? {
        return Ok(source.to_owned());
    }
    let base = base_dir.ok_or(
        "This script includes other files. Open it from its file path so those files can be loaded.",
    )?;
    let base_canon = std::fs::canonicalize(base)
        .map_err(|e| format!("canonicalize script base {}: {e}", base.display()))?;

    limo_cad_script::flatten_includes(source, |rel| {
        limo_cad_script::validate_include_path(rel)?;
        let joined = base.join(rel);
        let canon =
            std::fs::canonicalize(&joined).map_err(|e| format!("read include {rel}: {e}"))?;
        if !canon.starts_with(&base_canon) {
            return Err(format!("include {rel} escapes script base directory"));
        }
        if !canon.is_file() {
            return Err(format!("include {rel} is not a file"));
        }
        read_script_file(&canon, &format!("include {rel}"))
    })
}

/// The renderer and API consume the same product-owned grouping data.
pub use limo_cad_interface::catalog::{group_for, groups};

pub fn validate_script(script: &limo_cad_script::Script) -> Result<(), String> {
    script.validate_calls(validate_script_call)
}

pub(super) fn validate_script_call(group: &str, operation: &str) -> Result<(), String> {
    match group_for(operation) {
        _ if operation == "cad_computer_control" => Err(
            "Computer control requires one observed interactive action at a time, outside scripts"
                .into(),
        ),
        Some(expected) if group == expected => Ok(()),
        Some(expected) => Err(format!("{operation} belongs to {expected}, not {group}")),
        None => Err(format!("Unknown interface operation {operation}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bounded_reader_stops_after_limit_plus_one_and_rejects_invalid_utf8() {
        struct Growing {
            bytes: usize,
        }
        impl std::io::Read for Growing {
            fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
                output.fill(b'x');
                self.bytes += output.len();
                Ok(output.len())
            }
        }
        let mut growing = Growing { bytes: 0 };
        assert!(read_text_bounded(&mut growing, 8).is_err());
        assert_eq!(growing.bytes, 9, "Do not read an unbounded growing file");
        assert_eq!(read_text_bounded(&b"12345678"[..], 8).unwrap(), "12345678");
        assert!(read_text_bounded(&[255][..], 8).is_err());
        assert_eq!(read_text_bounded(&b""[..], 8).unwrap(), "");
    }

    #[test]
    fn authored_reader_retains_invalid_text_but_enforces_file_admission() {
        let directory = std::env::temp_dir().join(format!(
            "limo-authored-reader-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("unfinished.limo.jsonc");
        let source = "// original comment\n{";
        std::fs::write(&path, source).unwrap();
        assert_eq!(read_authored_script(&path).unwrap(), source);
        assert!(load_script(&json!({"path":path})).is_err());
        assert!(read_authored_script(std::path::Path::new("relative.limo.jsonc")).is_err());
        assert!(read_authored_script(&directory.join("wrong.json")).is_err());
        let folder = directory.join("folder.limo.jsonc");
        std::fs::create_dir(&folder).unwrap();
        assert!(read_authored_script(&folder).is_err());
        std::fs::write(&path, [255]).unwrap();
        assert!(read_authored_script(&path).is_err());
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(MAX_SCRIPT_BYTES as u64 + 1)
            .unwrap();
        assert!(read_authored_script(&path).is_err());
        assert!(read_script_file(&path, "include fixture").is_err());
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(folder).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn file_schema_exposes_sheet_and_exact_profile_exports() {
        let schema = with_file_options(json!({"properties":{"command":{"enum":["save"]}}}));
        let commands = schema["properties"]["command"]["enum"].as_array().unwrap();
        for command in [
            "export_drawing_svg",
            "export_drawing_dxf",
            "export_drawing_pdf",
            "export_profile_dxf",
        ] {
            assert!(commands.contains(&json!(command)));
        }
        assert_eq!(schema["properties"]["feature_id"]["type"], "integer");
        assert_eq!(schema["properties"]["profile_index"]["maximum"], u32::MAX);
    }

    #[test]
    fn bundled_recipe_calls_preflight_against_the_product_catalog() {
        for recipe in limo_cad_recipes::RECIPES {
            let script = limo_cad_script::Script::parse(recipe.source).unwrap();
            validate_script(&script).unwrap_or_else(|error| panic!("{}: {error}", recipe.id));
        }
        let script = limo_cad_script::Script::parse(r#"{"version":1,"name":"wrong late group","steps":[{"note":"No geometry should run"}],"checks":[{"id":"late_inspection","call":{"group":"solid/inspect","operation":"solid_scene","arguments":{}}}]}"#).unwrap();
        let error = validate_script(&script).unwrap_err();
        assert!(
            error.contains("late_inspection") && error.contains("solid/check"),
            "{error}"
        );
    }

    #[test]
    fn script_source_requires_one_explicit_text_source() {
        assert_eq!(
            script_source(&json!({"source":"// readable\n{}"})).unwrap(),
            "// readable\n{}"
        );
        for args in [
            json!({}),
            json!({"source":1}),
            json!({"recipe":1}),
            json!({"recipe":"not-a-recipe"}),
            json!({"recipe":"mounting-plate","source":"{}"}),
            json!({"recipe":"mounting-plate","path":"/part.limo.jsonc"}),
            json!({"source":"{}","path":"a.limo.jsonc"}),
            json!({"path":"relative.limo.jsonc"}),
            json!({"source":"x".repeat(MAX_SCRIPT_BYTES+1)}),
        ] {
            assert!(script_source(&args).is_err());
        }
    }

    #[test]
    fn includes_resolve_beside_the_including_file_and_stay_inside_the_root() {
        let unique = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let dir = std::env::temp_dir().join(format!("limo-cad-includes-{unique}"));
        let outside = std::env::temp_dir().join(format!("limo-cad-includes-outside-{unique}"));
        std::fs::create_dir_all(dir.join("collections")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let root = dir.join("root.limo.jsonc");
        std::fs::write(
            &root,
            r#"{"version":1,"name":"Root","includes":["collections/a.collection.jsonc"],"steps":[{"id":"root","note":"root"}]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("collections/a.collection.jsonc"),
            r#"{"includes":["b.collection.jsonc"],"steps":[{"id":"a","note":"a"}]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("collections/b.collection.jsonc"),
            r#"{"steps":[{"id":"nested","note":"beside the including fragment"}]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("b.collection.jsonc"),
            r#"{"steps":[{"id":"root-level","note":"not this one"}]}"#,
        )
        .unwrap();

        let loaded = load_script(&json!({"path": root.to_string_lossy()})).unwrap();
        assert!(loaded.authored.contains("\"includes\""));
        let value: Value = serde_json::from_str(&loaded.expanded).unwrap();
        assert!(value.get("includes").is_none());
        assert_eq!(value["steps"][0]["id"], "nested");
        assert_eq!(value["steps"][1]["id"], "a");

        let inspected = crate::inspect_script(json!({"path": root.to_string_lossy()})).unwrap();
        assert_eq!(inspected["step_count"], 3);
        assert_eq!(inspected["authored_source"], loaded.authored);
        assert!(!inspected["source"]
            .as_str()
            .unwrap()
            .contains("\"includes\""));

        let inline = load_script(&json!({"source": loaded.authored})).unwrap_err();
        assert!(
            !inline.contains("parse_with_includes") && inline.contains("file path"),
            "{inline}"
        );
        let replay = load_script(&json!({
            "source": loaded.authored,
            "include_base": dir.to_string_lossy(),
        }))
        .unwrap();
        assert_eq!(replay.expanded, loaded.expanded);

        std::fs::write(
            outside.join("secret.collection.jsonc"),
            r#"{"steps":[{"id":"secret","note":"outside"}]}"#,
        )
        .unwrap();
        let link = dir.join("linked.collection.jsonc");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.join("secret.collection.jsonc"), &link).unwrap();
            let linked_root = dir.join("linked.limo.jsonc");
            std::fs::write(
                &linked_root,
                r#"{"version":1,"name":"Link","includes":["linked.collection.jsonc"],"steps":[{"id":"after","note":"after"}]}"#,
            )
            .unwrap();
            let escaped = load_script(&json!({"path": linked_root.to_string_lossy()})).unwrap_err();
            assert!(escaped.contains("escapes"), "{escaped}");
        }
        #[cfg(not(unix))]
        {
            let _ = link;
        }

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&outside).ok();
    }
}
