use super::*;
use serde_json::{json, Value};

fn value(reply: String) -> Value {
    let reply: Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(reply["ok"], true, "{reply}");
    reply["value"].clone()
}

#[test]
fn projection_native_profile_archive_and_missing_source_are_consistent() {
    let host = NativeEngineHost::new();
    let plane = r#"{"type":"origin_plane","plane":"xy"}"#;
    value(host.engine_call("begin_sketch", plane));
    value(host.engine_call("set_grid_snap", r#"{"enabled":false}"#));
    let source = value(host.engine_call(
        "add_line",
        r#"{"from":{"x":0,"y":0},"to_raw":{"x":10,"y":0},"ctrl_held":true}"#,
    ))["entity_id"]
        .clone();
    value(host.engine_call("end_sketch", ""));
    value(host.engine_call("begin_sketch", plane));
    let sources = value(host.engine_call("sketch_projection_sources", ""));
    assert_eq!(
        sources["sources"][0]["source"],
        json!({"feature":1,"entity":source})
    );
    let projection = value(host.engine_call(
        "sketch_project",
        &json!({"sources":[{"feature":1,"entity":source}]}).to_string(),
    ));
    assert!(projection["entities"].as_array().unwrap().is_empty());
    assert_eq!(projection["projected_edges"].as_array().unwrap().len(), 1);
    for (from, to) in [([0, 0], [0, 10]), ([0, 10], [10, 10]), ([10, 10], [10, 0])] {
        value(host.engine_call("add_line", &json!({"from":{"x":from[0],"y":from[1]},"to_raw":{"x":to[0],"y":to[1]},"ctrl_held":true}).to_string()));
    }
    value(host.engine_call("end_sketch", ""));
    let profiles = value(host.engine_call("profile_catalog", ""));
    assert_eq!(
        profiles[1]["profiles"].as_array().unwrap().len(),
        1,
        "{profiles}"
    );
    value(host.solid_extrude(r#"{"sketch_name":"Sketch2","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":5},"target_body_ids":[]}"#));
    let before = host.solid_scene_snapshot();
    assert_eq!(before.bodies.len(), 1);
    assert!(before.errors.is_empty());
    let snapshot = host.project_snapshot().unwrap();
    let archive = limo_cad_project_file::ProjectArchive::new(
        snapshot.model_json().to_owned(),
        limo_cad_project_file::SaveMetadata {
            application_version: "0.3.0-rc.3",
            saved_at: "2026-11-09T00:00:00Z",
        },
    )
    .unwrap()
    .encode()
    .unwrap();
    let cold = NativeEngineHost::new();
    value(
        cold.project_load_snapshot(
            &limo_cad_project_file::ProjectArchive::decode(archive)
                .unwrap()
                .snapshot()
                .unwrap(),
        ),
    );
    assert_eq!(cold.solid_scene_snapshot().bodies.len(), 1);
    assert!(cold.solid_scene_snapshot().errors.is_empty());
    value(cold.engine_call("edit_sketch", r#"{"name":"Sketch1"}"#));
    value(cold.engine_call("delete_entity", &json!({"entity_id":source}).to_string()));
    value(cold.engine_call("end_sketch", ""));
    let profiles = value(cold.engine_call("profile_catalog", ""));
    assert!(!profiles
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["sketch_name"] == "Sketch2"));
    value(cold.engine_call("edit_sketch", r#"{"name":"Sketch2"}"#));
    assert!(
        value(cold.engine_call("active_sketch", ""))["projected_edges"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        value(cold.engine_call("sketch_projections", ""))["projections"][0]["error"].is_string()
    );
}
