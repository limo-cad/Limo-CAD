use super::*;
use limo_cad_project_file::SnapshotIntent;
use serde_json::json;

#[test]
fn native_retention_preserves_known_and_unknown_intent_across_repeated_thaws() {
    let state = NativeEngineHost::new();
    value(state.bind_project_session("a")).unwrap();
    let intent = SnapshotIntent {
        name: "intent/future-study.cbor".into(),
        payload: vec![0, 1, 255, 42],
    };
    let thermal = limo_cad_fem::ThermalStudyDocument {
        studies: vec![limo_cad_fem::SavedThermalStudy {
            id: 7,
            name: "Unmeshed thermal study".into(),
            input: limo_cad_fem::ThermalStudyInput {
                mesh: limo_cad_fem::ThermalMesh {
                    points_m: vec![],
                    elements: vec![],
                },
                materials: vec![],
                prescribed_k: vec![],
                nodal_load_w: vec![],
                surfaces: vec![],
                options: limo_cad_fem::SolveOptions::default(),
            },
        }],
    };
    let expected = {
        let mut workspace = state.inner.lock().unwrap();
        let manager = &mut workspace.active_mut().manager;
        manager
            .replace_project_intents(vec![
                intent,
                SnapshotIntent {
                    name: limo_cad_fem::THERMAL_STUDY_ENTRY.into(),
                    payload: thermal.encode_cbor().unwrap(),
                },
            ])
            .unwrap();
        manager.export_project_snapshot().unwrap()
    };
    value(state.create_project_session("b")).unwrap();
    for _ in 0..3 {
        assert!(state.evict_inactive_project_session("a").unwrap());
        value(state.activate_project_session("a")).unwrap();
        assert_eq!(
            state
                .inner
                .lock()
                .unwrap()
                .active()
                .manager
                .export_project_snapshot()
                .unwrap(),
            expected
        );
        value(state.activate_project_session("b")).unwrap();
        assert!(state
            .inner
            .lock()
            .unwrap()
            .active()
            .manager
            .project_intents()
            .is_empty());
    }
}

#[test]
fn native_retention_invalid_known_domain_keeps_cold_snapshot_and_active_tab() {
    let state = NativeEngineHost::new();
    value(state.bind_project_session("a")).unwrap();
    value(state.create_project_session("b")).unwrap();
    assert!(state.evict_inactive_project_session("a").unwrap());
    let original = {
        let mut workspace = state.inner.lock().unwrap();
        let NativeProject::Cold { snapshot, .. } = workspace.sessions.get_mut("a").unwrap() else {
            panic!()
        };
        let invalid = ProjectSnapshot::new(
            snapshot.model_json().to_owned(),
            vec![SnapshotIntent {
                name: limo_cad_fem::THERMAL_STUDY_ENTRY.into(),
                payload: vec![0xff],
            }],
        )
        .unwrap();
        std::mem::replace(snapshot, invalid)
    };
    let before = state.document_snapshot();
    let revision = state.geometry_revision();
    assert!(value(state.activate_project_session("a")).is_err());
    assert_eq!(state.active_project_session_id(), "b");
    assert_eq!(state.document_snapshot(), before);
    assert_eq!(state.geometry_revision(), revision);
    {
        let mut workspace = state.inner.lock().unwrap();
        let NativeProject::Cold { snapshot, .. } = workspace.sessions.get_mut("a").unwrap() else {
            panic!()
        };
        assert_eq!(snapshot.intents()[0].payload, [0xff]);
        *snapshot = original;
    }
    value(state.activate_project_session("a")).unwrap();
}

fn value(raw: String) -> Result<serde_json::Value, String> {
    let envelope: serde_json::Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
    if envelope["ok"] == true {
        Ok(envelope["value"].clone())
    } else {
        Err(envelope["error"]
            .as_str()
            .unwrap_or("engine error")
            .to_owned())
    }
}

