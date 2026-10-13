use super::*;
use serde_json::{json, Value};

fn value(response: String) -> Value {
    let response: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(response["ok"], true, "{response}");
    response["value"].clone()
}

fn call(host: &NativeEngineHost, method: &str, arguments: Value) -> Value {
    value(host.engine_call(method, &arguments.to_string()))
}

fn repeated_part() -> (NativeEngineHost, u64, u64) {
    let host = NativeEngineHost::new();
    call(
        &host,
        "begin_sketch",
        json!({"name":"Shared footprint","plane":{"type":"origin_plane","plane":"xy"}}),
    );
    let profile = call(
        &host,
        "add_rectangle_locked",
        json!({
            "mode":"two_point","anchor":{"x":0,"y":0},"corner_hint":{"x":20,"y":10},
            "width_mm":20,"height_mm":10,"ctrl_held":true
        }),
    );
    let origin = profile["sketch"]["entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entity| entity["kind"] == "point" && entity["position"] == json!({"x":0.0,"y":0.0}))
        .unwrap();
    call(
        &host,
        "add_constraint",
        json!({"type":"fix","entity":origin["id"]}),
    );
    value(host.engine_call("end_sketch", ""));
    value(host.solid_extrude(&json!({
        "sketch_name":"Shared footprint","profile_indices":[0],"operation":"new_body",
        "extent":{"type":"distance","distance":3},"taper_angle_deg":0,"flip":false,"target_body_ids":[]
    }).to_string()));
    let body = host.viewport_frame().document.scene.bodies[0].id.0;
    let component = call(
        &host,
        "assembly_create_component",
        json!({
            "name":"Shared part","body_ids":[body],"absorb_promoted_bodies":true,
            "local_coordinate_system":{"translation":[2,3,1],"rotation":[0,0,0,1]}
        }),
    );
    let half = std::f64::consts::FRAC_1_SQRT_2;
    let occurrence = call(
        &host,
        "assembly_create_occurrence",
        json!({
            "name":"Rotated repeat","component_id":component["id"],
            "local_pose":{"translation":[50,25,10],"rotation":[0,0,half,half]}
        }),
    );
    (host, body, occurrence["id"].as_u64().unwrap())
}

#[test]
fn in_place_edit_uses_occurrence_frame_and_keeps_shared_sources_local_after_reopen() {
    let (host, body_id, occurrence_id) = repeated_part();
    let before = host.viewport_frame();
    let poses = before.instance_body_poses.clone();
    assert_eq!(
        poses
            .iter()
            .filter(|pose| pose.body_id.0 == body_id)
            .count(),
        2
    );
    let occurrence_pose = poses
        .iter()
        .find(|pose| pose.occurrence_id.0 == occurrence_id)
        .unwrap();
    let placement = limo_cad_sketch::AssemblyTransformDto {
        translation: occurrence_pose.translation,
        rotation: occurrence_pose.rotation,
    };
    let edit = call(
        &host,
        "edit_sketch",
        json!({"name":"Shared footprint","occurrence_id":occurrence_id}),
    );
    let sketch: SketchDto = serde_json::from_value(edit).unwrap();
    assert_eq!(sketch.edit_occurrence_id.unwrap().0, occurrence_id);
    let local_basis = sketch.plane.origin_basis().unwrap();
    let expected = placement.transform_point(local_basis.to_3d([7., 4.]));
    let displayed = sketch.basis.to_3d([7., 4.]);
    for axis in 0..3 {
        assert!((displayed[axis] - expected[axis]).abs() < 1e-9);
    }
    for (actual, expected) in sketch.basis.to_2d(displayed).into_iter().zip([7., 4.]) {
        assert!((actual - expected).abs() < 1e-9);
    }
    assert_ne!(sketch.basis, local_basis);
    assert_eq!(
        host.viewport_frame()
            .document
            .active_sketch
            .as_ref()
            .unwrap(),
        &sketch
    );
    let dimension = sketch
        .dimensions
        .iter()
        .find(|dimension| (dimension.value - 20.).abs() < 1e-9)
        .unwrap();
    let changed = call(
        &host,
        "edit_dimension",
        json!({"constraint_id":dimension.constraint_id,"text":"30"}),
    );
    let changed: SketchDto = serde_json::from_value(changed["sketch"].clone()).unwrap();
    assert_eq!(changed.edit_occurrence_id, sketch.edit_occurrence_id);
    assert_eq!(changed.basis, sketch.basis);
    assert!(changed
        .dimensions
        .iter()
        .any(|dimension| (dimension.value - 30.).abs() < 1e-9));
    let assembly = value(host.engine_call("assembly_document", ""));
    let rejected: Value = serde_json::from_str(&host.engine_call("assembly_set_occurrence_pose",&json!({
        "occurrence_id":occurrence_id,"local_pose":{"translation":[0,0,0],"rotation":[0,0,0,1]}
    }).to_string())).unwrap();
    assert_eq!(rejected["ok"], false);
    assert!(rejected["error"]
        .as_str()
        .unwrap()
        .contains("Finish the in-place"));
    assert_eq!(value(host.engine_call("assembly_document", "")), assembly);
    value(host.engine_call("end_sketch", ""));
    value(host.solid_recompute());
    let updated = host.viewport_frame();
    assert!(updated.document.scene.errors.is_empty());
    assert_eq!(updated.instance_body_poses, poses);
    assert!(updated.document.active_sketch.is_none());
    let max_x = updated.document.scene.bodies[0]
        .mesh
        .positions
        .as_chunks::<3>()
        .0
        .iter()
        .map(|point| point[0])
        .fold(f32::NEG_INFINITY, f32::max);
    assert!((max_x - 30.).abs() < 1e-5);
    let saved = value(host.engine_call("project_export_model", ""));
    let reopened = NativeEngineHost::new();
    value(reopened.project_load(&serde_json::to_string(&saved.as_str().unwrap()).unwrap()));
    let local = call(&reopened, "edit_sketch", json!("Shared footprint"));
    let local: SketchDto = serde_json::from_value(local).unwrap();
    assert_eq!(local.basis, local_basis);
    assert!(local.edit_occurrence_id.is_none());
    assert!(local
        .dimensions
        .iter()
        .any(|dimension| (dimension.value - 30.).abs() < 1e-9));
    value(reopened.engine_call("end_sketch", ""));
    let reentered = call(
        &reopened,
        "edit_sketch",
        json!({"name":"Shared footprint","occurrence_id":occurrence_id}),
    );
    let reentered: SketchDto = serde_json::from_value(reentered).unwrap();
    assert_eq!(reentered.basis, sketch.basis);
}

#[test]
fn unrelated_or_missing_occurrence_rejects_before_entering_the_sketch() {
    let (host, _, occurrence_id) = repeated_part();
    call(
        &host,
        "begin_sketch",
        json!({"name":"Independent","plane":{"type":"origin_plane","plane":"xy"}}),
    );
    value(host.engine_call("end_sketch", ""));
    let before = value(host.engine_call("project_export_model", ""));
    for (name, id) in [
        ("Independent", occurrence_id),
        ("Shared footprint", u64::MAX),
    ] {
        let response: Value = serde_json::from_str(&host.engine_call(
            "edit_sketch",
            &json!({"name":name,"occurrence_id":id}).to_string(),
        ))
        .unwrap();
        assert_eq!(response["ok"], false);
        assert_eq!(value(host.engine_call("project_export_model", "")), before);
        assert!(host.viewport_frame().document.active_sketch.is_none());
    }
}
