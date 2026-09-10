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
    // The recipe-library layer supplies the shared catalog. The adapter itself
    // can inspect and run any source loaded by the user without bundled designs.
    json!([])
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
    fn source_inspection_uses_the_shared_preflight_parser() {
        let inspected = native_script_inspect(
            Some(r#"{"version":1,"name":"Loaded lesson","steps":[{"note":"Inspect before running"}]}"#.into()),
            None,
        ).unwrap();
        assert_eq!(inspected["step_count"], 1);
        assert_eq!(inspected["chapters"].as_array().unwrap().len(), 1);
        assert_eq!(
            native_script_inspect(Some("{}".into()), None).unwrap_err()["code"],
            "invalid_script"
        );
    }
}
