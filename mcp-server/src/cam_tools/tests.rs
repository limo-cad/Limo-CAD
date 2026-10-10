use crate::{interface, CadServer};
use serde_json::{json, Value};

/// A client discovers and validates the published contract before submitting
/// grouped operations. Direct dispatch alone cannot detect a stale schema.
fn contract(server: &mut CadServer, operation: &str) -> Value {
    let catalog = server
        .call_tool("cad_interface", json!({"action":"catalog"}))
        .unwrap();
    catalog["operations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == operation)
        .unwrap()["inputSchema"]
        .clone()
}

fn checked_call(
    server: &mut CadServer,
    operation: &str,
    arguments: Value,
) -> Result<Value, String> {
    let schema = contract(server, operation);
    let validator = jsonschema::validator_for(&schema).expect("advertised schema must compile");
    let errors: Vec<_> = validator
        .iter_errors(&arguments)
        .map(|error| error.to_string())
        .collect();
    assert!(
        errors.is_empty(),
        "{operation} contract rejected {arguments}: {errors:?}"
    );
    server.call_tool(
        "cad_interface",
        json!({"action":"execute", "group":interface::group_for(operation).unwrap(),
        "operation":operation, "arguments":arguments}),
    )
}

fn fixture(name: &str) -> Value {
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../../crates/cam/fixtures/lead-clearance.json"
    ))
    .unwrap();
    fixtures["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == name)
        .unwrap()["document"]
        .clone()
}

/// Only persistence assertions omit transport metadata. Validation and write
/// requests below receive the entire read result, including _disclosure.
fn document_fields(mut value: Value) -> Value {
    value.as_object_mut().unwrap().remove("_disclosure");
    value
}

fn drill_job() -> Value {
    let mut cam = fixture("chip-breaking-0.1-retract");
    cam["setups"][0]["name"] = json!("Contract setup");
    cam["setups"][0]["stock"] = json!({"min":{"x":-10,"y":-10,"z":-8},"max":{"x":10,"y":10,"z":0}});
    cam["setups"][0]["operations"][0]["points"] = json!([{"x":0,"y":0}]);
    cam["tools"][0]["name"] = json!("Contract drill");
    cam
}

fn two_drill_job() -> Value {
    let mut cam = drill_job();
    let mut second = cam["setups"][0]["operations"][0].clone();
    second["id"] = json!(2);
    second["points"] = json!([{"x":6,"y":0}]);
    cam["setups"][0]["operations"]
        .as_array_mut()
        .unwrap()
        .push(second);
    cam["next_operation_id"] = json!(3);
    cam
}