#[test]
fn native_retention_rebuilds_real_occt_geometry_and_preserves_model() {
    let state = NativeEngineHost::new();
    value(state.bind_project_session("a")).unwrap();
    value(state.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#)).unwrap();
    value(state.engine_call("add_rectangle", r#"{"mode":"two_point","p1":{"x":-10.0,"y":-10.0},"p2":{"x":10.0,"y":10.0},"ctrl_held":false}"#)).unwrap();
    value(state.engine_call("end_sketch", "")).unwrap();
    value(state.solid_extrude(r#"{"sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":10.0},"taper_angle_deg":0.0,"flip":false,"target_body_ids":[]}"#)).unwrap();
    let before = value(state.engine_call("project_export_model", "")).unwrap();
    let body = value(state.engine_call("solid_scene", "")).unwrap()["bodies"][0]["id"].clone();
    let geometry_revision = state.geometry_revision();
    value(state.engine_call("print_intent_set_part", &json!({
        "body_id":body,"settings":{"wall_count":6,"infill_density_percent":30,"infill_pattern":"gyroid"},
        "expected_model_json":before,
    }).to_string())).unwrap();
    assert_eq!(state.geometry_revision(), geometry_revision);
    let intent = value(state.engine_call("print_intent_get", "")).unwrap();
    let effective = value(state.engine_call("print_intent_effective", "{}")).unwrap();
    let model = value(state.engine_call("project_export_model", "")).unwrap();
    let request = json!({"expected_model_json":model}).to_string();
    let mesh = state.export_stl(&request).unwrap();
    assert!(!mesh.is_empty());
    let scene = serde_json::to_value(
        state
            .inner
            .lock()
            .unwrap()
            .active()
            .manager
            .solid_scene_ref(),
    )
    .unwrap();
    let revision = state.geometry_revision();
    let sketches = value(state.engine_call("finished_sketches", "")).unwrap();
    assert_eq!(sketches[0]["can_undo"], true);
    assert!(!state.evict_inactive_project_session("a").unwrap());
    value(state.create_project_session("b")).unwrap();
    assert!(state.evict_inactive_project_session("a").unwrap());
    assert_eq!(state.cold_project_sessions(), ["a"]);
    assert_eq!(state.active_project_session_id(), "b");
    value(state.activate_project_session("a")).unwrap();
    assert_eq!(
        value(state.engine_call("print_intent_get", "")).unwrap(),
        intent
    );
    assert_eq!(
        value(state.engine_call("print_intent_effective", "{}")).unwrap(),
        effective
    );
    assert!(state.cold_project_sessions().is_empty());
    assert_eq!(state.geometry_revision(), revision + 1);
    assert_eq!(
        value(state.engine_call("project_export_model", "")).unwrap(),
        model
    );
    assert_eq!(state.export_stl(&request).unwrap(), mesh);
    assert_eq!(
        value(state.engine_call("finished_sketches", "")).unwrap(),
        sketches,
        "Eviction must preserve finished sketch sessions as well as solid geometry"
    );
    assert_eq!(
        serde_json::to_value(
            state
                .inner
                .lock()
                .unwrap()
                .active()
                .manager
                .solid_scene_ref()
        )
        .unwrap(),
        scene
    );
}

#[test]
fn native_retention_preserves_sketch_undo_redo_across_repeated_eviction_and_failed_replay() {
    let state = NativeEngineHost::new();
    value(state.bind_project_session("a")).unwrap();
    value(state.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#)).unwrap();
    for request in [
        r#"{"mode":"two_point","p1":{"x":0.0,"y":0.0},"p2":{"x":10.0,"y":10.0},"ctrl_held":true}"#,
        r#"{"mode":"two_point","p1":{"x":20.0,"y":20.0},"p2":{"x":30.0,"y":30.0},"ctrl_held":true}"#,
    ] {
        value(state.engine_call("add_rectangle", request)).unwrap();
    }
    let redone = value(state.engine_call("active_sketch", "")).unwrap();
    value(state.engine_call("undo", "")).unwrap();
    let undone = value(state.engine_call("active_sketch", "")).unwrap();
    assert_ne!(redone["entities"], undone["entities"]);
    assert_eq!(undone["can_undo"], true);
    assert_eq!(undone["can_redo"], true);
    value(state.engine_call("end_sketch", "")).unwrap();
    let sketches = value(state.engine_call("finished_sketches", "")).unwrap();
    let model = value(state.engine_call("project_export_model", "")).unwrap();
    value(state.create_project_session("b")).unwrap();

    for _ in 0..2 {
        assert!(state.evict_inactive_project_session("a").unwrap());
        {
            let mut workspace = state.inner.lock().unwrap();
            let NativeProject::Cold { body_ids, .. } = workspace.sessions.get_mut("a").unwrap()
            else {
                panic!("Inactive document was not evicted");
            };
            body_ids.push(limo_cad_core::BodyId(999));
        }
        assert!(value(state.activate_project_session("a")).is_err());
        assert_eq!(state.active_project_session_id(), "b");
        {
            let mut workspace = state.inner.lock().unwrap();
            let NativeProject::Cold { body_ids, .. } = workspace.sessions.get_mut("a").unwrap()
            else {
                panic!("Failed replay consumed the retained document");
            };
            body_ids.clear();
        }
        value(state.activate_project_session("a")).unwrap();
        assert_eq!(
            value(state.engine_call("finished_sketches", "")).unwrap(),
            sketches
        );
        assert_eq!(
            value(state.engine_call("project_export_model", "")).unwrap(),
            model
        );
        value(state.engine_call("edit_sketch", r#""Sketch1""#)).unwrap();
        value(state.engine_call("redo", "")).unwrap();
        assert_eq!(
            value(state.engine_call("active_sketch", "")).unwrap()["entities"],
            redone["entities"]
        );
        value(state.engine_call("undo", "")).unwrap();
        assert_eq!(
            value(state.engine_call("active_sketch", "")).unwrap(),
            undone
        );
        value(state.engine_call("end_sketch", "")).unwrap();
        value(state.activate_project_session("b")).unwrap();
    }
}

#[test]
fn native_retention_failed_reconstruction_keeps_snapshot_and_active_document() {
    let state = NativeEngineHost::new();
    value(state.bind_project_session("a")).unwrap();
    value(state.create_project_session("b")).unwrap();
    state.evict_inactive_project_session("a").unwrap();
    let active = state.document_snapshot();
    let active_revision = state.geometry_revision();
    let original = {
        let mut workspace = state.inner.lock().unwrap();
        let NativeProject::Cold { snapshot, .. } = workspace.sessions.get_mut("a").unwrap() else {
            panic!();
        };
        let mut invalid: serde_json::Value = serde_json::from_str(snapshot.model_json()).unwrap();
        invalid["schema_version"] = json!(u32::MAX);
        let invalid =
            ProjectSnapshot::new(invalid.to_string(), snapshot.intents().to_vec()).unwrap();
        std::mem::replace(snapshot, invalid)
    };
    assert!(value(state.activate_project_session("a")).is_err());
    assert_eq!(state.active_project_session_id(), "b");
    assert_eq!(state.document_snapshot(), active);
    assert_eq!(state.geometry_revision(), active_revision);
    {
        let mut workspace = state.inner.lock().unwrap();
        let NativeProject::Cold { snapshot, .. } = workspace.sessions.get_mut("a").unwrap() else {
            panic!();
        };
        assert!(snapshot.model_json().contains(&u32::MAX.to_string()));
        *snapshot = original;
    }
    value(state.activate_project_session("a")).unwrap();
    assert_eq!(state.active_project_session_id(), "a");
}

#[test]
fn native_retention_rejects_replay_that_changes_body_identity() {
    let state = NativeEngineHost::new();
    value(state.bind_project_session("a")).unwrap();
    value(state.create_project_session("b")).unwrap();
    state.evict_inactive_project_session("a").unwrap();
    {
        let mut workspace = state.inner.lock().unwrap();
        let NativeProject::Cold { body_ids, .. } = workspace.sessions.get_mut("a").unwrap() else {
            panic!();
        };
        body_ids.push(limo_cad_core::BodyId(999));
    }
    let error = value(state.activate_project_session("a")).unwrap_err();
    assert!(error.contains("bodies or feature errors"));
    assert_eq!(state.active_project_session_id(), "b");
    assert_eq!(state.cold_project_sessions(), ["a"]);
}

#[test]
fn native_retention_rejects_mismatched_sketch_state_without_consuming_history() {
    let state = NativeEngineHost::new();
    value(state.bind_project_session("a")).unwrap();
    value(state.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#)).unwrap();
    value(state.engine_call(
        "add_rectangle",
        r#"{"mode":"two_point","p1":{"x":0.0,"y":0.0},"p2":{"x":10.0,"y":10.0},"ctrl_held":true}"#,
    ))
    .unwrap();
    value(state.engine_call("end_sketch", "")).unwrap();
    let sketches = value(state.engine_call("finished_sketches", "")).unwrap();
    value(state.create_project_session("b")).unwrap();
    assert!(state.evict_inactive_project_session("a").unwrap());
    let original = {
        let mut workspace = state.inner.lock().unwrap();
        let NativeProject::Cold { snapshot, .. } = workspace.sessions.get_mut("a").unwrap() else {
            panic!("Inactive document was not evicted");
        };
        let original = snapshot.clone();
        *snapshot = ProjectSnapshot::new(
            snapshot.model_json().replace("Sketch1", "ChangedSketch"),
            snapshot.intents().to_vec(),
        )
        .unwrap();
        original
    };
    let error = value(state.activate_project_session("a")).unwrap_err();
    assert!(error.contains("retained editing sessions"), "{error}");
    assert_eq!(state.active_project_session_id(), "b");
    {
        let mut workspace = state.inner.lock().unwrap();
        let NativeProject::Cold { snapshot, .. } = workspace.sessions.get_mut("a").unwrap() else {
            panic!("Rejected restoration consumed the retained document");
        };
        *snapshot = original;
    }
    value(state.activate_project_session("a")).unwrap();
    assert_eq!(
        value(state.engine_call("finished_sketches", "")).unwrap(),
        sketches
    );
}
