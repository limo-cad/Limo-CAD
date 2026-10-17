use limo_cad_core::{MaterialDetails, MaterialProperty, MaterialSource, MaterialValue};
use limo_cad_fem::*;
fn input() -> ThermalStudyInput {
    ThermalStudyInput {
        mesh: ThermalMesh {
            points_m: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            elements: vec![ThermalTet {
                nodes: [0, 1, 2, 3],
                material: 0,
                source_w_m3: 0.,
            }],
        },
        materials: vec![ThermalMaterial {
            details: MaterialDetails {
                kind: "other".into(),
                catalog_id: "test.conductor".into(),
                properties: vec![MaterialProperty {
                    name: "ThermalConductivity".into(),
                    value: MaterialValue::Number(2.),
                    unit: "W/(m*K)".into(),
                    context: "test".into(),
                    source_id: "fixture".into(),
                }],
                sources: vec![MaterialSource {
                    id: "fixture".into(),
                    repository: "test".into(),
                    revision: "1".repeat(40),
                    path: "conductor".into(),
                    sha256: "2".repeat(64),
                    license: "test".into(),
                    author: "test".into(),
                    reference: String::new(),
                }],
                ..Default::default()
            },
            context: Some("test".into()),
            source_id: Some("fixture".into()),
        }],
        prescribed_k: vec![(0, 300.), (1, 310.), (2, 300.), (3, 300.)],
        nodal_load_w: vec![0.; 4],
        surfaces: vec![],
        options: SolveOptions::default(),
    }
}
fn solved() -> ThermalStudy {
    let mut study = ThermalStudy::new(input());
    let completion = study.begin_solve().run();
    assert!(study.finish(completion));
    assert_eq!(study.status(), &ThermalStudyStatus::Solved);
    assert!(study.result_is_current());
    assert_eq!(
        study.result().unwrap().result().element_flux_w_m2,
        vec![[-20., 0., 0.]]
    );
    study
}
#[test]
fn worker_snapshot_retains_catalog_provenance_and_result() {
    let mut study = ThermalStudy::new(input());
    let job = study.begin_solve();
    let completion = std::thread::spawn(move || job.run()).join().unwrap();
    assert!(study.finish(completion));
    let result = study.result().unwrap();
    assert_eq!(result.input(), study.input());
    assert_eq!(
        result.input().materials[0].details.sources[0].sha256,
        "2".repeat(64)
    );
    assert_eq!(result.result().temperature_k, vec![300., 310., 300., 300.]);
}
#[test]
fn edits_cancel_old_work_and_undo_restores_exact_input_freshness() {
    let mut study = solved();
    let original = study.input().clone();
    let old = study.begin_solve();
    let mut edited = original.clone();
    edited.prescribed_k[1].1 = 320.;
    assert!(study.replace_input(edited));
    assert!(old.is_cancelled());
    assert!(!study.result_is_current());
    assert!(!study.finish(old.run()));
    assert_eq!(study.status(), &ThermalStudyStatus::Idle);
    assert_eq!(study.result().unwrap().result().temperature_k[1], 310.);
    study.replace_input(original);
    assert!(study.result_is_current());
}
#[test]
fn completed_old_attempt_cannot_replace_newer_result() {
    let mut study = solved();
    let old = study.begin_solve().run();
    let mut edited = study.input().clone();
    edited.prescribed_k[1].1 = 330.;
    study.replace_input(edited);
    let latest = study.begin_solve().run();
    assert!(study.finish(latest));
    assert!(!study.finish(old));
    assert_eq!(study.result().unwrap().result().temperature_k[1], 330.);
    assert_eq!(study.status(), &ThermalStudyStatus::Solved);
}
#[test]
fn same_input_new_attempt_and_foreign_owner_reject_old_completions() {
    let mut a = solved();
    let mut b = ThermalStudy::new(input());
    let foreign = a.begin_solve().run();
    let b_job = b.begin_solve();
    assert!(!b.finish(foreign));
    assert_eq!(b.status(), &ThermalStudyStatus::Running);
    assert!(b.finish(b_job.run()));
    let old = a.begin_solve().run();
    let latest = a.begin_solve();
    assert!(!a.finish(old));
    assert_eq!(a.status(), &ThermalStudyStatus::Running);
    assert!(a.finish(latest.run()));
}
#[test]
fn cancellation_and_owner_drop_retire_jobs_without_losing_good_result() {
    let mut study = solved();
    let completion = study.begin_solve().run();
    assert!(study.cancel());
    assert!(!study.cancel());
    assert!(!study.finish(completion));
    assert_eq!(study.status(), &ThermalStudyStatus::Cancelled);
    assert!(study.result_is_current());
    let job = study.begin_solve();
    drop(study);
    assert!(job.is_cancelled());
    // A cancelled job can still finish; its retired owner cannot adopt it.
    let _completion = job.run();
}
#[test]
fn invalid_draft_solve_preserves_last_success_and_reports_failure() {
    let mut study = solved();
    let mut bad = study.input().clone();
    bad.materials[0].context = Some("missing".into());
    study.replace_input(bad);
    let completion = study.begin_solve().run();
    assert!(study.finish(completion));
    assert!(matches!(study.status(), ThermalStudyStatus::Failed(_)));
    assert!(!study.result_is_current());
    assert_eq!(study.result().unwrap().result().temperature_k[1], 310.);
    assert_eq!(
        study.result().unwrap().input().materials[0]
            .context
            .as_deref(),
        Some("test")
    );
}
#[test]
fn no_op_edit_preserves_running_job() {
    let mut study = solved();
    let job = study.begin_solve();
    assert!(!study.replace_input(study.input().clone()));
    assert!(!job.is_cancelled());
    assert_eq!(study.status(), &ThermalStudyStatus::Running);
    assert!(study.finish(job.run()));
}
#[test]
fn every_input_category_invalidates_retained_result() {
    let mut study = solved();
    let original = study.input().clone();
    type InputEdit = Box<dyn Fn(&mut ThermalStudyInput)>;
    let changes: Vec<InputEdit> = vec![
        Box::new(|i| i.mesh.points_m[1][0] = 2.),
        Box::new(|i| i.mesh.elements[0].nodes.swap(0, 1)),
        Box::new(|i| i.mesh.elements[0].material = 1),
        Box::new(|i| i.mesh.elements[0].source_w_m3 = 1.),
        Box::new(|i| i.materials[0].details.properties[0].value = MaterialValue::Number(3.)),
        Box::new(|i| i.materials[0].details.sources[0].revision = "3".repeat(40)),
        Box::new(|i| i.materials[0].context = None),
        Box::new(|i| i.materials[0].source_id = None),
        Box::new(|i| i.prescribed_k[0].1 = 301.),
        Box::new(|i| i.nodal_load_w[0] = 1.),
        Box::new(|i| {
            i.surfaces.push(ThermalSurface {
                nodes: [0, 1, 2],
                heat_flux_in_w_m2: 1.,
                convection: None,
            })
        }),
        Box::new(|i| i.options.relative_tolerance = 1e-8),
    ];
    for change in changes {
        let mut edited = original.clone();
        change(&mut edited);
        assert!(study.replace_input(edited));
        assert!(!study.result_is_current());
        study.replace_input(original.clone());
        assert!(study.result_is_current());
    }
}