#[test]
fn advertised_document_contract_round_trips_empty_and_populated_documents() {
    let mut server = CadServer::new().unwrap();
    let mut empty = checked_call(&mut server, "cam_get_document", json!({})).unwrap();
    assert!(empty.get("post_defaults").is_some());
    let schema = contract(&mut server, "cam_set_document");
    assert!(
        jsonschema::is_valid(&schema, &empty),
        "empty read: {empty}; errors: {:?}",
        jsonschema::validator_for(&schema)
            .unwrap()
            .iter_errors(&empty)
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
    );
    empty["units"] = json!("inches");
    checked_call(&mut server, "cam_set_document", empty.clone()).unwrap();
    assert_eq!(
        document_fields(checked_call(&mut server, "cam_get_document", json!({})).unwrap()),
        document_fields(empty)
    );

    let mut cam = drill_job();

    let mut face = fixture("face-inner-bounds-low-feed")["setups"][0]["operations"][0].clone();
    face["id"] = json!(2);
    face["enabled"] = json!(false);
    face["tool_id"] = json!(1);
    cam["setups"][0]["operations"]
        .as_array_mut()
        .unwrap()
        .push(face);
    cam["next_operation_id"] = json!(3);
    let plane = |reference: &str, offset: f64| json!({"reference":reference,"offset":offset});
    cam["height_expressions"] = json!([{"operation_id":2,
        "clearance":plane("retract", 10.0), "retract":plane("top", 2.0), "feed":plane("top", 1.0),
        "top":plane("stock_top", 0.0), "bottom":plane("stock_top", -1.0)}]);
    cam["linking"] = json!([{"operation_id":2, "same_as_lead_in":false,
        "lead_in":{"horizontal_radius":1.2}, "lead_out":{"horizontal_radius":0.6}}]);
    cam["post_defaults"]["program_number"] = json!(4123);
    cam["post_defaults"]["sequence_numbers"] = json!(true);
    checked_call(&mut server, "cam_set_document", cam).unwrap();
    checked_call(
        &mut server,
        "cam_regenerate_operation",
        json!({"operation_id":1}),
    )
    .unwrap();
    let mut read = checked_call(&mut server, "cam_get_document", json!({})).unwrap();
    for field in [
        "toolpath_generations",
        "height_expressions",
        "linking",
        "load_warnings",
    ] {
        assert!(
            !read[field].as_array().unwrap().is_empty(),
            "fixture must exercise {field}"
        );
    }
    assert!(
        jsonschema::is_valid(&schema, &read),
        "the complete read result must satisfy the advertised write schema"
    );
    read["units"] = json!("inches");
    checked_call(&mut server, "cam_set_document", read.clone()).unwrap();
    let after = checked_call(&mut server, "cam_get_document", json!({})).unwrap();
    assert_eq!(document_fields(after.clone()), document_fields(read.clone()), "round-trip must retain post settings, fingerprints, height and linking intent, and recomputed diagnostics");
    let statuses = checked_call(&mut server, "cam_toolpath_statuses", json!({})).unwrap();
    assert_eq!(
        statuses[0]["state"], "current",
        "display units must not discard the valid generation stamp"
    );
    let model = server.call_tool("cad_project_model", json!({})).unwrap();
    let model: Value = serde_json::from_str(model.as_str().unwrap()).unwrap();
    assert_eq!(model["cam"], document_fields(after));
    assert!(
        model["cam"].get("_disclosure").is_none(),
        "transport metadata must not be persisted"
    );
    let mut unknown = read;
    unknown["unexpected_top_level_setting"] = json!(true);
    assert!(
        !jsonschema::is_valid(&schema, &unknown),
        "the document contract remains strict"
    );
}

fn stock_mesh() -> Value {
    json!({"positions":[-10,-10,-8, 10,-10,-8, 10,10,-8, -10,10,-8, -10,-10,0, 10,-10,0, 10,10,0, -10,10,0],
        "indices":[0,2,1,0,3,2, 4,5,6,4,6,7, 0,1,5,0,5,4, 1,2,6,1,6,5, 2,3,7,2,7,6, 3,0,4,3,4,7]})
}

#[test]
fn advertised_cam_simulation_contract_accepts_modeled_stock_target_and_playback_scope() {
    let mut server = CadServer::new().unwrap();
    let mut cam = two_drill_job();
    cam["setups"][0]["stock_spec"] = json!({"mode":"model_body","body_id":42});
    cam["setups"][0]["resolved_stock"] = json!({"shape":"model_body","body_id":42});
    checked_call(&mut server, "cam_set_document", cam).unwrap();
    let mut args = json!({"setup_id":1,"voxel_size":0.5,"max_voxels":50000,
        "stock_mesh":stock_mesh(), "target":{"cache_key":"mcp-cam-contract","meshes":[stock_mesh()],"tolerance_mm":0.0},
        "through_operation_id":1,"completed_steps":0,"playback_time_seconds":0.0});
    let uncut = checked_call(&mut server, "cam_simulate_setup", args.clone()).unwrap();
    assert_eq!(uncut["through_operation_id"], 1);
    assert_eq!(uncut["removed_voxels"], 0);
    assert!(uncut["initial_voxels"].as_u64().unwrap() > 0);
    assert!(
        uncut["comparison"].is_null(),
        "partial-time frames must not claim a final comparison verdict"
    );
    args["playback_time_seconds"] = Value::Null;
    let measured = checked_call(&mut server, "cam_simulate_setup", args.clone()).unwrap();
    assert!(measured["comparison"].is_object());
    args["target"] = json!({"cache_key":"mcp-cam-contract","tolerance_mm":0.0});
    let cached = checked_call(&mut server, "cam_simulate_setup", args.clone()).unwrap();
    assert_eq!(cached["comparison"], measured["comparison"]);

    args["completed_steps"] = Value::Null;
    args["playback_time_seconds"] = Value::Null;
    let cut = checked_call(&mut server, "cam_simulate_setup", args.clone()).unwrap();
    assert!(cut["removed_voxels"].as_u64().unwrap() > 0);
    let mut whole_args = args.clone();
    whole_args["through_operation_id"] = Value::Null;
    let whole = checked_call(&mut server, "cam_simulate_setup", whole_args).unwrap();
    assert!(
        whole["removed_voxels"].as_u64().unwrap() > cut["removed_voxels"].as_u64().unwrap(),
        "the advertised operation scope must reach the engine, not be silently ignored"
    );
    let schema = contract(&mut server, "cam_simulate_setup");
    for (field, value) in [
        ("completed_steps", json!(-1)),
        ("completed_steps", json!(0.5)),
        ("max_voxels", json!(0)),
        ("playback_time_seconds", json!(-0.1)),
        ("unknown", json!(1)),
    ] {
        let mut invalid = args.clone();
        invalid[field] = value;
        assert!(
            !jsonschema::is_valid(&schema, &invalid),
            "invalid {field} must not be accepted"
        );
    }
    args["target"]["unknown"] = json!(true);
    assert!(
        !jsonschema::is_valid(&schema, &args),
        "nested target contracts must remain strict"
    );
}

