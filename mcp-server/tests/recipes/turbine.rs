//! Behavioral checks against the constructed turbine, independent of the
//! recipe's assertions. Temporary probes run in disposable native documents.
use super::*;

#[path = "turbine_assembly.rs"]
mod assembly_paths;

#[path = "turbine_export_reader.rs"]
mod export_reader_tests;

pub(super) fn check_assembly(exports: &Value) {
    assembly_paths::check(exports);
}

pub(super) fn check_adjuster_access(exports: &Value) {
    assembly_paths::check_adjuster_access(exports);
}

fn part<'a>(exports: &'a Value, id: &str) -> &'a Value {
    exports["parts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|part| part["id"] == id)
        .unwrap_or_else(|| panic!("missing turbine part {id}"))
}

fn body<'a>(scene: &'a Value, id: &Value) -> &'a Value {
    scene["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|body| body["id"] == *id)
        .unwrap()
}

/// Read the actual exported meshes, including oriented edge incidence. An
/// object count alone cannot detect open shells, reversed faces or stale
/// geometry in a repeated occurrence.
fn exported_meshes(export: &Value) -> Vec<Value> {
    let bytes = BASE64
        .decode(export["bytes_base64"].as_str().unwrap())
        .unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
    let mut xml = String::new();
    archive
        .by_name("3D/3dmodel.model")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    assert!(xml.contains("unit=\"millimeter\""));
    let meshes = limo_cad_export::test_reader::read_package(&bytes)
        .expect("read the exported 3MF build in world coordinates");
    assert!(
        !meshes.is_empty(),
        "the 3MF build must contain printable meshes"
    );
    meshes.into_iter().map(|mesh| {
        let vertices = mesh.vertices;
        assert!(!vertices.is_empty());
        assert!(vertices.iter().flatten().all(|value| value.is_finite()));
        let mut edges = std::collections::BTreeMap::<(usize, usize), (usize, i32)>::new();
        let mut indices = Vec::new();
        let mut volume = 0.;
        for triangle in mesh.triangles {
            assert!(triangle.iter().all(|index| *index < vertices.len()));
            let [a, b, c] = triangle.map(|index| vertices[index]);
            volume += (a[0] * (b[1] * c[2] - b[2] * c[1])
                + a[1] * (b[2] * c[0] - b[0] * c[2])
                + a[2] * (b[0] * c[1] - b[1] * c[0])) / 6.;
            for [a, b] in [[triangle[0], triangle[1]], [triangle[1], triangle[2]], [triangle[2], triangle[0]]] {
                assert_ne!(a, b);
                let edge = edges.entry((a.min(b), a.max(b))).or_default();
                edge.0 += 1;
                edge.1 += if a < b { 1 } else { -1 };
            }
            indices.extend(triangle);
        }
        assert!(volume.is_finite() && volume > 0., "exported shell has positive volume: {volume}");
        assert!(!edges.is_empty() && edges.values().all(|edge| *edge == (2, 0)),
            "every exported edge must have two oppositely oriented incident faces");
        json!({"mesh":{"positions":vertices.into_iter().flatten().collect::<Vec<_>>(), "indices":indices}})
    }).collect()
}

