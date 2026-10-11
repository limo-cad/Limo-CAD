//! Independent physical overlap/clearance and private static source fences.
use super::*;
use limo_cad_sketch::{AssemblyTransformDto, SetOccurrencePoseRequestDto};
use limo_cad_solid::RecomputePlanDto;
use serde_json::{json, Value};
fn value(raw: String) -> Value {
    let reply: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        reply["ok"], true,
        "Native interference fixture operation must succeed"
    );
    reply["value"].clone()
}
fn extrude(state: &NativeEngineHost) {
    value(state.solid_extrude(r#"{"sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":3.0},"taper_angle_deg":0.0,"flip":false,"target_body_ids":[]}"#));
}
#[test]
fn detached_static_interference_preserves_repeated_poses_and_rejects_stale_sources() {
    let empty = NativeEngineHost::new();
    let empty_finished = empty
        .prepare_interference_review(r#"{"occurrence_ids":[],"clearance_threshold_mm":0}"#)
        .unwrap()
        .inspect()
        .unwrap();
    empty.validate_interference_review(&empty_finished).unwrap();
    assert!(empty_finished.report().exact && empty_finished.report().pairs.is_empty());
    let state = NativeEngineHost::new();
    value(state.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#));
    value(state.engine_call(
        "add_rectangle",
        r#"{"mode":"two_point","p1":{"x":0,"y":0},"p2":{"x":10,"y":6},"ctrl_held":true}"#,
    ));
    value(state.engine_call("end_sketch", ""));
    extrude(&state);
    let body = state.solid_scene_snapshot().bodies[0].id;
    let component = value(state.engine_call("assembly_create_component", &json!({"name":"Repeated interference stock","body_ids":[body],"absorb_promoted_bodies":true}).to_string()));
    let mut repeats = Vec::new();
    for x in [5., 17.] {
        repeats.push(value(state.engine_call("assembly_create_occurrence", &json!({"component_id":component["id"],"name":format!("At {x}"),"local_pose":{"translation":[x,0,0],"rotation":[0,0,0,1]}}).to_string()))["id"].as_u64().unwrap());
    }
    let payload = r#"{"occurrence_ids":[],"clearance_threshold_mm":2.5}"#;
    let before = value(state.engine_call("project_export_model", ""));
    let scene = state.solid_scene_snapshot();
    let prepared = state.prepare_interference_review(payload).unwrap();
    assert_eq!(prepared.source.poses.len(), 3);
    assert!(prepared
        .source
        .poses
        .iter()
        .all(|pose| pose.body_id == body));
    assert_eq!(prepared.source.identities.len(), 1);
    // Genuine exact native distance/boolean work cannot re-enter the live host.
    let finished = {
        let _live = state.inner.lock().unwrap();
        prepared.inspect().unwrap()
    };
    state.validate_interference_review(&finished).unwrap();
    let report = finished.report();
    assert!(report.exact);
    assert_eq!(report.pairs.len(), 2);
    let overlap = report.pairs.iter().find(|pair| pair.interfering).unwrap();
    assert!((overlap.overlap_volume_mm3 - 90.).abs() < 1e-6);
    assert!(overlap.minimum_clearance_mm.abs() < 1e-6);
    assert!(overlap.below_clearance);
    assert!(overlap.occurrence_a.0 == repeats[0] || overlap.occurrence_b.0 == repeats[0]);
    let clearance = report.pairs.iter().find(|pair| !pair.interfering).unwrap();
    assert!((clearance.minimum_clearance_mm - 2.).abs() < 1e-6);
    assert!(clearance.overlap_volume_mm3.abs() < 1e-6);
    assert!(clearance.below_clearance);
    assert!(clearance.occurrence_a.0 == repeats[1] || clearance.occurrence_b.0 == repeats[1]);
    let sync = value(state.assembly_interference_check(payload));
    assert!(
        sync == serde_json::to_value(report).unwrap(),
        "Detached and synchronous exact reports must agree"
    );
    assert!(
        value(state.engine_call("project_export_model", "")) == before,
        "Inspection must preserve complete model"
    );
    assert!(Arc::ptr_eq(&scene, &state.solid_scene_snapshot()));
    assert!(NativeEngineHost::new()
        .validate_interference_review(&finished)
        .is_err());

    // Actual authoritative pose change without bumping host render revisions.
    {
        let mut workspace = state.inner.lock().unwrap();
        let inner = workspace.active_mut();
        let stamps = (inner.geometry_revision, inner.viewport_revision);
        inner
            .manager
            .set_occurrence_pose(SetOccurrencePoseRequestDto {
                occurrence_id: limo_cad_sketch::OccurrenceId(repeats[0]),
                local_pose: AssemblyTransformDto {
                    translation: [6., 0., 0.],
                    rotation: [0., 0., 0., 1.],
                },
            })
            .unwrap();
        assert_eq!((inner.geometry_revision, inner.viewport_revision), stamps);
    }
    assert!(Arc::ptr_eq(&scene, &state.solid_scene_snapshot()));
    assert!(state
        .validate_interference_review(&finished)
        .unwrap_err()
        .contains("poses"));
    value(state.project_load(&serde_json::to_string(before.as_str().unwrap()).unwrap()));
    let zero_payload =
        json!({"occurrence_ids":[repeats[1]],"clearance_threshold_mm":0}).to_string();
    let zero = state
        .prepare_interference_review(&zero_payload)
        .unwrap()
        .inspect()
        .unwrap();
    assert!(zero.report().exact && zero.report().pairs.is_empty());
    state.validate_interference_review(&zero).unwrap();
    let native = state
        .prepare_interference_review(payload)
        .unwrap()
        .inspect()
        .unwrap();
    let unchanged_scene = state.solid_scene_snapshot();
    // Same serialized source and scene, but real native reset/replay successors.
    {
        let mut workspace = state.inner.lock().unwrap();
        let inner = workspace.active_mut();
        let plan = inner.manager.prepare_recompute().unwrap();
        inner.manager.cancel_solid_recompute(plan.transaction_id);
        inner
            .kernel
            .recompute(&RecomputePlanDto {
                transaction_id: 900,
                jobs: vec![],
                errors: vec![],
            })
            .unwrap();
        assert!(inner.kernel.recompute(&plan).unwrap().errors.is_empty());
    }
    assert!(
        value(state.engine_call("project_export_model", "")) == before,
        "Native replay must leave authored source unchanged"
    );
    assert!(Arc::ptr_eq(&unchanged_scene, &state.solid_scene_snapshot()));
    assert!(state
        .validate_interference_review(&native)
        .unwrap_err()
        .contains("native geometry"));
    assert!(state
        .validate_interference_review(&zero)
        .unwrap_err()
        .contains("native geometry"));
    value(state.project_load(&serde_json::to_string(before.as_str().unwrap()).unwrap()));
    let replaced = state
        .prepare_interference_review(payload)
        .unwrap()
        .inspect()
        .unwrap();
    value(state.project_load(&serde_json::to_string(before.as_str().unwrap()).unwrap()));
    assert!(state.validate_interference_review(&replaced).is_err());
    let edited = state
        .prepare_interference_review(payload)
        .unwrap()
        .inspect()
        .unwrap();
    extrude(&state);
    assert!(
        value(state.engine_call("project_export_model", "")) != before,
        "Real geometry edit must change authored model"
    );
    assert!(state.validate_interference_review(&edited).is_err());
}
