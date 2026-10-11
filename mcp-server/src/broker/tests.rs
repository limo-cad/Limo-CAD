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
fn receipt_only_status_preserves_fences_without_reading_operation_values() {
    // Synthetic files in this test's private registry qualify the public receipt
    // contract, not native geometry, a live desktop or a connector's byte limit.
    let _environment = session::env_lock();
    let registry = Registry::new();
    let route = registry.first.clone();
    let public_specs = specs();
    let status_schema = &public_specs[0].input_schema["oneOf"][1];
    assert_eq!(
        status_schema["properties"]["include_values"]["type"],
        "boolean"
    );
    assert_eq!(
        status_schema["properties"]["include_values"]["default"],
        true
    );
    let read = |ticket: &Ticket, include_values: Option<bool>| {
        let mut request = json!({"action":"status","ticket":ticket});
        if let Some(include_values) = include_values {
            request["include_values"] = json!(include_values);
        }
        call(request)
    };
    let ticket = Ticket {
        route: route.clone(),
        operation: Pending::Inbox { seq: 41 },
    };
    let expected_ticket = serde_json::to_value(&ticket).unwrap();
    let pending = read(&ticket, Some(false)).unwrap();
    assert_eq!(pending["status"], "pending");
    assert_eq!(pending["applied"], false);
    assert_eq!(pending["ticket"], expected_ticket);
    assert!(pending.get("value").is_none());

    session::write_session(
        &route.session_id,
        "inbox/applied/41.json",
        &json!({"name":"solid_move_copy","base_generation":1}).to_string(),
    )
    .unwrap();
    let result_path = "inbox/results/41.json";
    session::write_session(&route.session_id, result_path, "not-json").unwrap();
    let unpublished = read(&ticket, Some(false)).unwrap();
    assert_eq!(unpublished["status"], "timeout");
    assert_eq!(unpublished["applied"], true);
    assert_eq!(unpublished["published"], false);
    assert!(unpublished["published_generation"].is_null());

    let mut heartbeat: Value = serde_json::from_str(
        &session::read_session_file(&route.session_id, "heartbeat.json").unwrap(),
    )
    .unwrap();
    heartbeat["generation"] = json!(2);
    heartbeat["published_generation"] = json!(2);
    heartbeat["model_generation"] = json!(2);
    let heartbeat = heartbeat.to_string();
    session::write_session(&route.session_id, "heartbeat.json", &heartbeat).unwrap();
    let compact = read(&ticket, Some(false)).unwrap();
    assert_eq!(compact["status"], "applied");
    assert_eq!(compact["applied"], true);
    assert_eq!(compact["published"], true);
    assert_eq!(compact["base_generation"], 1);
    assert_eq!(compact["published_generation"], 2);
    assert_eq!(compact["model_generation"], 2);
    assert_eq!(compact["ticket"], expected_ticket);
    assert!(compact.get("value").is_none());
    // A malformed retained value proves receipt-only status never reads/parses it.
    for full in [None, Some(true)] {
        assert!(read(&ticket, full).is_err());
    }
    assert_eq!(
        session::read_session_file(&route.session_id, result_path).unwrap(),
        "not-json"
    );

    let value = json!({"synthetic_operation_output":"geometry-sized-value".repeat(2048)});
    let encoded = value.to_string();
    assert!(encoded.len() > 32 * 1024);
    session::write_session(&route.session_id, result_path, &encoded).unwrap();
    for full in [None, Some(true)] {
        let observed = read(&ticket, full).unwrap();
        assert_eq!(observed["value"], value);
        assert_eq!(observed["ticket"], expected_ticket);
    }
    assert!(
        serde_json::to_vec(&read(&ticket, Some(false)).unwrap())
            .unwrap()
            .len()
            < 4096
    );
    assert_eq!(
        session::read_session_file(&route.session_id, result_path).unwrap(),
        encoded
    );
    assert_eq!(
        session::read_session_file(&route.session_id, "heartbeat.json").unwrap(),
        heartbeat
    );
    assert_eq!(
        session::read_session_file(&route.session_id, "model.json").unwrap(),
        "{}\n"
    );

    let failed_ticket = Ticket {
        operation: Pending::Inbox { seq: 42 },
        ..ticket.clone()
    };
    session::write_session(
        &route.session_id,
        "inbox/failed/42.json",
        &json!({"name":"solid_move_copy","base_generation":1,"error":"generation_conflict"})
            .to_string(),
    )
    .unwrap();
    let failed = read(&failed_ticket, Some(false)).unwrap();
    assert_eq!(failed["status"], "failed");
    assert_eq!(failed["applied"], false);
    assert_eq!(failed["dead_lettered"], true);
    assert_eq!(failed["error"], "generation_conflict");
    assert_eq!(
        failed["ticket"],
        serde_json::to_value(&failed_ticket).unwrap()
    );

    let expiry = session::now_ms().saturating_add(1000);
    let control = Ticket {
        route: route.clone(),
        operation: Pending::Control {
            request_id: "700-1".into(),
            expires_ms: expiry,
        },
    };
    session::write_session(
        &route.session_id,
        "controls/700-1.result.json",
        &json!({"status":"applied","value":value,"engine_revision":2}).to_string(),
    )
    .unwrap();
    for full in [None, Some(true)] {
        assert_eq!(read(&control, full).unwrap()["value"], value);
    }
    let compact_control = read(&control, Some(false)).unwrap();
    assert_eq!(compact_control["status"], "applied");
    assert_eq!(compact_control["engine_revision"], 2);
    assert_eq!(
        compact_control["ticket"],
        serde_json::to_value(&control).unwrap()
    );
    assert!(compact_control.get("value").is_none());
    session::write_session(&route.session_id, "controls/700-1.result.json",
        &json!({"status":"failed","code":"source_changed","error":"Original source retired","value":value}).to_string()).unwrap();
    let failed_control = read(&control, Some(false)).unwrap();
    assert_eq!(failed_control["status"], "failed");
    assert_eq!(failed_control["code"], "source_changed");
    assert_eq!(failed_control["error"], "Original source retired");
    assert!(failed_control.get("value").is_none());

    let marker = NativeControlAdmission {
        version: 1,
        session_id: route.session_id.clone(),
        window_id: route.window_id.clone(),
        document_id: route.document_id.clone(),
        process_instance_id: route.process_instance_id.clone(),
        request_id: "700-2".into(),
        epoch: 1,
        revision: 1,
        expires_ms: expiry,
        admitted_ms: session::now_ms(),
    };
    session::write_session(
        &route.session_id,
        "controls/700-2.admitted.json",
        &marker.encode().unwrap(),
    )
    .unwrap();
    let wrong_owner = Ticket {
        route: Route {
            document_id: "replacement".into(),
            ..route
        },
        operation: Pending::Control {
            request_id: "700-2".into(),
            expires_ms: expiry,
        },
    };
    assert!(read(&wrong_owner, Some(false))
        .unwrap_err()
        .contains("original control ticket"));
}

