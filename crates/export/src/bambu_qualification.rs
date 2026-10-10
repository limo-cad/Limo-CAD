//! Opt-in local fixture generation. Complete profile input is supplied by the operator, never committed.
use super::*;
use limo_cad_core::{PartPrintIntentDto, Rgba8};

#[test]
#[ignore = "requires LIMO_BAMBU_TEMPLATE and a fresh owned LIMO_BAMBU_QUALIFICATION_DIR; then run the installed slicer separately"]
fn write_five_part_four_plate_native_qualification_fixtures() {
    let input = std::env::var_os("LIMO_BAMBU_TEMPLATE")
        .expect("LIMO_BAMBU_TEMPLATE points to a complete saved 2.8.2.61 project");
    let directory = std::path::PathBuf::from(
        std::env::var_os("LIMO_BAMBU_QUALIFICATION_DIR").expect("owned output directory"),
    );
    assert!(directory.is_absolute() && directory.is_dir());
    let original = std::fs::read(&input).unwrap();
    let mut template = parse_template(&original).unwrap();
    assert_eq!(
        template.summary.objects.len(),
        5,
        "five-part reference template required"
    );
    assert_eq!(
        template.summary.plate_count, 4,
        "four-plate reference template required"
    );
    let config_source = text(&template.entries, CONFIG).unwrap().to_string();
    let config = xml(&config_source).unwrap();
    let removals = config
        .descendants()
        .filter(|n| {
            n.has_tag_name("metadata")
                && n.attribute("key")
                    .is_some_and(|k| SETTING_KEYS.contains(&k))
        })
        .map(|n| (n.range(), String::new()))
        .collect();
    template.entries.insert(
        CONFIG.into(),
        apply_edits(&config_source, removals).unwrap().into_bytes(),
    );
    template.entries.remove(MANIFEST);
    let clean_template = write_archive(&template.entries).unwrap();
    let clean = parse_template(&clean_template).unwrap();
    let mut meshes = Vec::new();
    let mut appearances = Vec::new();
    let mut instances = Vec::new();
    let mut bindings = Vec::new();
    let mut configured = Vec::new();
    let root = xml(text(&clean.entries, ROOT).unwrap()).unwrap();
    for (index, object) in clean.summary.objects.iter().enumerate() {
        assert_eq!(
            object.parts.len(),
            1,
            "qualification fixture must explicitly bind one normal volume per object"
        );
        assert_eq!(
            object.instance_count, 1,
            "fixture expects original five intentional instances"
        );
        let part = &object.parts[0];
        let body_id = BodyId(index as u64 + 1);
        let occurrence_id = index as u64 + 1;
        let target = &clean.targets[&(object.object_id, part.part_id)];
        let mesh_doc = xml(text(&clean.entries, &target.path).unwrap()).unwrap();
        let mesh_node = mesh_doc
            .descendants()
            .find(|n| n.has_tag_name((CORE_NS, "mesh")) && n.range() == target.mesh_range)
            .unwrap();
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for vertex in mesh_node
            .descendants()
            .filter(|n| n.has_tag_name((CORE_NS, "vertex")))
        {
            for (axis, key) in ["x", "y", "z"].into_iter().enumerate() {
                let value = vertex.attribute(key).unwrap().parse::<f64>().unwrap();
                lo[axis] = lo[axis].min(value);
                hi[axis] = hi[axis].max(value);
            }
        }
        let range = &clean.build_ranges[&(object.object_id, 0)];
        let build = root.descendants().find(|n| n.range() == *range).unwrap();
        let world = Matrix::parse(build.attribute("transform"))
            .unwrap()
            .compose(target.component_transform);
        let dimensions: [f64; 3] = std::array::from_fn(|axis| {
            if world.0[8 + axis].abs() > 0.001 {
                hi[axis] - lo[axis]
            } else {
                30.
            }
        });
        let mut mesh = super::tests::cube(body_id.0);
        for point in mesh.positions.as_chunks_mut::<3>().0 {
            for axis in 0..3 {
                point[axis] = (point[axis] / 10. - 0.5) * dimensions[axis];
            }
        }
        mesh.name = format!("Synthetic reference part {}", body_id.0);
        meshes.push(mesh);
        let filament = part
            .settings
            .get("extruder")
            .or_else(|| object.settings.get("extruder"))
            .unwrap()
            .parse::<usize>()
            .unwrap()
            - 1;
        let mut appearance = BodyAppearance::default_for(body_id);
        appearance.filament_type = clean.summary.filament_types[filament].clone();
        let color = clean.summary.filament_colors[filament].trim_start_matches('#');
        appearance.color = Rgba8::opaque(
            u8::from_str_radix(&color[0..2], 16).unwrap(),
            u8::from_str_radix(&color[2..4], 16).unwrap(),
            u8::from_str_radix(&color[4..6], 16).unwrap(),
        );
        appearances.push(appearance);
        instances.push(MeshInstance {
            body_id,
            occurrence_id,
            translation: [0.; 3],
            rotation: [0., 0., 0., 1.],
            visible: true,
        });
        bindings.push(BambuPartBinding {
            body_id,
            occurrence_id,
            object_id: object.object_id,
            instance_id: 0,
            part_id: part.part_id,
        });
        let name = object.name.to_ascii_lowercase();
        if !name.contains("sleeve") {
            let (density, pattern) = if name.contains("adapter") {
                (100., InfillPatternDto::Rectilinear)
            } else if name.contains("auger") {
                (40., InfillPatternDto::Gyroid)
            } else {
                assert!(name.contains("housing"));
                (30., InfillPatternDto::Gyroid)
            };
            configured.push(PartPrintIntentDto {
                body_id,
                settings: PrintSettingsDto {
                    wall_count: Some(6),
                    infill_density_percent: Some(density),
                    infill_pattern: Some(pattern),
                    top_shell_layers: Some(6),
                    bottom_shell_layers: Some(6),
                },
            });
        }
    }
    let namespace = "c832ce9c-765e-4b68-bf99-4b8130142a1c";
    let request = BambuProjectRequest {
        source_document_id: namespace.into(),
        bindings,
        placement: BambuPlacementMode::Template,
        allow_template_appearance: false,
        ..Default::default()
    };
    let structure = limo_cad_assembly::ComponentStructureDto::default();
    let mut intent = PrintIntentDocumentDto {
        source_document_id: Some(namespace.into()),
        ..Default::default()
    };
    for (name, parts) in [("baseline", Vec::new()), ("configured", configured)] {
        intent.parts = parts;
        let output = write_bambu_project(
            &clean_template,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        if name == "configured" {
            if let Some(native_path) = std::env::var_os("LIMO_BAMBU_ROUNDTRIP_TEMPLATE") {
                let native = std::fs::read(native_path).unwrap();
                let refresh_request = BambuProjectRequest {
                    bindings: Vec::new(),
                    refresh_reference: Some(output.report.refresh_reference.clone()),
                    ..request.clone()
                };
                let refreshed = write_bambu_project(
                    &native,
                    &meshes,
                    &appearances,
                    &instances,
                    &structure,
                    &intent,
                    &refresh_request,
                )
                .unwrap();
                std::fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(directory.join("bambu-native-roundtrip-refreshed.3mf"))
                    .unwrap()
                    .write_all(&refreshed.bytes)
                    .unwrap();
                std::fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(directory.join("bambu-native-roundtrip-refreshed.json"))
                    .unwrap()
                    .write_all(&serde_json::to_vec_pretty(&refreshed.report).unwrap())
                    .unwrap();
            }
        }
        let path = directory.join(format!("bambu-synthetic-{name}.3mf"));
        std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .unwrap()
            .write_all(&output.bytes)
            .unwrap();
        let report = serde_json::json!({"original_template_sha256":hash(&original),"qualification_template_sha256":hash(&clean_template),"project_sha256":hash(&output.bytes),"synthetic_geometry":true,"report":output.report});
        std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(directory.join(format!("bambu-synthetic-{name}.json")))
            .unwrap()
            .write_all(&serde_json::to_vec_pretty(&report).unwrap())
            .unwrap();
    }
    assert_eq!(
        hash(&std::fs::read(input).unwrap()),
        hash(&original),
        "source template must remain unchanged"
    );
}

#[test]
#[ignore = "requires complete operator template and a fresh owned qualification directory; slice generated fixtures separately"]
fn write_resolved_repeated_and_thin_native_qualification_fixtures() {
    let input = std::env::var_os("LIMO_BAMBU_TEMPLATE").expect("complete saved source template");
    let directory = std::path::PathBuf::from(
        std::env::var_os("LIMO_BAMBU_QUALIFICATION_DIR").expect("owned output directory"),
    );
    assert!(directory.is_absolute() && directory.is_dir());
    let original = std::fs::read(&input).unwrap();
    let complete = parse_template(&original).unwrap();
    let (fixture, mut meshes, mut appearances, instances, structure, intent, request) =
        super::tests::fixture();
    let mut entries = archive(&fixture).unwrap();
    entries.insert(
        PROFILE.into(),
        serde_json::to_vec(&complete.profile).unwrap(),
    );
    let config = text(&entries, CONFIG)
        .unwrap()
        .replace("first-volume", "7f397df8-a10a-4d87-a0b2-90b73dc18b5d")
        .replace("second-volume", "c153b5f8-e2e7-49c8-a904-29ce82ef632b");
    entries.insert(CONFIG.into(), config.into_bytes());
    let selected_color = complete.summary.filament_colors[0].trim_start_matches('#');
    for appearance in &mut appearances {
        appearance.filament_type = complete.summary.filament_types[0].clone();
        appearance.color = Rgba8::opaque(
            u8::from_str_radix(&selected_color[0..2], 16).unwrap(),
            u8::from_str_radix(&selected_color[2..4], 16).unwrap(),
            u8::from_str_radix(&selected_color[4..6], 16).unwrap(),
        );
    }
    let source = write_archive(&entries).unwrap();
    for (name, thin) in [
        ("resolved-repeated", false),
        ("resolved-thin-six-walls", true),
    ] {
        if thin {
            for point in meshes[0].positions.as_chunks_mut::<3>().0 {
                point[0] *= 0.12;
            }
        }
        let output = write_bambu_project(
            &source,
            &meshes,
            &appearances,
            &instances,
            &structure,
            &intent,
            &request,
        )
        .unwrap();
        std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(directory.join(format!("bambu-{name}.3mf")))
            .unwrap()
            .write_all(&output.bytes)
            .unwrap();
        let report = serde_json::json!({"original_template_sha256":hash(&original),"qualification_template_sha256":hash(&source),"synthetic_geometry":true,"thin_dimension_mm":if thin {Some(1.2)}else{None},"report":output.report});
        std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(directory.join(format!("bambu-{name}.json")))
            .unwrap()
            .write_all(&serde_json::to_vec_pretty(&report).unwrap())
            .unwrap();
    }
    assert_eq!(hash(&std::fs::read(input).unwrap()), hash(&original));
}

#[test]
#[ignore = "requires completed local slices in LIMO_BAMBU_QUALIFICATION_DIR"]
fn verify_resolved_native_geometry_grouping_and_requested_wall_evidence() {
    let directory = std::path::PathBuf::from(
        std::env::var_os("LIMO_BAMBU_QUALIFICATION_DIR").expect("owned completed output directory"),
    );
    let mut results = Vec::new();
    for name in ["resolved-repeated", "resolved-thin-six-walls"] {
        let input = std::fs::read(directory.join(format!("bambu-{name}.3mf"))).unwrap();
        let native =
            std::fs::read(directory.join(format!("{name}-validated/{name}-sliced.3mf"))).unwrap();
        let expected = crate::test_reader::read_package(&input).unwrap();
        let actual = crate::test_reader::read_package(&native).unwrap();
        assert_eq!(expected.len(), 4);
        assert_eq!(actual.len(), 4);
        let group_counts = |meshes: &[crate::test_reader::ModelMesh]| {
            let mut counts = BTreeMap::new();
            for mesh in meshes {
                *counts.entry(mesh.build_item).or_insert(0usize) += 1;
            }
            counts
        };
        assert_eq!(group_counts(&expected), BTreeMap::from([(0, 2), (1, 2)]));
        assert_eq!(group_counts(&actual), group_counts(&expected));
        let bounds = |meshes: &[crate::test_reader::ModelMesh]| {
            let mut bounds: Vec<_> = meshes
                .iter()
                .map(|mesh| {
                    let lo: [f64; 3] = std::array::from_fn(|axis| {
                        mesh.vertices
                            .iter()
                            .map(|v| v[axis])
                            .fold(f64::INFINITY, f64::min)
                    });
                    let hi: [f64; 3] = std::array::from_fn(|axis| {
                        mesh.vertices
                            .iter()
                            .map(|v| v[axis])
                            .fold(f64::NEG_INFINITY, f64::max)
                    });
                    (lo, hi, mesh.triangles.len())
                })
                .collect();
            bounds.sort_by(|a, b| {
                a.0[0]
                    .total_cmp(&b.0[0])
                    .then_with(|| a.0[1].total_cmp(&b.0[1]))
            });
            bounds
        };
        let expected_bounds = bounds(&expected);
        let actual_bounds = bounds(&actual);
        for (left, right) in expected_bounds.iter().zip(&actual_bounds) {
            assert_eq!(left.2, right.2);
            for (x, y) in left
                .0
                .iter()
                .chain(&left.1)
                .zip(right.0.iter().chain(&right.1))
            {
                assert!((x - y).abs() <= 0.001, "native geometry moved: {x} vs {y}");
            }
        }
        let input_entries = archive(&input).unwrap();
        let native_entries = archive(&native).unwrap();
        let requested: Value = serde_json::from_slice(&input_entries[PROFILE]).unwrap();
        let saved: Value = serde_json::from_slice(&native_entries[PROFILE]).unwrap();
        for key in [
            "filament_type",
            "filament_colour",
            "filament_settings_id",
            "support_filament",
            "support_interface_filament",
        ] {
            assert_eq!(
                requested[key], saved[key],
                "material/support identity changed: {key}"
            );
        }
        results.push(serde_json::json!({"fixture":name,"source_sha256":hash(&input),"native_saved_sha256":hash(&native),"normal_volume_occurrences":actual.len(),"multipart_group_part_counts":group_counts(&actual),"expected_world_bounds":expected_bounds,"native_world_bounds":actual_bounds,"world_tolerance_mm":0.001,"material_support_identity_preserved":true}));
    }
    std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(directory.join("native-resolved-readback-evidence.json"))
        .unwrap()
        .write_all(&serde_json::to_vec_pretty(&results).unwrap())
        .unwrap();
}
