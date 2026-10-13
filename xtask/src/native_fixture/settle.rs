//! Read-only settlement after a single delivered native OS gesture.
use crate::replay::Client;
use anyhow::{ensure, Result};
use serde_json::{json, Value};
use std::{
    fs,
    path::Path,
    thread,
    time::{Duration, Instant},
};

fn inspection_reply(result: Value) -> Result<Option<Value>> {
    let busy = result["isError"] != true
        && result["content"]
            .as_array()
            .and_then(|content| content.iter().find(|item| item["type"] == "text"))
            .and_then(|item| item["text"].as_str())
            .and_then(|text| serde_json::from_str::<Value>(text).ok())
            .is_some_and(|value| {
                value["status"] == "failed"
                    && value["code"] == "native_busy"
                    && value["mutation_applied"] == false
            });
    if busy {
        return Ok(None);
    }
    let value = Client::decode_call_result("cad_interface", result)?;
    ensure!(value["status"] == "applied", "Interface failed: {value}");
    Ok(Some(value))
}

/// Native previews can still be solving after the preceding UI receipt. Wait
/// through read-only inspection rather than replaying an input or mutation.
pub(super) fn inspect_ready(c: &mut Client) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        ensure!(
            !remaining.is_zero(),
            "Native modeling worker did not settle for read-only inspection"
        );
        let reply = c.rpc_with_timeout(
            "tools/call",
            json!({"name":"cad_interface","arguments":{"action":"inspect"}}),
            remaining,
        )?;
        if let Some(view) = inspection_reply(reply)? {
            return Ok(view);
        }
        thread::sleep(
            Duration::from_millis(50).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
}

pub(crate) fn inspect_after_gesture(
    c: &mut Client,
    out: &Path,
    name: &str,
    owner: &Value,
) -> Result<()> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(15);
    let mut attempts = Vec::new();
    let result = (|| loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        ensure!(
            !remaining.is_zero(),
            "Native modeling worker did not settle after the original OS gesture {name}"
        );

        let reply = c.rpc_with_timeout(
            "tools/call",
            json!({"name":"cad_interface","arguments":{"action":"inspect"}}),
            remaining,
        )?;
        match inspection_reply(reply)? {
            None => {
                attempts.push(json!({"elapsed_ms":started.elapsed().as_millis(),
                        "code":"native_busy", "mutation_applied":false}));
                thread::sleep(
                    Duration::from_millis(50)
                        .min(deadline.saturating_duration_since(Instant::now())),
                );
            }
            Some(view) => {
                attempts.push(json!({"elapsed_ms":started.elapsed().as_millis(),
                        "inspection":view}));
                ensure!(
                        &view["active_session_id"] == owner,
                        "Owned native session changed while settling OS gesture {name}: expected {owner}, observed {}",
                        view["active_session_id"]
                    );
                return Ok::<_, anyhow::Error>(());
            }
        }
    })();
    fs::write(
        out.join(format!("{name}-settle.json")),
        serde_json::to_vec_pretty(&json!({"read_only":true,"gesture_replayed":false,
            "owner":owner,"attempts":attempts,"elapsed_ms":started.elapsed().as_millis(),
            "error":result.as_ref().err().map(|error| format!("{error:#}"))}))?,
    )?;
    result
}
#[cfg(test)]
mod tests {
    use super::*;

    fn reply(value: Value) -> Value {
        json!({"content":[{"type":"text","text":value.to_string()}]})
    }

    #[test]
    fn only_explicitly_unapplied_native_busy_inspection_is_retryable() {
        let busy = json!({"status":"failed","code":"native_busy","mutation_applied":false});
        assert!(inspection_reply(reply(busy.clone())).unwrap().is_none());
        for changed in [
            json!({"status":"failed","code":"native_busy","mutation_applied":true}),
            json!({"status":"failed","code":"native_busy"}),
            json!({"status":"failed","code":"owner_changed","mutation_applied":false}),
            json!({"status":"failed","error":"native_busy; mutation_applied:false"}),
        ] {
            assert!(inspection_reply(reply(changed)).is_err());
        }
        let mut tool_error = reply(busy);
        tool_error["isError"] = json!(true);
        assert!(inspection_reply(tool_error).is_err());
        assert!(
            inspection_reply(json!({"content":[{"type":"text","text":"invalid JSON"}]})).is_err()
        );
        let applied = json!({"status":"applied","active_session_id":"owned"});
        assert_eq!(
            inspection_reply(reply(applied.clone())).unwrap(),
            Some(applied)
        );
    }
}
