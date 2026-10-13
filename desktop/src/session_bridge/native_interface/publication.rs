//! Native hosts use the existing snapshot reservation/write contract. A
//! coherent live-engine capture never relies on a later interface callback.

use super::*;
use crate::session_bridge::{parse_engine_envelope, reserve_project_export, PublishPayload};

impl SessionBridgeState {
    pub(crate) fn publish_native_document(
        &self,
        engine: &AppState,
        expected: &DocumentContext,
        focus: &str,
    ) -> Result<Value, String> {
        for _ in 0..4 {
            let payload = {
                let mut publishers = self
                    .publishers
                    .lock()
                    .map_err(|_| "Session publisher lock poisoned")?;
                let publisher = publishers
                    .get_mut(&expected.window_id)
                    .ok_or("Native interface window is no longer available")?;
                check_owner(publisher, engine, expected)?;
                let active = parse_engine_envelope(engine.engine_call("active_sketch", ""))?;
                let model_json =
                    match parse_engine_envelope(engine.engine_call("project_export_model", "")) {
                        Ok(Value::String(model)) => Some(model),
                        Ok(_) => {
                            return Err("Native project export did not return model text".into())
                        }
                        Err(_) if !active.is_null() => None,
                        Err(error) => return Err(error),
                    };
                let reservation = reserve_project_export(publisher, &expected.window_id)?;
                PublishPayload {
                    focus: focus.to_owned(),
                    model_json,
                    active_sketch_json: (!active.is_null()).then(|| active.to_string()),
                    generation: reservation["generation"]
                        .as_u64()
                        .ok_or("Native export ticket is invalid")?,
                    session_id: reservation["session_id"].as_str().map(str::to_owned),
                    project_session_id: Some(expected.document_id.clone()),
                }
            };
            let result = self.write_for_window(&expected.window_id, payload)?;
            if result["skipped"] == false {
                return Ok(result);
            }
            if result["reason"] != "engine_revision_changed"
                && result["reason"] != "stale_generation"
            {
                return Err(
                    "Native document was replaced before its snapshot could publish".into(),
                );
            }
        }
        Err(
            "Native document kept changing during snapshot publication; inspect before retrying"
                .into(),
        )
    }
}