/// Match every oriented triangle, allowing only source seam welding (1e-5 mm)
/// and the exporter’s f32 placement precision. Cyclic corner order is harmless;
/// reversed winding, reflection, stale surfaces and missing triangles are not.
fn same_placed_surface(mesh: &Value, source: &Value, pose: &Value) -> bool {
    type Triangle = [[f64; 3]; 3];
    let triangles = |body: &Value, placement: Option<&Value>| -> Vec<Triangle> {
        let positions = body["mesh"]["positions"].as_array().unwrap();
        let points: Vec<[f64; 3]> = positions
            .as_chunks::<3>()
            .0
            .iter()
            .map(|point| {
                let point = std::array::from_fn(|i| point[i].as_f64().unwrap());
                if let Some(pose) = placement {
                    let q = vector::<4>(&pose["rotation"]);
                    let norm = q.iter().map(|v| v * v).sum::<f64>().sqrt();
                    let point = rotate(q.map(|v| v / norm), point);
                    let t = vector::<3>(&pose["translation"]);
                    std::array::from_fn(|i| point[i] + t[i])
                } else {
                    point
                }
            })
            .collect();
        body["mesh"]["indices"]
            .as_array()
            .unwrap()
            .as_chunks::<3>()
            .0
            .iter()
            .map(|indices| std::array::from_fn(|i| points[indices[i].as_u64().unwrap() as usize]))
            .collect()
    };
    let actual = triangles(mesh, None);
    let expected = triangles(source, Some(pose));
    if actual.len() != expected.len() {
        return false;
    }
    let magnitude = expected
        .iter()
        .flatten()
        .flatten()
        .map(|v| v.abs())
        .fold(1., f64::max);
    let tolerance = 1e-5 + magnitude * f64::from(f32::EPSILON);
    let cell_size = tolerance * 4.;
    let cell = |triangle: &Triangle| -> [i64; 3] {
        std::array::from_fn(|axis| {
            ((triangle.iter().map(|p| p[axis]).sum::<f64>() / 3.) / cell_size).floor() as i64
        })
    };
    let mut cells = std::collections::BTreeMap::<[i64; 3], Vec<usize>>::new();
    for (i, triangle) in actual.iter().enumerate() {
        cells.entry(cell(triangle)).or_default().push(i);
    }
    let mut used = vec![false; actual.len()];
    for triangle in expected {
        let base = cell(&triangle);
        let mut matched = None;
        'search: for x in -1..=1 {
            for y in -1..=1 {
                for z in -1..=1 {
                    if let Some(candidates) = cells.get(&[base[0] + x, base[1] + y, base[2] + z]) {
                        for &i in candidates {
                            if !used[i]
                                && (0..3).any(|shift| {
                                    (0..3).all(|corner| {
                                        (0..3).all(|axis| {
                                            (triangle[corner][axis]
                                                - actual[i][(corner + shift) % 3][axis])
                                                .abs()
                                                <= tolerance
                                        })
                                    })
                                })
                            {
                                matched = Some(i);
                                break 'search;
                            }
                        }
                    }
                }
            }
        }
        let Some(i) = matched else {
            return false;
        };
        used[i] = true;
    }
    true
}

#[test]
fn surface_comparison_rejects_a_closed_mirror_with_the_same_bounds_and_volume() {
    let original = json!({"mesh":{"positions":[0.,0.,0.,3.,0.,0.,0.,2.,0.,0.,0.,5.],
        "indices":[0,2,1,0,1,3,0,3,2,1,2,3]}});
    let identity = json!({"translation":[0.,0.,0.],"rotation":[0.,0.,0.,1.]});
    let mut mirror = original.clone();
    for p in mirror["mesh"]["positions"]
        .as_array_mut()
        .unwrap()
        .as_chunks_mut::<3>()
        .0
    {
        p[1] = json!(2. - p[1].as_f64().unwrap());
    }
    for triangle in mirror["mesh"]["indices"]
        .as_array_mut()
        .unwrap()
        .as_chunks_mut::<3>()
        .0
    {
        triangle.swap(1, 2);
    }
    assert_eq!(mesh_measurement(&mirror), mesh_measurement(&original));
    assert!(same_placed_surface(&original, &original, &identity));
    assert!(!same_placed_surface(&mirror, &original, &identity));
}

