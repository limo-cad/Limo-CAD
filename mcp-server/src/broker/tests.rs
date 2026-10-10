use super::*;
use std::{ffi::OsString, path::PathBuf};

struct Registry {
    root: PathBuf,
    previous: Option<OsString>,
    first: Route,
    second: Route,
}

impl Registry {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("limo-cad-broker-{}", session::test_session_uuid()));
        let previous = std::env::var_os("LIMO_CAD_SESSION_DIR");
        std::env::set_var("LIMO_CAD_SESSION_DIR", &root);
        let first = Route {
            session_id: session::test_session_uuid(),
            window_id: "main".into(),
            document_id: "first-design".into(),
            process_instance_id: "first-process".into(),
        };
        let second = Route {
            session_id: session::test_session_uuid(),
            window_id: "main".into(),
            document_id: "second-design".into(),
            process_instance_id: "second-process".into(),
        };
        for route in [&first, &second] {
            session::write_session(&route.session_id, "model.json", "{}\n").unwrap();
            session::write_session(
                &route.session_id,
                "focus.json",
                &serde_json::to_string(route).unwrap(),
            )
            .unwrap();
            let mut heartbeat = serde_json::to_value(route).unwrap();
            heartbeat["updated_ms"] = json!(session::now_ms());
            heartbeat["generation"] = json!(1);
            heartbeat["model_generation"] = json!(1);
            heartbeat["interface_version"] = json!(1);
            session::write_session(&route.session_id, "heartbeat.json", &heartbeat.to_string())
                .unwrap();
            limo_cad_session_storage::atomic_write(
                &root.join("_ui/processes").join(format!("{}.json", route.process_instance_id)),
                json!({"process_instance_id":route.process_instance_id,"updated_ms":session::now_ms(),
                    "windows":[{"window_id":route.window_id,"active_document_id":route.document_id,
                        "active_session_id":route.session_id}]}).to_string().as_bytes(),
            ).unwrap();
        }
        Self {
            root,
            previous,
            first,
            second,
        }
    }
}

