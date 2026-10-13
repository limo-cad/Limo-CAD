use super::*;
use limo_cad_occt::measurement::{MeasurementSelection, MeasurementTarget};
use serde_json::{json, Value};

fn value(raw: String) -> Value {
    let reply: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(reply["ok"], true, "measurement fixture: {reply}");
    reply["value"].clone()
}
fn stock(circle: bool) -> NativeEngineHost {
    let state = NativeEngineHost::new();
    value(state.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#));
    if circle {
        value(state.engine_call(
            "add_circle",
            r#"{"mode":"center_diameter","p1":{"x":0,"y":0},"p2":{"x":5,"y":0},"ctrl_held":true}"#,
        ));
    } else {
        value(state.engine_call(
            "add_rectangle",
            r#"{"mode":"two_point","p1":{"x":0,"y":0},"p2":{"x":10,"y":6},"ctrl_held":true}"#,
        ));
    }
    value(state.engine_call("end_sketch", ""));
    value(state.solid_extrude(r#"{"sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":3},"taper_angle_deg":0,"flip":false,"target_body_ids":[]}"#));
    state
}
fn selection(state: &NativeEngineHost, target: MeasurementTarget) -> MeasurementSelection {
    let frame = state.viewport_frame();
    let body_id = frame.document.scene.bodies[0].id;
    MeasurementSelection {
        body_id,
        target,
        occurrence_id: frame
            .instance_body_poses
            .iter()
            .find(|p| p.body_id == body_id && p.visible)
            .map(|p| p.occurrence_id.0),
    }
}
fn inspect(
    state: &NativeEngineHost,
    selections: Vec<MeasurementSelection>,
) -> FinishedMeasurementReview {
    let prepared = state
        .prepare_measurement_review(&MeasurementRequest { selections })
        .unwrap();
    // Holding the actual live lock proves the native query uses detached shapes.
    let finished = {
        let _live = state.inner.lock().unwrap();
        prepared.inspect().unwrap()
    };
    state.validate_measurement_review(&finished).unwrap();
    finished
}
fn near(actual: Option<f64>, expected: f64) {
    assert!(
        (actual.unwrap() - expected).abs() < 1e-7,
        "{actual:?} != {expected}"
    );
}

#[test]
fn measure_native_box_properties_face_distances_and_angles_are_read_only() {
    let state = stock(false);
    let scene = state.solid_scene_snapshot();
    let body = &scene.bodies[0];
    let before = value(state.engine_call("project_export_model", ""));
    let report = inspect(&state, vec![selection(&state, MeasurementTarget::Body)]);
    near(report.report().selections[0].volume, 180.);
    near(report.report().selections[0].area, 216.);
    assert_eq!(report.report().selections[0].size, Some([10., 6., 3.]));
    let top = body
        .faces
        .iter()
        .find(|f| f.plane.is_some_and(|p| p.normal[2] > 0.99))
        .unwrap();
    let bottom = body
        .faces
        .iter()
        .find(|f| f.plane.is_some_and(|p| p.normal[2] < -0.99))
        .unwrap();
    let side = body
        .faces
        .iter()
        .find(|f| f.plane.is_some_and(|p| p.normal[0].abs() > 0.99))
        .unwrap();
    let faces = |a, b| {
        vec![
            selection(&state, MeasurementTarget::Face { face_id: a }),
            selection(&state, MeasurementTarget::Face { face_id: b }),
        ]
    };
    let parallel = inspect(&state, faces(top.id, bottom.id));
    near(parallel.report().distance, 3.);
    near(parallel.report().angle_degrees, 0.);
    near(parallel.report().selections[0].area, 60.);
    near(parallel.report().selections[0].loop_length, 32.);
    let perpendicular = inspect(&state, faces(top.id, side.id));
    near(perpendicular.report().distance, 0.);
    near(perpendicular.report().angle_degrees, 90.);
    assert_eq!(value(state.engine_call("project_export_model", "")), before);
    assert!(Arc::ptr_eq(&scene, &state.solid_scene_snapshot()));
}

#[test]
fn measure_native_edge_and_vertex_use_exact_topology_without_parent_body_totals() {
    let state = stock(false);
    let scene = state.solid_scene_snapshot();
    let edge = scene.bodies[0]
        .edges
        .iter()
        .find(|e| {
            let a = e.points.first().unwrap();
            let b = e.points.last().unwrap();
            (a.x - b.x).abs() > 9.9
        })
        .unwrap();
    let report = inspect(
        &state,
        vec![selection(
            &state,
            MeasurementTarget::Edge { edge_id: edge.id },
        )],
    );
    near(report.report().selections[0].length, 10.);
    assert!(
        report.report().selections[0].volume.is_none()
            && report.report().selections[0].area.is_none()
    );
    let vertices = inspect(
        &state,
        vec![
            selection(
                &state,
                MeasurementTarget::Vertex {
                    edge_id: edge.id,
                    end: false,
                },
            ),
            selection(
                &state,
                MeasurementTarget::Vertex {
                    edge_id: edge.id,
                    end: true,
                },
            ),
        ],
    );
    near(vertices.report().distance, 10.);
    let [a, b] = vertices.report().closest_points.unwrap();
    assert!(((a[0] - b[0]).abs() - 10.).abs() < 1e-7);
    assert!(vertices
        .report()
        .selections
        .iter()
        .all(|s| s.position.is_some() && s.length.is_none()));
    let duplicate = selection(&state, MeasurementTarget::Edge { edge_id: edge.id });
    assert!(state
        .prepare_measurement_review(&MeasurementRequest {
            selections: vec![duplicate.clone(), duplicate]
        })
        .is_err());
    let mut wrong = selection(
        &state,
        MeasurementTarget::Face {
            face_id: limo_cad_core::FaceId(u64::MAX),
        },
    );
    assert!(state
        .prepare_measurement_review(&MeasurementRequest {
            selections: vec![wrong.clone()]
        })
        .is_err());
    wrong.target = MeasurementTarget::Body;
    wrong.occurrence_id = Some(u64::MAX);
    assert!(state
        .prepare_measurement_review(&MeasurementRequest {
            selections: vec![wrong]
        })
        .is_err());
}

#[test]
fn measure_native_circles_report_exact_curves_centers_and_parallel_extrema() {
    let state = stock(true);
    let scene = state.solid_scene_snapshot();
    let circles: Vec<_> = scene.bodies[0]
        .edges
        .iter()
        .filter(|e| e.circle.is_some_and(|c| c.closed))
        .collect();
    assert_eq!(circles.len(), 2);
    let radius = circles[0].circle.unwrap().radius;
    let report = inspect(
        &state,
        circles
            .iter()
            .map(|e| selection(&state, MeasurementTarget::Edge { edge_id: e.id }))
            .collect(),
    );
    near(
        report.report().selections[0].length,
        std::f64::consts::TAU * radius,
    );
    near(report.report().selections[0].radius, radius);
    near(report.report().distance, 3.);
    near(report.report().center_distance, 3.);
    near(
        report.report().maximum_distance,
        (4. * radius * radius + 9.).sqrt(),
    );
    let [a, b] = report.report().maximum_points.unwrap();
    let maximum = a
        .iter()
        .zip(b)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>()
        .sqrt();
    near(Some(maximum), report.report().maximum_distance.unwrap());
    let cap = scene.bodies[0]
        .faces
        .iter()
        .find(|f| f.plane.is_some())
        .unwrap();
    let cap_report = inspect(
        &state,
        vec![selection(
            &state,
            MeasurementTarget::Face { face_id: cap.id },
        )],
    );
    near(cap_report.report().selections[0].radius, radius);
    near(
        cap_report.report().selections[0].area,
        std::f64::consts::PI * radius * radius,
    );
    near(
        cap_report.report().selections[0].loop_length,
        std::f64::consts::TAU * radius,
    );
    assert!(cap_report.report().selections[0].center.is_some());
    let center = inspect(
        &state,
        vec![selection(
            &state,
            MeasurementTarget::Center {
                edge_id: circles[0].id,
            },
        )],
    );
    assert_eq!(
        center.report().selections[0].position,
        report.report().selections[0].center
    );
    let body = inspect(&state, vec![selection(&state, MeasurementTarget::Body)]);
    near(
        body.report().selections[0].volume,
        std::f64::consts::PI * radius * radius * 3.,
    );
}

#[test]
fn measure_native_repeated_occurrences_preserve_placements_and_reject_stale_results() {
    let state = stock(false);
    let body = state.solid_scene_snapshot().bodies[0].id;
    let component = value(
        state.engine_call(
            "assembly_create_component",
            &json!({"name":"Measured repeats","body_ids":[body],"absorb_promoted_bodies":true})
                .to_string(),
        ),
    );
    let second=value(state.engine_call("assembly_create_occurrence",&json!({"component_id":component["id"],"name":"Second","local_pose":{"translation":[17,0,0],"rotation":[0,0,0,1]}}).to_string()))["id"].as_u64().unwrap();
    let first = selection(&state, MeasurementTarget::Body);
    let mut b = first.clone();
    b.occurrence_id = Some(second);
    let before = value(state.engine_call("project_export_model", ""));
    let finished = inspect(&state, vec![first, b]);
    near(finished.report().distance, 7.);
    assert_eq!(value(state.engine_call("project_export_model", "")), before);
    assert!(NativeEngineHost::new()
        .validate_measurement_review(&finished)
        .is_err());
    // Placement authority changes even if no host render revision is bumped.
    {
        let mut workspace = state.inner.lock().unwrap();
        workspace
            .active_mut()
            .manager
            .set_occurrence_pose(limo_cad_sketch::SetOccurrencePoseRequestDto {
                occurrence_id: limo_cad_sketch::OccurrenceId(second),
                local_pose: limo_cad_sketch::AssemblyTransformDto {
                    translation: [18., 0., 0.],
                    rotation: [0., 0., 0., 1.],
                },
            })
            .unwrap();
    }
    assert!(state
        .validate_measurement_review(&finished)
        .unwrap_err()
        .contains("placement"));
    value(state.project_load(&serde_json::to_string(before.as_str().unwrap()).unwrap()));
    let current = inspect(&state, vec![selection(&state, MeasurementTarget::Body)]);
    {
        let mut workspace = state.inner.lock().unwrap();
        let inner = workspace.active_mut();
        let plan = inner.manager.prepare_recompute().unwrap();
        inner.manager.cancel_solid_recompute(plan.transaction_id);
        inner
            .kernel
            .recompute(&limo_cad_solid::RecomputePlanDto {
                transaction_id: 900,
                jobs: vec![],
                errors: vec![],
            })
            .unwrap();
        assert!(inner.kernel.recompute(&plan).unwrap().errors.is_empty());
    }
    assert!(state
        .validate_measurement_review(&current)
        .unwrap_err()
        .contains("native measurement geometry"));
}
