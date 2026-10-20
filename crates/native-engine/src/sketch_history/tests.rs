use super::*;

fn call(host: &NativeEngineHost, method: &str, arguments: Value) -> Value {
    value(host.engine_call(method, &arguments.to_string())).unwrap()
}

fn sketch(host: &NativeEngineHost, x: f64) {
    call(
        host,
        "begin_sketch",
        json!({"type":"origin_plane","plane":"xy"}),
    );
    call(
        host,
        "add_rectangle",
        json!({
            "mode":"two_point", "p1":{"x":x,"y":2.}, "p2":{"x":x+12.,"y":10.},
            "ctrl_held":true
        }),
    );
    value(host.engine_call("end_sketch", "")).unwrap();
}

fn feature(host: &NativeEngineHost, revolve: bool, sketch_name: &str) {
    let response = if revolve {
        host.solid_revolve(
            &json!({
                "sketch_name":sketch_name,"profile_indices":[0],"operation":"new_body",
                "axis_origin":{"x":0.,"y":0.},"axis_direction":{"x":0.,"y":1.},
                "angle_deg":180.,"flip":false
            })
            .to_string(),
        )
    } else {
        host.solid_extrude(
            &json!({
                "sketch_name":sketch_name,"profile_indices":[0],"operation":"new_body",
                "extent":{"type":"distance","distance":8.}
            })
            .to_string(),
        )
    };
    value(response).unwrap();
    assert!(host.solid_scene_snapshot().errors.is_empty());
}

#[test]
fn sketch_history_view_hides_extrude_and_revolve_outputs_and_restores_the_complete_model() {
    for revolve in [false, true] {
        let host = NativeEngineHost::new();
        sketch(&host, 4.);
        feature(&host, revolve, "Sketch1");
        sketch(&host, 30.);
        feature(&host, false, "Sketch2");
        call(
            &host,
            "datum_plane_create",
            json!({"name":"Later plane","source":{
                "type":"offset","reference":{"type":"origin_plane","plane":"xy"},"distance":25.
            }}),
        );
        let original = value(host.engine_call("project_export_model", "")).unwrap();
        let scene = host.solid_scene_snapshot();
        let position = host.document_history_position();
        call(&host, "edit_sketch", json!("Sketch1"));
        let frame = host.viewport_frame();
        assert!(frame.document.scene.bodies.is_empty());
        assert!(frame.document.finished_sketches.is_empty());
        assert!(frame.document.datum_planes.is_empty());
        assert_eq!(
            frame.document.active_sketch.as_ref().unwrap().name,
            "Sketch1"
        );
        assert!(frame
            .document
            .profile_catalog
            .iter()
            .all(|p| p.sketch_name == "Sketch1"));
        assert_eq!(host.document_history_position(), position);
        assert!(Arc::ptr_eq(&scene, &host.solid_scene_snapshot()));
        assert!(Arc::ptr_eq(
            &frame.document,
            &host.viewport_frame().document
        ));
        call(
            &host,
            "move_point",
            json!({"point_id":1,"to_raw":{"x":5.,"y":3.},"ctrl_held":true}),
        );
        let changed = host.viewport_frame();
        assert!(Arc::ptr_eq(&frame.document.scene, &changed.document.scene));
        assert_ne!(
            frame.document.active_sketch.as_ref().unwrap().entities,
            changed.document.active_sketch.as_ref().unwrap().entities
        );
        value(host.engine_call("undo", "")).unwrap();
        value(host.engine_call("end_sketch", "")).unwrap();
        value(host.solid_recompute()).unwrap();
        assert_eq!(
            value(host.engine_call("project_export_model", "")).unwrap(),
            original
        );
        assert_eq!(host.viewport_frame().document.scene.bodies.len(), 2);
        assert_eq!(host.viewport_frame().document.finished_sketches.len(), 2);
        assert_eq!(host.viewport_frame().document.datum_planes.len(), 1);
    }
}

