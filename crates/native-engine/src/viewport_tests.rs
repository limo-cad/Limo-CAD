use super::*;
use serde_json::{json, Value};

fn value(response: String) -> Value {
    let response: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(response["ok"], true, "{response}");
    response["value"].clone()
}

#[test]
fn viewport_reads_share_authored_data_without_advancing_revisions() {
    let host = NativeEngineHost::new();
    let before = host.viewport_frame();
    for (method, payload) in [
        ("document", ""),
        ("project_export_model", ""),
        ("active_sketch", ""),
        ("finished_sketches", ""),
        ("profile_catalog", ""),
        ("body_appearances", ""),
        ("project_visibility", ""),
        ("named_views", ""),
        ("drawing_document", ""),
        ("assembly_document", ""),
        ("assembly_solution", ""),
        ("print_intent_get", "{}"),
        ("print_intent_effective", "{}"),
        ("print_modifier_effective", "{}"),
        ("eval_expression", "{invalid"),
    ] {
        let response: Value = serde_json::from_str(&host.engine_call(method, payload)).unwrap();
        assert!(response["ok"].is_boolean());
        let after = host.viewport_frame();
        assert!(Arc::ptr_eq(&before.document, &after.document), "{method}");
        assert!(
            Arc::ptr_eq(&before.body_poses, &after.body_poses),
            "{method}"
        );
        assert!(
            Arc::ptr_eq(&before.instance_body_poses, &after.instance_body_poses),
            "{method}"
        );
        assert_eq!(
            after.geometry_revision, before.geometry_revision,
            "{method}"
        );
    }
    value(host.engine_call("document_set_name", r#""Named design""#));
    value(host.engine_call("set_grid_step", r#"{"step_mm":2.0}"#));
    assert!(Arc::ptr_eq(
        &before.document,
        &host.viewport_frame().document
    ));
    assert_eq!(host.document_name(), "Named design");
}

#[test]
fn authored_sketch_changes_invalidate_metadata_without_replaying_geometry() {
    let host = NativeEngineHost::new();
    let before = host.viewport_frame();
    value(host.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#));
    let editing = host.viewport_frame();
    assert!(editing.document.active_sketch.is_some());
    assert!(!Arc::ptr_eq(&before.document, &editing.document));
    assert_eq!(editing.geometry_revision, before.geometry_revision);
    assert!(editing.document.metadata_revision > before.document.metadata_revision);
    assert!(Arc::ptr_eq(&before.document.scene, &editing.document.scene));
    value(host.engine_call(
        "add_rectangle",
        r#"{"mode":"two_point","p1":{"x":0.0,"y":0.0},"p2":{"x":20.0,"y":10.0},"ctrl_held":false}"#,
    ));
    let drawn = host.viewport_frame();
    assert!(!Arc::ptr_eq(&editing.document, &drawn.document));
    assert!(editing
        .document
        .active_sketch
        .as_ref()
        .unwrap()
        .entities
        .is_empty());
    assert!(!drawn
        .document
        .active_sketch
        .as_ref()
        .unwrap()
        .entities
        .is_empty());
    value(host.engine_call("set_grid_step", r#"{"step_mm":2.0}"#));
    let grid = host.viewport_frame();
    assert!(!Arc::ptr_eq(&drawn.document, &grid.document));
    value(host.engine_call("end_sketch", ""));
    let finished = host.viewport_frame();
    assert!(finished.document.active_sketch.is_none());
    assert_eq!(finished.document.finished_sketches.len(), 1);
    assert!(!finished.document.profile_catalog.is_empty());
    assert_eq!(finished.geometry_revision, before.geometry_revision);
    assert!(Arc::ptr_eq(
        &finished.document,
        &host.viewport_frame().document
    ));
}

#[test]
fn assembly_placement_changes_refresh_poses_without_copying_authored_data() {
    let host = NativeEngineHost::new();
    value(host.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#));
    value(host.engine_call(
        "add_rectangle",
        r#"{"mode":"two_point","p1":{"x":0,"y":0},"p2":{"x":10,"y":6},"ctrl_held":false}"#,
    ));
    value(host.engine_call("end_sketch", ""));
    let sketch = host.viewport_frame();
    value(
        host.solid_extrude(
            &json!({
                "sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body",
                "extent":{"type":"distance","distance":3},"taper_angle_deg":0,
                "flip":false,"target_body_ids":[]
            })
            .to_string(),
        ),
    );
    let solid = host.viewport_frame();
    assert!(Arc::ptr_eq(
        &solid.instance_body_poses,
        &host.viewport_frame().instance_body_poses
    ));
    assert!(!Arc::ptr_eq(&sketch.document, &solid.document));
    assert!(solid.geometry_revision > sketch.geometry_revision);
    let component = value(
        host.engine_call(
            "assembly_create_component",
            &json!({
                "name":"Repeated part","body_ids":[solid.document.scene.bodies[0].id.0],
                "absorb_promoted_bodies":true
            })
            .to_string(),
        ),
    );
    let assembled = host.viewport_frame();
    assert!(Arc::ptr_eq(&solid.document, &assembled.document));
    let repeat = value(
        host.engine_call(
            "assembly_create_occurrence",
            &json!({
                "component_id":component["id"],"name":"Placed repeat",
                "local_pose":{"translation":[20,0,0],"rotation":[0,0,0,1]}
            })
            .to_string(),
        ),
    );
    let placed = host.viewport_frame();
    assert!(!Arc::ptr_eq(
        &assembled.instance_body_poses,
        &placed.instance_body_poses
    ));
    assert!(Arc::ptr_eq(
        &placed.instance_body_poses,
        &host.viewport_frame().instance_body_poses
    ));
    assert!(Arc::ptr_eq(&solid.document, &placed.document));
    assert_eq!(
        placed.instance_body_poses.len(),
        assembled.instance_body_poses.len() + 1
    );
    let pose = placed
        .instance_body_poses
        .iter()
        .find(|pose| pose.occurrence_id.0 == repeat["id"].as_u64().unwrap())
        .unwrap();
    assert_eq!(pose.translation, [20., 0., 0.]);
    value(host.engine_call("construction_set_visibility", r#"{"visible":false}"#));
    assert!(Arc::ptr_eq(
        &solid.document,
        &host.viewport_frame().document
    ));
}

#[test]
fn drawing_edits_and_tab_switches_reuse_metadata_and_eviction_releases_it() {
    let host = NativeEngineHost::new();
    value(host.bind_project_session("first"));
    let first = host.viewport_frame();
    let revision = first.document.metadata_revision;
    let weak = Arc::downgrade(&first.document);
    let placement_weak = Arc::downgrade(&first.instance_body_poses);
    value(
        host.engine_call(
            "drawing_set_document",
            &json!({
                "sheets":[
                    {"id":1,"name":"First","format":"a4","orientation":"landscape"},
                    {"id":2,"name":"Second","format":"a4","orientation":"landscape"}
                ],"active_sheet_id":2,"next_sheet_id":3
            })
            .to_string(),
        ),
    );
    assert!(Arc::ptr_eq(
        &first.document,
        &host.viewport_frame().document
    ));
    value(host.create_project_session("second"));
    let second = host.viewport_frame();
    assert!(!Arc::ptr_eq(&first.document, &second.document));
    value(host.activate_project_session("first"));
    assert!(Arc::ptr_eq(
        &first.document,
        &host.viewport_frame().document
    ));
    assert_eq!(
        host.with_drawing(|drawing| drawing.active_sheet_id),
        Some(2)
    );
    value(host.activate_project_session("second"));
    drop(first);
    assert!(weak.upgrade().is_some());
    assert!(host.evict_inactive_project_session("first").unwrap());
    assert!(weak.upgrade().is_none());
    assert!(placement_weak.upgrade().is_none());
    assert!(Arc::ptr_eq(
        &second.document,
        &host.viewport_frame().document
    ));
    value(host.activate_project_session("first"));
    assert!(host.viewport_frame().document.metadata_revision > revision);
    assert_eq!(
        host.with_drawing(|drawing| drawing.active_sheet_id),
        Some(2)
    );
}
