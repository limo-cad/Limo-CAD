//! Genuine repeated native body motion with independent physical event oracles.
use super::*;
use limo_cad_sketch::{
    MotionCoordinateDto, MotionDriverDto, MotionDriverId, MotionDriverLawDto, MotionStudyDto,
    SampleMotionStudyRequestDto,
};
use serde_json::{json, Value};
fn value(raw: String) -> Value {
    let reply: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        reply["ok"], true,
        "Native motion path fixture operation must succeed"
    );
    reply["value"].clone()
}
#[test]
fn detached_motion_path_preserves_physical_csv_and_rejects_stale_inputs() {
    let state = NativeEngineHost::new();
    value(state.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#));
    value(state.engine_call(
        "add_rectangle",
        r#"{"mode":"two_point","p1":{"x":0,"y":0},"p2":{"x":10,"y":6},"ctrl_held":true}"#,
    ));
    value(state.engine_call("end_sketch", ""));
    value(state.solid_extrude(r#"{"sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":3},"taper_angle_deg":0,"flip":false,"target_body_ids":[]}"#));
    let scene = state.solid_scene_snapshot();
    let body = &scene.bodies[0];
    let face = body
        .faces
        .iter()
        .find(|f| {
            f.plane
                .is_some_and(|p| p.normal[2] > 0.9 && (p.origin[2] - 3.).abs() < 1e-6)
        })
        .unwrap();
    let plane = face.plane.unwrap();
    let root = state.with_assembly(|a| a.component_structure.occurrences[0].clone());
    let moving=value(state.engine_call("assembly_create_occurrence",&json!({"component_id":root.component_id,"name":"Moving repeat","local_pose":{"translation":[0,0,5],"rotation":[0,0,0,1]}}).to_string()))["id"].as_u64().unwrap();
    let connector = json!({"body_id":body.id,"face_id":face.id,"face_key":face.key,"frame":{"origin":plane.origin,"primary_axis":plane.normal,"secondary_axis":plane.u}});
    let joint=value(state.engine_call("assembly_create_joint",&json!({"name":"Physical axial slider","kind":"slider","connector_a":connector,"connector_b":connector,"flipped":true,"linear_offset_mm":5,"advanced":{"connector_a_occurrence_id":root.id,"connector_b_occurrence_id":moving},"grounded_occurrence_id":root.id}).to_string()));
    let mut study: MotionStudyDto = serde_json::from_value(value(state.engine_call(
        "assembly_create_motion_study",
        r#"{"name":"Approach stock","duration_seconds":1}"#,
    )))
    .unwrap();
    study.drivers.push(MotionDriverDto {
        id: MotionDriverId(1),
        name: "Axial approach".into(),
        joint_id: serde_json::from_value(joint["id"].clone()).unwrap(),
        coordinate: MotionCoordinateDto::PrimaryLinear,
        law: MotionDriverLawDto::Motor {
            initial_value: 5.,
            velocity_per_second: -5.,
            acceleration_per_second2: 0.,
        },
        enabled: true,
    });
    study.next_driver_id = 2;
    value(state.engine_call(
        "assembly_update_motion_study",
        &serde_json::to_string(&study).unwrap(),
    ));
    for (t, z) in [(0., 5.), (1., 0.)] {
        let workspace = state.inner.lock().unwrap();
        let sample = workspace
            .active()
            .manager
            .sample_motion_study(SampleMotionStudyRequestDto {
                study_id: study.id,
                time_seconds: t,
            })
            .unwrap();
        assert!(sample.solution.solved);
        let poses = &sample.solution.instance_body_poses;
        assert_eq!(poses.len(), 2);
        assert!(poses.iter().all(|p| p.body_id == body.id));
        let moving = poses.iter().find(|p| p.occurrence_id.0 == moving).unwrap();
        assert!((moving.translation[2] - z).abs() < 1e-6);
        assert!(moving.translation[0].abs() < 1e-6 && moving.translation[1].abs() < 1e-6);
    }
    let before = value(state.engine_call("project_export_model", ""));
    let scene = state.solid_scene_snapshot();
    let payload =
        json!({"study_id":study.id,"sample_rate_hz":4.,"occurrence_ids":[moving]}).to_string();
    let prepared = state.prepare_motion_path_export(&payload).unwrap();
    let finished = {
        let _live = state.inner.lock().unwrap();
        prepared.export().unwrap()
    };
    state.validate_motion_path_export(&finished).unwrap();
    let csv = finished.csv();
    assert!(csv.starts_with("time_seconds,occurrence_id,component_id,x_mm,y_mm,z_mm,qx,qy,qz,qw,linear_speed_mm_s,linear_acceleration_mm_s2,angular_speed_deg_s,angular_acceleration_deg_s2\n"));
    let rows: Vec<_> = csv
        .lines()
        .skip(1)
        .map(|line| line.split(',').collect::<Vec<_>>())
        .collect();
    assert_eq!(rows.len(), 5);
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(row.len(), 14);
        assert_eq!(row[1].parse::<u64>().unwrap(), moving);
        assert_eq!(row[2].parse::<u64>().unwrap(), root.component_id.0);
        let number = |column: usize| row[column].parse::<f64>().unwrap();
        let time = index as f64 * 0.25;
        assert!((number(0) - time).abs() < 1e-9);
        assert!(number(3).abs() < 1e-6 && number(4).abs() < 1e-6);
        assert!((number(5) - (5. - 5. * time)).abs() < 1e-6);
        let quaternion_norm = (6..10).map(|column| number(column).powi(2)).sum::<f64>();
        assert!((quaternion_norm - 1.).abs() < 1e-9);
        assert!((number(10) - if index == 0 { 0. } else { 5. }).abs() < 1e-6);
        // Original finite-difference convention starts with zero previous speed.
        assert!((number(11) - if index == 1 { 20. } else { 0. }).abs() < 1e-6);
        assert!(number(12).abs() < 1e-6 && number(13).abs() < 1e-6);
    }
    let original = {
        let workspace = state.inner.lock().unwrap();
        workspace
            .active()
            .manager
            .export_motion_path_csv(serde_json::from_str(&payload).unwrap())
            .unwrap()
    };
    assert!(
        original == csv,
        "Original and detached CSV bytes must agree"
    );
    assert!(
        value(state.engine_call("assembly_export_motion_path_csv", &payload))
            .as_str()
            .unwrap()
            == csv,
        "Synchronous CSV wire value must retain exact bytes"
    );
    assert!(
        value(state.engine_call("project_export_model", "")) == before,
        "CSV sampling must preserve complete model"
    );
    assert!(Arc::ptr_eq(&scene, &state.solid_scene_snapshot()));
    assert!(NativeEngineHost::new()
        .validate_motion_path_export(&finished)
        .is_err());
    {
        let mut workspace = state.inner.lock().unwrap();
        let inner = workspace.active_mut();
        let stamps = (inner.geometry_revision, inner.viewport_revision);
        let mut changed = study.clone();
        changed.drivers[0].law = MotionDriverLawDto::Motor {
            initial_value: 8.,
            velocity_per_second: 0.,
            acceleration_per_second2: 0.,
        };
        inner.manager.update_motion_study(changed).unwrap();
        assert_eq!((inner.geometry_revision, inner.viewport_revision), stamps);
    }
    assert!(state
        .validate_motion_path_export(&finished)
        .unwrap_err()
        .contains("path inputs"));
    value(state.project_load(&serde_json::to_string(before.as_str().unwrap()).unwrap()));
    let changed_scene = state
        .prepare_motion_path_export(&payload)
        .unwrap()
        .export()
        .unwrap();
    let scene_before = state.solid_scene_snapshot();
    let assembly_before = state.with_assembly(Clone::clone);
    {
        // Actual native recompute/commit replaces the sampler scene without
        // relying on a host revision or changing authored sampler intent.
        let mut workspace = state.inner.lock().unwrap();
        let inner = workspace.active_mut();
        let stamps = (inner.geometry_revision, inner.viewport_revision);
        let plan = inner.manager.prepare_recompute().unwrap();
        let queries = inner.manager.history_support_queries();
        let transaction_id = plan.transaction_id;
        let (scene, verified) = inner
            .kernel
            .recompute_with_supports(&plan, &queries)
            .unwrap();
        inner
            .manager
            .commit_solid_with_verified_supports(
                limo_cad_solid::CommitKernelRequest {
                    transaction_id,
                    scene,
                },
                &verified,
            )
            .unwrap();
        assert_eq!((inner.geometry_revision, inner.viewport_revision), stamps);
    }
    assert!(
        state.with_assembly(|current| current == &assembly_before),
        "Scene replacement must preserve exact authored sampler inputs"
    );
    assert!(
        value(state.engine_call("project_export_model", "")) == before,
        "Native scene recompute must preserve complete authored model"
    );
    assert!(!Arc::ptr_eq(&scene_before, &state.solid_scene_snapshot()));
    assert!(state
        .validate_motion_path_export(&changed_scene)
        .unwrap_err()
        .contains("path scene"));
    value(state.project_load(&serde_json::to_string(before.as_str().unwrap()).unwrap()));
    let replaced = state
        .prepare_motion_path_export(&payload)
        .unwrap()
        .export()
        .unwrap();
    value(state.project_load(&serde_json::to_string(before.as_str().unwrap()).unwrap()));
    assert!(state.validate_motion_path_export(&replaced).is_err());
    assert!(
        value(state.engine_call("project_export_model", "")) == before,
        "Owner replacement must retain exact original model"
    );
    // Original missing-study priority precedes invalid rate.
    let bad = json!({"study_id":99999,"sample_rate_hz":0.,"occurrence_ids":[]}).to_string();
    assert!(state
        .prepare_motion_path_export(&bad)
        .unwrap()
        .export()
        .err()
        .unwrap()
        .contains("does not exist"));
}