fn saved_document() -> ThermalStudyDocument {
    ThermalStudyDocument {
        studies: vec![SavedThermalStudy {
            id: 7,
            name: "Thermal study".into(),
            input: input(),
        }],
    }
}
fn raw_wire(document: &ThermalStudyDocument, version: u32) -> Vec<u8> {
    #[derive(serde::Serialize)]
    struct Wire<'a> {
        format: &'static str,
        version: u32,
        studies: &'a [SavedThermalStudy],
    }
    let mut bytes = Vec::new();
    ciborium::into_writer(
        &Wire {
            format: "limo-cad-thermal-studies",
            version,
            studies: &document.studies,
        },
        &mut bytes,
    )
    .unwrap();
    bytes
}
#[test]
fn cbor_roundtrip_reopens_idle_and_recomputes_without_persisting_results() {
    let mut document = saved_document();
    document.studies[0].name = "Heat \u{0394} study".into();
    document.studies[0].input.surfaces.push(ThermalSurface {
        nodes: [0, 1, 2],
        heat_flux_in_w_m2: 2.,
        convection: Some(ThermalConvection {
            coefficient_w_m2_k: 4.,
            ambient_k: 290.,
        }),
    });
    let bytes = document.encode_cbor().unwrap();
    let reopened = ThermalStudyDocument::decode_cbor(&bytes).unwrap();
    assert_eq!(document, reopened);
    assert_eq!(bytes, reopened.encode_cbor().unwrap());
    let mut study = reopened.studies[0].open();
    assert_eq!(study.status(), &ThermalStudyStatus::Idle);
    assert!(study.result().is_none());
    assert!(!study.result_is_current());
    let result = study.begin_solve().run();
    assert!(study.finish(result));
    assert!(study.result_is_current());
    assert_eq!(
        study.result().unwrap().result().temperature_k,
        vec![300., 310., 300., 300.]
    );
    assert_eq!(bytes, reopened.encode_cbor().unwrap());
}
#[test]
fn cbor_preserves_incomplete_draft_without_claiming_a_solution() {
    let mut document = saved_document();
    document.studies[0].input.prescribed_k.clear();
    document.studies[0].input.mesh.elements[0].nodes[0] = 99;
    let reopened = ThermalStudyDocument::decode_cbor(&document.encode_cbor().unwrap()).unwrap();
    assert_eq!(document, reopened);
    let mut study = reopened.studies[0].open();
    let done = study.begin_solve().run();
    study.finish(done);
    assert!(matches!(study.status(), ThermalStudyStatus::Failed(_)));
    assert!(study.result().is_none());
}
#[test]
fn cbor_rejects_new_versions_truncation_trailing_data_and_unknown_fields() {
    let doc = saved_document();
    let bytes = doc.encode_cbor().unwrap();
    for end in 0..bytes.len() {
        assert!(
            ThermalStudyDocument::decode_cbor(&bytes[..end]).is_err(),
            "prefix {end}"
        );
    }
    for version in [0, 2, u32::MAX] {
        assert!(ThermalStudyDocument::decode_cbor(&raw_wire(&doc, version)).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(ThermalStudyDocument::decode_cbor(&trailing).is_err());
    let mut value: ciborium::Value = ciborium::from_reader(bytes.as_slice()).unwrap();
    value
        .as_map_mut()
        .unwrap()
        .push(("future".into(), true.into()));
    let mut unknown = Vec::new();
    ciborium::into_writer(&value, &mut unknown).unwrap();
    assert!(ThermalStudyDocument::decode_cbor(&unknown).is_err());
    let mut value: ciborium::Value = ciborium::from_reader(bytes.as_slice()).unwrap();
    value
        .as_map_mut()
        .unwrap()
        .push(("version".into(), 1.into()));
    let mut duplicate = Vec::new();
    ciborium::into_writer(&value, &mut duplicate).unwrap();
    assert!(ThermalStudyDocument::decode_cbor(&duplicate).is_err());
}
#[test]
fn cbor_storage_validation_rejects_invalid_identity_numeric_data_and_provenance() {
    type Edit = Box<dyn Fn(&mut ThermalStudyDocument)>;
    let edits: Vec<Edit> = vec![
        Box::new(|d| d.studies[0].id = 0),
        Box::new(|d| d.studies.push(d.studies[0].clone())),
        Box::new(|d| d.studies[0].name = " ".into()),
        Box::new(|d| d.studies[0].name = "x".repeat(257)),
        Box::new(|d| d.studies[0].input.mesh.points_m[0][0] = f64::NAN),
        Box::new(|d| d.studies[0].input.nodal_load_w[0] = f64::INFINITY),
        Box::new(|d| d.studies[0].input.materials[0].details.sources[0].sha256 = "invalid".into()),
        Box::new(|d| d.studies[0].input.materials[0].context = Some("x".repeat(513))),
        Box::new(|d| d.studies[0].input.options.absolute_tolerance_w = f64::NEG_INFINITY),
    ];
    for edit in edits {
        let mut doc = saved_document();
        edit(&mut doc);
        assert!(doc.encode_cbor().is_err());
        assert!(ThermalStudyDocument::decode_cbor(&raw_wire(&doc, 1)).is_err());
    }
}
#[test]
fn cbor_limits_bound_input_and_collections() {
    assert!(ThermalStudyDocument::decode_cbor(&vec![0; MAX_THERMAL_STUDY_BYTES + 1]).is_err());
    let mut doc = saved_document();
    doc.studies[0].input.mesh.points_m = vec![[0.; 3]; 65_537];
    assert!(doc.encode_cbor().is_err());
    assert!(ThermalStudyDocument::decode_cbor(&raw_wire(&doc, 1)).is_err());
    let mut doc = saved_document();
    let item = doc.studies[0].clone();
    doc.studies = (1..=65)
        .map(|id| SavedThermalStudy { id, ..item.clone() })
        .collect();
    assert!(doc.encode_cbor().is_err());
    assert!(ThermalStudyDocument::decode_cbor(&raw_wire(&doc, 1)).is_err());
    // Declared huge arrays and deeply nested containers must reject promptly.
    for bytes in [
        vec![0x9b, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff],
        vec![0x81; 100],
    ] {
        assert!(ThermalStudyDocument::decode_cbor(&bytes).is_err());
    }
}

#[test]
fn emitted_studies_conform_to_cddl_and_contract_rejects_future_versions() {
    let contract = include_str!("../../../docs/thermal-study.cddl").trim_start_matches('\u{feff}');
    let mut doc = saved_document();
    doc.studies[0].input.surfaces.push(ThermalSurface {
        nodes: [0, 1, 2],
        heat_flux_in_w_m2: -2.,
        convection: Some(ThermalConvection {
            coefficient_w_m2_k: 4.,
            ambient_k: 290.,
        }),
    });
    for d in [&doc, &ThermalStudyDocument::default()] {
        cddl_cat::validate_cbor_bytes("thermal-studies", contract, &d.encode_cbor().unwrap())
            .unwrap();
    }
    assert!(
        cddl_cat::validate_cbor_bytes("thermal-studies", contract, &raw_wire(&doc, 2)).is_err()
    );
}
#[test]
fn cbor_nested_recursion_limit_is_exercised_within_material_value() {
    let bytes = saved_document().encode_cbor().unwrap();
    let mut value: ciborium::Value = ciborium::from_reader(bytes.as_slice()).unwrap();
    fn field<'a>(v: &'a mut ciborium::Value, name: &str) -> &'a mut ciborium::Value {
        &mut v
            .as_map_mut()
            .unwrap()
            .iter_mut()
            .find(|(k, _)| k.as_text() == Some(name))
            .unwrap()
            .1
    }
    let studies = field(&mut value, "studies");
    let study = &mut studies.as_array_mut().unwrap()[0];
    let input = field(study, "input");
    let materials = field(input, "materials");
    let material = &mut materials.as_array_mut().unwrap()[0];
    let details = field(material, "details");
    let properties = field(details, "properties");
    let property = &mut properties.as_array_mut().unwrap()[0];
    let mut nested = ciborium::Value::Null;
    for _ in 0..40 {
        nested = ciborium::Value::Array(vec![nested]);
    }
    *field(property, "value") = nested;
    let mut encoded = Vec::new();
    ciborium::into_writer(&value, &mut encoded).unwrap();
    let error = ThermalStudyDocument::decode_cbor(&encoded).unwrap_err();
    assert!(error.to_lowercase().contains("recursion"), "{error}");
}

