use serde_json::{json, Value};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

#[path = "desktop_process.rs"]
mod desktop_process;

fn unique_recipe_window(sessions: &Value) -> Option<&str> {
    let windows = sessions["windows"].as_array()?;
    if windows.len() != 1 {
        return None;
    }
    windows[0]["active_session_id"].as_str()
}

pub fn open_recipe(recipe: &str) -> Result<bool, String> {
    let sessions = crate::session::sessions_list_json();
    let Some(session_id) = unique_recipe_window(&sessions) else {
        return Ok(false);
    };
    let reply = crate::session::request_ui(
        &json!({
            "action":"open_recipe", "recipe":recipe, "session_id":session_id
        }),
        None,
    )?;

    Ok(recipe_was_queued(&reply))
}

fn recipe_was_queued(reply: &Value) -> bool {
    reply["status"] == "applied" && reply["recipe"]["status"] == "queued"
}

/// Managed desktop workers launch their own installed runtime. Standalone
/// workers retain explicit artifact selection, without a shell or
/// inherited stdio handles. Correlate readiness with the child's PID lease.
pub fn launch(arguments: &Value) -> Result<Value, String> {
    let configured = std::env::var("LIMO_CAD_LOCAL_RUNTIME")
        .ok()
        .or_else(|| {
            arguments
                .get("executable")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| std::env::var("LIMO_CAD_DESKTOP_BIN").ok())
        })
        .ok_or("Set LIMO_CAD_DESKTOP_BIN or provide the CAD executable path")?;
    let path = PathBuf::from(configured)
        .canonicalize()
        .map_err(|e| format!("CAD executable: {e}"))?;
    if !path.is_file() {
        return Err("CAD executable is not a file".into());
    }
    let mut child =
        desktop_process::spawn(&path).map_err(|e| format!("Could not launch CAD: {e}"))?;
    let pid = child.id();
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Err(format!("CAD exited before becoming ready: {status}"));
        }
        let dir = crate::session::session_dir().join("_ui/processes");
        if let Ok(entries) = limo_cad_session_storage::read_dir(dir) {
            for entry in entries.filter_map(Result::ok) {
                let Ok(body) = limo_cad_session_storage::read_to_string(entry.path()) else {
                    continue;
                };
                let Ok(lease) = serde_json::from_str::<Value>(&body) else {
                    continue;
                };
                if lease.get("pid").and_then(Value::as_u64) != Some(pid as u64) {
                    continue;
                }
                let session = lease
                    .get("windows")
                    .and_then(Value::as_array)
                    .and_then(|windows| windows.first())
                    .and_then(|w| w.get("active_session_id"))
                    .and_then(Value::as_str);
                if let Some(session_id) = session {
                    if crate::session::heartbeat_meta(session_id)
                        .get("stale")
                        .and_then(Value::as_bool)
                        != Some(false)
                    {
                        continue;
                    }
                    let ui = crate::session::request_ui(
                        &json!({"session_id":session_id,"action":"inspect"}),
                        None,
                    )?;
                    if ui.get("status").and_then(Value::as_str) == Some("applied") {
                        return Ok(
                            json!({"status":"ready","pid":pid,"session_id":session_id,"executable":path,"ui":ui}),
                        );
                    }
                    return Ok(
                        json!({"status":"starting","pid":pid,"session_id":session_id,"executable":path,"ui":ui}),
                    );
                }
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    Ok(json!({"status":"starting","pid":pid,"executable":path,
        "hint":"Launch is not yet acknowledged. Inspect sessions; do not launch a duplicate automatically."}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipe_handoff_never_guesses_between_windows() {
        assert_eq!(unique_recipe_window(&json!({"windows":[]})), None);
        assert_eq!(
            unique_recipe_window(&json!({"windows":[{"active_session_id":"a"}]})),
            Some("a")
        );
        assert_eq!(
            unique_recipe_window(
                &json!({"windows":[{"active_session_id":"a"},{"active_session_id":"b"}]})
            ),
            None
        );
        assert_eq!(unique_recipe_window(&json!({"windows":[{}]})), None);
    }

    #[test]
    fn older_desktop_rejection_falls_back_to_the_current_recipe_window() {
        for reply in [
            json!({"status":"failed","error":"Unknown UI action: open_recipe"}),
            json!({"status":"applied","ui":{}}),
            json!({"status":"timeout"}),
        ] {
            assert!(
                !recipe_was_queued(&reply),
                "No queued receipt: the launcher must retain the URL for a new window"
            );
        }
        assert!(recipe_was_queued(
            &json!({"status":"applied","recipe":{"status":"queued"}})
        ));
    }
}
