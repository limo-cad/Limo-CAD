//! MCP prompts capability — help-search golden path (single prompt this slice).
use serde_json::{json, Value};

pub(crate) const HELP_SEARCH: &str = "help_search";

pub(crate) fn list() -> Value {
    json!({
        "prompts": [{
            "name": HELP_SEARCH,
            "description": "Form a contextual OKF help query and call cad_help (search → get 1–2 ids; optional resources/read for the full page).",
            "arguments": [{
                "name": "query",
                "description": "What you are stuck on or the current design/process task (used as the cad_help search string).",
                "required": false
            }]
        }]
    })
}

/// Build `prompts/get` result. `arguments` may include optional `query` (or alias `context`).
pub(crate) fn get(name: &str, arguments: Option<&Value>) -> Result<Value, String> {
    match name {
        HELP_SEARCH => Ok(help_search_prompt(arguments)),
        _ => Err(format!("unknown prompt: {name}")),
    }
}

fn help_search_prompt(arguments: Option<&Value>) -> Value {
    let query = arguments
        .and_then(|args| {
            args.get("query")
                .or_else(|| args.get("context"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
        .map(|s| s.to_string());

    let search_line = match &query {
        Some(q) => format!("Search for: {q}"),
        None => {
            "Search for: (derive a short query from the current design or process task)".to_string()
        }
    };

    let text = format!(
        r#"Use the bundled OKF help corpus for design and process guidance.

{search_line}

Golden path:
1. Call cad_help with action "search" and the query above (default limit 5, max 10). Prefer sharp, task-specific wording.
2. From the hits, pick 1–2 strongest ids and call cad_help with action "get" for each (id-only).
3. When the full markdown page is useful after selection, resources/read the matching limo-cad://knowledge/... URI (start from resources/list / limo-cad://knowledge/index.md when browsing).
4. cad_help topics is available when you want label discovery before searching.

Prefer cad_help and the OKF corpus first. Datasheets, recipes, and web search remain available after local retrieval when the task needs them.

After reading, continue the modeling or process work with the guidance you found."#
    );

    json!({
        "description": "Help-search golden path: cad_help search → get, optional resources/read.",
        "messages": [{
            "role": "user",
            "content": {
                "type": "text",
                "text": text
            }
        }]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_advertises_help_search_with_optional_query() {
        let listed = list();
        let prompts = listed["prompts"].as_array().unwrap();
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0]["name"], HELP_SEARCH);
        assert_eq!(prompts[0]["arguments"][0]["name"], "query");
        assert_eq!(prompts[0]["arguments"][0]["required"], false);
    }

    #[test]
    fn get_help_search_mentions_cad_help_and_interpolates_query() {
        let got = get(HELP_SEARCH, Some(&json!({"query": "clearance fit"}))).unwrap();
        let text = got["messages"][0]["content"]["text"].as_str().unwrap();
        assert!(text.contains("cad_help"), "{text}");
        assert!(text.contains("Search for: clearance fit"), "{text}");
        assert!(text.contains("resources/read"), "{text}");
    }

    #[test]
    fn get_accepts_context_alias_and_rejects_unknown_name() {
        let got = get(HELP_SEARCH, Some(&json!({"context": "snap fit"}))).unwrap();
        let text = got["messages"][0]["content"]["text"].as_str().unwrap();
        assert!(text.contains("Search for: snap fit"), "{text}");
        assert!(get("missing", None).is_err());
    }
}
