use super::*;

#[path = "vise_assembly.rs"]
mod assembly_paths;

fn part<'a>(exports: &'a Value, id: &str) -> &'a Value {
    exports["parts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|part| part["id"] == id)
        .unwrap_or_else(|| panic!("missing vise part {id}"))
}

fn body<'a>(exports: &'a Value, id: &str) -> &'a Value {
    let id = &part(exports, id)["body_id"];
    exports["final_scene"]["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|body| body["id"] == *id)
        .unwrap()
}

fn interference(client: &mut Client, exports: &Value, parts: &[&str]) -> Value {
    let occurrences: Vec<_> = parts
        .iter()
        .map(|id| part(exports, id)["occurrence_id"].clone())
        .collect();
    client.call(
        "assembly_interference_check",
        json!({"occurrence_ids":occurrences,"clearance_threshold_mm":0}),
    )
}

fn check_stationary_parts(client: &mut Client, exports: &Value, installed: &[&str]) {
    if installed.len() > 1 {
        client.stage(format!(
            "vise / stationary assembly clearance: {installed:?}"
        ));
        no_overlap(&interference(client, exports, installed));
    }
}

fn check_moving_part(
    client: &mut Client,
    exports: &Value,
    moving: &str,
    installed: &[&str],
    sample: &str,
) {
    assert!(!installed.contains(&moving));
    for fixed in installed {
        client.stage(format!("vise / {moving} against {fixed} at {sample}"));
        let report = interference(client, exports, &[moving, fixed]);
        assert!(
            !has_overlap(&report),
            "{moving} against {fixed} at {sample}: {report}"
        );
        no_overlap(&report);
    }
}

