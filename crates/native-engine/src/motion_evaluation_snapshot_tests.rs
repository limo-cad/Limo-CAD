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
        "Native contact fixture operation must succeed"
    );
    reply["value"].clone()
}
#[test]
fn detached_contact_motion_preserves_physical_stop_and_rejects_stale_inputs() {
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
    let mut contact: limo_cad_sketch::ContactSetDto = serde_json::from_value(value(
        state.engine_call(
            "assembly_create_contact_set",
            &json!({"name":"Stock clearance", "occurrence_a":root.id,"body_a":body.id,
                "occurrence_b":moving,"body_b":body.id,"clearance_mm":0.5,"stop_motion":true})
            .to_string(),
        ),
    ))
    .unwrap();
    let before = value(state.engine_call("project_export_model", ""));
    let scene = state.solid_scene_snapshot();
    let payload = |enforce| {
        json!({"study_id":study.id,"time_seconds":1.,"previous_time_seconds":0.,"enforce_contacts":enforce}).to_string()
    };
    for enforce in [true, false] {
        let prepared = state.prepare_motion_evaluation(&payload(enforce)).unwrap();
        let finished = {
            let _live = state.inner.lock().unwrap();
            prepared.evaluate().unwrap()
        };
        state.validate_motion_evaluation(&finished).unwrap();
        let report = finished.report();
        assert!(report.sample.solution.solved);
        assert_eq!(report.contacts.pairs.len(), 1);
        let pose = report
            .sample
            .solution
            .instance_body_poses
            .iter()
            .find(|pose| pose.occurrence_id.0 == moving)
            .unwrap();
        let pair = &report.contacts.pairs[0];
        assert_eq!(pair.body_a, pair.body_b);
        assert_ne!(pair.occurrence_a, pair.occurrence_b);
        if enforce {
            // Stock top Z3 and moving bottom Z(5-5t): clearance0.5 first at t0.3.
            assert_eq!(report.stopped_by_contact, Some(contact.id));
            assert!((report.stop_time_seconds.unwrap() - 0.3).abs() < 1e-5);
            assert!((report.sample.time_seconds - 0.3).abs() < 1e-5);
            assert!((pose.translation[2] - 3.5).abs() < 1e-5);
            assert!((pair.minimum_clearance_mm - 0.5).abs() < 1e-5);
            assert!(pair.overlap_volume_mm3.abs() < 1e-6);
        } else {
            assert!(report.stopped_by_contact.is_none() && report.stop_time_seconds.is_none());
            assert!(
                (report.sample.time_seconds - 1.).abs() < 1e-9 && pose.translation[2].abs() < 1e-6
            );
            assert!((pair.overlap_volume_mm3 - 180.).abs() < 1e-6);
        }
        let original = {
            let workspace = state.inner.lock().unwrap();
            let inner = workspace.active();
            limo_cad_occt::evaluate_motion_study(
                &inner.manager,
                &inner.kernel,
                &serde_json::from_str(&payload(enforce)).unwrap(),
            )
            .unwrap()
        };
        assert!(
            serde_json::to_value(&original).unwrap() == serde_json::to_value(report).unwrap(),
            "Original and detached contact evaluators must agree"
        );
        assert!(
            value(state.engine_call("project_export_model", "")) == before,
            "Contact evaluation must preserve complete model"
        );
        assert!(Arc::ptr_eq(&scene, &state.solid_scene_snapshot()));
    }
    let finished = state
        .prepare_motion_evaluation(&payload(true))
        .unwrap()
        .evaluate()
        .unwrap();
    assert!(NativeEngineHost::new()
        .validate_motion_evaluation(&finished)
        .is_err());
    {
        let mut workspace = state.inner.lock().unwrap();
        let inner = workspace.active_mut();
        let stamps = (inner.geometry_revision, inner.viewport_revision);
        contact.enabled = false;
        inner.manager.update_contact_set(contact.clone()).unwrap();
        assert_eq!((inner.geometry_revision, inner.viewport_revision), stamps);
    }
    assert!(state
        .validate_motion_evaluation(&finished)
        .unwrap_err()
        .contains("evaluation inputs"));
    // Disabled contacts use no native shape and remain valid even when kernel shapes are absent.
    {
        let mut workspace = state.inner.lock().unwrap();
        workspace
            .active_mut()
            .kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 910,
                jobs: vec![],
                errors: vec![],
            })
            .unwrap();
    }
    let disabled = state
        .prepare_motion_evaluation(&payload(true))
        .unwrap()
        .evaluate()
        .unwrap();
    state.validate_motion_evaluation(&disabled).unwrap();
    assert!(disabled.report().contacts.exact && disabled.report().contacts.pairs.is_empty());
    assert!(disabled.report().stopped_by_contact.is_none());
    assert!((disabled.report().sample.time_seconds - 1.).abs() < 1e-9);
    value(state.project_load(&serde_json::to_string(before.as_str().unwrap()).unwrap()));
    let motion = state
        .prepare_motion_evaluation(&payload(true))
        .unwrap()
        .evaluate()
        .unwrap();
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
        .validate_motion_evaluation(&motion)
        .unwrap_err()
        .contains("evaluation inputs"));
    // A clear broadphase remains valid with absent noncandidate native shapes.
    {
        let mut workspace = state.inner.lock().unwrap();
        workspace
            .active_mut()
            .kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 911,
                jobs: vec![],
                errors: vec![],
            })
            .unwrap();
    }
    let clear = state
        .prepare_motion_evaluation(&payload(true))
        .unwrap()
        .evaluate()
        .unwrap();
    state.validate_motion_evaluation(&clear).unwrap();
    assert!(!clear.report().contacts.exact && clear.report().contacts.pairs.len() == 1);
    assert!((clear.report().contacts.pairs[0].minimum_clearance_mm - 5.).abs() < 1e-6);
    assert!(clear.report().stopped_by_contact.is_none());
    value(state.project_load(&serde_json::to_string(before.as_str().unwrap()).unwrap()));
    let native = state
        .prepare_motion_evaluation(&payload(true))
        .unwrap()
        .evaluate()
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
                transaction_id: 912,
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
        .validate_motion_evaluation(&native)
        .unwrap_err()
        .contains("native geometry"));
    value(state.project_load(&serde_json::to_string(before.as_str().unwrap()).unwrap()));
    let replaced = state
        .prepare_motion_evaluation(&payload(true))
        .unwrap()
        .evaluate()
        .unwrap();
    value(state.project_load(&serde_json::to_string(before.as_str().unwrap()).unwrap()));
    assert!(state.validate_motion_evaluation(&replaced).is_err());
}
