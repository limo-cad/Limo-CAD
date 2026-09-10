use serde_json::{json, Value};
use std::sync::OnceLock;

const MAX_SCRIPT_BYTES: usize = 16 * 1024 * 1024;

/// Load an authored text script, never executable code or a model snapshot.
pub fn script_source(arguments: &Value) -> Result<String, String> {
    if let Some(recipe) = arguments.get("recipe") {
        if arguments.get("source").is_some() || arguments.get("path").is_some() {
            return Err("script requires exactly one of recipe, source or path".into());
        }
        return Ok(
            nbcad_recipes::find(recipe.as_str().ok_or("recipe must be an ID string")?)?
                .source
                .into(),
        );
    }
    let source = arguments.get("source");
    let path = arguments.get("path");
    let source = match (source, path) {
        (Some(source), None) => source
            .as_str()
            .ok_or("script source must be text")?
            .to_owned(),
        (None, Some(path)) => {
            let path = path.as_str().ok_or("script path must be a string")?;
            let file = std::path::Path::new(path);
            if !file.is_absolute() || !path.to_lowercase().ends_with(".nbcad.jsonc") {
                return Err("script path must be an absolute .nbcad.jsonc file path".into());
            }
            let metadata =
                std::fs::metadata(file).map_err(|e| format!("read script {path}: {e}"))?;
            if !metadata.is_file() || metadata.len() > MAX_SCRIPT_BYTES as u64 {
                return Err("script must be a regular file no larger than 16 MiB".into());
            }
            std::fs::read_to_string(file).map_err(|e| format!("read script {path}: {e}"))?
        }
        _ => return Err("script requires exactly one of source or path".into()),
    };
    if source.len() > MAX_SCRIPT_BYTES {
        return Err("script exceeds 16 MiB".into());
    }
    Ok(source)
}

/// The renderer and API consume the same product-owned grouping data.
pub fn groups() -> &'static Vec<Value> {
    static GROUPS: OnceLock<Vec<Value>> = OnceLock::new();
    GROUPS.get_or_init(|| {
        let catalog: Value = serde_json::from_str(include_str!("../../interface/catalog.json")).unwrap();
        let mut groups = catalog["groups"].as_array().unwrap().clone();
        for workspace in catalog["workspaces"].as_array().unwrap() {
            for panel in workspace["panels"].as_array().unwrap() {
                groups.push(json!({"id":format!("{}/{}",workspace["id"].as_str().unwrap(),panel["id"].as_str().unwrap()),
                    "labelKey":panel["labelKey"],"operations":panel["operations"]}));
            }
        }
        groups
    })
}

pub fn group_for(operation: &str) -> Option<&'static str> {
    groups()
        .iter()
        .find(|g| {
            g["operations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|n| n == operation)
        })
        .and_then(|g| g["id"].as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

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
            json!({"recipe":"mounting-plate","path":"/part.nbcad.jsonc"}),
            json!({"source":"{}","path":"a.nbcad.jsonc"}),
            json!({"path":"relative.nbcad.jsonc"}),
            json!({"source":"x".repeat(MAX_SCRIPT_BYTES+1)}),
        ] {
            assert!(script_source(&args).is_err());
        }
    }
}
