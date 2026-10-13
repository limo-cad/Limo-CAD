use super::*;
use serde_json::{json, Value};

fn value(response: String) -> Value {
    let response: Value = serde_json::from_str(&response).unwrap();
    assert_eq!(response["ok"], true, "{response}");
    response["value"].clone()
}

#[test]
fn shared_drawing_snapshot_reuses_reads_and_invalidates_only_drawing_intent() {
    let host = NativeEngineHost::new();
    let empty = host.shared_drawing_snapshot();
    let viewport = host.viewport_frame();
    assert!(Arc::ptr_eq(&empty, &host.shared_drawing_snapshot()));
    value(host.engine_call("document_set_name", r#""Unrelated metadata""#));
    assert!(Arc::ptr_eq(&empty, &host.shared_drawing_snapshot()));
    value(host.engine_call("assembly_set_document", "{}"));
    assert!(
        Arc::ptr_eq(&empty, &host.shared_drawing_snapshot()),
        "Assembly invalidation without a released assembly sheet must preserve drawing identity"
    );
    value(
        host.engine_call(
            "drawing_set_document",
            &json!({"sheets":[
        {"id":1,"name":"Original","format":"a4","orientation":"landscape"}],
        "active_sheet_id":1,"next_sheet_id":2})
            .to_string(),
        ),
    );
    let original = host.shared_drawing_snapshot();
    assert!(!Arc::ptr_eq(&empty, &original) && empty.sheets.is_empty());
    let geometry = host.viewport_frame().document.scene.clone();
    let mut edited = (*original).clone();
    edited.sheets[0].name = "Edited".into();
    value(host.engine_call(
        "drawing_set_document",
        &serde_json::to_string(&edited).unwrap(),
    ));
    let current = host.shared_drawing_snapshot();
    assert!(!Arc::ptr_eq(&original, &current));
    assert!(original.sheets[0].name == "Original" && current.sheets[0].name == "Edited");
    assert!(Arc::ptr_eq(
        &geometry,
        &host.viewport_frame().document.scene
    ));
    value(host.engine_call(
        "drawing_set_document",
        &serde_json::to_string(current.as_ref()).unwrap(),
    ));
    assert!(
        Arc::ptr_eq(&current, &host.shared_drawing_snapshot()),
        "An identical document must not republish storage"
    );
    let mut invalid = (*current).clone();
    invalid.sheets.push(invalid.sheets[0].clone());
    let rejected: Value = serde_json::from_str(&host.engine_call(
        "drawing_set_document",
        &serde_json::to_string(&invalid).unwrap(),
    ))
    .unwrap();
    assert!(rejected["ok"] == false && Arc::ptr_eq(&current, &host.shared_drawing_snapshot()));
    let mut owned = host.drawing_snapshot();
    owned.sheets.clear();
    assert!(
        host.shared_drawing_snapshot().sheets.len() == 1,
        "Compatibility edit copies remain independently owned"
    );
    assert!(viewport.document.scene.bodies.is_empty());

    // Exercise both authoritative release-status mutation paths while readers
    // retain the issued drawing. Neither path may edit a retained snapshot.
    let mut released = (*current).clone();
    released.sheets[0].views.push(
        serde_json::from_value(json!({
            "id":1,"name":"Placed assembly","kind":"top","scope":"assembly",
            "occurrence_ids":[],"direction":[0,0,1],"up":[0,1,0],
            "position":[80,60],"scale":1
        }))
        .unwrap(),
    );
    released.next_view_id = 2;
    released.sheets[0].release = limo_cad_sketch::DrawingReleaseDto {
        status: limo_cad_sketch::DrawingReleaseStatus::Released,
        released_revision: "A".into(),
        released_at: "2026-10-10".into(),
    };
    value(host.engine_call(
        "drawing_set_document",
        &serde_json::to_string(&released).unwrap(),
    ));
    let issued_assembly = host.shared_drawing_snapshot();
    value(host.engine_call("assembly_set_document", "{}"));
    let assembly_draft = host.shared_drawing_snapshot();
    let mut expected = (*issued_assembly).clone();
    expected.sheets[0].release.status = limo_cad_sketch::DrawingReleaseStatus::Draft;
    assert!(!Arc::ptr_eq(&issued_assembly, &assembly_draft));
    assert!(
        issued_assembly.sheets[0].release.status == limo_cad_sketch::DrawingReleaseStatus::Released
    );
    assert!(
        *assembly_draft == expected,
        "Assembly invalidation changes only the existing release policy status"
    );
    value(host.engine_call("assembly_set_document", "{}"));
    assert!(Arc::ptr_eq(
        &assembly_draft,
        &host.shared_drawing_snapshot()
    ));

    value(host.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#));
    value(host.engine_call(
        "add_rectangle",
        r#"{"mode":"two_point","p1":{"x":0,"y":0},"p2":{"x":10,"y":6},"ctrl_held":false}"#,
    ));
    value(host.engine_call("end_sketch", ""));
    assert!(Arc::ptr_eq(
        &assembly_draft,
        &host.shared_drawing_snapshot()
    ));
    let mut reissued = (*assembly_draft).clone();
    reissued.sheets[0].release.status = limo_cad_sketch::DrawingReleaseStatus::Released;
    value(host.engine_call(
        "drawing_set_document",
        &serde_json::to_string(&reissued).unwrap(),
    ));
    let issued_geometry = host.shared_drawing_snapshot();
    let extrude = |distance| {
        host.solid_extrude(
            &json!({
                "sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body",
                "extent":{"type":"distance","distance":distance},"taper_angle_deg":0,
                "flip":false,"target_body_ids":[]
            })
            .to_string(),
        )
    };
    value(extrude(3));
    let geometry_draft = host.shared_drawing_snapshot();
    let mut expected = (*issued_geometry).clone();
    expected.sheets[0].release.status = limo_cad_sketch::DrawingReleaseStatus::Draft;
    assert!(!Arc::ptr_eq(&issued_geometry, &geometry_draft));
    assert!(
        issued_geometry.sheets[0].release.status == limo_cad_sketch::DrawingReleaseStatus::Released
    );
    assert!(
        *geometry_draft == expected,
        "Geometry invalidation changes only the existing release policy status"
    );
    let bodies = host.viewport_frame().document.scene.bodies.len();
    value(extrude(4));
    assert!(host.viewport_frame().document.scene.bodies.len() > bodies);
    assert!(
        Arc::ptr_eq(&geometry_draft, &host.shared_drawing_snapshot()),
        "A further real geometry edit must not copy an already-Draft drawing"
    );
}

#[test]
fn shared_drawing_snapshot_respects_tab_close_and_cold_retirement_lifetimes() {
    let host = NativeEngineHost::new();
    value(host.bind_project_session("drawing-a"));
    value(
        host.engine_call(
            "drawing_set_document",
            &json!({"sheets":[
        {"id":1,"name":"Retained","format":"a4","orientation":"landscape"}],
        "active_sheet_id":1,"next_sheet_id":2})
            .to_string(),
        ),
    );
    let first = host.shared_drawing_snapshot();
    let weak = Arc::downgrade(&first);
    value(host.create_project_session("drawing-b"));
    let second = host.shared_drawing_snapshot();
    value(host.activate_project_session("drawing-a"));
    assert!(Arc::ptr_eq(&first, &host.shared_drawing_snapshot()));
    value(host.activate_project_session("drawing-b"));
    assert!(host.evict_inactive_project_session("drawing-a").unwrap());
    assert!(
        weak.upgrade().is_some() && first.sheets[0].name == "Retained",
        "A reader retains an immutable value after engine eviction"
    );
    drop(first);
    assert!(
        weak.upgrade().is_none(),
        "The cold engine must not retain an extra drawing cache"
    );
    assert!(Arc::ptr_eq(&second, &host.shared_drawing_snapshot()));
    value(host.activate_project_session("drawing-a"));
    let restored = host.shared_drawing_snapshot();
    assert!(restored.sheets[0].name == "Retained" && restored.active_sheet_id == Some(1));
    let restored_weak = Arc::downgrade(&restored);
    value(host.activate_project_session("drawing-b"));
    value(host.drop_project_session("drawing-a"));
    assert!(restored.sheets[0].name == "Retained");
    drop(restored);
    assert!(restored_weak.upgrade().is_none());
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