#[test]
fn native_control_status_retains_late_receipt_and_original_timeout_ticket() {
    let _environment = session::env_lock();
    let registry = Registry::new();
    let route = registry.first.clone();
    let expiry = session::now_ms().saturating_sub(1);
    let marker = NativeControlAdmission {
        version: 1,
        session_id: route.session_id.clone(),
        window_id: route.window_id.clone(),
        document_id: route.document_id.clone(),
        process_instance_id: route.process_instance_id.clone(),
        request_id: "500-1".into(),
        epoch: 1,
        revision: 1,
        expires_ms: expiry,
        admitted_ms: expiry.saturating_sub(1),
    };
    session::write_session(
        &route.session_id,
        "controls/500-1.admitted.json",
        &marker.encode().unwrap(),
    )
    .unwrap();
    session::write_session(&route.session_id, session::CLOSED_TOMBSTONE, "{}").unwrap();
    let ticket = Ticket {
        route: route.clone(),
        operation: Pending::Control {
            request_id: "500-1".into(),
            expires_ms: expiry,
        },
    };
    let pending = status(ticket.clone()).unwrap();
    assert!(pending["status"] == "pending" && pending["applied"].is_null());
    assert!(pending["execution_deadline_elapsed"] == true);
    std::fs::remove_file(
        registry
            .root
            .join("_ui/processes")
            .join(format!("{}.json", route.process_instance_id)),
    )
    .unwrap();
    let unknown = status(ticket.clone()).unwrap();
    assert!(unknown["status"] == "completion_unknown" && unknown["applied"].is_null());
    let actual = json!({"status":"applied","request_id":"500-1","session_id":route.session_id,"value":{"completed":true}});
    session::write_session(
        &route.session_id,
        "controls/500-1.result.json",
        &actual.to_string(),
    )
    .unwrap();
    for _ in 0..2 {
        let observed = status(ticket.clone()).unwrap();
        assert!(observed["status"] == "applied" && observed["value"]["completed"] == true);
    }
    let control = session::ControlTicket {
        session_id: route.session_id.clone(),
        request_id: "500-1".into(),
        expires_ms: expiry,
        route: Some(serde_json::to_value(&route).unwrap()),
    };
    let retained = retained_control_ticket(control.route.as_ref().unwrap(), &control).unwrap();
    let parsed: Ticket = serde_json::from_value(retained).unwrap();
    assert!(
        parsed.route.session_id == route.session_id
            && parsed.route.document_id == route.document_id
    );
    session::clear_closed_tombstone(&route.session_id).unwrap();
    limo_cad_session_storage::atomic_write(
        &registry
            .root
            .join("_ui/processes")
            .join(format!("{}.json", route.process_instance_id)),
        json!({"process_instance_id":route.process_instance_id,"updated_ms":session::now_ms(),
            "windows":[{"window_id":route.window_id,"active_document_id":route.document_id,
                "active_session_id":route.session_id}]})
        .to_string()
        .as_bytes(),
    )
    .unwrap();
    let owner = json!({"session_id":route.session_id,"window_id":route.window_id,
        "document_id":route.document_id,"process_instance_id":route.process_instance_id,
        "base_generation":1});
    let submitted = session::submit_ui(
        &json!({"session_id":route.session_id,"action":"inspect"}),
        &owner,
    )
    .unwrap();
    let submitted_id = submitted.request_id.clone();
    // Expired wait budget is deterministic; this runs no sleep or desktop input.
    let timeout = session::await_control(submitted, std::time::Instant::now()).unwrap();
    assert!(timeout["status"] == "timeout" && timeout["applied"].is_null());
    let original: Ticket = serde_json::from_value(timeout["ticket"].clone()).unwrap();
    assert!(
        original.route.session_id == route.session_id
            && original.route.document_id == route.document_id
    );
    assert!(
        matches!(original.operation, Pending::Control{request_id,..} if request_id == submitted_id)
    );
    assert!(!registry
        .root
        .join(&route.session_id)
        .join(format!("controls/{submitted_id}.request.json"))
        .exists());
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
