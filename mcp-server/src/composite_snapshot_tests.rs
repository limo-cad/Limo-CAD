use super::*;
use limo_cad_project_file::{
    ProjectPublication, ProjectSnapshot, PublicationReceipt, SnapshotIntent,
};

struct Environment {
    root: std::path::PathBuf,
    previous: Option<std::ffi::OsString>,
}
impl Environment {
    fn new(root: std::path::PathBuf) -> Self {
        limo_cad_session_storage::create_registry(&root).unwrap();
        let previous = std::env::var_os("LIMO_CAD_SESSION_DIR");
        std::env::set_var("LIMO_CAD_SESSION_DIR", &root);
        Self { root, previous }
    }
}
impl Drop for Environment {
    fn drop(&mut self) {
        if let Some(value) = self.previous.take() {
            std::env::set_var("LIMO_CAD_SESSION_DIR", value);
        } else {
            std::env::remove_var("LIMO_CAD_SESSION_DIR");
        }
        assert_eq!(self.root.parent(), Some(std::env::temp_dir().as_path()));
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}

fn publish(
    id: &str,
    previous: Option<&PublicationReceipt>,
    snapshot: ProjectSnapshot,
    revision: u64,
) -> ProjectPublication {
    let publication = ProjectPublication::new(
        PublicationReceipt {
            process_instance_id: "test-process".into(),
            session_id: id.into(),
            window_id: "main".into(),
            document_id: "test-document".into(),
            epoch: 1,
            engine_revision: revision,
            export_sequence: revision,
        },
        snapshot,
    )
    .unwrap();
    let directory = session::session_dir().join(id);
    limo_cad_session_publication::commit(&directory, previous, &publication).unwrap();
    session::write_session(
        id,
        "heartbeat.json",
        &json!({
            "publication_version":1,"publication_receipt":publication.receipt(),
            "session_id":id,"process_instance_id":"test-process","window_id":"main",
            "document_id":"test-document","model_generation":revision
        })
        .to_string(),
    )
    .unwrap();
    publication
}

#[test]
fn composite_refresh_observes_domain_only_changes_and_preserves_state_on_invalid_domain() {
    let _lock = session::env_lock();
    let id = session::test_session_uuid();
    let directory = std::env::temp_dir().join(format!("limo-composite-consumer-{id}"));
    let _environment = Environment::new(directory);
    let mut server = CadServer::new().unwrap();
    let model = server
        .manager
        .export_project_snapshot()
        .unwrap()
        .model_json()
        .to_owned();
    let snapshot = |name: &str, bytes| {
        ProjectSnapshot::new(
            model.clone(),
            vec![SnapshotIntent {
                name: name.into(),
                payload: bytes,
            }],
        )
        .unwrap()
    };
    let first = publish(
        &id,
        None,
        snapshot("intent/future.cbor", vec![0, 255, 1]),
        1,
    );
    assert!(server.load_snapshot_model(&id, true).unwrap());
    let first_trace = server.tool_trace.clone();
    assert!(!server.load_snapshot_model(&id, true).unwrap());
    assert_eq!(server.tool_trace, first_trace);
    let second = publish(
        &id,
        Some(first.receipt()),
        snapshot("intent/future.cbor", vec![0, 255, 2]),
        2,
    );
    assert!(server.load_snapshot_model(&id, true).unwrap());
    assert_eq!(
        server.manager.project_intents(),
        second.snapshot().intents()
    );
    assert_eq!(server.attached_generation, Some(2));
    assert_eq!(server.tool_trace.len(), 1);
    assert_eq!(server.tool_trace[0]["name"], "cad_load_project_snapshot");
    let encoded = server.tool_trace[0]["arguments"]["snapshot_cbor_base64"]
        .as_str()
        .unwrap();
    assert_eq!(
        ProjectSnapshot::decode_base64(encoded).unwrap(),
        *second.snapshot()
    );
    let mut replay = CadServer::new().unwrap();
    for call in &server.tool_trace {
        replay
            .call_tool(call["name"].as_str().unwrap(), call["arguments"].clone())
            .unwrap();
    }
    assert_eq!(
        replay.manager.export_project_snapshot().unwrap(),
        server.manager.export_project_snapshot().unwrap()
    );
    replay
        .call_tool("cad_set_document_name", json!({"name":"After baseline"}))
        .unwrap();
    assert_eq!(replay.manager.document_dto().name, "After baseline");
    assert_eq!(
        replay.manager.project_intents(),
        second.snapshot().intents()
    );
    assert!(server.require_complete_script_baseline().is_err());
    assert!(server.export_script(&json!({})).is_err());
    let before = server.manager.export_project_snapshot().unwrap();
    let trace = server.tool_trace.clone();
    publish(
        &id,
        Some(second.receipt()),
        snapshot("intent/thermal-studies.cbor", vec![0x80]),
        3,
    );
    assert!(server.load_snapshot_model(&id, true).is_err());
    assert_eq!(server.manager.export_project_snapshot().unwrap(), before);
    assert_eq!(server.attached_generation, Some(2));
    assert_eq!(server.loaded_snapshot_intents, before.intents());
    assert_eq!(server.tool_trace, trace);
}

#[test]
fn composite_capability_and_receipt_failures_never_use_legacy_model() {
    let _lock = session::env_lock();
    let id = session::test_session_uuid();
    let directory = std::env::temp_dir().join(format!("limo-composite-capability-{id}"));
    let _environment = Environment::new(directory);
    let server = CadServer::new().unwrap();
    let snapshot = server.manager.export_project_snapshot().unwrap();
    session::write_session(&id, "model.json", snapshot.model_json()).unwrap();
    assert!(session::read_composite_publication(&id).unwrap().is_none());
    let publication = publish(&id, None, snapshot, 1);
    assert_eq!(
        session::read_composite_publication(&id).unwrap().unwrap(),
        publication
    );
    let good: Value =
        serde_json::from_str(&session::read_session_file(&id, "heartbeat.json").unwrap()).unwrap();
    for case in 0..4 {
        let mut heartbeat = good.clone();
        match case {
            0 => {
                heartbeat
                    .as_object_mut()
                    .unwrap()
                    .remove("publication_version");
            }
            1 => heartbeat["publication_version"] = json!(2),
            2 => heartbeat["document_id"] = json!("other"),
            _ => heartbeat["publication_receipt"]["export_sequence"] = json!(99),
        }
        session::write_session(&id, "heartbeat.json", &heartbeat.to_string()).unwrap();
        assert!(session::read_composite_publication(&id).is_err(), "{case}");
    }
}

#[test]
fn model_only_composite_keeps_legacy_baseline_and_can_replay() {
    let _lock = session::env_lock();
    let id = session::test_session_uuid();
    let directory = std::env::temp_dir().join(format!("limo-composite-legacy-trace-{id}"));
    let _environment = Environment::new(directory);
    let mut server = CadServer::new().unwrap();
    server
        .call_tool(
            "cad_set_document_name",
            json!({"name":"Legacy-compatible baseline"}),
        )
        .unwrap();
    let snapshot = server.manager.export_project_snapshot().unwrap();
    publish(&id, None, snapshot.clone(), 1);
    server.load_snapshot_model(&id, false).unwrap();
    assert_eq!(server.tool_trace.len(), 1);
    assert_eq!(server.tool_trace[0]["name"], "cad_load_project_model");
    assert!(server.require_complete_script_baseline().is_ok());
    let mut replay = CadServer::new().unwrap();
    replay
        .call_tool(
            "cad_load_project_model",
            server.tool_trace[0]["arguments"].clone(),
        )
        .unwrap();
    assert_eq!(replay.manager.export_project_snapshot().unwrap(), snapshot);
}