#[test]
fn nc_simulation_and_post_events_use_the_advertised_grouped_contract() {
    let mut server = CadServer::new().unwrap();
    checked_call(&mut server, "cam_set_document", two_drill_job()).unwrap();
    let mut nc = json!({"setup_id":1,"source":"G21 G90 G17 G94\nT1 M6\nS5000 M3\nG0 X0 Y0 Z15\nG1 Z-5 F100\nG0 Z15\nM5\nM30",
        "file_name":"contract.nc","dialect":"iso","voxel_size":0.5,"max_voxels":50000,
        "stock_mesh":stock_mesh(),"target":{"meshes":[stock_mesh()],"tolerance_mm":0.1},"completed_steps":0});
    let initial = checked_call(&mut server, "cam_simulate_gcode", nc.clone()).unwrap();
    assert_eq!(initial["removed_voxels"], 0);
    assert!(initial["comparison"].is_object());
    nc["completed_steps"] = Value::Null;
    let cut = checked_call(&mut server, "cam_simulate_gcode", nc.clone()).unwrap();
    assert!(cut["removed_voxels"].as_u64().unwrap() > 0);
    nc["source"] = json!("G999 X0 Y0 Z0");
    assert!(
        checked_call(&mut server, "cam_simulate_gcode", nc).is_err(),
        "exposing NC simulation must not bypass unsupported-code rejection"
    );
    let ungenerated =
        checked_call(&mut server, "cam_post_events", json!({"setup_id":1})).unwrap_err();
    assert!(ungenerated.contains("regenerat"), "{ungenerated}");
    checked_call(&mut server, "cam_regenerate_setup", json!({"setup_id":1})).unwrap();
    let events = checked_call(&mut server, "cam_post_events", json!({"setup_id":1})).unwrap();
    assert_eq!(events["format"], "limo-cad-post-events");
    assert!(
        events["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| {
                warning
                    .as_str()
                    .is_some_and(|message| message.contains("part-gouge clearance is UNVERIFIED"))
            }),
        "the grouped export must disclose that the fixture has no target bodies"
    );
    assert!(events["events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| event["callback"] == "onToolChange"));

    let whole = checked_call(&mut server, "cam_plan_setup", json!({"setup_id":1})).unwrap();
    let nullable = checked_call(
        &mut server,
        "cam_plan_setup",
        json!({"setup_id":1,"through_operation_id":null}),
    )
    .unwrap();
    let scoped = checked_call(
        &mut server,
        "cam_plan_setup",
        json!({"setup_id":1,"through_operation_id":1}),
    )
    .unwrap();
    assert_eq!(nullable, whole);
    let legacy: Value =
        serde_json::from_str(&crate::host::handle(&mut server.manager, "cam_plan", "1")).unwrap();
    assert_eq!(legacy["ok"], true);
    assert_eq!(legacy["value"], document_fields(whole.clone()));
    assert_eq!(whole["per_operation"].as_array().unwrap().len(), 2);
    assert_eq!(scoped["per_operation"].as_array().unwrap().len(), 1);
}