fn displaced(client: &mut Client, exports: &Value, id: &str, offset: [f64; 3]) {
    let home = exports["final_solution"]["instance_body_poses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pose| pose["body_id"] == part(exports, id)["body_id"])
        .unwrap();
    let translation: Vec<_> = (0..3)
        .map(|axis| home["translation"][axis].as_f64().unwrap() + offset[axis])
        .collect();
    client.call(
        "assembly_set_occurrence_pose",
        json!({
            "occurrence_id":part(exports,id)["occurrence_id"],
            "local_pose":{"translation":translation,"rotation":home["rotation"]}
        }),
    );
}

fn has_overlap(result: &Value) -> bool {
    result["pairs"]
        .as_array()
        .unwrap()
        .iter()
        .any(|pair| pair["interfering"] == true)
}

fn release_joints(client: &mut Client, exports: &Value) {
    for joint in exports["final_assembly"]["joints"].as_array().unwrap() {
        client.call(
            "assembly_set_joint_enabled",
            json!({"joint_id":joint["id"],"enabled":false}),
        );
    }
}

fn check_edits(client: &mut Client, exports: &Value) {
    let original_volume = mesh_measurement(body(exports, "jaw")).2;
    for (name, original, changed, increases) in [
        ("Moving jaw / 100 mm gripping face", 100., 104., true),
        ("Jaw dovetail left / profile clearance", 12.8, 13.2, false),
    ] {
        client.stage(format!("vise / driving dimension: {name}"));
        let sketch = client.call("sketch_edit", json!({"name":name}));
        let dimension = sketch["dimensions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|dimension| (dimension["value"].as_f64().unwrap() - original).abs() < 1e-7)
            .unwrap();
        assert_eq!(dimension["mode"], "driving");
        let constraint = dimension["constraint_id"].clone();
        client.call(
            "sketch_edit_dimension",
            json!({"constraint_id":constraint,"text":changed.to_string()}),
        );
        assert_eq!(client.call("sketch_active", json!({}))["dof"]["value"], 0);
        client.call("sketch_finish", json!({}));
        let scene = client.call("solid_recompute", json!({}))["scene"].clone();
        assert_eq!(scene["errors"], json!([]));
        let changed_body = scene["bodies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"] == part(exports, "jaw")["body_id"])
            .unwrap();
        let delta = mesh_measurement(changed_body).2 - original_volume;
        assert!(
            if increases { delta > 100. } else { delta < -1. },
            "{name}: expected physical stock/clearance change, got {delta}"
        );
        let drawing = client.call(
            "drawing_export",
            json!({"sheet_id":exports["vise_jaw_svg"]["sheet_id"],"format":"svg"}),
        );
        assert!(
            drawing["content"]
                .as_str()
                .unwrap()
                .contains(&format!(">{changed:.2} mm</text>")),
            "{name}: drawing must measure edited geometry"
        );
        client.call("sketch_edit", json!({"name":name}));
        client.call(
            "sketch_edit_dimension",
            json!({"constraint_id":constraint,"text":original.to_string()}),
        );
        client.call("sketch_finish", json!({}));
        let restored = client.call("solid_recompute", json!({}));
        let residual = restored_geometry_residual(
            &restored["scene"]["bodies"],
            &exports["final_scene"]["bodies"],
            String::new(),
        );
        assert!(residual.0 < 1e-6, "{name}: restore residual {residual:?}");
    }

    let mut request = exports["male_thread_request"].clone();
    client.stage("vise / simplify and restore the modeled screw thread");
    request["thread"]["representation"] = json!("simplified");
    let simplified = client.call(
        "solid_edit_external_thread",
        json!({"feature_id":exports["male_thread_feature_id"],"request":request}),
    );
    assert_eq!(simplified["scene"]["errors"], json!([]));
    let screw = simplified["scene"]["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == part(exports, "screw")["body_id"])
        .unwrap();
    assert!(mesh_measurement(screw).2 > mesh_measurement(body(exports, "screw")).2 + 100.);
    let restored=client.call("solid_edit_external_thread",json!({"feature_id":exports["male_thread_feature_id"],"request":exports["male_thread_request"]}));
    assert_eq!(restored["scene"]["errors"], json!([]));
    assert!(
        restored_geometry_residual(
            &restored["scene"]["bodies"],
            &exports["final_scene"]["bodies"],
            String::new()
        )
        .0 < 1e-6
    );
}

fn check_capture_and_entry(exports: &Value) {
    eprintln!("vise / restore for physical capture and assembly entry");
    let mut client = Client::restore(&exports["final_model"]);
    client.stage("vise / physical guide capture and carriage entry");
    release_joints(&mut client, exports);

    for (offset, blocked) in [
        ([0., 0., 0.1], false),
        ([0., 0., 1.5], true),
        ([0., 1.5, 0.], true),
        ([0., -1.5, 0.], true),
    ] {
        client.stage(format!("vise / physical guide capture at {offset:?} mm"));
        displaced(&mut client, exports, "jaw", offset);
        assert_eq!(
            has_overlap(&interference(&mut client, exports, &["frame", "jaw"])),
            blocked,
            "physical guide capture at {offset:?}"
        );
    }

    let frame_min = mesh_measurement(body(exports, "frame")).0[0];
    let jaw_max = mesh_measurement(body(exports, "jaw")).1[0];
    let start = frame_min - jaw_max - 5.;
    for step in 0..=12 {
        let x = start * (1. - step as f64 / 12.);
        client.stage(format!("vise / carriage entry at {x:.2} mm"));
        displaced(&mut client, exports, "jaw", [x, 0., 0.]);
        no_overlap(&interference(&mut client, exports, &["frame", "jaw"]));
    }
    check_stationary_parts(&mut client, exports, &["frame", "jaw"]);
    for lift in [70., 40., 20., 8., 2., 0.] {
        client.stage(format!("vise / bridge entry at {lift} mm"));
        displaced(&mut client, exports, "nut", [0., 0., lift]);
        check_moving_part(
            &mut client,
            exports,
            "nut",
            &["frame", "jaw"],
            &format!("bridge lift {lift} mm"),
        );
    }

    for x in [-1.5, 1.5] {
        client.stage(format!("vise / bridge axial restraint at {x} mm"));
        displaced(&mut client, exports, "nut", [x, 0., 0.]);
        assert!(has_overlap(&interference(
            &mut client,
            exports,
            &["frame", "nut"]
        )));
    }
    displaced(&mut client, exports, "nut", [0., 0., 0.]);
    displaced(&mut client, exports, "jaw", [85., 0., 0.]);

    let screw_pose = exports["final_solution"]["instance_body_poses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|pose| pose["body_id"] == part(exports, "screw")["body_id"])
        .unwrap();
    let rotation = &screw_pose["rotation"];
    assert!(
        rotation[1].as_f64().unwrap().abs() < 1e-7 && rotation[2].as_f64().unwrap().abs() < 1e-7
    );
    let phase = 2.
        * rotation[0]
            .as_f64()
            .unwrap()
            .atan2(rotation[3].as_f64().unwrap());
    let axis_z = 50.;
    check_stationary_parts(&mut client, exports, &["frame", "nut", "jaw"]);
    for advance in [-120., -95., -90., -85., -60., -30., -5., 0.] {
        client.stage(format!("vise / coupled screw insertion at {advance} mm"));
        let angle = phase + advance / 4. * std::f64::consts::TAU;
        client.call("assembly_set_occurrence_pose",json!({
            "occurrence_id":part(exports,"screw")["occurrence_id"],
            "local_pose":{"translation":[advance,axis_z*angle.sin(),axis_z*(1.-angle.cos())],"rotation":[(angle/2.).sin(),0.,0.,(angle/2.).cos()]}
        }));
        check_moving_part(
            &mut client,
            exports,
            "screw",
            &["frame", "nut", "jaw"],
            &format!("coupled thread advance {advance} mm"),
        );
    }
    check_stationary_parts(&mut client, exports, &["frame", "nut", "jaw", "screw"]);
    for approach in [35., 25., 15., 5., 0.] {
        client.stage(format!("vise / thrust fitting entry at {approach} mm"));
        displaced(&mut client, exports, "thrust", [approach, 0., 0.]);
        check_moving_part(
            &mut client,
            exports,
            "thrust",
            &["frame", "nut", "jaw", "screw"],
            &format!("thrust approach {approach} mm"),
        );
    }
    check_stationary_parts(&mut client, exports, &["frame", "nut", "screw", "thrust"]);
    for advance in [85., 60., 40., 20., 5., 0.] {
        client.stage(format!("vise / jaw assembly entry at {advance} mm"));
        displaced(&mut client, exports, "jaw", [advance, 0., 0.]);
        check_moving_part(
            &mut client,
            exports,
            "jaw",
            &["frame", "nut", "screw", "thrust"],
            &format!("jaw advance {advance} mm"),
        );
    }
    check_stationary_parts(&mut client, exports, &["jaw", "screw", "thrust"]);
    for lift in [70., 40., 20., 8., 2., 0.] {
        client.stage(format!("vise / keeper entry at {lift} mm"));
        displaced(&mut client, exports, "keeper", [0., 0., lift]);
        check_moving_part(
            &mut client,
            exports,
            "keeper",
            &["jaw", "screw", "thrust"],
            &format!("keeper lift {lift} mm"),
        );
    }

    let seating = exports["design_inputs"]["keeper_axial_seating_mm"]
        .as_f64()
        .unwrap();
    client.stage("vise / keeper shoulder seating and pin clearance");
    assert!((seating - 0.4).abs() < 1e-9);
    displaced(&mut client, exports, "keeper", [-seating, 0., 0.]);
    let seated = interference(&mut client, exports, &["jaw", "keeper"]);
    no_overlap(&seated);
    assert_eq!(seated["pairs"].as_array().unwrap().len(), 1);
    assert!(
        seated["pairs"][0]["minimum_clearance_mm"].as_f64().unwrap() < 1e-6,
        "keeper must touch its jaw shoulders after {seating} mm: {seated}"
    );
    let pin = interference(&mut client, exports, &["keeper", "retainer_screw"]);
    no_overlap(&pin);
    assert_eq!(pin["pairs"].as_array().unwrap().len(), 1);
    assert!(
        pin["pairs"][0]["minimum_clearance_mm"].as_f64().unwrap() >= 0.1 - 1e-7,
        "seated keeper must retain clearance around the fixed pin: {pin}"
    );
    displaced(&mut client, exports, "keeper", [-(seating + 0.2), 0., 0.]);
    let shoulder = interference(&mut client, exports, &["jaw", "keeper"]);
    assert_eq!(shoulder["exact"], true);
    assert!(
        has_overlap(&shoulder),
        "keeper must meet physical jaw shoulders beyond its seating travel: {shoulder}"
    );
    displaced(&mut client, exports, "keeper", [0.; 3]);
}

#[test]
fn d_screw_vise_builds_editable_native_geometry() {
    let mut client = Client::start();
    client.stage("vise / initial construction and drawings");
    let report = client.recipe("d-screw-vise");
    let exports = &report["exports"];
    let artifacts = RecipeArtifacts::new();
    let directory = &artifacts.path;
    std::fs::write(
        directory.join("replay-report.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    write_native_project(
        &directory.join("d-screw-vise.limo"),
        &exports["final_model"],
    );
    assert_eq!(exports["final_scene"]["errors"], json!([]));
    assert_eq!(exports["final_solution"]["solved"], true);
    assert_eq!(exports["final_solution"]["diagnostics"], json!([]));
    assert_retained_component_bodies(&exports["final_scene"], &exports["final_assembly"]);
    assert!(exports["final_sketches"]
        .as_array()
        .unwrap()
        .iter()
        .all(|sketch| sketch["dof"]["value"] == 0));

    let (frame_min, frame_max, _) = mesh_measurement(body(exports, "frame"));
    let (jaw_min, jaw_max, _) = mesh_measurement(body(exports, "jaw"));
    assert!(frame_max[0] - frame_min[0] <= 230.001);
    assert!(
        jaw_max[1] - jaw_min[1] >= 100.,
        "carriage supports the 100 mm gripping face"
    );

    let grip: Vec<_> = body(exports, "jaw")["mesh"]["positions"]
        .as_array()
        .unwrap()
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|p| {
            (p[0].as_f64().unwrap() - jaw_max[0]).abs() < 1e-4 && p[2].as_f64().unwrap() > 34.001
        })
        .map(|p| p[1].as_f64().unwrap())
        .collect();
    let grip_span = grip.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        - grip.iter().copied().fold(f64::INFINITY, f64::min);
    assert!(
        (94. ..=100.001).contains(&grip_span),
        "rounded 100 mm gripping face: {grip_span}"
    );
    for part in exports["parts"].as_array().unwrap() {
        assert!(mesh_measurement(body(exports, part["id"].as_str().unwrap())).2 > 0.);
    }

    let screw = body(exports, "screw");
    let swept_radius = screw["mesh"]["positions"]
        .as_array()
        .unwrap()
        .as_chunks::<3>()
        .0
        .iter()
        .map(|p| p[1].as_f64().unwrap().hypot(p[2].as_f64().unwrap() - 50.))
        .fold(0_f64, f64::max);
    assert!(
        50. - swept_radius >= 25.,
        "tabletop handle clearance: {}",
        50. - swept_radius
    );
    let front_of_grip = screw["mesh"]["positions"]
        .as_array()
        .unwrap()
        .as_chunks::<3>()
        .0
        .iter()
        .filter(|p| p[1].as_f64().unwrap().hypot(p[2].as_f64().unwrap() - 50.) > 12.001)
        .map(|p| p[0].as_f64().unwrap())
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(front_of_grip.is_finite());
    assert!(
        mesh_measurement(body(exports, "nut")).0[0] - (front_of_grip + 90.) >= 14.999,
        "closed-stroke grip must leave axial space before the bridge"
    );

    let parts = exports["parts"].as_array().unwrap();
    let printable_count = parts
        .iter()
        .filter(|part| part["printable"] == true)
        .count();
    let mut plate_count = 0;
    for plate in exports["print_plates"].as_array().unwrap() {
        let name = plate["name"].as_str().unwrap();
        let count = plate["model"]["assembly"]["definitions"]
            .as_array()
            .map(Vec::len);
        let expected = parts
            .iter()
            .filter(|part| part["printable"] == true && part["print_plate"] == plate["name"])
            .count();
        assert!(
            expected > 0,
            "{name} has no printable assignments: {count:?}"
        );
        let expected_hidden: std::collections::BTreeSet<_> = parts
            .iter()
            .filter(|part| part["print_plate"] != plate["name"])
            .map(|part| part["body_id"].as_u64().unwrap())
            .collect();
        let actual_hidden: std::collections::BTreeSet<_> = plate["model"]["visibility"]
            ["hidden_body_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_u64().unwrap())
            .collect();
        assert_eq!(
            actual_hidden, expected_hidden,
            "{name}: saved Browser isolation"
        );
        validate_print_3mf(
            &plate["export"],
            expected,
            [235.5, 256., 256.],
            &directory.join(format!("d-screw-vise-{name}.3mf")),
        );
        write_native_project(
            &directory.join(format!("d-screw-vise-{name}.limo")),
            &plate["model"],
        );
        plate_count += expected;
    }
    assert_eq!(
        plate_count, printable_count,
        "every printed part is assigned exactly once"
    );

    let sheets = exports["final_model"]["drawings"]["sheets"]
        .as_array()
        .unwrap();
    assert_eq!(
        sheets.len(),
        printable_count + 1,
        "assembly plus each printed part"
    );
    for sheet in sheets {
        assert_eq!(sheet["projection_method"], "third_angle");
    }
    for (name, export) in exports.as_object().unwrap() {
        if name.ends_with("_svg") || name.ends_with("_dxf") {
            let content = export["content"].as_str().unwrap();
            assert!(!content.is_empty());
            std::fs::write(
                directory.join(format!("{name}.{}", export["format"].as_str().unwrap())),
                content,
            )
            .unwrap();
        }
    }

    let lead = 4.;
    let travel = 90.;

    assert!(
        parts
            .iter()
            .any(|part| part["printable"] == false
                && part["id"].as_str().unwrap().starts_with("mount")),
        "modeled installation hardware is required"
    );
    no_overlap(&exports["final_interference"]);
    for advance in [0., 10., 45., 85., 89., travel, 0.] {
        client.stage(format!(
            "vise / full assembly stroke interference at {advance} mm"
        ));
        client.call("assembly_set_joint_motion",json!({"joint_id":exports["screw_joint_id"],"angle_offset_deg":advance/lead*360.,"linear_offset_mm":0}));
        let solution = client.call("assembly_solution", json!({}));
        assert_eq!(solution["solved"], true, "{solution}");
        assert_eq!(solution["diagnostics"], json!([]));
        for id in ["jaw", "screw", "thrust", "keeper"] {
            let pose = solution["instance_body_poses"]
                .as_array()
                .unwrap()
                .iter()
                .find(|pose| pose["body_id"] == part(exports, id)["body_id"])
                .unwrap();
            assert!(
                (pose["translation"][0].as_f64().unwrap() - advance).abs() < 1e-5,
                "{id} travel: {pose}"
            );
        }
        no_overlap(&client.call(
            "assembly_interference_check",
            json!({"clearance_threshold_mm":0}),
        ));
    }
    let before_rejection = client.call("cad_project_model", json!({}));
    let rejected=client.rpc("tools/call",json!({"name":"assembly_set_joint_motion","arguments":{"joint_id":exports["screw_joint_id"],"angle_offset_deg":(travel+1.)/lead*360.,"linear_offset_mm":0}}));
    assert_eq!(rejected["isError"], true);
    assert_eq!(
        client.call("cad_project_model", json!({})),
        before_rejection,
        "travel rejection is atomic"
    );

    check_capture_and_entry(exports);
    eprintln!("vise / restore and check purchased hardware assembly paths");
    assembly_paths::check_hardware_paths(exports);
    check_edits(&mut client, exports);

    eprintln!("vise / restore saved model and compare geometry");
    let mut restored = Client::restore(&exports["final_model"]);
    assert_same_json(
        &restored.call("solid_scene", json!({}))["bodies"],
        &exports["final_scene"]["bodies"],
        "native save/reload geometry",
    );
    assert_eq!(
        restored.call("assembly_solution", json!({}))["instance_body_poses"],
        exports["final_solution"]["instance_body_poses"]
    );
    let mut repeat = Client::start();
    repeat.stage("vise / independent deterministic construction and drawings");
    let repeated = repeat.recipe("d-screw-vise");
    for key in [
        "final_model",
        "final_scene",
        "final_sketches",
        "final_assembly",
        "final_solution",
        "print_plates",
    ] {
        assert_same_json(
            &exports[key],
            &repeated["exports"][key],
            &format!("independent vise replay: {key}"),
        );
    }
    for (name, export) in exports.as_object().unwrap() {
        if name.ends_with("_svg") || name.ends_with("_dxf") {
            restored.stage(format!("vise / restored drawing export: {name}"));
            assert_same_json(
                export,
                &repeated["exports"][name],
                &format!("independent drawing replay:{name}"),
            );
            assert_same_json(
                &restored.call(
                    "drawing_export",
                    json!({"sheet_id":export["sheet_id"],"format":export["format"]}),
                )["content"],
                &export["content"],
                &format!("restored drawing:{name}"),
            );
        }
    }
}
