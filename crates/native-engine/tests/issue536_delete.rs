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
    let h = fixture_with_support(true);
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
