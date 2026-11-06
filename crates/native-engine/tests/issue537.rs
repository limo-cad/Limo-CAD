#![cfg(feature = "native-occt")]
use limo_cad_native_engine::NativeEngineHost;
use serde_json::{json, Value};

#[track_caller]
fn value(raw: String) -> Value {
    let reply: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(reply["ok"], true, "{reply}");
    reply["value"].clone()
}

fn circles(host: &NativeEngineHost, radii: &[f64]) {
    for radius in radii {
        value(host.engine_call("add_circle_locked", &json!({"mode":"center_diameter",
            "anchor":{"x":0,"y":0},"edge_hint":{"x":radius,"y":0},"diameter_mm":2. * radius,"ctrl_held":true}).to_string()));
    }
    value(host.engine_call("end_sketch", ""));
}

fn fixture() -> NativeEngineHost {
    let host = NativeEngineHost::new();
    value(host.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#));
    circles(&host, &[30., 10.]);
    value(host.solid_extrude(r#"{"sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":10},"taper_angle_deg":0,"flip":false,"target_body_ids":[]}"#));
    let scene = host.solid_scene_snapshot();
    let body = &scene.bodies[0];
    let face = body
        .faces
        .iter()
        .find(|f| {
            f.plane
                .is_some_and(|p| p.normal[2] > 0.9 && (p.origin[2] - 10.).abs() < 1e-6)
        })
        .unwrap();
    value(host.engine_call(
        "begin_sketch",
        &json!({"type":"planar_face","face_id":face.id}).to_string(),
    ));
    circles(&host, &[20., 11.]);
    value(host.solid_extrude(&json!({"sketch_name":"Sketch2","profile_indices":[0],"operation":"join","extent":{"type":"distance","distance":30},"taper_angle_deg":0,"flip":false,"target_body_ids":[body.id]}).to_string()));
    host
}

fn request(host: &NativeEngineHost, distance: f64) -> Value {
    let scene = host.solid_scene_snapshot();
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    let body = &scene.bodies[0];
    assert!(
        body.edges.iter().any(|e| e
            .circle
            .is_some_and(|c| (c.radius - 11.).abs() < 1e-6 && (c.center.z - 40.).abs() < 1e-6)),
        "tube must remain hollow and reach z40"
    );
    let edge = body
        .edges
        .iter()
        .find(|e| {
            e.circle
                .is_some_and(|c| (c.radius - 30.).abs() < 1e-6 && (c.center.z - 10.).abs() < 1e-6)
        })
        .unwrap();
    json!({"body_id":body.id,"edge_ids":[edge.id],"distance":distance,"angle_deg":null,"tangent_chain":false})
}
fn volume(host: &NativeEngineHost) -> f64 {
    let scene = host.solid_scene_snapshot();
    let mesh = &scene.bodies[0].mesh;
    mesh.indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| {
            let p = |i: u32| {
                let i = i as usize * 3;
                [
                    mesh.positions[i] as f64,
                    mesh.positions[i + 1] as f64,
                    mesh.positions[i + 2] as f64,
                ]
            };
            let a = p(t[0]);
            let b = p(t[1]);
            let c = p(t[2]);
            (a[0] * (b[1] * c[2] - b[2] * c[1])
                + a[1] * (b[2] * c[0] - b[0] * c[2])
                + a[2] * (b[0] * c[1] - b[1] * c[0]))
                / 6.
        })
        .sum::<f64>()
        .abs()
}
#[test]
fn hollow_flange_staged_chamfer_edit_matches_fresh_three_mm() {
    let host = fixture();
    let selected = request(&host, 2.);
    value(host.solid_chamfer(&selected.to_string()));
    assert!(host.solid_scene_snapshot().errors.is_empty());
    let snapshot = host.project_snapshot().unwrap();
    let model = host.project_model_projection().unwrap();
    let model: Value = serde_json::from_str(&model).unwrap();
    let feature = model["chamfers"][0]["feature_id"].clone();
    let restore = host.document_history_position().0;
    // Same load / rollback / edit / restore sequence as desktop feature::Stage.
    let stage = NativeEngineHost::new();
    value(stage.project_load_snapshot(&snapshot));
    value(stage.solid_set_rollback(&json!({"rollback_index":restore-1}).to_string()));
    let mut changed = request(&stage, 3.);
    value(stage.solid_edit_chamfer(&json!({"feature_id":feature,"chamfer":changed}).to_string()));
    value(stage.solid_set_rollback(&json!({"rollback_index":restore}).to_string()));
    assert!(stage.solid_scene_snapshot().errors.is_empty());
    assert!(stage
        .document_snapshot()
        .features
        .iter()
        .all(|f| matches!(f.status, limo_cad_core::FeatureStatus::Ok)));
    let edited: Value = serde_json::from_str(&stage.project_model_projection().unwrap()).unwrap();
    assert_eq!(
        edited["sketches"], model["sketches"],
        "native replay must retain the authored support plane, basis and geometry"
    );
    let fresh = fixture();
    value(fresh.solid_chamfer(&request(&fresh, 3.).to_string()));
    assert!(fresh.solid_scene_snapshot().errors.is_empty());
    assert!(
        (volume(&stage) - volume(&fresh)).abs() < 1e-6,
        "edited and fresh native volumes differ"
    );
    let expected = std::f64::consts::PI * (16370. - 261.);
    assert!(
        (volume(&stage) - expected).abs() / expected < 0.005,
        "native geometry must have intended flange/tube and chamfer volume: {}",
        volume(&stage)
    );
    // Desktop does not install a failed isolated stage. The live model remains exact.
    let before = host.project_model_projection().unwrap();
    let before_scene = host.solid_scene_snapshot();
    value(stage.project_load_snapshot(&snapshot));
    value(stage.solid_set_rollback(&json!({"rollback_index":restore-1}).to_string()));
    changed["distance"] = json!(100.);
    value(stage.solid_edit_chamfer(&json!({"feature_id":feature,"chamfer":changed}).to_string()));
    value(stage.solid_set_rollback(&json!({"rollback_index":restore}).to_string()));
    assert!(
        !stage.solid_scene_snapshot().errors.is_empty(),
        "oversized chamfer must fail"
    );
    assert_eq!(host.project_model_projection().unwrap(), before);
    assert!(std::sync::Arc::ptr_eq(
        &before_scene,
        &host.solid_scene_snapshot()
    ));
}

// Video-guided history, not the reporter's unavailable model/dimensions:
// Sketch1, Extrude1, Sketch2, Extrude2, Extrude3, Combine1, Chamfer.
fn combined_fixture() -> NativeEngineHost {
    let host = NativeEngineHost::new();
    value(host.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#));
    circles(&host, &[20., 10.]);
    let extrude = |sketch: &str, operation: &str, distance: f64, targets: Value| {
        value(
            host.solid_extrude(
                &json!({"sketch_name":sketch,"profile_indices":[0],
            "operation":operation,"extent":{"type":"distance","distance":distance},
            "target_body_ids":targets})
                .to_string(),
            ),
        );
    };
    extrude("Sketch1", "new_body", 30., json!([]));
    let scene = host.solid_scene_snapshot();
    let tube = scene.bodies[0].id;
    let face = scene.bodies[0]
        .faces
        .iter()
        .find(|f| {
            f.plane
                .is_some_and(|p| p.normal[2] > 0.9 && (p.origin[2] - 30.).abs() < 1e-6)
        })
        .unwrap();
    value(host.engine_call(
        "begin_sketch",
        &json!({"type":"planar_face","face_id":face.id}).to_string(),
    ));
    circles(&host, &[30., 11.]);
    extrude("Sketch2", "new_body", 5., json!([]));
    extrude("Sketch1", "join", 35., json!([tube]));
    let scene = host.solid_scene_snapshot();
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    assert_eq!(scene.bodies.len(), 2);
    let flange = scene.bodies.iter().find(|b| b.id != tube).unwrap().id;
    value(
        host.solid_body_feature(
            &json!({"type":"combine","request":{
                "target_body_id":flange,"tool_body_ids":[tube],"operation":"join","keep_tools":false
            }})
            .to_string(),
        ),
    );
    assert!(host.solid_scene_snapshot().errors.is_empty());
    assert_eq!(host.solid_scene_snapshot().bodies.len(), 1);
    host
}

fn combined_request(host: &NativeEngineHost, distance: f64) -> Value {
    let scene = host.solid_scene_snapshot();
    assert!(scene.errors.is_empty(), "{:?}", scene.errors);
    let body = &scene.bodies[0];
    let edge = body
        .edges
        .iter()
        .find(|e| {
            e.circle
                .is_some_and(|c| (c.radius - 30.).abs() < 1e-6 && (c.center.z - 35.).abs() < 1e-6)
        })
        .unwrap();
    json!({"body_id":body.id,"edge_ids":[edge.id],"distance":distance,
        "angle_deg":45.,"tangent_chain":false})
}

#[test]
fn video_guided_combined_history_staged_edit_matches_fresh() {
    for distance in [3., 4.] {
        let live = combined_fixture();
        value(live.solid_chamfer(&combined_request(&live, 2.).to_string()));
        assert!(live.solid_scene_snapshot().errors.is_empty());
        let original = live.project_model_projection().unwrap();
        let live_scene = live.solid_scene_snapshot();
        assert!(live
            .document_snapshot()
            .features
            .iter()
            .all(|f| matches!(f.status, limo_cad_core::FeatureStatus::Ok)));
        let original_json: Value = serde_json::from_str(&original).unwrap();
        let feature = original_json["chamfers"][0]["feature_id"].clone();
        let restore = live.document_history_position().0;
        assert_eq!(restore, 7, "match the video's seven-feature history");
        let stage = NativeEngineHost::new();
        value(stage.project_load_snapshot(&live.project_snapshot().unwrap()));
        value(stage.solid_set_rollback(r#"{"rollback_index":2}"#));
        let support = stage.solid_scene_snapshot().bodies[0]
            .faces
            .iter()
            .find(|f| {
                f.plane
                    .is_some_and(|p| p.normal[2] > 0.9 && (p.origin[2] - 30.).abs() < 1e-6)
            })
            .unwrap()
            .id;
        value(stage.solid_set_rollback(&json!({"rollback_index":restore-1}).to_string()));
        assert!(
            !stage
                .solid_scene_snapshot()
                .bodies
                .iter()
                .any(|b| b.faces.iter().any(|f| f.id == support)),
            "the later combined scene legitimately consumed Sketch2's historical support"
        );
        value(stage.solid_edit_chamfer(
            &json!({"feature_id":feature,"chamfer":combined_request(&stage, distance)}).to_string(),
        ));
        value(stage.solid_set_rollback(&json!({"rollback_index":restore}).to_string()));
        assert!(stage.solid_scene_snapshot().errors.is_empty());
        assert!(stage
            .document_snapshot()
            .features
            .iter()
            .all(|f| matches!(f.status, limo_cad_core::FeatureStatus::Ok)));
        let edited: Value =
            serde_json::from_str(&stage.project_model_projection().unwrap()).unwrap();
        assert_eq!(edited["sketches"], original_json["sketches"]);
        let fresh = combined_fixture();
        value(fresh.solid_chamfer(&combined_request(&fresh, distance).to_string()));
        assert!((volume(&stage) - volume(&fresh)).abs() < 1e-6);
        // Analytic flange + hollow tube, less the outer 45-degree chamfer.
        let expected =
            std::f64::consts::PI * (13000. - 30. * distance * distance + distance.powi(3) / 3.);
        assert!(
            (volume(&fresh) - expected).abs() / expected < 0.005,
            "{} vs {expected}",
            volume(&fresh)
        );
        let reopened = NativeEngineHost::new();
        value(reopened.project_load(
            &serde_json::to_string(&stage.project_model_projection().unwrap()).unwrap(),
        ));
        assert!(reopened.solid_scene_snapshot().errors.is_empty());
        assert!((volume(&reopened) - volume(&fresh)).abs() < 1e-6);
        assert_eq!(live.project_model_projection().unwrap(), original);
        assert!(std::sync::Arc::ptr_eq(
            &live_scene,
            &live.solid_scene_snapshot()
        ));
        println!(
            "Combined staged edit 2->{distance} matches fresh and cold reopen; volume={}",
            volume(&stage)
        );
    }
}

#[test]
fn direct_edit_after_metadata_rename_retains_historical_support() {
    for kind in ["sketch", "combine", "chamfer"] {
        let host = combined_fixture();
        let mut request = combined_request(&host, 2.);
        value(host.solid_chamfer(&request.to_string()));
        let feature = host
            .document_snapshot()
            .features
            .iter()
            .find(|f| match kind {
                "sketch" => f.name == "Sketch2",
                "combine" => f.name == "Combine1",
                _ => f.name == "Chamfer1",
            })
            .unwrap()
            .id;
        value(
            host.engine_call(
                "solid_rename_feature",
                &json!({
                    "feature_id":feature,"name":format!("Renamed {kind}")
                })
                .to_string(),
            ),
        );
        let model: Value = serde_json::from_str(&host.project_model_projection().unwrap()).unwrap();
        request["distance"] = json!(3.);
        // Direct edit deliberately does not reload a Stage or renew proof by replay.
        value(
            host.solid_edit_chamfer(
                &json!({
                    "feature_id":model["chamfers"][0]["feature_id"],
                    "chamfer":request
                })
                .to_string(),
            ),
        );
        assert!(host.solid_scene_snapshot().errors.is_empty());
        let fresh = combined_fixture();
        value(fresh.solid_chamfer(&combined_request(&fresh, 3.).to_string()));
        assert!((volume(&host) - volume(&fresh)).abs() < 1e-6);
    }
}

#[test]
fn rejected_commit_preserves_historical_support_for_direct_edit() {
    let host = combined_fixture();
    let mut request = combined_request(&host, 2.);
    value(host.solid_chamfer(&request.to_string()));
    let before = host.project_model_projection().unwrap();
    let scene = host.solid_scene_snapshot();
    let model: Value = serde_json::from_str(&before).unwrap();

    let rejected: Value = serde_json::from_str(&host.engine_call(
        "solid_commit",
        &json!({"transaction_id":0,"scene":{"bodies":[],"errors":[]}}).to_string(),
    ))
    .unwrap();
    assert_eq!(rejected["ok"], false, "{rejected}");
    assert!(
        rejected
            .to_string()
            .contains("no solid recompute is pending"),
        "{rejected}"
    );
    assert_eq!(host.project_model_projection().unwrap(), before);
    assert!(std::sync::Arc::ptr_eq(&scene, &host.solid_scene_snapshot()));

    request["distance"] = json!(3.);
    // Rejection must preserve the existing proof; no Stage reload or replay here.
    value(host.solid_edit_chamfer(
        &json!({"feature_id":model["chamfers"][0]["feature_id"],"chamfer":request}).to_string(),
    ));
    assert!(host.solid_scene_snapshot().errors.is_empty());
    let fresh = combined_fixture();
    value(fresh.solid_chamfer(&combined_request(&fresh, 3.).to_string()));
    assert!((volume(&host) - volume(&fresh)).abs() < 1e-6);
}
