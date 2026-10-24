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
                let snapshot = match engine.project_snapshot() {
                    Ok(snapshot) => Some(snapshot),
                    Err(_) if !active.is_null() => None,
                    Err(error) => return Err(error),
                };
                let reservation = reserve_project_export(publisher, &expected.window_id)?;
                PublishPayload {
                    focus: focus.to_owned(),
                    model_json: None,
                    snapshot,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_bridge::native_interface::tests::Fixture;
    use limo_cad_project_file::{ProjectSnapshot, SnapshotIntent};
    use std::fs;

    fn directory(fixture: &Fixture) -> std::path::PathBuf {
        crate::session_bridge::session_root().join(
            fixture
                .bridge
                .session_id_for_window("main")
                .unwrap()
                .unwrap(),
        )
    }
    fn heartbeat(fixture: &Fixture) -> Value {
        serde_json::from_str(
            &fs::read_to_string(directory(fixture).join("heartbeat.json")).unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn domains_publish_as_one_capsule_and_active_sketch_retains_completed_receipt() {
        let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
        let fixture = Fixture::new();
        let owner = fixture.owner();
        let revision = fixture
            .bridge
            .engine_revision_for_window("main")
            .unwrap()
            .unwrap();
        let initial = fixture.engine.project_snapshot().unwrap();
        let snapshot = ProjectSnapshot::new(
            initial.model_json().to_owned(),
            vec![SnapshotIntent {
                name: "intent/future.cbor".into(),
                payload: vec![0, 255, 7],
            }],
        )
        .unwrap();
        let loaded = fixture
            .bridge
            .apply_native_snapshot_at(&fixture.engine, &owner, revision, &snapshot, || Ok(()))
            .unwrap();
        fixture
            .bridge
            .publish_native_document(&fixture.engine, &loaded.context, "solid")
            .unwrap();
        let publication =
            limo_cad_session_publication::read_committed(&directory(&fixture)).unwrap();
        assert_eq!(publication.snapshot().intents(), snapshot.intents());
        let published = heartbeat(&fixture);
        assert_eq!(published["publication_version"], 1);
        assert_eq!(
            published["publication_receipt"],
            json!(publication.receipt())
        );
        let legacy = fs::read_to_string(directory(&fixture).join("model.json")).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&legacy).unwrap()["format"],
            "limo-cad-composite-publication-required"
        );
        assert!(
            parse_engine_envelope(AppState::new().project_load(&json!(legacy).to_string()))
                .is_err()
        );
        fixture
            .bridge
            .apply_native_mutation(
                &fixture.engine,
                &loaded.context,
                "sketch_begin",
                &json!({"type":"origin_plane","plane":"xy"}),
                || Ok(()),
            )
            .unwrap();
        assert_eq!(
            heartbeat(&fixture)["publication_receipt"],
            published["publication_receipt"]
        );
        fixture
            .bridge
            .publish_native_document(&fixture.engine, &loaded.context, "sketch")
            .unwrap();
        assert_eq!(
            limo_cad_session_publication::read_committed(&directory(&fixture)).unwrap(),
            publication
        );
        assert_eq!(
            heartbeat(&fixture)["model_generation"],
            published["model_generation"]
        );
        assert!(
            heartbeat(&fixture)["active_sketch_generation"]
                .as_u64()
                .unwrap()
                > publication.receipt().engine_revision
        );
    }
    #[test]
    fn failed_heartbeat_does_not_acknowledge_committed_capsule_and_retry_recovers() {
        let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
        let fixture = Fixture::new();
        let owner = fixture.owner();
        fixture
            .bridge
            .publish_native_document(&fixture.engine, &owner, "solid")
            .unwrap();
        let acknowledged = heartbeat(&fixture);
        fixture.rename(&owner, "New publication").unwrap();
        let path = directory(&fixture).join("heartbeat.json");
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(fixture
            .bridge
            .publish_native_document(&fixture.engine, &owner, "solid")
            .is_err());
        let committed = limo_cad_session_publication::read_committed(&directory(&fixture)).unwrap();
        assert!(
            committed.receipt().engine_revision
                > acknowledged["model_generation"].as_u64().unwrap()
        );
        fs::remove_dir(&path).unwrap();
        fixture.bridge.heartbeat_for_window("main").unwrap();
        assert_eq!(
            heartbeat(&fixture)["publication_receipt"],
            acknowledged["publication_receipt"]
        );
        assert_eq!(
            heartbeat(&fixture)["model_generation"],
            acknowledged["model_generation"]
        );
        fixture
            .bridge
            .publish_native_document(&fixture.engine, &owner, "solid")
            .unwrap();
        let recovered = limo_cad_session_publication::read_committed(&directory(&fixture)).unwrap();
        assert_eq!(
            heartbeat(&fixture)["publication_receipt"],
            json!(recovered.receipt())
        );
        assert!(recovered.receipt().export_sequence > committed.receipt().export_sequence);
    }
}
