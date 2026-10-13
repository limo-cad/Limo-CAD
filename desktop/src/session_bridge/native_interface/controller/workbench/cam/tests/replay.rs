use super::*;

fn result(value: String) -> Value {
    parse_engine_envelope(value).unwrap()
}
fn call(engine: &AppState, operation: &str, payload: Value) -> Value {
    result(engine.engine_call(operation, &payload.to_string()))
}
fn differences(a: &Value, b: &Value, path: &str, out: &mut Vec<String>) {
    if a == b || out.len() >= 20 {
        return;
    }
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            for key in a.keys().chain(b.keys()) {
                differences(
                    a.get(key).unwrap_or(&Value::Null),
                    b.get(key).unwrap_or(&Value::Null),
                    &format!("{path}/{key}"),
                    out,
                );
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (index, (a, b)) in a.iter().zip(b).enumerate() {
                differences(a, b, &format!("{path}/{index}"), out);
            }
        }
        _ => out.push(format!("{path}: {a} -> {b}")),
    }
}

#[test]
fn native_cam_cylindrical_model_generation_survives_project_replay() {
    let source = AppState::new();
    call(
        &source,
        "begin_sketch",
        json!({"type":"origin_plane","plane":"xy"}),
    );
    call(
        &source,
        "add_rectangle",
        json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":40.,"y":30.},"ctrl_held":true}),
    );
    result(source.engine_call("end_sketch", ""));
    result(source.solid_extrude(&json!({"sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":8.},"taper_angle_deg":0.,"flip":false,"target_body_ids":[]}).to_string()));
    let body = source.viewport_snapshot().2.bodies[0].id;
    call(
        &source,
        "begin_sketch",
        json!({"type":"origin_plane","plane":"xy"}),
    );
    call(
        &source,
        "add_circle",
        json!({"mode":"center_diameter","p1":{"x":20.,"y":15.},"p2":{"x":26.,"y":15.},"ctrl_held":true}),
    );
    result(source.engine_call("end_sketch", ""));
    result(source.solid_extrude(&json!({"sketch_name":"Sketch2","profile_indices":[0],"operation":"cut","extent":{"type":"distance","distance":8.},"taper_angle_deg":0.,"flip":false,"target_body_ids":[body]}).to_string()));
    let initial = result(source.engine_call("project_export_model", ""));
    let loaded = AppState::new();
    result(loaded.project_load(&initial.to_string()));
    let mut cam = job();
    cam.setups[0].body_ids = vec![body];
    cam.setups[0].wcs.origin = limo_cad_cam::Point3Dto::new(-2., -2., 9.);
    cam.setups[0].stock = serde_json::from_value(
        json!({"min":{"x":0.,"y":0.,"z":-11.},"max":{"x":44.,"y":34.,"z":0.}}),
    )
    .unwrap();
    cam.tools[0].diameter = 4.;
    let scene = source.viewport_snapshot().2;
    let edge = scene.bodies[0]
        .edges
        .iter()
        .find(|edge| edge.circle.is_none() && edge.points.iter().all(|p| (p.z - 8.).abs() < 1e-6))
        .unwrap();
    let chain = limo_cad_sketch::resolve_edge_chain(
        &scene,
        &[],
        &limo_cad_sketch::EdgeChainRequest {
            source: limo_cad_sketch::ChainSource::Model,
            body_ids: vec![body],
            normal: Some([0., 0., 1.]),
            keys: vec![format!("edge:{}:{}", body.0, edge.key)],
            mode: limo_cad_sketch::ChainMode::Closed,
            reversed: false,
        },
    )
    .unwrap();
    cam.setups[0].operations[0] = serde_json::from_value(json!({
        "kind":"contour2d","id":7,"name":"Native contour2d geometry","enabled":true,"tool_id":5,
        "path":chain.points.iter().map(|p|json!({"x":p[0]+2.,"y":p[1]+2.})).collect::<Vec<_>>(),
        "closed":chain.closed,"top_z":0.,"bottom_z":-3.,"step_down":2.,"compensation":"outside",
        "lead_in":3.,"lead_out":3.,"clearance_z":10.,"retract_z":5.,"feed_height_z":5.,
        "chain_ref":{"source":"model","keys":chain.keys,"reversed":false},
        "cutting":{"spindle_rpm":6000,"feed_xy":600.,"feed_z":150.,"coolant":"off"}
    }))
    .unwrap();
    cam.height_expressions.clear();
    let mut failures = vec![];
    for (name, live) in [("fresh native", &source), ("already replayed", &loaded)] {
        call(
            live,
            "cam_set_document",
            serde_json::to_value(&cam).unwrap(),
        );
        call(live, "cam_regenerate_operation", json!(7));
        let before = result(live.engine_call("cam_toolpath_statuses", ""));
        assert_eq!(before[0]["state"], "current", "{before}");
        let model = result(live.engine_call("project_export_model", ""));
        let replay = AppState::new();
        result(replay.project_load(&model.to_string()));
        let after = result(replay.engine_call("cam_toolpath_statuses", ""));
        let mut changes = vec![];
        differences(
            &serde_json::to_value(live.viewport_snapshot().2).unwrap(),
            &serde_json::to_value(replay.viewport_snapshot().2).unwrap(),
            "scene",
            &mut changes,
        );
        differences(
            &result(live.engine_call("finished_sketches", "")),
            &result(replay.engine_call("finished_sketches", "")),
            "sketches",
            &mut changes,
        );
        if after[0]["state"] != "current" {
            failures.push(format!("{name}: {after}\nReplay differences: {changes:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