fn project() -> limo_cad_project_file::ProjectArchive {
    limo_cad_project_file::ProjectArchive::new(
        r#"{"format":"limo-cad-project","schema_version":14}"#.into(),
        limo_cad_project_file::SaveMetadata {
            application_version: "test",
            saved_at: "2026-10-17",
        },
    )
    .unwrap()
}
#[test]
fn thermal_intent_survives_project_container_replacement_and_legacy_resave() {
    let mut archive = project();
    assert_eq!(
        ThermalStudyDocument::from_project(&archive).unwrap(),
        ThermalStudyDocument::default()
    );
    archive
        .set_intent("intent/vendor.cbor", Some(b"keep vendor bytes".to_vec()))
        .unwrap();
    let mut doc = saved_document();
    doc.store_in_project(&mut archive).unwrap();
    let first = archive.encode().unwrap();
    let mut reopened = limo_cad_project_file::ProjectArchive::decode(first).unwrap();
    assert_eq!(ThermalStudyDocument::from_project(&reopened).unwrap(), doc);
    // A container-only reader preserves the unknown intent through model save.
    reopened
        .update_model(
            r#"{"format":"limo-cad-project","schema_version":14,"document":{"name":"Renamed"}}"#
                .into(),
            limo_cad_project_file::SaveMetadata {
                application_version: "test",
                saved_at: "later",
            },
        )
        .unwrap();
    reopened = limo_cad_project_file::ProjectArchive::decode(reopened.encode().unwrap()).unwrap();
    assert_eq!(ThermalStudyDocument::from_project(&reopened).unwrap(), doc);
    doc.studies[0].input.prescribed_k[1].1 = 320.;
    doc.store_in_project(&mut reopened).unwrap();
    let latest = limo_cad_project_file::ProjectArchive::decode(reopened.encode().unwrap()).unwrap();
    assert_eq!(ThermalStudyDocument::from_project(&latest).unwrap(), doc);
    assert_eq!(
        latest
            .read_intent("intent/vendor.cbor", 100)
            .unwrap()
            .unwrap(),
        b"keep vendor bytes"
    );
}
#[test]
fn future_or_invalid_thermal_intent_is_not_silently_overwritten() {
    let doc = saved_document();
    for original in [raw_wire(&doc, 2), vec![0xff, 1, 2]] {
        let mut archive = project();
        archive
            .set_intent(THERMAL_STUDY_ENTRY, Some(original.clone()))
            .unwrap();
        assert!(ThermalStudyDocument::from_project(&archive).is_err());
        assert!(doc.store_in_project(&mut archive).is_err());
        assert_eq!(
            archive
                .read_intent(THERMAL_STUDY_ENTRY, MAX_THERMAL_STUDY_BYTES)
                .unwrap()
                .unwrap(),
            original
        );
        assert!(archive.encode().is_ok());
    }
    let mut archive = project();
    doc.store_in_project(&mut archive).unwrap();
    let before = archive
        .read_intent(THERMAL_STUDY_ENTRY, MAX_THERMAL_STUDY_BYTES)
        .unwrap();
    let mut invalid = doc;
    invalid.studies[0].id = 0;
    assert!(invalid.store_in_project(&mut archive).is_err());
    assert_eq!(
        archive
            .read_intent(THERMAL_STUDY_ENTRY, MAX_THERMAL_STUDY_BYTES)
            .unwrap(),
        before
    );
}

