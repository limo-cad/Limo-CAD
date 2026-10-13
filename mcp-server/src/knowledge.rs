//! Read-only, offline MCP access to the repository's existing OKF Markdown bundle.
//!
//! The inventory is the `limo-cad-help` embed of every `knowledge/**/*.md` file, so
//! `cad_help` search and these resources always describe the same revision.
use limo_cad_help::{knowledge_file_by_uri, knowledge_files};
use serde_json::{json, Value};

fn frontmatter<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let mut lines = text.lines();
    if lines.next()? != "---" {
        return None;
    }
    lines.take_while(|line| *line != "---").find_map(|line| {
        let (name, value) = line.split_once(':')?;
        (name == key).then(|| value.trim())
    })
}

pub(crate) fn list() -> Value {
    let resources: Vec<Value> = knowledge_files()
        .iter()
        .map(|file| {
            let title = frontmatter(file.text, "title")
                .or_else(|| file.text.lines().find_map(|line| line.strip_prefix("# ")))
                .unwrap_or(file.path);
            let mut resource = json!({
                "uri": file.uri(),
                "name": file.path,
                "title": title,
                "mimeType": "text/markdown",
                "size": file.text.len(),
            });
            if let Some(description) = frontmatter(file.text, "description") {
                resource["description"] = json!(description);
            }
            resource
        })
        .collect();
    json!({"resources": resources})
}

pub(crate) fn read(uri: &str) -> Option<Value> {
    let file = knowledge_file_by_uri(uri)?;
    Some(json!({"contents": [{"uri": uri, "mimeType": "text/markdown", "text": file.text}]}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, fs, path::Path};

    #[test]
    fn knowledge_recipe_references_name_published_recipes() {
        for file in knowledge_files() {
            let Some(references) = frontmatter(file.text, "related_recipes") else {
                continue;
            };
            if references == "[]" {
                continue;
            }
            for id in references.split(',').map(str::trim) {
                assert!(
                    limo_cad_recipes::find(id).is_ok(),
                    "knowledge article {} refers to an unpublished recipe: {id}",
                    file.path
                );
            }
        }
    }

    #[test]
    fn knowledge_resources_include_every_repository_markdown_file_unchanged() {
        fn collect(root: &Path, dir: &Path, result: &mut BTreeMap<String, String>) {
            for entry in fs::read_dir(dir).unwrap() {
                let entry = entry.unwrap();
                let kind = entry.file_type().unwrap();
                if kind.is_dir() {
                    collect(root, &entry.path(), result);
                } else if kind.is_file() && entry.path().extension().is_some_and(|ext| ext == "md")
                {
                    result.insert(
                        entry
                            .path()
                            .strip_prefix(root)
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .replace('\\', "/"),
                        fs::read_to_string(entry.path()).unwrap(),
                    );
                }
            }
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../knowledge");
        let mut actual = BTreeMap::new();
        collect(&root, &root, &mut actual);
        let bundled: BTreeMap<_, _> = knowledge_files()
            .iter()
            .map(|file| (file.path.to_string(), file.text.to_string()))
            .collect();
        assert_eq!(bundled, actual);
    }
}
