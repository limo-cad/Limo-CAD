use super::*;

fn authored_job() -> String {
    let cam = json!({
        "setups":[{"id":1,"name":"Face test","stock":{"min":{"x":0.,"y":0.,"z":-4.},"max":{"x":12.,"y":8.,"z":0.}},
            "operations":[{"kind":"face","id":1,"name":"Face top","enabled":true,"tool_id":1,
                "bounds":{"min":{"x":0.,"y":0.},"max":{"x":12.,"y":8.}},
                "top_z":0.,"target_z":-1.,"step_over":1.,"step_down":1.,
                "clearance_z":5.,"retract_z":2.,"feed_height_z":1.,
                "cutting":{"spindle_rpm":8000,"feed_xy":600.,"feed_z":150.,"coolant":"off"}}]}],
        "active_setup_id":1,"tools":[{"id":1,"number":1,"name":"2 mm end mill","kind":"flat_end_mill",
            "diameter":2.,"flute_length":8.,"overall_length":30.,"flute_count":2}],
        "next_setup_id":2,"next_tool_id":2,"next_operation_id":2
    });
    json!({"version":1,"name":"Authored CAM job","starting_state":"empty","steps":[
        {"call":{"group":"solid/primitives","operation":"solid_box","arguments":{"size":[12,8,4]}}},
        {"call":{"group":"cam/setup","operation":"cam_set_document","arguments":cam}},
        {"call":{"group":"cam/toolpaths","operation":"cam_regenerate_setup","arguments":{"setup_id":1}}}
    ]}).to_string()
}

fn generated_job() -> CadServer {
    let mut server = CadServer::new().unwrap();
    server
        .call_tool(
            "cad_interface",
            json!({"action":"script","source":authored_job(),"mode":"fast"}),
        )
        .unwrap();
    assert_eq!(
        server.call_tool("solid_scene", json!({})).unwrap()["bodies"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        server
            .call_tool("cam_toolpath_statuses", json!({}))
            .unwrap()[0]["state"],
        "current"
    );
    server
}

#[test]
fn successful_cam_plan_keeps_authored_source_and_trace_current() {
    let mut server = generated_job();
    let model = server.manager.export_project_model().unwrap();
    let trace = server.tool_trace.clone();
    let mutations = server.modeling_mutations;
    let source = server.last_script_source.clone();
    for grouped in [false, true] {
        let result = if grouped {
            server.call_tool("cad_interface", json!({"action":"execute","group":"cam/toolpaths","operation":"cam_plan_setup","arguments":{"setup_id":1}}))
        } else {
            server.call_tool("cam_plan_setup", json!({"setup_id":1}))
        }.unwrap();
        assert!(
            !result["commands"].as_array().unwrap().is_empty(),
            "A real CAM query must succeed"
        );
        assert_eq!(server.manager.export_project_model().unwrap(), model);
    }
    println!(
        "cam_read_provenance {}",
        json!({"mutations_before":mutations,"mutations_after":server.modeling_mutations,"trace_before":trace.len(),"trace_after":server.tool_trace.len()})
    );
    assert_eq!(
        server.modeling_mutations, mutations,
        "Successful CAM reads must not stale authored source"
    );
    assert_eq!(
        server.tool_trace, trace,
        "Reads must not become replay edits"
    );
    assert_eq!(server.last_script_source, source);
    let exported = server.export_script(&json!({"from":"auto"})).unwrap();
    assert_eq!(exported["fidelity"], "lossless_authored");
    assert_eq!(exported["stale"], false);
    assert!(server
        .call_tool("cam_plan_setup", json!({"setup_id":999}))
        .is_err());
    assert_eq!(server.modeling_mutations, mutations);
    assert_eq!(server.tool_trace, trace);
    server
        .call_tool(
            "cad_set_document_name",
            json!({"name":"Edited after reads"}),
        )
        .unwrap();
    assert!(server.modeling_mutations > mutations);
    assert_eq!(
        server.export_script(&json!({"from":"auto"})).unwrap()["fidelity"],
        "lossy_session_trace"
    );
}

#[test]
fn owning_cam_reads_stay_inbox_routed_but_catalog_and_script_trace_are_read_only() {
    let catalog = full_tool_catalog();
    for name in [
        "cam_plan_setup",
        "cam_post_setup",
        "cam_simulate_setup",
        "cam_simulate_gcode",
        "cam_post_events",
    ] {
        assert!(
            is_modeling_mutate(name),
            "Retain the owning-engine inbox route"
        );
        assert!(limo_cad_mcp_mutate::lookup_mutate(name)
            .unwrap()
            .is_read_only());
        let entry = catalog
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["name"] == name)
            .unwrap();
        assert_eq!(entry["mutates"], false, "{name}");
        assert!(!records_in_script(name), "{name}");
    }
    for name in [
        "cam_set_document",
        "cam_regenerate_setup",
        "cam_regenerate_operation",
    ] {
        let entry = catalog
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["name"] == name)
            .unwrap();
        assert_eq!(entry["mutates"], true, "{name}");
        assert!(records_in_script(name));
    }
}

