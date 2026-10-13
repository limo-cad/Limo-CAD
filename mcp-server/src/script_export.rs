//! Version-1 `.limo.jsonc` export helpers.
//!
//! Distinct from `cad_script`, which dumps the forward MCP tool trace as
//! `{ calls: [{ name, arguments }] }`.

use serde_json::{json, Value};

use crate::interface;

/// Build a minimal version-1 script source from a portable session tool trace.
///
/// Skips `cad_load_project_model` baselines. Arguments are copied literally — this
/// is intentionally **lossy** relative to hand-authored `$select` / comments.
pub fn session_trace_to_v1_source(calls: &[Value], name: &str) -> Result<String, String> {
    if name.trim().is_empty() {
        return Err("export_script name must be nonempty".into());
    }
    let mut steps = Vec::new();
    for (index, call) in calls.iter().enumerate() {
        let op = call["name"]
            .as_str()
            .ok_or_else(|| format!("trace[{index}] missing name"))?;
        if op == "cad_load_project_model" {
            continue;
        }
        let group = interface::group_for(op).ok_or_else(|| {
            format!("trace[{index}] operation {op} is not in the interface catalog")
        })?;
        let arguments = call.get("arguments").cloned().unwrap_or_else(|| json!({}));
        if !arguments.is_object() {
            return Err(format!("trace[{index}] arguments must be an object"));
        }
        steps.push(json!({
            "call": {
                "group": group,
                "operation": op,
                "arguments": arguments
            }
        }));
    }
    if steps.is_empty() {
        return Err(
            "session_trace has no portable modeling calls to export (only attach baselines?)"
                .into(),
        );
    }
    let document = json!({
        "version": 1,
        "name": name,
        "starting_state": "empty",
        "steps": steps
    });
    serde_json::to_string_pretty(&document).map_err(|e| e.to_string())
}

pub fn export_script_result(
    source: String,
    fidelity: &str,
    stale: bool,
    notes: Vec<&str>,
) -> Result<Value, String> {
    let source = pretty_json(source);
    let script = limo_cad_script::Script::parse(&source)?;
    let meta = script.metadata();
    Ok(json!({
        "format": "limo.jsonc",
        "version": 1,
        "fidelity": fidelity,
        "stale": stale,
        "source": source,
        "name": meta["name"],
        "step_count": meta["step_count"],
        "check_count": meta["check_count"],
        "notes": notes,
    }))
}

/// Compact retained JSON becomes a readable export. JSONC comments are already
/// gone once includes have been flattened, so pretty-printing does not drop them.
fn pretty_json(source: String) -> String {
    serde_json::from_str::<Value>(&source)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .unwrap_or(source)
}