impl Drop for Registry {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            std::env::set_var("LIMO_CAD_SESSION_DIR", previous);
        } else {
            std::env::remove_var("LIMO_CAD_SESSION_DIR");
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn one_broker_routes_two_documents_without_loading_or_switching_its_model() {
    let _environment = session::env_lock();
    let registry = Registry::new();
    let mut server = CadServer::new().unwrap();
    let before = server.manager.export_project_model().unwrap();
    let mut tickets = Vec::new();
    for route in [&registry.first, &registry.second] {
        let submitted = server
            .call_tool(
                "cad_route",
                json!({"action":"submit","route":route,
            "name":"sketch_begin","arguments":{"plane":{"type":"origin_plane","plane":"xy"}},
            "base_generation":1}),
            )
            .unwrap();
        let seq = submitted["ticket"]["operation"]["seq"].as_u64().unwrap();
        let op = session::read_inbox_op(&route.session_id, seq).unwrap();
        assert_eq!(op.session_id.as_deref(), Some(route.session_id.as_str()));
        assert_eq!(op.document_id.as_deref(), Some(route.document_id.as_str()));
        assert_eq!(op.window_id.as_deref(), Some("main"));
        tickets.push(submitted["ticket"].clone());
        assert_eq!(
            session::read_session_file(&route.session_id, "model.json").unwrap(),
            "{}\n"
        );
    }
    for ticket in tickets {
        let observed = server
            .call_tool("cad_route", json!({"action":"status","ticket":ticket}))
            .unwrap();
        assert_ne!(observed["status"], "applied");
    }
    assert!(server.attached_document_id.is_none());
    assert_eq!(server.manager.export_project_model().unwrap(), before);
    assert!(server.tool_trace.is_empty());
}

#[test]
fn process_selector_disambiguates_equal_window_names_and_stale_routes_publish_nothing() {
    let _environment = session::env_lock();
    let registry = Registry::new();
    let selected = resolve(Selectors {
        session_id: None,
        window_id: Some("main".into()),
        document_id: None,
        process_instance_id: Some(registry.second.process_instance_id.clone()),
    })
    .unwrap();
    assert_eq!(selected.session_id, registry.second.session_id);
    let request = json!({"action":"submit","route":{"window_id":"main"},
        "name":"sketch_begin","base_generation":1,"arguments":{}});
    assert!(call(request).unwrap_err().contains("ambiguous"));
    assert!(call(
        json!({"action":"submit","route":registry.first,"name":"sketch_begin",
        "base_generation":0,"arguments":{}})
    )
    .unwrap_err()
    .contains("generation_conflict"));
    session::write_closed_tombstone(&registry.first.session_id).unwrap();
    assert!(call(
        json!({"action":"submit","route":registry.first,"name":"sketch_begin",
        "base_generation":1,"arguments":{}})
    )
    .is_err());
    assert!(session::pending_inbox_seqs(&registry.first.session_id)
        .unwrap()
        .is_empty());
    assert!(session::pending_inbox_seqs(&registry.second.session_id)
        .unwrap()
        .is_empty());
}

#[test]
fn routed_queries_and_ui_actions_capture_owner_and_retain_receipts_after_close() {
    let _environment = session::env_lock();
    let registry = Registry::new();
    let query =
        call(json!({"action":"submit","route":registry.first,"name":"cad_document"})).unwrap();
    let control = &query["ticket"]["operation"];
    let id = control["request_id"].as_str().unwrap();
    let request: Value = serde_json::from_str(
        &session::read_session_file(
            &registry.first.session_id,
            &format!("controls/{id}.request.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(request["owner"]["document_id"], registry.first.document_id);
    assert_eq!(request["sketch_query"]["method"], "document");
    let inspect = call(
        json!({"action":"submit","route":registry.second,"name":"cad_interface",
        "arguments":{"action":"inspect"}}),
    )
    .unwrap();
    assert_eq!(inspect["status"], "submitted");
    assert!(call(
        json!({"action":"submit","route":registry.first,"name":"cad_interface",
        "arguments":{"action":"file","command":"rename","name":"changed"}})
    )
    .is_err());
    session::write_session(
        &registry.first.session_id,
        &format!("controls/{id}.result.json"),
        &json!({"status":"applied","value":{"name":"Captured first design"}}).to_string(),
    )
    .unwrap();
    session::write_closed_tombstone(&registry.first.session_id).unwrap();
    for _ in 0..2 {
        let receipt = call(json!({"action":"status","ticket":query["ticket"]})).unwrap();
        assert_eq!(receipt["status"], "applied");
        assert_eq!(receipt["value"]["name"], "Captured first design");
        assert_eq!(
            receipt["ticket"]["route"]["document_id"],
            registry.first.document_id
        );
    }
}

#[test]
fn routed_writes_are_ordered_and_later_conflicts_do_not_reach_the_engine() {
    let _environment = session::env_lock();
    let registry = Registry::new();
    let mut tickets = Vec::new();
    for name in ["First", "Second"] {
        let submitted = call(json!({"action":"submit","route":registry.first,
            "name":"upsert_named_view","base_generation":1,"arguments":{
                "name":name,"camera":{"position":[0,-100,100],"target":[0,0,0],"up":[0,0,1]},
                "visible_body_ids":[],"part_offsets":[]}}))
        .unwrap();
        tickets.push(submitted["ticket"].clone());
    }
    let first_seq = tickets[0]["operation"]["seq"].as_u64().unwrap();
    let second_seq = tickets[1]["operation"]["seq"].as_u64().unwrap();
    assert!(second_seq > first_seq);
    let mut engine = CadServer::new().unwrap();
    let applied = session::apply_inbox_op(&registry.first.session_id, |name, arguments| {
        engine.call_tool(name, arguments)
    })
    .unwrap();
    assert_eq!(applied.seq, first_seq);
    let mut heartbeat: Value = serde_json::from_str(
        &session::read_session_file(&registry.first.session_id, "heartbeat.json").unwrap(),
    )
    .unwrap();
    heartbeat["generation"] = json!(2);
    session::write_session(
        &registry.first.session_id,
        "heartbeat.json",
        &heartbeat.to_string(),
    )
    .unwrap();
    let snapshot = engine.manager.export_project_model().unwrap();
    session::publish_applied_snapshot(&registry.first.session_id, &snapshot).unwrap();
    session::apply_inbox_op(&registry.first.session_id, |_, _| {
        panic!("stale write reached engine")
    })
    .unwrap_err();
    let rejected = call(json!({"action":"status","ticket":tickets[1]})).unwrap();
    assert_eq!(rejected["status"], "failed");
    assert!(
        rejected["error"]
            .as_str()
            .unwrap()
            .contains("generation_conflict"),
        "{rejected}"
    );
    assert_eq!(engine.manager.named_views().views.len(), 1);
    assert_eq!(engine.manager.named_views().views[0].name, "First");
}

#[test]
fn broker_control_fences_reject_every_changed_owner_and_generation() {
    let owner = json!({"session_id":"session","window_id":"window","document_id":"document",
        "process_instance_id":"process","base_generation":10});
    let request = json!({"owner":owner});
    assert!(session::control_owner_error(
        &request,
        "session",
        "window",
        Some("document"),
        "process",
        10
    )
    .is_none());
    for field in [
        "session_id",
        "window_id",
        "document_id",
        "process_instance_id",
    ] {
        let mut stale = request.clone();
        stale["owner"][field] = json!("replacement");
        assert!(session::control_owner_error(
            &stale,
            "session",
            "window",
            Some("document"),
            "process",
            10
        )
        .is_some());
    }
    assert!(session::control_owner_error(
        &request,
        "session",
        "window",
        Some("document"),
        "process",
        11
    )
    .is_some());
    assert!(session::control_owner_error(
        &json!({}),
        "session",
        "window",
        Some("document"),
        "process",
        10
    )
    .is_none());
}