#[test]
fn sketch_history_view_keeps_the_exact_upstream_geometry_and_retires_on_replacement() {
    let host = NativeEngineHost::new();
    sketch(&host, 4.);
    feature(&host, false, "Sketch1");
    let upstream = host.viewport_frame().document.scene.clone();
    sketch(&host, 30.);
    feature(&host, false, "Sketch2");
    // A downstream writer changes the existing body's geometry without changing
    // its ID. Hiding only newly created body IDs would leave the wrong shape.
    let source_body = upstream.bodies[0].id;
    value(
        host.solid_body_feature(
            &json!({"type":"move_copy","request":{"body_ids":[source_body],
                "translation":{"x":0.,"y":0.,"z":20.},
                "pivot":{"x":0.,"y":0.,"z":0.},"copy":false}
            })
            .to_string(),
        ),
    )
    .unwrap();
    assert_ne!(
        host.solid_scene_snapshot().bodies[0].mesh.positions,
        upstream.bodies[0].mesh.positions
    );
    call(&host, "edit_sketch", json!("Sketch2"));
    let frame = host.viewport_frame();
    assert_eq!(frame.document.scene.bodies, upstream.bodies);
    assert_eq!(frame.document.finished_sketches.len(), 1);
    assert_eq!(frame.document.finished_sketches[0].name, "Sketch1");
    assert!(frame
        .document
        .body_geometry_key(&frame.session_id, source_body)
        .is_some());
    let rejected: Value =
        serde_json::from_str(&host.engine_call("edit_sketch", r#""Missing""#)).unwrap();
    assert_eq!(rejected["ok"], false);
    assert!(Arc::ptr_eq(
        &frame.document.scene,
        &host.viewport_frame().document.scene
    ));
    value(host.project_new()).unwrap();
    let replacement = host.viewport_frame();
    assert!(replacement.document.scene.bodies.is_empty());
    assert!(replacement.document.active_sketch.is_none());
    assert!(replacement.document.finished_sketches.is_empty());
}

#[test]
fn sketch_history_placement_tracks_live_visibility_and_assembly_without_replacing_geometry() {
    for hide_occurrence in [false, true] {
        let host = NativeEngineHost::new();
        sketch(&host, 4.);
        feature(&host, false, "Sketch1");
        let upstream = host.solid_scene_snapshot();
        let body = upstream.bodies[0].id;
        let component = call(
            &host,
            "assembly_create_component",
            json!({"name":"Part","body_ids":[body],"absorb_promoted_bodies":true}),
        );
        let occurrence = call(
            &host,
            "assembly_create_occurrence",
            json!({
                "component_id":component["id"],"name":"Repeat",
                "local_pose":{"translation":[20.,0.,0.],"rotation":[0.,0.,0.,1.]}
            }),
        )["id"]
            .as_u64()
            .unwrap();
        sketch(&host, 30.);
        feature(&host, false, "Sketch2");
        value(
            host.solid_body_feature(
                &json!({"type":"move_copy","request":{"body_ids":[body],
                    "translation":{"x":0.,"y":0.,"z":20.},
                    "pivot":{"x":0.,"y":0.,"z":0.},"copy":false}
                })
                .to_string(),
            ),
        )
        .unwrap();
        let live_scene = host.solid_scene_snapshot();
        assert_ne!(
            live_scene.bodies[0].mesh.positions,
            upstream.bodies[0].mesh.positions
        );
        if hide_occurrence {
            call(
                &host,
                "assembly_update_occurrence",
                json!({"occurrence":{"id":occurrence,"visible":false}}),
            );
        } else {
            call(
                &host,
                "project_set_visibility",
                json!({"hidden_body_ids":[body]}),
            );
        }
        call(&host, "edit_sketch", json!("Sketch2"));
        let hidden = host.viewport_frame();
        assert_eq!(hidden.document.scene.bodies, upstream.bodies);
        let previous_poses = hidden.instance_body_poses.as_ref().clone();
        let hidden_repeat = previous_poses
            .iter()
            .find(|pose| pose.occurrence_id.0 == occurrence)
            .unwrap();
        assert!(!hidden_repeat.visible);
        assert_eq!(hidden_repeat.translation, [20., 0., 0.]);
        if hide_occurrence {
            call(
                &host,
                "assembly_update_occurrence",
                json!({"occurrence":{"id":occurrence,"visible":true}}),
            );
        } else {
            call(&host, "project_set_visibility", json!({}));
        }
        let shown = host.viewport_frame();
        assert!(shown
            .instance_body_poses
            .iter()
            .filter(|pose| pose.body_id == body)
            .all(|pose| pose.visible));
        assert!(Arc::ptr_eq(&hidden.document, &shown.document));
        assert!(!Arc::ptr_eq(
            &hidden.instance_body_poses,
            &shown.instance_body_poses
        ));
        call(
            &host,
            "assembly_set_occurrence_pose",
            json!({
                "occurrence_id":occurrence,
                "local_pose":{"translation":[50.,7.,9.],"rotation":[0.,0.,0.,1.]}
            }),
        );
        let moved = host.viewport_frame();
        let moved_repeat = moved
            .instance_body_poses
            .iter()
            .find(|pose| pose.occurrence_id.0 == occurrence)
            .unwrap();
        assert!(moved_repeat.visible);
        assert_eq!(moved_repeat.translation, [50., 7., 9.]);
        assert!(Arc::ptr_eq(&hidden.document, &moved.document));
        assert!(Arc::ptr_eq(&hidden.document.scene, &moved.document.scene));
        assert!(Arc::ptr_eq(&live_scene, &host.solid_scene_snapshot()));
        assert_eq!(hidden.instance_body_poses.as_ref(), &previous_poses);
        assert!(Arc::ptr_eq(
            &moved.instance_body_poses,
            &host.viewport_frame().instance_body_poses
        ));
        value(host.engine_call("end_sketch", "")).unwrap();
        assert!(Arc::ptr_eq(
            &live_scene,
            &host.viewport_frame().document.scene
        ));
    }
}