#[test]
fn thermal_project_file_roundtrip_uses_existing_atomic_boundary() {
    use limo_cad_project_file::{
        read_binary_file, write_binary_file_atomic, write_binary_file_new, ProjectArchive,
    };
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("study.limo");
    let mut archive = project();
    let mut doc = saved_document();
    doc.store_in_project(&mut archive).unwrap();
    let first = archive.encode().unwrap();
    write_binary_file_new(&path, &first).unwrap();
    let reopened = ProjectArchive::decode(read_binary_file(&path).unwrap()).unwrap();
    assert_eq!(ThermalStudyDocument::from_project(&reopened).unwrap(), doc);
    assert!(write_binary_file_new(&path, b"must not replace").is_err());
    assert_eq!(read_binary_file(&path).unwrap(), first);
    doc.studies[0].name = "Edited study".into();
    doc.store_in_project(&mut archive).unwrap();
    write_binary_file_atomic(&path, &archive.encode().unwrap()).unwrap();
    let reopened = ProjectArchive::decode(read_binary_file(&path).unwrap()).unwrap();
    assert_eq!(ThermalStudyDocument::from_project(&reopened).unwrap(), doc);
    assert!(ThermalStudyDocument::from_project(&reopened)
        .unwrap()
        .studies[0]
        .open()
        .result()
        .is_none());
    assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
}
