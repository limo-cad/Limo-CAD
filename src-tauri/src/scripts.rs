//! Native script workspace adapter. The app embeds the same MCP library as
//! the command-line server, so it needs neither a sidecar nor another runner.
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use serde_json::{json, Value};

use crate::{session_bridge::SessionBridgeState, state::AppState};

#[derive(Default)]
pub struct NativeScriptState {
    active: Arc<AtomicBool>,
}

struct RunningScript(Arc<AtomicBool>);
impl Drop for RunningScript {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
impl NativeScriptState {
    fn acquire(&self) -> Result<RunningScript, Value> {
        self.active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| failure("script_busy", "A script is already running. Use its playback controls or wait for it to finish."))?;
        Ok(RunningScript(self.active.clone()))
    }
}

fn failure(code: &str, message: impl ToString) -> Value {
    json!({"code":code,"message":message.to_string()})
}

#[tauri::command]
pub fn native_script_inspect(source: Option<String>, path: Option<String>) -> Result<Value, Value> {
    let mut arguments = json!({});
    if let Some(source) = source {
        arguments["source"] = json!(source);
    }
    if let Some(path) = path {
        arguments["path"] = json!(path);
    }
    nbcad_mcp::inspect_script(arguments).map_err(|error| failure("invalid_script", error))
}

#[tauri::command]
pub async fn native_script_run(
    window: tauri::WebviewWindow,
    bridge: tauri::State<'_, SessionBridgeState>,
    engine: tauri::State<'_, AppState>,
    state: tauri::State<'_, NativeScriptState>,
    source: String,
    mode: String,
    speed: f64,
) -> Result<Value, Value> {
    let running = state.acquire()?;
    let session_id = bridge
        .active_script_session(window.label(), &engine)
        .map_err(|error| failure("script_session_unavailable", error))?;
    // The async command returns control to the UI while its blocking Rust
    // interpreter waits for the existing presentation and mutation receipts.
    tauri::async_runtime::spawn_blocking(move || {
        let _running = running;
        nbcad_mcp::run_script(&source, Some(&session_id), &mode, speed)
            .map_err(|error| failure("script_failed", error))
    })
    .await
    .map_err(|error| failure("script_worker_failed", error))?
}

#[tauri::command]
pub async fn native_script_preview(
    state: tauri::State<'_, NativeScriptState>,
    source: String,
) -> Result<Value, Value> {
    let running = state.acquire()?;
    tauri::async_runtime::spawn_blocking(move || {
        let _running = running;
        nbcad_mcp::preview_script(&source).map_err(|error| failure("preview_failed", error))
    })
    .await
    .map_err(|error| failure("script_worker_failed", error))?
}

#[tauri::command]
pub fn native_script_examples() -> Value {
    let mut examples = json!([
        {
            "id":"garden-bench",
            "name":"Garden bench",
            "summary":"Build and validate the complete parametric timber bench, its assembly, and manufacturing checks.",
            "group":"document/scripts",
            "operation":"cad_script",
            "preview":false,
            "source":include_str!("../../examples/scripts/garden-bench.nbcad.jsonc"),
        },
        {
            "id":"fillet-basics",
            "name":"Sketch, extrude, ease the edges",
            "summary":"Make a dimensioned timber block, extrude 12 mm of stock, then round its four top edges by 2 mm.",
            "group":"solid/refine",
            "operation":"solid_fillet",
            "preview":true,
            "source":include_str!("../../examples/scripts/fillet-basics.nbcad.jsonc"),
        }
    ]);
    for example in examples.as_array_mut().unwrap() {
        let inspected = nbcad_mcp::inspect_script(json!({"source":example["source"]}))
            .expect("bundled script must pass its shared preflight test");
        example["operations"] = inspected["operations"].clone();
    }
    examples
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_script_guard_releases_after_failure_or_completion() {
        let state = NativeScriptState::default();
        let first = state.acquire().unwrap();
        assert_eq!(state.acquire().err().unwrap()["code"], "script_busy");
        drop(first);
        assert!(state.acquire().is_ok());
    }

    #[test]
    fn bundled_scripts_use_the_shared_preflight_parser() {
        for example in native_script_examples().as_array().unwrap() {
            let inspected =
                native_script_inspect(example["source"].as_str().map(str::to_owned), None).unwrap();
            assert!(inspected["step_count"].as_u64().unwrap() > 0);
            assert!(!inspected["chapters"].as_array().unwrap().is_empty());
            assert_eq!(example["operations"], inspected["operations"]);
        }
        assert_eq!(
            native_script_inspect(Some("{}".into()), None).unwrap_err()["code"],
            "invalid_script"
        );
    }
}
