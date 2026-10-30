//! Trusted, offline bundle authoring. This is not an MCP tool or a live-session API.
use serde_json::Value;

#[path = "../../authoring/measure.rs"]
mod measure;
#[path = "lesson_authoring/native_measure.rs"]
mod native_measure;

pub const MAX_SOURCE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_OPERATIONS: u64 = 512;

/// Run an installed recipe before it has a public Help/guide Id. Its immutable
/// source comes only from the bundled registry, never a file, session or URL.
pub fn run_bundled(recipe_id: &str) -> Result<Value, String> {
    let source = limo_cad_recipes::find(recipe_id)?.source;
    let script = limo_cad_script::Script::parse(source)?;
    let metadata = script.metadata();
    let operations = metadata["step_count"]
        .as_u64()
        .unwrap_or(u64::MAX)
        .checked_add(metadata["check_count"].as_u64().unwrap_or(u64::MAX))
        .ok_or("Offline operation count overflow")?;
    if source.len() > MAX_SOURCE_BYTES || operations > MAX_OPERATIONS {
        return Err("Bundled recipe exceeds the offline authoring budget".into());
    }
    crate::interface::validate_script(&script)?;
    if recipe_id == "show-me-measure" {
        native_measure::run(&script)
    } else {
        // Preserve the existing same-checkout authoring path. No session id is
        // accepted here and run_script creates a fresh independent CadServer.
        crate::run_script(source, None, None, "fast", 1.0)
    }
}
