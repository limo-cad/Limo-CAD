//! Genuine native selected-body sections and private publication authority.
use super::*;
use limo_cad_occt::section_review::SectionOutcome;
use limo_cad_solid::{ExtrudeOperation, KernelJobDto, RecomputePlanDto};
use serde_json::{json, Value};

fn value(reply: String) -> Value {
    let reply: Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(reply["ok"], true, "Native fixture operation must succeed");
    reply["value"].clone()
}
fn stock() -> NativeEngineHost {
    let state = NativeEngineHost::new();
    value(state.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#));
    value(state.engine_call(
        "add_rectangle",
        r#"{"mode":"two_point","p1":{"x":0.0,"y":0.0},"p2":{"x":18.0,"y":12.0},"ctrl_held":true}"#,
    ));
    value(state.engine_call("end_sketch", ""));
    value(state.solid_extrude(r#"{"sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":8.0},"taper_angle_deg":0.0,"flip":false,"target_body_ids":[]}"#));
    state
}
#[test]
fn cached_section_source_supports_live_offsets_and_keeps_publication_proof() {
    let state = stock();
    let scene = state.solid_scene_snapshot();
    let body = scene.bodies[0].id;
    let before = value(state.engine_call("project_export_model", ""));
    let mut request: SectionReviewRequest = serde_json::from_value(
        json!({"body_id":body,"plane":"xy","offset_mm":2.,"include_cutaway":true}),
    )
    .unwrap();
    let source = state
        .prepare_section_review(&serde_json::to_string(&request).unwrap())
        .unwrap();
    for z in [2., 4., 6.] {
        request.offset_mm = z;
        let finished = {
            let _live = state.inner.lock().unwrap();
            source.inspect_at(&request).unwrap()
        };
        state.validate_section_review(&finished).unwrap();
        assert_eq!(finished.report().request.offset_mm, z);
        assert_eq!(finished.report().outcome, SectionOutcome::MaterialSection);
        assert!(finished.report().cutaway.is_some());
    }
    request.body_id = BodyId(body.0 + 1);
    assert!(source.inspect_at(&request).is_err());
    request.body_id = body;
    request.offset_mm = f64::NAN;
    assert!(source.inspect_at(&request).is_err());
    assert!(Arc::ptr_eq(&scene, &state.solid_scene_snapshot()));
    assert_eq!(value(state.engine_call("project_export_model", "")), before);
    request.offset_mm = 4.;
    let finished = source.inspect_at(&request).unwrap();
    value(state.project_load(&serde_json::to_string(before.as_str().unwrap()).unwrap()));
    assert!(state.validate_section_review(&finished).is_err());
    assert!(state
        .validate_section_review(&source.inspect_at(&request).unwrap())
        .is_err());
}
#[test]
fn detached_section_preserves_real_geometry_and_rejects_uncommitted_native_sources() {
    let state = stock();
    let body = state.solid_scene_snapshot().bodies[0].id;
    let payload = |z, cutaway| {
        json!({"body_id":body,"plane":"xy","offset_mm":z,"probe_mm":6.,"include_cutaway":cutaway})
            .to_string()
    };
    let before = value(state.engine_call("project_export_model", ""));
    let scene = state.solid_scene_snapshot();
    let geometry_revision = state.geometry_revision();
    for (z, outcome) in [
        (4., SectionOutcome::MaterialSection),
        (0., SectionOutcome::BoundaryContact),
        (9., SectionOutcome::NoIntersection),
    ] {
        let prepared = state.prepare_section_review(&payload(z, true)).unwrap();
        // Actual native computation while the live engine is deliberately held.
        let finished = {
            let _live = state.inner.lock().unwrap();
            prepared.inspect().unwrap()
        };
        state.validate_section_review(&finished).unwrap();
        let report = finished.report();
        assert_eq!(report.outcome, outcome);
        if outcome == SectionOutcome::MaterialSection {
            assert_eq!(report.bounds_mm.unwrap(), [0., 0., 18., 12.]);
            let area: f64 = report
                .section
                .iter()
                .map(|line| {
                    line.points
                        .windows(2)
                        .map(|pair| pair[0][0] * pair[1][1] - pair[1][0] * pair[0][1])
                        .sum::<f64>()
                        * 0.5
                })
                .sum();
            assert!((area.abs() - 216.).abs() < 1e-6);
            assert_eq!(report.probe_spans.len(), 1);
            let span = &report.probe_spans[0];
            assert!((span.start_mm - 0.).abs() < 1e-6);
            assert!((span.end_mm - 18.).abs() < 1e-6);
            assert!((span.length_mm - 18.).abs() < 1e-6);
            let mesh = report.cutaway.as_ref().unwrap();
            assert!(!mesh.indices.is_empty());
            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];
            let (points, remainder) = mesh.positions.as_chunks::<3>();
            assert!(remainder.is_empty());
            for point in points {
                for ((min, max), coordinate) in min.iter_mut().zip(&mut max).zip(point) {
                    *min = min.min(*coordinate);
                    *max = max.max(*coordinate);
                }
            }
            for (actual, expected) in min.into_iter().chain(max).zip([0., 0., 0., 18., 12., 4.]) {
                assert!((actual - expected).abs() < 1e-5);
            }
            let synchronous = value(state.section_review(&payload(z, true)));
            assert_eq!(synchronous["outcome"], "material_section");
            assert_eq!(
                synchronous["probe_spans"],
                serde_json::to_value(&report.probe_spans).unwrap()
            );
        } else if outcome == SectionOutcome::BoundaryContact {
            assert_eq!(report.bounds_mm.unwrap(), [0., 0., 18., 12.]);
            assert!(!report.section.is_empty());
            let perimeter: f64 = report
                .section
                .iter()
                .flat_map(|line| line.points.windows(2))
                .map(|pair| (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]))
                .sum();
            assert!((perimeter - 60.).abs() < 1e-6);
            assert!(report.probe_spans.is_empty());
            assert!(report.cutaway.is_none());
            assert!(report.svg.contains("Boundary contact"));
        } else {
            assert!(report.section.is_empty());
            assert!(report.bounds_mm.is_none());
            assert!(report.probe_spans.is_empty());
            assert!(report.cutaway.is_none());
            assert!(report.svg.is_empty());
        }
    }
    assert!(
        value(state.engine_call("project_export_model", "")) == before,
        "Section fixture must preserve the complete authored model"
    );
    assert!(Arc::ptr_eq(&scene, &state.solid_scene_snapshot()));
    assert_eq!(state.geometry_revision(), geometry_revision);

    let finished = state
        .prepare_section_review(&payload(4., false))
        .unwrap()
        .inspect()
        .unwrap();
    assert!(NativeEngineHost::new()
        .validate_section_review(&finished)
        .unwrap_err()
        .contains("replaced"));
    let plan = {
        let mut workspace = state.inner.lock().unwrap();
        let inner = workspace.active_mut();
        let plan = inner.manager.prepare_recompute().unwrap();
        inner.manager.cancel_solid_recompute(plan.transaction_id);
        plan
    };
    // Same authored model and scene, but genuine native reset/replay rebirth.
    {
        let mut workspace = state.inner.lock().unwrap();
        let inner = workspace.active_mut();
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
        "Section fixture must preserve the complete authored model"
    );
    assert!(Arc::ptr_eq(&scene, &state.solid_scene_snapshot()));
    assert!(state
        .validate_section_review(&finished)
        .unwrap_err()
        .contains("native geometry"));

    // Restore via the real supported load, then exercise a partial native cut.
    value(state.project_load(&serde_json::to_string(before.as_str().unwrap()).unwrap()));
    let partial = state
        .prepare_section_review(&payload(4., false))
        .unwrap()
        .inspect()
        .unwrap();
    {
        let mut workspace = state.inner.lock().unwrap();
        let inner = workspace.active_mut();
        let mut failed = inner.manager.prepare_recompute().unwrap();
        inner.manager.cancel_solid_recompute(failed.transaction_id);
        let KernelJobDto::Extrude(mut cut) = failed.jobs[0].clone() else {
            panic!("stock must have a real extrude");
        };
        cut.feature_id = limo_cad_core::FeatureId(999);
        cut.operation = ExtrudeOperation::Cut;
        cut.target_body_ids = vec![body, BodyId(9999)];
        cut.result_body_ids.clear();
        cut.end_offset = 2.;
        failed.jobs.push(KernelJobDto::Extrude(cut));
        let failed_scene = inner.kernel.recompute(&failed).unwrap();
        assert!(!failed_scene.errors.is_empty());
        let changed = failed_scene
            .bodies
            .iter()
            .find(|candidate| candidate.body_id == body)
            .unwrap();
        let minimum_z = changed
            .positions
            .as_chunks::<3>()
            .0
            .iter()
            .map(|point| point[2])
            .fold(f32::INFINITY, f32::min);
        assert!(
            (minimum_z - 2.).abs() < 1e-5,
            "first target must actually change before missing second target fails"
        );
    }
    assert!(
        value(state.engine_call("project_export_model", "")) == before,
        "Section fixture must preserve the complete authored model"
    );
    assert!(state
        .validate_section_review(&partial)
        .unwrap_err()
        .contains("native geometry"));
    value(state.project_load(&serde_json::to_string(before.as_str().unwrap()).unwrap()));
    let replacement = state
        .prepare_section_review(&payload(4., false))
        .unwrap()
        .inspect()
        .unwrap();
    value(state.project_load(&serde_json::to_string(before.as_str().unwrap()).unwrap()));
    assert!(
        value(state.engine_call("project_export_model", "")) == before,
        "Section fixture must preserve the complete authored model"
    );
    assert!(state.validate_section_review(&replacement).is_err());
}