pub(super) fn check_print_placement(export: &Value, source: &Value, pose: &Value) {
    let meshes = exported_meshes(export);
    assert_eq!(meshes.len(), 1, "one selected printable occurrence");
    let (min, max, volume) = mesh_measurement(&meshes[0]);
    let (a, b) = world_bounds(source, pose);
    let expected_volume = mesh_measurement(source).2;
    assert!(
        min[2].abs() < 1e-5 && min[0] >= 0. && min[1] >= 0.,
        "the actual native print pose lies on the positive bed: {min:?}"
    );
    assert!(
        max[0] <= 235.5 && max[1] <= 256. && max[2] <= 256.,
        "native print fits the declared bed"
    );
    assert!(
        (0..3).all(|axis| (min[axis] - a[axis]).abs() < 1e-4 && (max[axis] - b[axis]).abs() < 1e-4)
    );
    assert!((volume - expected_volume).abs() / expected_volume < 1e-5);
    assert!(
        same_placed_surface(&meshes[0], source, pose),
        "the print must preserve every oriented native surface at its actual solved pose"
    );
}

pub(super) fn check_print_plates(exports: &Value, directory: Option<&std::path::Path>) {
    let mut checked = std::collections::BTreeSet::new();
    for plate in exports["print_plates"]
        .as_array()
        .expect("recipe-owned native print plates are required")
    {
        let id = plate["part_id"].as_str().unwrap();
        assert!(checked.insert(id));
        let part = part(exports, id);
        assert_eq!(part["printable"], true);
        assert_eq!(plate["body_id"], part["body_id"]);
        assert_eq!(plate["occurrence_id"], part["occurrence_id"]);
        assert_eq!(plate["model"]["format"], "limo-cad-project");
        assert_eq!(plate["solution"]["solved"], true);
        let selected = plate["model"]["assembly"]["component_structure"]["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .find(|occurrence| occurrence["id"] == plate["occurrence_id"])
            .unwrap();
        assert_eq!(
            selected["grounded"], true,
            "{id}: the printable occurrence has a fixed native bed pose"
        );
        for key in [
            "sketches",
            "extrudes",
            "revolves",
            "sweeps",
            "lofts",
            "ribs",
            "fillets",
            "chamfers",
            "holes",
            "datum_planes",
            "body_features",
            "body_appearances",
        ] {
            assert_eq!(
                plate["model"][key], exports["final_model"][key],
                "{id}: printing must preserve authored geometry and materials"
            );
        }
        let hidden = plate["model"]["visibility"]["hidden_body_ids"]
            .as_array()
            .unwrap();
        let visible: Vec<_> = plate["solution"]["instance_body_poses"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|pose| pose["visible"] == true && !hidden.contains(&pose["body_id"]))
            .collect();
        assert_eq!(
            visible.len(),
            1,
            "{id}: saved native plate must display exactly its printable occurrence"
        );
        assert_eq!(visible[0]["occurrence_id"], plate["occurrence_id"]);
        check_print_placement(
            &plate["export"],
            body(&exports["final_scene"], &part["body_id"]),
            visible[0],
        );
        if let Some(directory) = directory {
            std::fs::create_dir_all(directory).unwrap();
            std::fs::write(
                directory.join(format!("{id}.3mf")),
                BASE64
                    .decode(plate["export"]["bytes_base64"].as_str().unwrap())
                    .unwrap(),
            )
            .unwrap();
            write_native_project(&directory.join(format!("{id}.limo")), &plate["model"]);
        }
        if id == "stage" {
            let mut cold = Client::restore(&plate["model"]);
            assert_eq!(
                cold.call("assembly_solution", json!({}))["instance_body_poses"],
                plate["solution"]["instance_body_poses"]
            );
            let restored = cold.call(
                "solid_export_3mf",
                json!({"scope":"assembly","slicer_target":"standard"}),
            );
            check_print_placement(
                &restored,
                body(&exports["final_scene"], &part["body_id"]),
                visible[0],
            );
        }
    }
    let expected: std::collections::BTreeSet<_> = exports["parts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|part| part["printable"] == true)
        .map(|part| part["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        checked, expected,
        "each printable definition has exactly one official plate"
    );
}

pub(super) fn check_coupon_layout(client: &mut Client, exports: &Value) -> Value {
    let solution = solved(client);
    assert_eq!(
        solution["instance_body_poses"],
        exports["final_solution"]["instance_body_poses"]
    );
    no_overlap(&client.call(
        "assembly_interference_check",
        json!({"clearance_threshold_mm":0.}),
    ));
    let print = client.call(
        "solid_export_3mf",
        json!({"scope":"assembly","slicer_target":"standard"}),
    );
    let meshes = exported_meshes(&print);
    let poses = solution["instance_body_poses"].as_array().unwrap();
    assert_eq!(meshes.len(), poses.len());
    let expected: Vec<_> = poses
        .iter()
        .map(|pose| {
            let source = body(&exports["final_scene"], &pose["body_id"]);
            let (min, max) = world_bounds(source, pose);
            (min, max, mesh_measurement(source).2)
        })
        .collect();
    let mut matched = std::collections::BTreeSet::new();
    for mesh in &meshes {
        let (min, max, volume) = mesh_measurement(mesh);
        assert!(min[2].abs() < 1e-5, "each coupon has actual bed contact");
        assert!(
            min[0] >= 0. && min[1] >= 0. && max[0] <= 235.5 && max[1] <= 256. && max[2] <= 256.,
            "the native arranged coupon plate fits its declared bed"
        );
        let index = expected
            .iter()
            .enumerate()
            .position(|(i, (a, b, v))| {
                (0..3).all(|axis| {
                    (min[axis] - a[axis]).abs() < 1e-4 && (max[axis] - b[axis]).abs() < 1e-4
                }) && (volume - v).abs() / v < 1e-5
                    && same_placed_surface(
                        mesh,
                        body(&exports["final_scene"], &poses[i]["body_id"]),
                        &poses[i],
                    )
            })
            .expect("each print mesh must match a distinct actual solved coupon pose");
        assert!(matched.insert(index));
    }
    print
}

fn world_bounds(source: &Value, pose: &Value) -> ([f64; 3], [f64; 3]) {
    let rotation = vector::<4>(&pose["rotation"]);
    let translation = vector::<3>(&pose["translation"]);
    let positions = source["mesh"]["positions"].as_array().unwrap();
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for point in positions.as_chunks::<3>().0 {
        let transformed = rotate(
            rotation,
            std::array::from_fn(|i| point[i].as_f64().unwrap()),
        );
        for i in 0..3 {
            min[i] = min[i].min(transformed[i] + translation[i]);
            max[i] = max[i].max(transformed[i] + translation[i]);
        }
    }
    (min, max)
}

pub(super) fn check_enclosure_clearance(exports: &Value) {
    let poses = exports["final_solution"]["instance_body_poses"]
        .as_array()
        .unwrap();
    let rotor = exports["final_assembly"]["joints"]
        .as_array()
        .unwrap()
        .iter()
        .find(|joint| joint["id"] == exports["rotor_joint_id"])
        .unwrap();
    let anchor = poses
        .iter()
        .find(|pose| pose["occurrence_id"] == rotor["advanced"]["connector_a_occurrence_id"])
        .unwrap();
    let axis = rotate(
        vector(&anchor["rotation"]),
        vector(&rotor["connector_a"]["frame"]["primary_axis"]),
    );
    assert!(
        axis[0].abs() < 1e-10 && axis[1].abs() < 1e-10 && (axis[2].abs() - 1.).abs() < 1e-10,
        "continuous axial separation applies to the actual vertical rotor axis"
    );
    let bounds = |id: &Value| {
        let pose = poses
            .iter()
            .find(|pose| pose["occurrence_id"] == *id)
            .unwrap();
        world_bounds(body(&exports["final_scene"], &pose["body_id"]), pose)
    };
    let stage_bottom = bounds(&exports["occurrences"]["stage"]).0[2];
    let top = exports["occurrences"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(name, _)| name.starts_with("guard"))
        .map(|(_, id)| bounds(id).1[2])
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(stage_bottom-top >= 3., "the actual lid and all installed lid/base fastener envelopes clear the stage throughout its full axial rotation");
}

fn solved(client: &mut Client) -> Value {
    let solution = client.call("assembly_solution", json!({}));
    assert_eq!(solution["solved"], true);
    assert_eq!(solution["diagnostics"], json!([]));
    solution
}

fn stages(client: &mut Client, exports: &Value, native_scene: &Value) {
    let stage_id = &part(exports, "stage")["body_id"];
    let solution = solved(client);
    let poses: Vec<_> = solution["instance_body_poses"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|pose| pose["body_id"] == *stage_id)
        .collect();
    let home: Vec<_> = exports["final_solution"]["instance_body_poses"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|pose| pose["body_id"] == *stage_id)
        .collect();
    assert_eq!(poses.len(), 2);
    assert_eq!(
        poses, home,
        "editing the common solid preserves both assembled placements"
    );
    let export = client.call(
        "solid_export_3mf",
        json!({
            "body_ids":[stage_id],"scope":"assembly","slicer_target":"standard"
        }),
    );
    let mut meshes = exported_meshes(&export);
    meshes.sort_by(|a, b| mesh_measurement(a).0[2].total_cmp(&mesh_measurement(b).0[2]));
    assert_eq!(meshes.len(), 2);
    let (source_min, source_max, source_volume) = mesh_measurement(body(native_scene, stage_id));
    for (mesh, pose) in meshes.iter().zip(poses) {
        assert!(
            same_placed_surface(mesh, body(native_scene, stage_id), pose),
            "each edited stage occurrence must retain the definition's oriented surfaces"
        );
        let (min, max, volume) = mesh_measurement(mesh);
        let z = pose["translation"][2].as_f64().unwrap();
        assert!((min[2] - z - source_min[2]).abs() < 1e-4);
        assert!((max[2] - z - source_max[2]).abs() < 1e-4);
        assert!(
            (volume - source_volume).abs() / source_volume < 1e-5,
            "each assembled rotor occurrence must export the edited definition, not stale geometry"
        );
    }
}

fn stage_drawing(client: &mut Client, exports: &Value, diameter: f64) {
    let sheet = exports["drawings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|drawing| drawing["part"] == "stage")
        .unwrap();
    let drawing = client.call(
        "drawing_export",
        json!({"sheet_id":sheet["sheet_id"],"format":"svg"}),
    );
    assert!(
        drawing["content"]
            .as_str()
            .unwrap()
            .contains(&format!(">Ø{diameter:.2} mm</text>")),
        "the associative stage drawing must measure its actual edited shaft bore"
    );
}

fn check_edits(client: &mut Client, exports: &Value) {
    let original = client.call("solid_scene", json!({}));
    assert_eq!(original["bodies"], exports["final_scene"]["bodies"]);
    let stage_id = &part(exports, "stage")["body_id"];
    let feature = exports["final_model"]["extrudes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|feature| feature["feature_id"] == exports["stage_plate_feature"])
        .unwrap();
    let request = json!({"sketch_name":feature["sketch_name"],"profile_indices":feature["profile_indices"],
        "operation":feature["operation"],"extent":feature["extent"],"taper_angle_deg":feature["taper_angle_deg"],
        "flip":feature["flip"],"target_body_ids":feature["target_body_ids"]});
    let mut edited = request.clone();
    edited["extent"] = json!({"type":"distance","distance":4.});
    let changed = client.call(
        "solid_edit_extrude",
        json!({"feature_id":exports["stage_plate_feature"],"extrude":edited}),
    );
    assert_eq!(changed["scene"]["errors"], json!([]));
    let added_volume = mesh_measurement(body(&changed["scene"], stage_id)).2
        - mesh_measurement(body(&original, stage_id)).2;
    assert!((29_000.0..31_000.0).contains(&added_volume),
        "one extra millimetre must fill the free disc area around the existing hub and bucket walls: {added_volume}");
    stages(client, exports, &changed["scene"]);
    let edited_model = client.call("cad_project_model", json!({}));
    let mut cold = Client::restore(&edited_model);
    let cold_scene = cold.call("solid_scene", json!({}));
    assert_eq!(cold_scene["bodies"], changed["scene"]["bodies"]);
    stages(&mut cold, exports, &cold_scene);
    drop(cold);
    let restored = client.call(
        "solid_edit_extrude",
        json!({"feature_id":exports["stage_plate_feature"],"extrude":request}),
    );
    let residual = restored_geometry_residual(
        &restored["scene"]["bodies"],
        &original["bodies"],
        String::new(),
    );
    assert!(residual.0 < 1e-6, "stage thickness restore: {residual:?}");
    stages(client, exports, &restored["scene"]);

    let sketch = client.call("sketch_edit", json!({"name":"stage_shaft_fit"}));
    let dimension = sketch["dimensions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|dimension| dimension["kind"] == "diameter" && dimension["mode"] == "driving")
        .unwrap();
    let constraint = dimension["constraint_id"].clone();
    let original_diameter = dimension["value"].as_f64().unwrap();
    let diameter = original_diameter + 0.2;
    client.call(
        "sketch_edit_dimension",
        json!({"constraint_id":constraint,"text":diameter.to_string()}),
    );
    assert_eq!(client.call("sketch_active", json!({}))["dof"]["value"], 0);
    client.call("sketch_finish", json!({}));
    let changed = client.call("solid_recompute", json!({}));
    assert_eq!(changed["scene"]["errors"], json!([]));
    let edited_body = body(&changed["scene"], stage_id);
    assert!(mesh_measurement(edited_body).2 < mesh_measurement(body(&original, stage_id)).2 - 1.);
    assert!(edited_body["faces"]
        .as_array()
        .unwrap()
        .iter()
        .any(|face| face["cylinder"]["radius"]
            .as_f64()
            .is_some_and(|radius| (radius * 2. - diameter).abs() < 1e-8)));
    stages(client, exports, &changed["scene"]);
    stage_drawing(client, exports, diameter);
    client.call("sketch_edit", json!({"name":"stage_shaft_fit"}));
    client.call(
        "sketch_edit_dimension",
        json!({"constraint_id":constraint,"text":original_diameter.to_string()}),
    );
    client.call("sketch_finish", json!({}));
    let restored = client.call("solid_recompute", json!({}));
    let residual = restored_geometry_residual(
        &restored["scene"]["bodies"],
        &original["bodies"],
        String::new(),
    );
    assert!(residual.0 < 1e-6, "shaft fit restore: {residual:?}");
    stages(client, exports, &restored["scene"]);
    stage_drawing(client, exports, original_diameter);
}

/// Use the same conservative broad phase as the native interference API,
/// then ask that API for exact geometry. Rigidly connected pairs have an
/// invariant relative placement and need checking only once for the sweep.
struct MotionChecks {
    scene: limo_cad_solid::SolidSceneDto,
    rigid_groups: std::collections::BTreeMap<u64, u64>,
    checked_rigid_pairs: std::collections::BTreeSet<(u64, u64)>,
}

fn vector<const N: usize>(value: &Value) -> [f64; N] {
    std::array::from_fn(|index| value[index].as_f64().unwrap())
}

fn rotate(q: [f64; 4], v: [f64; 3]) -> [f64; 3] {
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let u = [q[0], q[1], q[2]];
    let uv = cross(u, v);
    let uuv = cross(u, uv);
    std::array::from_fn(|i| v[i] + 2. * (q[3] * uv[i] + uuv[i]))
}

impl MotionChecks {
    fn new(scene: &Value, document: &Value, solution: &Value) -> Self {
        assert!(
            solution["instance_body_poses"]
                .as_array()
                .unwrap()
                .iter()
                .all(|pose| pose["visible"] == true),
            "physical collision acceptance must include every modeled part and fastener"
        );
        let mut groups: std::collections::BTreeMap<_, _> = solution["instance_body_poses"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pose| {
                let id = pose["occurrence_id"].as_u64().unwrap();
                (id, id)
            })
            .collect();
        for joint in document["joints"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|joint| joint["enabled"] == true && joint["kind"] == "rigid")
        {
            let a = joint["advanced"]["connector_a_occurrence_id"]
                .as_u64()
                .unwrap();
            let b = joint["advanced"]["connector_b_occurrence_id"]
                .as_u64()
                .unwrap();
            let (from, to) = (groups[&b], groups[&a]);
            for group in groups.values_mut() {
                if *group == from {
                    *group = to;
                }
            }
        }
        Self {
            scene: serde_json::from_value(scene.clone()).unwrap(),
            rigid_groups: groups,
            checked_rigid_pairs: Default::default(),
        }
    }

    fn check(&mut self, client: &mut Client, solution: &Value, sample: &str) {
        let poses: Vec<limo_cad_sketch::InstanceBodyPoseDto> =
            serde_json::from_value(solution["instance_body_poses"].clone()).unwrap();
        let candidates = limo_cad_sketch::broad_phase_interference_pairs(
            &self.scene,
            &poses,
            &limo_cad_sketch::InterferenceCheckRequestDto {
                occurrence_ids: vec![],
                clearance_threshold_mm: 0.,
            },
        )
        .unwrap();
        for (a, b) in candidates {
            let a = poses[a].occurrence_id.0;
            let b = poses[b].occurrence_id.0;
            let pair = (a.min(b), a.max(b));
            if self.rigid_groups[&a] == self.rigid_groups[&b]
                && !self.checked_rigid_pairs.insert(pair)
            {
                continue;
            }
            let report = client.call(
                "assembly_interference_check",
                json!({
                    "occurrence_ids":[a,b],"clearance_threshold_mm":0.
                }),
            );
            assert!(
                !report["pairs"].as_array().unwrap().is_empty(),
                "the native query must check broad-phase candidate {pair:?} at {sample}"
            );
            no_overlap(&report);
        }
    }

    fn check_driven_poses(
        &self,
        home: &Value,
        current: &Value,
        document: &Value,
        joint_id: &Value,
        angle: f64,
    ) {
        let joint = document["joints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|joint| joint["id"] == *joint_id)
            .unwrap();
        let a = joint["advanced"]["connector_a_occurrence_id"]
            .as_u64()
            .unwrap();
        let b = joint["advanced"]["connector_b_occurrence_id"]
            .as_u64()
            .unwrap();
        let home_poses = home["instance_body_poses"].as_array().unwrap();
        let anchor = home_poses
            .iter()
            .find(|pose| pose["occurrence_id"] == a)
            .unwrap();
        let anchor_q = vector(&anchor["rotation"]);
        let offset = rotate(anchor_q, vector(&joint["connector_a"]["frame"]["origin"]));
        let pivot: [f64; 3] =
            std::array::from_fn(|i| anchor["translation"][i].as_f64().unwrap() + offset[i]);
        let axis = rotate(
            anchor_q,
            vector(&joint["connector_a"]["frame"]["primary_axis"]),
        );
        let half = angle.to_radians() / 2.;
        let rotation = [
            axis[0] * half.sin(),
            axis[1] * half.sin(),
            axis[2] * half.sin(),
            half.cos(),
        ];
        let group = self.rigid_groups[&b];
        for old in home_poses
            .iter()
            .filter(|pose| self.rigid_groups[&pose["occurrence_id"].as_u64().unwrap()] == group)
        {
            let now = current["instance_body_poses"]
                .as_array()
                .unwrap()
                .iter()
                .find(|pose| {
                    pose["occurrence_id"] == old["occurrence_id"]
                        && pose["body_id"] == old["body_id"]
                })
                .unwrap();
            let relative =
                std::array::from_fn(|i| old["translation"][i].as_f64().unwrap() - pivot[i]);
            let expected_position = rotate(rotation, relative);
            for i in 0..3 {
                assert!((now["translation"][i].as_f64().unwrap() - pivot[i] - expected_position[i]).abs() < 1e-6,
                    "all driven parts, including both stages and clamp hardware, must follow the shaft");
            }

            for basis in [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]] {
                let expected = rotate(rotation, rotate(vector(&old["rotation"]), basis));
                let actual = rotate(vector(&now["rotation"]), basis);
                assert!(
                    (0..3).all(|i| (actual[i] - expected[i]).abs() < 1e-8),
                    "driven occurrence {} must rotate with its physical shaft",
                    old["occurrence_id"]
                );
            }
        }
    }
}

fn check_motion(client: &mut Client, exports: &Value) {
    let home = solved(client);
    let home_document = client.call("assembly_document", json!({}));
    let joint_angle = |document: &Value, id: &Value| {
        document["joints"]
            .as_array()
            .unwrap()
            .iter()
            .find(|joint| joint["id"] == *id)
            .unwrap()["angle_offset_deg"]
            .as_f64()
            .unwrap()
    };
    let rotor_home = joint_angle(&home_document, &exports["rotor_joint_id"]);
    let generator_home = joint_angle(&home_document, &exports["generator_joint_id"]);
    let scene = client.call("solid_scene", json!({}));
    let mut collisions = MotionChecks::new(&scene, &home_document, &home);
    collisions.check(client, &home, "home");

    let phases = (1..=20)
        .map(|step| step as f64 * 0.25)
        .chain((1..=24).map(|step| step as f64 * 15.))
        .chain([810., -90.]);
    for angle in phases {
        client.call("assembly_set_joint_motion", json!({"joint_id":exports["rotor_joint_id"],"angle_offset_deg":rotor_home + angle,"linear_offset_mm":0.}));
        let document = client.call("assembly_document", json!({}));
        assert!(
            (joint_angle(&document, &exports["generator_joint_id"])
                - (generator_home - 4. * angle))
                .abs()
                < 1e-8
        );
        let solution = solved(client);
        collisions.check_driven_poses(
            &home,
            &solution,
            &home_document,
            &exports["rotor_joint_id"],
            angle,
        );
        collisions.check_driven_poses(
            &home,
            &solution,
            &home_document,
            &exports["generator_joint_id"],
            -4. * angle,
        );
        if angle % 360. != 0. {
            assert_ne!(
                solution["instance_body_poses"], home["instance_body_poses"],
                "the native occurrences must move, not only joint metadata"
            );
        }
        collisions.check(client, &solution, &format!("rotor {angle} degrees"));
    }
    for generator_delta in [120., -480., 1440.] {
        client.call("assembly_set_joint_motion", json!({"joint_id":exports["generator_joint_id"],"angle_offset_deg":generator_home + generator_delta,"linear_offset_mm":0.}));
        let document = client.call("assembly_document", json!({}));
        assert!(
            (joint_angle(&document, &exports["rotor_joint_id"])
                - (rotor_home - generator_delta / 4.))
                .abs()
                < 1e-8,
            "driving the generator must drive the rotor through the same persistent relation"
        );
        let solution = solved(client);
        collisions.check_driven_poses(
            &home,
            &solution,
            &home_document,
            &exports["rotor_joint_id"],
            -generator_delta / 4.,
        );
        collisions.check_driven_poses(
            &home,
            &solution,
            &home_document,
            &exports["generator_joint_id"],
            generator_delta,
        );
        collisions.check(
            client,
            &solution,
            &format!("generator {generator_delta} degrees"),
        );
    }
    client.call("assembly_set_joint_motion", json!({"joint_id":exports["rotor_joint_id"],"angle_offset_deg":rotor_home,"linear_offset_mm":0.}));
    assert_eq!(
        client.call("assembly_document", json!({}))["joints"],
        home_document["joints"]
    );
    assert_eq!(
        solved(client)["instance_body_poses"],
        home["instance_body_poses"]
    );
}

pub(super) fn check_edits_and_motion(client: &mut Client, exports: &Value) {
    eprintln!("turbine acceptance: rotor edits, drawing reevaluation and cold edited model");
    check_edits(client, exports);
    eprintln!("turbine acceptance: rotor edits passed; driven motion and clearances");
    check_motion(client, exports);
    eprintln!("turbine acceptance: driven motion and clearances passed");
}
