#![cfg(feature = "native-occt")]
use limo_cad_native_engine::NativeEngineHost;
use serde_json::{json, Value};
fn ok(raw: String) -> Value {
    let v: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["ok"], true, "{v}");
    v["value"].clone()
}
fn model(h: &NativeEngineHost) -> Value {
    serde_json::from_str(&h.project_model_projection().unwrap()).unwrap()
}
fn rim(h: &NativeEngineHost, r: f64, y: f64) -> limo_cad_core::EdgeId {
    h.solid_scene_snapshot().bodies[0]
        .edges
        .iter()
        .find(|e| {
            e.circle
                .is_some_and(|c| (c.radius - r).abs() < 1e-6 && (c.center.y - y).abs() < 1e-6)
        })
        .unwrap_or_else(|| {
            panic!(
                "missing rim R{r}/Y{y}; circles {:?}",
                h.solid_scene_snapshot().bodies[0]
                    .edges
                    .iter()
                    .filter_map(|e| e.circle)
                    .collect::<Vec<_>>()
            )
        })
        .id
}
fn fixture() -> NativeEngineHost {
    fixture_with_support(false)
}
fn fixture_with_support(generated: bool) -> NativeEngineHost {
    fixture_with_prefix(generated, false, false)
}
fn fixture_with_prefix(generated: bool, extra_sketch: bool, extra_body: bool) -> NativeEngineHost {
    let h = NativeEngineHost::new();
    ok(h.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#));
    ok(h.engine_call("set_grid_snap", r#"{"enabled":false}"#));
    let pts = [
        (10., 0.),
        (10., 30.),
        (15., 30.),
        (15., 3.),
        (22., 3.),
        (22., 0.),
    ];
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        ok(h.engine_call(
            "add_line",
            &json!({"from":{"x":a.0,"y":a.1},"to_raw":{"x":b.0,"y":b.1},"ctrl_held":true})
                .to_string(),
        ));
    }
    ok(h.engine_call("end_sketch", ""));
    ok(h.solid_revolve(&json!({"sketch_name":"Sketch1","profile_indices":[0],"axis_origin":{"x":0.,"y":0.},"axis_direction":{"x":0.,"y":1.},"angle_deg":360.,"flip":false,"operation":"new_body","target_body_ids":[]}).to_string()));
    rim(&h, 22., 0.);
    rim(&h, 22., 3.);
    if extra_sketch {
        ok(h.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#));
        ok(h.engine_call("set_grid_snap", r#"{"enabled":false}"#));
        ok(h.engine_call("add_circle_locked", &json!({"mode":"center_diameter","anchor":{"x":100.,"y":0.},"edge_hint":{"x":105.,"y":0.},"diameter_mm":10.,"ctrl_held":true}).to_string()));
        ok(h.engine_call("end_sketch", ""));
        if extra_body {
            ok(h.solid_extrude(&json!({"sketch_name":"Sketch2","profile_indices":[0],"extent":{"type":"distance","distance":10.},"taper_angle_deg":0.,"flip":false,"operation":"new_body","target_body_ids":[]}).to_string()));
        }
    }
    let inner = rim(&h, 10., 30.);
    if generated {
        ok(h.solid_chamfer(
            &json!({"body_id":1,"edge_ids":[inner],"distance":1.,"tangent_chain":false})
                .to_string(),
        ));
    } else {
        ok(h.solid_fillet(
            &json!({"body_id":1,"edge_ids":[inner],"radius":1.,"tangent_chain":false}).to_string(),
        ));
    }
    let outer = if generated {
        rim(&h, 11., 30.)
    } else {
        rim(&h, 22., 3.)
    };
    let distance = if generated { 0.1 } else { 0.5 };
    ok(h.solid_chamfer(&json!({"body_id":1,"edge_ids":[outer],"distance":distance,"angle_deg":null,"tangent_chain":false}).to_string()));
    assert!(h.solid_scene_snapshot().errors.is_empty());
    h
}
fn correct(h: &NativeEngineHost) {
    assert!(h.solid_scene_snapshot().errors.is_empty());
    rim(h, 21.5, 3.); // chamfer terminates on the intended upper flange
    rim(h, 22., 0.); // bottom outer rim remains untouched
}
#[test]
fn production_delete_preserves_rim_and_cold_replay() {
    let h = fixture();
    let original = h.project_snapshot().unwrap();
    let id = model(&h)["fillets"][0]["feature_id"].clone();
    let old_key = model(&h)["chamfers"][0]["edge_keys"].clone();
    ok(h.solid_delete_feature(&json!({"feature_id":id}).to_string()));
    correct(&h);
    eprintln!("536 native delete: original chamfer keys {old_key}, rewritten keys {}; upper R21.5/Y3 and untouched bottom R22/Y0 verified", model(&h)["chamfers"][0]["edge_keys"]);
    let saved = h.project_snapshot().unwrap();
    assert!(model(&h)["fillets"].as_array().unwrap().is_empty());
    let cold = NativeEngineHost::new();
    // Exercise the actual .limo container codec, then reconstruct in a cold
    // host; no pointer, kernel cache or in-memory proof crosses this boundary.
    let archive = limo_cad_project_file::ProjectArchive::new(
        saved.model_json().to_owned(),
        limo_cad_project_file::SaveMetadata {
            application_version: "0.3.0-rc.3",
            saved_at: "2026-11-04T00:00:00Z",
        },
    )
    .unwrap();
    let bytes = archive.encode().unwrap();
    let reopened = limo_cad_project_file::ProjectArchive::decode(bytes)
        .unwrap()
        .snapshot()
        .unwrap();
    ok(cold.project_load_snapshot(&reopened));
    correct(&cold);
    eprintln!("536 cold .limo container decode/replay: intended upper chamfer and untouched bottom verified");
    // Snapshot history undo/redo, then repeat deletion from the restored input.
    ok(h.project_load_snapshot(&original));
    ok(h.solid_delete_feature(&json!({"feature_id":id}).to_string()));
    correct(&h);
    ok(h.project_load_snapshot(&saved));
    correct(&h);
    ok(h.solid_set_rollback(r#"{"rollback_index":2}"#));
    ok(h.solid_set_rollback(r#"{"rollback_index":3}"#));
    correct(&h);
}
#[test]
fn unsupported_inactive_history_rejects_atomically() {
    let h = fixture();
    let id = model(&h)["fillets"][0]["feature_id"].clone();
    ok(h.solid_set_rollback(r#"{"rollback_index":3}"#));
    let before = h.project_snapshot().unwrap();
    let scene = h.solid_scene_snapshot();
    let rev = h.geometry_revision();
    let v: Value =
        serde_json::from_str(&h.solid_delete_feature(&json!({"feature_id":id}).to_string()))
            .unwrap();
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(h.project_snapshot().unwrap(), before);
    assert!(std::sync::Arc::ptr_eq(&scene, &h.solid_scene_snapshot()));
    assert_eq!(rev, h.geometry_revision());
}

#[test]
fn generated_support_rejects_and_retains_live_state() {
    let h = fixture_with_prefix(true, true, true);
    let id = model(&h)["chamfers"][0]["feature_id"].clone();
    let before = h.project_snapshot().unwrap();
    let scene = h.solid_scene_snapshot();
    let revision = h.geometry_revision();
    let reply: Value =
        serde_json::from_str(&h.solid_delete_feature(&json!({"feature_id":id}).to_string()))
            .unwrap();
    assert_eq!(reply["ok"], false, "{reply}");
    assert!(reply.to_string().contains("survivor"), "{reply}");
    assert_eq!(h.project_snapshot().unwrap(), before);
    assert!(std::sync::Arc::ptr_eq(&scene, &h.solid_scene_snapshot()));
    assert_eq!(revision, h.geometry_revision());
}

#[test]
fn opaque_domain_rejects_and_retains_snapshot() {
    use limo_cad_project_file::{ProjectSnapshot, SnapshotIntent};
    let h = fixture();
    let id = model(&h)["fillets"][0]["feature_id"].clone();
    let snapshot = ProjectSnapshot::new(
        h.project_model_projection().unwrap(),
        vec![SnapshotIntent {
            name: "intent/future.cbor".into(),
            payload: vec![0xf6],
        }],
    )
    .unwrap();
    ok(h.project_load_snapshot(&snapshot));
    let before = h.project_snapshot().unwrap();
    let scene = h.solid_scene_snapshot();
    let reply: Value =
        serde_json::from_str(&h.solid_delete_feature(&json!({"feature_id":id}).to_string()))
            .unwrap();
    assert_eq!(reply["ok"], false, "{reply}");
    assert_eq!(h.project_snapshot().unwrap(), before);
    assert!(std::sync::Arc::ptr_eq(&scene, &h.solid_scene_snapshot()));
}

#[test]
fn encoded_desktop_ipc_delete_uses_the_same_proof_transaction() {
    let h = fixture();
    let id = model(&h)["fillets"][0]["feature_id"].clone();
    ok(h.apply_encoded_mutate(
        "solid_prepare_delete_feature",
        &json!({"feature_id":id}).to_string(),
        true,
    ));
    correct(&h);
    let cold = NativeEngineHost::new();
    ok(cold.project_load_snapshot(&h.project_snapshot().unwrap()));
    correct(&cold);
}

#[test]
fn delete_preserves_pending_project_load() {
    let h = fixture();
    let id = model(&h)["fillets"][0]["feature_id"].clone();
    let before = h.project_snapshot().unwrap();
    let scene = h.solid_scene_snapshot();
    let revision = h.geometry_revision();
    let empty = NativeEngineHost::new();
    let pending = ok(h.engine_call(
        "project_prepare_load",
        &serde_json::to_string(&empty.project_model_projection().unwrap()).unwrap(),
    ));
    let reply: Value =
        serde_json::from_str(&h.solid_delete_feature(&json!({"feature_id":id}).to_string()))
            .unwrap();
    assert_eq!(reply["ok"], false, "deletion must not abandon Open");
    assert_eq!(h.project_snapshot().unwrap(), before);
    assert!(std::sync::Arc::ptr_eq(&scene, &h.solid_scene_snapshot()));
    assert_eq!(h.geometry_revision(), revision);
    ok(h.engine_call(
        "solid_commit",
        &json!({
            "transaction_id":pending["transaction_id"],
            "scene":{"bodies":[],"errors":[]}
        })
        .to_string(),
    ));
    assert!(h.document_snapshot().features.is_empty());
}

#[test]
fn delete_preserves_sketch_undo_and_adaptive_grid() {
    let h = fixture();
    let id = model(&h)["fillets"][0]["feature_id"].clone();
    ok(h.engine_call("set_grid_step", r#"{"step_mm":0.001}"#));
    assert_eq!(
        ok(h.engine_call("finished_sketches", ""))[0]["can_undo"],
        true
    );
    ok(h.solid_delete_feature(&json!({"feature_id":id}).to_string()));
    assert_eq!(
        ok(h.engine_call("finished_sketches", ""))[0]["can_undo"],
        true
    );
    let edited = ok(h.engine_call("edit_sketch", r#""Sketch1""#));
    let undone = ok(h.engine_call("undo", ""));
    assert_eq!(undone["sketch"]["can_redo"], true);
    assert_ne!(undone["sketch"]["entities"], edited["entities"]);
    let redone = ok(h.engine_call("redo", ""));
    assert_eq!(redone["sketch"]["can_undo"], true);
    assert_eq!(redone["sketch"]["entities"], edited["entities"]);
    ok(h.engine_call("set_grid_snap", r#"{"enabled":true}"#));
    let preview = ok(h.engine_call(
        "preview_segment",
        &json!({
            "from":{"x":20.,"y":20.},"to_raw":{"x":12.34515,"y":8.76585},"ctrl_held":true
        })
        .to_string(),
    ));
    assert!(
        (preview["snapped_to"]["x"].as_f64().unwrap() - 12.345).abs() < 1e-12,
        "{preview}"
    );
    assert!(
        (preview["snapped_to"]["y"].as_f64().unwrap() - 8.766).abs() < 1e-12,
        "{preview}"
    );
}

fn check_unchanged_prefix(extra_body: bool) {
    let h = fixture_with_prefix(false, true, extra_body);
    let before = model(&h);
    let other = h
        .solid_scene_snapshot()
        .bodies
        .iter()
        .find(|b| b.id.0 == 2)
        .cloned();
    let id = before["fillets"][0]["feature_id"].clone();
    ok(h.solid_delete_feature(&json!({"feature_id":id}).to_string()));
    correct(&h);
    let after = model(&h);
    assert_eq!(after["sketches"], before["sketches"]);
    assert_eq!(after["extrudes"], before["extrudes"]);
    assert_eq!(after["revolves"], before["revolves"]);
    assert_eq!(
        after["document"]["history"]["rollback_index"]
            .as_u64()
            .unwrap(),
        before["document"]["history"]["rollback_index"]
            .as_u64()
            .unwrap()
            - 1
    );
    assert_eq!(
        h.solid_scene_snapshot()
            .bodies
            .iter()
            .find(|b| b.id.0 == 2)
            .cloned(),
        other
    );
    let saved = h.project_snapshot().unwrap();
    let archive = limo_cad_project_file::ProjectArchive::new(
        saved.model_json().to_owned(),
        limo_cad_project_file::SaveMetadata {
            application_version: "0.3.0-rc.3",
            saved_at: "2026-11-04T00:00:00Z",
        },
    )
    .unwrap();
    let reopened = limo_cad_project_file::ProjectArchive::decode(archive.encode().unwrap())
        .unwrap()
        .snapshot()
        .unwrap();
    let cold = NativeEngineHost::new();
    ok(cold.project_load_snapshot(&reopened));
    correct(&cold);
    assert_eq!(
        cold.solid_scene_snapshot()
            .bodies
            .iter()
            .find(|b| b.id.0 == 2)
            .cloned(),
        other
    );
    eprintln!("536 unchanged prefix extra_body={extra_body}: upper R21.5/Y3, untouched bottom R22/Y0, sketches and retained body cold-replay verified");
}

#[test]
fn unchanged_prefix_with_unrelated_sketch_cold_replays() {
    check_unchanged_prefix(false);
}

#[test]
fn unchanged_prefix_with_unrelated_body_cold_replays() {
    check_unchanged_prefix(true);
}

#[test]
fn downstream_sketch_rejects_without_changing_live_prefix() {
    let h = fixture_with_prefix(false, true, true);
    ok(h.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xz"}"#));
    ok(h.engine_call("end_sketch", ""));
    let before = h.project_snapshot().unwrap();
    let scene = h.solid_scene_snapshot();
    let revision = h.geometry_revision();
    let id = model(&h)["fillets"][0]["feature_id"].clone();
    let reply: Value =
        serde_json::from_str(&h.solid_delete_feature(&json!({"feature_id":id}).to_string()))
            .unwrap();
    assert_eq!(reply["ok"], false, "{reply}");
    assert_eq!(h.project_snapshot().unwrap(), before);
    assert!(std::sync::Arc::ptr_eq(&scene, &h.solid_scene_snapshot()));
    assert_eq!(h.geometry_revision(), revision);
}