struct SessionRoot {
    path: std::path::PathBuf,
    previous: Option<std::ffi::OsString>,
}
impl Drop for SessionRoot {
    fn drop(&mut self) {
        if let Some(value) = &self.previous {
            std::env::set_var("LIMO_CAD_SESSION_DIR", value);
        } else {
            std::env::remove_var("LIMO_CAD_SESSION_DIR");
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn attached_cam_plan_uses_owner_without_refreshing_or_dirtying_authored_source() {
    let _guard = session::env_lock();
    let id = session::test_session_uuid();
    let root = SessionRoot {
        path: std::env::temp_dir().join(format!("limo-cad-cam-read-provenance-{id}")),
        previous: std::env::var_os("LIMO_CAD_SESSION_DIR"),
    };
    std::env::set_var("LIMO_CAD_SESSION_DIR", &root.path);
    let donor = generated_job();
    let model = donor.manager.export_project_model().unwrap();
    let authored = donor.last_script_source.clone();
    session::write_session(&id, "model.json", &model).unwrap();
    session::write_session(&id, "heartbeat.json", &json!({"session_id":id,"updated_ms":session::now_ms(),"generation":7,"published_generation":7,"model_generation":7,"interface_version":1}).to_string()).unwrap();
    let peer = id.clone();
    let owner_model = model.clone();
    let owner = std::thread::spawn(move || {
        let mut engine = CadServer::new().unwrap();
        engine
            .call_tool("cad_load_project_model", json!({"model_json":owner_model}))
            .unwrap();
        let before = engine.manager.export_project_model().unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut calls = 0;
        while calls < 4 {
            if let Some(seq) = session::pending_inbox_seqs(&peer).unwrap().first().copied() {
                session::apply_inbox_op(&peer, |name, arguments| {
                    assert_eq!(name, "cam_plan_setup");
                    let value = engine.call_tool(name, arguments)?;
                    session::write_session(
                        &peer,
                        &format!("inbox/results/{seq}.json"),
                        &value.to_string(),
                    )?;
                    Ok(value)
                })
                .unwrap();
                calls += 1;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Owning inbox query timed out"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(engine.manager.export_project_model().unwrap(), before);
        calls
    });
    let mut server = CadServer::new().unwrap();
    server
        .call_tool("cad_attach", json!({"session_id":id}))
        .unwrap();
    server.last_script_source = authored.clone();
    server.last_script_mutations = server.modeling_mutations;
    let mutations = server.modeling_mutations;
    let trace = server.tool_trace.clone();
    let mut observed = vec![];
    for running in [false, true] {
        server.script_running = running;
        for grouped in [false, true] {
            let result = if grouped {
                server.call_tool("cad_interface", json!({"action":"execute","group":"cam/toolpaths","operation":"cam_plan_setup","arguments":{"setup_id":1}}))
            } else { server.call_tool("cam_plan_setup", json!({"setup_id":1})) }.unwrap();
            assert!(!result["commands"].as_array().unwrap().is_empty());
            observed.push((
                server.last_script_source.clone(),
                server.tool_trace.clone(),
                server.live_snapshot_dirty,
                server.modeling_mutations,
            ));
        }
    }
    server.script_running = false;
    assert_eq!(
        owner.join().unwrap(),
        4,
        "All reads must execute in the owner inbox"
    );
    for (source, recorded, dirty, count) in observed {
        assert_eq!(
            source, authored,
            "An immutable live read must not refresh away authored source"
        );
        assert_eq!(recorded, trace);
        assert!(
            !dirty,
            "Existing publication is not evidence of a read mutation"
        );
        assert_eq!(count, mutations);
    }
    assert_eq!(
        serde_json::from_str::<Value>(&server.manager.export_project_model().unwrap()).unwrap(),
        serde_json::from_str::<Value>(&model).unwrap(),
        "Attachment may reorder JSON object keys but must preserve every model value"
    );
    assert_eq!(server.attached_generation, Some(7));
}
