use super::*;
use limo_cad_export::test_reader::{read_package, ModelMesh};

fn value(raw: String) -> serde_json::Value {
    let reply: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(reply["ok"], true, "{reply}");
    reply["value"].clone()
}

fn fixture() -> (NativeEngineHost, serde_json::Value) {
    let host = NativeEngineHost::new();
    value(host.bind_project_session("print-model"));
    for i in 1..=2 {
        value(host.engine_call("begin_sketch", r#"{"type":"origin_plane","plane":"xy"}"#));
        value(host.engine_call(
            "add_rectangle",
            r#"{"mode":"two_point","p1":{"x":0,"y":0},"p2":{"x":10,"y":6},"ctrl_held":false}"#,
        ));
        value(host.engine_call("end_sketch", ""));
        value(
            host.solid_extrude(
                &serde_json::json!({
                    "sketch_name":format!("Sketch{i}"),"profile_indices":[0],"operation":"new_body",
                    "extent":{"type":"distance","distance":3},"taper_angle_deg":0,
                    "flip":false,"target_body_ids":[]
                })
                .to_string(),
            ),
        );
    }
    let bodies: Vec<_> = host
        .viewport_snapshot()
        .2
        .bodies
        .iter()
        .map(|b| b.id.0)
        .collect();
    let component = value(
        host.engine_call(
            "assembly_create_component",
            &serde_json::json!({
                "name":"Repeated pair","body_ids":[bodies[0]],"absorb_promoted_bodies":true
            })
            .to_string(),
        ),
    );
    let assembly = value(host.engine_call("assembly_document", ""));
    let root = assembly["component_structure"]["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["component_id"] == component["id"])
        .unwrap()["id"]
        .clone();
    value(host.engine_call("assembly_create_occurrence", &serde_json::json!({
        "component_id":component["id"],"name":"Intentional nested repeat",
        "parent_occurrence_id":root,"local_pose":{"translation":[20,0,0],"rotation":[0,0,0,1]}
    }).to_string()));
    let single = assembly["component_structure"]["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["component_id"] != component["id"])
        .unwrap()["id"]
        .clone();
    let q = std::f64::consts::FRAC_1_SQRT_2;
    let view = serde_json::json!({
        "name":"Print pair","camera":{"position":[100,-100,100],"target":[0,0,0],"up":[0,0,1]},
        "visible_body_ids":bodies,"part_offsets":[],"print_layout":true,
        "occurrence_offsets":[
            {"occurrence_id":root,"translation":[30,30,0],"rotation":[0,0,q,q]},
            {"occurrence_id":single,"translation":[70,30,0],"rotation":[0,0,0,1]}
        ]
    });
    value(host.engine_call("upsert_named_view", &view.to_string()));
    (host, view)
}

fn quantities(meshes: &[ModelMesh]) -> Vec<usize> {
    let mut groups = std::collections::BTreeMap::<usize, usize>::new();
    for mesh in meshes {
        *groups.entry(mesh.build_item).or_default() += 1;
    }
    let mut quantities: Vec<_> = groups.into_values().collect();
    quantities.sort_unstable();
    quantities
}

#[test]
fn native_named_layout_export_preserves_repeats_and_nested_poses() {
    let (host, _) = fixture();
    let source = value(host.engine_call("project_export_model", ""));
    let assembled = value(host.engine_call("assembly_solution", ""));
    let recalled = value(host.engine_call("recall_named_view", r#"{"name":"Print pair"}"#));
    assert_eq!(
        recalled["solution"],
        value(host.engine_call("named_view_solution", r#"{"name":"Print pair"}"#))
    );
    assert_ne!(recalled["solution"], assembled);
    let snapshot = host.viewport_snapshot();
    assert_eq!(
        serde_json::to_value(snapshot.9).unwrap(),
        recalled["solution"]["instance_body_poses"]
    );
    let bytes = host.export_3mf(r#"{"named_view":"Print pair"}"#).unwrap();
    use base64::Engine as _;
    let encoded = value(host.engine_call("solid_export_3mf", r#"{"named_view":"Print pair"}"#));
    assert_eq!(encoded["format"], "3mf");
    assert_eq!(encoded["byte_length"], bytes.len());
    assert_eq!(
        base64::engine::general_purpose::STANDARD
            .decode(encoded["bytes_base64"].as_str().unwrap())
            .unwrap(),
        bytes
    );
    let meshes = read_package(&bytes).unwrap();
    assert_eq!(meshes.len(), 3);
    assert_eq!(quantities(&meshes), [1, 2]);
    let body_id = host.viewport_snapshot().2.bodies[0].id.0;
    let selected = read_package(
        &host
            .export_3mf(
                &serde_json::json!({
                    "named_view":"Print pair","body_ids":[body_id]
                })
                .to_string(),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        selected.len(),
        2,
        "Body selection must preserve intentional repeats"
    );
    assert_eq!(quantities(&selected), [2]);
    let selected_report = value(
        host.engine_call(
            "print_layout_check",
            &serde_json::json!({
                "name":"Print pair","body_ids":[body_id]
            })
            .to_string(),
        ),
    );
    assert_eq!(selected_report["printable_instances"], 2);
    assert_eq!(selected_report["printable_groups"], 1);
    let definition = read_package(
        &host
            .export_3mf(
                &serde_json::json!({
                    "scope":"definition","body_ids":[body_id]
                })
                .to_string(),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(definition.len(), 1);
    assert!(host
        .export_3mf(r#"{"scope":"definition","named_view":"Print pair"}"#)
        .unwrap_err()
        .contains("assembly scope"));
    assert!(definition[0]
        .vertices
        .iter()
        .all(|p| p[0] >= 0. && p[0] <= 10. && p[1] >= 0. && p[1] <= 6.));
    let expected: limo_cad_sketch::AssemblySolutionDto =
        serde_json::from_value(recalled["solution"].clone()).unwrap();
    for (mesh, pose) in meshes.iter().zip(expected.instance_body_poses.iter()) {
        let world_min: [f64; 3] = std::array::from_fn(|axis| {
            mesh.vertices
                .iter()
                .map(|v| v[axis])
                .fold(f64::INFINITY, f64::min)
        });

        let source_corners = [0., 10.].into_iter().flat_map(|x| {
            [0., 6.]
                .into_iter()
                .flat_map(move |y| [0., 3.].into_iter().map(move |z| [x, y, z]))
        });
        let rotation = pose.rotation;
        let transform = limo_cad_sketch::AssemblyTransformDto {
            translation: pose.translation,
            rotation,
        };
        let corners: Vec<_> = source_corners
            .map(|p| transform.transform_point(p))
            .collect();
        let expected_min: [f64; 3] = std::array::from_fn(|axis| {
            corners
                .iter()
                .map(|p| p[axis])
                .fold(f64::INFINITY, f64::min)
        });
        assert!((0..3).all(|a| (world_min[a] - expected_min[a]).abs() < 0.001));
    }
    if let Ok(path) = std::env::var("LIMO_CAD_3MF_FIXTURE") {
        std::fs::write(path, &bytes).unwrap();
    }
    let visibility = value(host.engine_call("project_visibility", ""));
    let mut hidden = visibility.clone();
    hidden["hidden_body_ids"] = serde_json::json!([body_id]);
    value(host.engine_call("project_set_visibility", &hidden.to_string()));
    let displayed = host.viewport_snapshot().9;
    assert_eq!(displayed.iter().filter(|p| p.visible).count(), 1);
    assert_eq!(
        value(host.engine_call("named_views", ""))["active"],
        "Print pair"
    );
    let current = read_package(&host.export_3mf("{}").unwrap()).unwrap();
    assert_eq!(
        current.len(),
        1,
        "Default export must match live displayed visibility"
    );
    let current_report = value(host.engine_call("print_layout_check", "{}"));
    assert_eq!(current_report["printable_instances"], 1);
    let current_preflight = value(host.engine_call("solid_export_preflight", "{}"));
    assert_eq!(current_preflight["ok"], true);
    assert_eq!(current_preflight["layout"]["printable_instances"], 1);
    assert_eq!(current_preflight["layout"]["bed"], current_report["bed"]);
    let saved_preflight =
        value(host.engine_call("solid_export_preflight", r#"{"named_view":"Print pair"}"#));
    assert_eq!(saved_preflight["layout"]["printable_instances"], 3);
    let stale = serde_json::json!({"expected_model_json":source});
    let rejected: serde_json::Value =
        serde_json::from_str(&host.engine_call("solid_export_3mf", &stale.to_string())).unwrap();
    assert_eq!(
        rejected["ok"], false,
        "Owning encoded exports must retain the snapshot fence"
    );
    let stale_preflight: serde_json::Value =
        serde_json::from_str(&host.engine_call("solid_export_preflight", &stale.to_string()))
            .unwrap();
    assert_eq!(stale_preflight["ok"], false);
    assert_eq!(
        host.export_3mf(r#"{"named_view":"Print pair"}"#).unwrap(),
        bytes,
        "Live eye toggles must not rewrite saved layout visibility"
    );
    value(host.engine_call("project_set_visibility", &visibility.to_string()));
    assert_eq!(
        host.export_3mf("{}").unwrap(),
        bytes,
        "Current export must retain active layout poses"
    );
    let assembled_bytes = host.export_3mf(r#"{"named_view":""}"#).unwrap();
    assert_ne!(
        assembled_bytes, bytes,
        "Explicit assembled export must bypass active layout placement"
    );
    assert_eq!(
        value(host.engine_call("named_views", ""))["active"],
        "Print pair"
    );
    let reloaded = NativeEngineHost::new();
    value(reloaded.project_load(&source.to_string()));
    assert!(value(reloaded.engine_call("named_views", ""))["active"].is_null());
    assert_eq!(
        reloaded
            .export_3mf(r#"{"named_view":"Print pair"}"#)
            .unwrap(),
        bytes
    );

    value(host.create_project_session("other"));
    assert!(host.evict_inactive_project_session("print-model").unwrap());
    value(host.activate_project_session("print-model"));
    assert_eq!(
        value(host.engine_call("named_views", ""))["active"],
        "Print pair"
    );
    value(host.engine_call("edit_sketch", r#""Sketch1""#));
    assert!(value(host.engine_call("named_views", ""))["active"].is_null());
    assert_eq!(value(host.engine_call("assembly_solution", "")), assembled);
}

#[test]
fn native_layout_checks_are_read_only_and_corrections_are_atomic() {
    let (host, mut view) = fixture();
    view["occurrence_offsets"][0]["translation"] = serde_json::json!([-30, -30, -5]);
    value(host.engine_call("upsert_named_view", &view.to_string()));
    let before = value(host.engine_call("project_export_model", ""));
    let report = value(host.engine_call("print_layout_check", r#"{"name":"Print pair"}"#));
    assert!(report["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["code"] == "below_bed"));
    assert_eq!(value(host.engine_call("project_export_model", "")), before);
    assert_eq!(
        value(host.engine_call("named_view_resolve", &view.to_string())),
        value(host.engine_call("named_view_solution", r#"{"name":"Print pair"}"#))
    );

    assert_eq!(
        read_package(&host.export_3mf(r#"{"named_view":"Print pair"}"#).unwrap())
            .unwrap()
            .len(),
        3
    );
    assert_eq!(report["proposal_fits"], true);
    for correction in report["proposed_translations"].as_array().unwrap() {
        let offset = view["occurrence_offsets"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|o| o["occurrence_id"] == correction["occurrence_id"])
            .unwrap();
        for a in 0..3 {
            offset["translation"][a] = serde_json::json!(
                offset["translation"][a].as_f64().unwrap()
                    + correction["translation"][a].as_f64().unwrap()
            );
        }
    }
    let bad =
        serde_json::json!({"views":[view.clone()],"expected_model_json":"stale owner snapshot"});
    let rejected: serde_json::Value =
        serde_json::from_str(&host.engine_call("set_named_views", &bad.to_string())).unwrap();
    assert_eq!(rejected["ok"], false);
    assert_eq!(value(host.engine_call("project_export_model", "")), before);
    value(host.engine_call(
        "set_named_views",
        &serde_json::json!({"views":[view],"expected_model_json":before}).to_string(),
    ));
    let corrected = value(host.engine_call("print_layout_check", r#"{"name":"Print pair"}"#));
    assert!(!corrected["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|i| i["code"] == "below_bed" || i["code"] == "outside_bed"));
}

#[test]
fn later_broken_assembly_clears_active_layout_and_refuses_export_without_panicking() {
    let (host, _) = fixture();
    value(host.engine_call("recall_named_view", r#"{"name":"Print pair"}"#));
    let bodies: Vec<_> = host
        .viewport_snapshot()
        .2
        .bodies
        .iter()
        .map(|body| body.id.0)
        .collect();
    let mut assembly = value(host.engine_call("assembly_document", ""));
    let connector = |body| {
        serde_json::json!({
            "body_id":body,"face_id":999999,"face_key":"removed topology","kind":"planar_face",
            "frame":{"origin":[0,0,0],"primary_axis":[0,0,1],"secondary_axis":[1,0,0]}
        })
    };
    assembly["joints"] = serde_json::json!([{"id":1,"name":"Obsolete topology", "kind":"rigid",
        "connector_a":connector(bodies[0]),"connector_b":connector(bodies[1])}]);
    assembly["next_joint_id"] = serde_json::json!(2);
    value(host.engine_call("assembly_set_document", &assembly.to_string()));
    assert!(value(host.engine_call("named_views", ""))["active"].is_null());
    assert_eq!(
        value(host.engine_call("assembly_solution", ""))["solved"],
        false
    );
    let _ = host.viewport_snapshot();
    assert!(host
        .export_3mf("{}")
        .unwrap_err()
        .contains("Resolve assembly errors"));
    let preflight: serde_json::Value =
        serde_json::from_str(&host.engine_call("solid_export_preflight", "{}")).unwrap();
    assert_eq!(preflight["ok"], false);
}

#[test]
fn preflight_rejects_empty_display_and_checks_empty_document_snapshot() {
    let empty = NativeEngineHost::new();
    assert_eq!(
        value(empty.engine_call("solid_export_preflight", "{}"))["ok"],
        false
    );
    let stale: serde_json::Value = serde_json::from_str(&empty.engine_call(
        "solid_export_preflight",
        r#"{"expected_model_json":"stale"}"#,
    ))
    .unwrap();
    assert_eq!(stale["ok"], false);
    let (host, _) = fixture();
    let mut visibility = value(host.engine_call("project_visibility", ""));
    visibility["hidden_body_ids"] = serde_json::json!(host
        .viewport_snapshot()
        .2
        .bodies
        .iter()
        .map(|b| b.id.0)
        .collect::<Vec<_>>());
    value(host.engine_call("project_set_visibility", &visibility.to_string()));
    let preflight = value(host.engine_call("solid_export_preflight", "{}"));
    assert_eq!(preflight["ok"], false);
    assert_eq!(preflight["layout"]["printable_instances"], 0);
    assert!(host.export_3mf("{}").is_err());
}

#[test]
fn native_prusa_adapter_keeps_flat_objects_and_matching_material_config_ids() {
    use limo_cad_export::test_reader::{read_package_object_ids, read_package_text};
    let (host, _) = fixture();
    let portable = host.export_3mf(r#"{"named_view":"Print pair"}"#).unwrap();
    for target in ["bambu_studio", "orca_slicer"] {
        let bytes = host
            .export_3mf(
                &serde_json::json!({"named_view":"Print pair","slicer_target":target}).to_string(),
            )
            .unwrap();
        assert_eq!(
            quantities(&read_package(&bytes).unwrap()),
            [1, 2],
            "{target} must preserve the CAD hierarchy"
        );
        assert_eq!(bytes, portable);
    }
    let prusa = host
        .export_3mf(r#"{"named_view":"Print pair","slicer_target":"prusa_slicer"}"#)
        .unwrap();
    let meshes = read_package(&prusa).unwrap();
    assert_eq!(meshes.len(), 3);
    assert_eq!(quantities(&meshes), [1, 1, 1]);
    let mut model_ids = read_package_object_ids(&prusa, "3D/3dmodel.model").unwrap();
    let mut config_ids =
        read_package_object_ids(&prusa, "Metadata/Slic3r_PE_model.config").unwrap();
    model_ids.sort_unstable();
    config_ids.sort_unstable();
    assert_eq!(
        model_ids.len(),
        3,
        "Flat metadata must refer only to the three actual mesh objects"
    );
    assert_eq!(config_ids, model_ids);
    assert!(read_package_text(&prusa, "Metadata/Slic3r_PE.config")
        .unwrap()
        .contains("filament_colour ="));
    let points = |meshes: Vec<ModelMesh>| {
        meshes
            .into_iter()
            .flat_map(|m| m.vertices)
            .map(|p| p.map(|v| (v * 1000.).round() as i64))
            .collect::<std::collections::BTreeSet<_>>()
    };
    assert_eq!(
        points(meshes),
        points(read_package(&portable).unwrap()),
        "Prusa flattening must preserve every placed repeat"
    );
}

#[test]
#[ignore = "Requires installed Bambu Studio and OrcaSlicer roundtrip artifacts"]
fn installed_slicer_roundtrips_preserve_native_named_layout() {
    let folder = std::env::var("LIMO_CAD_SLICER_RESULTS").expect("Set LIMO_CAD_SLICER_RESULTS");
    let source = std::fs::read(format!("{folder}/../multipart-acceptance.3mf")).unwrap();
    let expected = read_package(&source).unwrap();
    assert_eq!(quantities(&expected), [1, 2]);
    let points = |meshes: Vec<ModelMesh>| {
        meshes
            .into_iter()
            .flat_map(|m| m.vertices)
            .map(|p| p.map(|v| (v * 1000.).round() as i64))
            .collect::<std::collections::BTreeSet<_>>()
    };
    let expected_points = points(expected);
    for slicer in ["bambu", "orca"] {
        let actual =
            read_package(&std::fs::read(format!("{folder}/{slicer}-roundtrip.3mf")).unwrap())
                .unwrap();
        assert_eq!(
            actual.len(),
            3,
            "{slicer} changed intentional repeat quantity"
        );
        assert_eq!(
            quantities(&actual),
            [1, 2],
            "{slicer} changed multipart groups"
        );
        assert_eq!(
            points(actual),
            expected_points,
            "{slicer} changed world vertices at 0.001mm tolerance"
        );
    }
}
