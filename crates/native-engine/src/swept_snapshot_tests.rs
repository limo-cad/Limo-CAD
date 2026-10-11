//! Genuine repeated native body motion with independent physical event oracles.
use super::*;
use limo_cad_sketch::{
    MotionCoordinateDto, MotionDriverDto, MotionDriverId, MotionDriverLawDto, MotionStudyDto,
    SampleMotionStudyRequestDto,
};
use limo_cad_solid::RecomputePlanDto;
use serde_json::{json, Value};
fn value(raw: String) -> Value {
    let reply: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        reply["ok"], true,
        "Native swept fixture operation must succeed"
    );
    reply["value"].clone()
}
#[test]
fn detached_swept_motion_preserves_physical_events_and_rejects_stale_inputs() {
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
    let payload = |stop| {
        json!({"study_id":study.id,"sample_rate_hz":4,"clearance_threshold_mm":0.5,"stop_at_first":stop}).to_string()
    };
    for stop in [false, true] {
        let prepared = state
            .prepare_swept_collision_review(&payload(stop))
            .unwrap();
        let finished = {
            let _live = state.inner.lock().unwrap();
            prepared.inspect().unwrap()
        };
        state.validate_swept_collision_review(&finished).unwrap();
        let report = finished.report();
        assert!(report.exact);
        assert_eq!(report.sample_count, if stop { 3 } else { 5 });
        assert_eq!(report.events.len(), 1);
        let event = &report.events[0];
        assert_eq!(event.body_a, event.body_b);
        assert_ne!(event.occurrence_a, event.occurrence_b);
        assert!((event.first_time_seconds - 0.5).abs() < 1e-9);
        assert!((event.last_time_seconds - if stop { 0.5 } else { 1. }).abs() < 1e-9);
        assert!(event.minimum_clearance_mm.abs() < 1e-6);
        assert!((event.maximum_overlap_volume_mm3 - if stop { 30. } else { 180. }).abs() < 1e-6);
        assert!(
            value(state.assembly_swept_collision_check(&payload(stop)))
                == serde_json::to_value(report).unwrap(),
            "Synchronous and detached swept report must agree"
        );
        assert!(
            value(state.engine_call("project_export_model", "")) == before,
            "Swept computation must preserve complete model"
        );
        assert!(Arc::ptr_eq(&scene, &state.solid_scene_snapshot()));
    }
    let finished = state
        .prepare_swept_collision_review(&payload(false))
        .unwrap()
        .inspect()
        .unwrap();
    assert!(NativeEngineHost::new()
        .validate_swept_collision_review(&finished)
        .is_err());
    // Actual authored motion input mutation with unchanged render revisions.
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
        .validate_swept_collision_review(&finished)
        .unwrap_err()
        .contains("motion inputs"));
    // Missing noncandidate native shapes must not become a new admission gate.
    {
        let mut workspace = state.inner.lock().unwrap();
        workspace
            .active_mut()
            .kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 900,
                jobs: vec![],
                errors: vec![],
            })
            .unwrap();
    }
    let missing = state
        .prepare_swept_collision_review(&payload(false))
        .unwrap()
        .inspect()
        .unwrap();
    state.validate_swept_collision_review(&missing).unwrap();
    assert!(missing.report().exact && missing.report().events.is_empty());
    value(state.project_load(&serde_json::to_string(before.as_str().unwrap()).unwrap()));
    let native = state
        .prepare_swept_collision_review(&payload(false))
        .unwrap()
        .inspect()
        .unwrap();
    let scene = state.solid_scene_snapshot();
    {
        let mut workspace = state.inner.lock().unwrap();
        let inner = workspace.active_mut();
        let plan = inner.manager.prepare_recompute().unwrap();
        inner.manager.cancel_solid_recompute(plan.transaction_id);
        inner
            .kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 901,
                jobs: vec![],
                errors: vec![],
            })
            .unwrap();
        assert!(inner.kernel.recompute(&plan).unwrap().errors.is_empty());
    }
    assert!(
        value(state.engine_call("project_export_model", "")) == before,
        "Native replay must preserve complete authored model"
    );
    assert!(Arc::ptr_eq(&scene, &state.solid_scene_snapshot()));
    assert!(state
        .validate_swept_collision_review(&native)
        .unwrap_err()
        .contains("native geometry"));
    value(state.project_load(&serde_json::to_string(before.as_str().unwrap()).unwrap()));
    let replaced = state
        .prepare_swept_collision_review(&payload(false))
        .unwrap()
        .inspect()
        .unwrap();
    value(state.project_load(&serde_json::to_string(before.as_str().unwrap()).unwrap()));
    assert!(state.validate_swept_collision_review(&replaced).is_err());
}
