use super::*;
use limo_cad_project_file::{ProjectSnapshot, SnapshotIntent};
use serde_json::Value;

#[test]
fn native_save_open_retains_known_and_unknown_domain_bytes() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut workspace = DocumentWorkspace::default();
    let initial = observe(&mut workspace, &fixture);
    let model = fixture.engine.project_snapshot().unwrap();
    let mut thermal = Vec::new();
    ciborium::into_writer(
        &json!({
            "format":"limo-cad-thermal-studies", "version":1, "studies":[]
        }),
        &mut thermal,
    )
    .unwrap();
    let snapshot = ProjectSnapshot::new(
        model.model_json().to_owned(),
        vec![
            SnapshotIntent {
                name: "intent/thermal-studies.cbor".into(),
                payload: thermal,
            },
            SnapshotIntent {
                name: "intent/future-domain.cbor".into(),
                payload: vec![0, 255, 7, 42],
            },
        ],
    )
    .unwrap();
    fixture
        .bridge
        .apply_native_snapshot_at(
            &fixture.engine,
            &initial.owner,
            initial.revision,
            &snapshot,
            || Ok(()),
        )
        .unwrap();
    let mut receipt = observe(&mut workspace, &fixture);
    for index in 0..2 {
        let destination = path(&format!("composite-roundtrip-{index}.limo"));
        let prepared = workspace
            .prepare_save_guarded(
                &fixture.bridge,
                &fixture.engine,
                &receipt,
                (destination.clone(), false),
                metadata(),
                || Ok(()),
            )
            .unwrap();
        workspace
            .complete_save(&fixture.bridge, prepared.write())
            .unwrap();
        let disk = ProjectArchive::decode(fs::read(&destination).unwrap()).unwrap();
        assert_eq!(disk.snapshot().unwrap().intents(), snapshot.intents());
        let opened = workspace
            .open_guarded(
                &fixture.bridge,
                &fixture.engine,
                &receipt,
                destination,
                true,
                || Ok(()),
            )
            .unwrap();
        assert_ne!(opened.context, receipt.owner);
        assert_eq!(
            fixture.engine.project_snapshot().unwrap().intents(),
            snapshot.intents()
        );
        receipt = observe(&mut workspace, &fixture);
        assert!(
            !workspace
                .summaries(&fixture.bridge, &receipt.owner)
                .unwrap()[0]
                .dirty
        );
    }
}

#[test]
fn native_legacy_open_and_resave_preserve_complete_project_intent() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut workspace = DocumentWorkspace::default();
    let initial = observe(&mut workspace, &fixture);
    let current: Value =
        serde_json::from_str(fixture.engine.project_snapshot().unwrap().model_json()).unwrap();
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/testPiece.nbcad");
    let mut archive = ProjectArchive::decode(fs::read(source).unwrap()).unwrap();
    let legacy_model = archive.model_json().to_owned();
    let mut thermal = Vec::new();
    ciborium::into_writer(
        &json!({"format":"limo-cad-thermal-studies","version":1,"studies":[]}),
        &mut thermal,
    )
    .unwrap();
    let intents = vec![
        SnapshotIntent {
            name: "intent/future-domain.cbor".into(),
            payload: vec![0, 255, 7, 42],
        },
        SnapshotIntent {
            name: "intent/thermal-studies.cbor".into(),
            payload: thermal,
        },
    ];
    for intent in &intents {
        archive
            .set_intent(&intent.name, Some(intent.payload.clone()))
            .unwrap();
    }
    let legacy_bytes = archive.encode().unwrap();
    let legacy_path = path("legacy-source.nbcad");
    fs::write(&legacy_path, &legacy_bytes).unwrap();
    let opened = workspace
        .open_guarded(
            &fixture.bridge,
            &fixture.engine,
            &initial,
            legacy_path.clone(),
            true,
            || Ok(()),
        )
        .unwrap();
    assert_ne!(opened.context, initial.owner);
    let snapshot = fixture.engine.project_snapshot().unwrap();
    assert_eq!(snapshot.intents(), intents.as_slice());
    let migrated: Value = serde_json::from_str(snapshot.model_json()).unwrap();
    assert_eq!(migrated["format"], limo_cad_project_file::PROJECT_FORMAT);
    assert_eq!(migrated["schema_version"], current["schema_version"]);
    assert_eq!(migrated["document"]["name"], "testPiece");
    assert_eq!(migrated["sketches"].as_array().unwrap().len(), 1);
    assert_eq!(migrated["extrudes"].as_array().unwrap().len(), 1);
    assert_eq!(
        workspace.tabs[0]
            .archive
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .model_json(),
        legacy_model
    );
    fixture
        .rename(&opened.context, "Edited legacy project")
        .unwrap();
    let receipt = observe(&mut workspace, &fixture);
    let saved_path = path("legacy-migrated.limo");
    let prepared = workspace
        .prepare_save_guarded(
            &fixture.bridge,
            &fixture.engine,
            &receipt,
            (saved_path.clone(), false),
            metadata(),
            || Ok(()),
        )
        .unwrap();
    workspace
        .complete_save(&fixture.bridge, prepared.write())
        .unwrap();
    let saved = ProjectArchive::decode(fs::read(&saved_path).unwrap()).unwrap();
    let saved_snapshot = saved.snapshot().unwrap();
    assert_eq!(saved_snapshot.intents(), intents.as_slice());
    assert_eq!(
        saved.manifest()["format"],
        limo_cad_project_file::PROJECT_FORMAT
    );
    assert_eq!(fs::read(&legacy_path).unwrap(), legacy_bytes);
    let receipt = observe(&mut workspace, &fixture);
    workspace
        .open_guarded(
            &fixture.bridge,
            &fixture.engine,
            &receipt,
            saved_path,
            true,
            || Ok(()),
        )
        .unwrap();
    let reopened = fixture.engine.project_snapshot().unwrap();
    assert_eq!(reopened.intents(), intents.as_slice());
    let model: Value = serde_json::from_str(reopened.model_json()).unwrap();
    assert_eq!(model["document"]["name"], "Edited legacy project");
    assert_eq!(model["schema_version"], current["schema_version"]);
}

#[test]
fn invalid_domain_open_preserves_live_owner_archive_and_path() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut workspace = DocumentWorkspace::default();
    let initial = observe(&mut workspace, &fixture);
    fixture.rename(&initial.owner, "Keep my work").unwrap();
    let receipt = observe(&mut workspace, &fixture);
    let destination = path("composite-original.limo");
    let prepared = workspace
        .prepare_save_guarded(
            &fixture.bridge,
            &fixture.engine,
            &receipt,
            (destination.clone(), false),
            metadata(),
            || Ok(()),
        )
        .unwrap();
    workspace
        .complete_save(&fixture.bridge, prepared.write())
        .unwrap();
    let before = fixture.engine.project_snapshot().unwrap();
    let summaries = workspace
        .summaries(&fixture.bridge, &receipt.owner)
        .unwrap();
    let retained = workspace.tabs[0].archive.as_ref().unwrap().clone();
    let mut invalid = ProjectArchive::new(before.model_json().to_owned(), metadata()).unwrap();
    invalid
        .set_intent("intent/thermal-studies.cbor", Some(vec![0xff]))
        .unwrap();
    let invalid_path = path("composite-invalid.limo");
    fs::write(&invalid_path, invalid.encode().unwrap()).unwrap();
    assert!(workspace
        .open_guarded(
            &fixture.bridge,
            &fixture.engine,
            &receipt,
            invalid_path,
            true,
            || Ok(())
        )
        .is_err());
    assert_eq!(observe(&mut workspace, &fixture), receipt);
    assert_eq!(fixture.engine.project_snapshot().unwrap(), before);
    assert_eq!(workspace.tabs[0].path.as_ref(), Some(&destination));
    assert!(Arc::ptr_eq(
        workspace.tabs[0].archive.as_ref().unwrap(),
        &retained
    ));
    let after = workspace
        .summaries(&fixture.bridge, &receipt.owner)
        .unwrap();
    assert_eq!(after[0].dirty, summaries[0].dirty);
    assert_eq!(after[0].path, summaries[0].path);
}

#[test]
fn snapshot_replacement_rejects_stale_revision_before_loading() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut workspace = DocumentWorkspace::default();
    let stale = observe(&mut workspace, &fixture);
    let candidate = fixture.engine.project_snapshot().unwrap();
    fixture.rename(&stale.owner, "Newer work").unwrap();
    let current = observe(&mut workspace, &fixture);
    let before = fixture.engine.project_snapshot().unwrap();
    assert!(fixture
        .bridge
        .apply_native_snapshot_at(
            &fixture.engine,
            &stale.owner,
            stale.revision,
            &candidate,
            || Ok(())
        )
        .is_err());
    assert_eq!(observe(&mut workspace, &fixture), current);
    assert_eq!(fixture.engine.project_snapshot().unwrap(), before);
}

#[test]
fn clean_sketch_requires_matching_saved_domain_intent() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    for case in ["matching", "different", "missing"] {
        let fixture = Fixture::new();
        let mut workspace = DocumentWorkspace::default();
        let initial = observe(&mut workspace, &fixture);
        let mutate = |owner: &DocumentContext, operation: &str, arguments| {
            fixture
                .bridge
                .apply_native_mutation(&fixture.engine, owner, operation, &arguments, || Ok(()))
                .unwrap()
        };
        mutate(
            &initial.owner,
            "sketch_begin",
            json!({"type":"origin_plane","plane":"xy"}),
        );
        mutate(
            &initial.owner,
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":10.,"y":10.},"ctrl_held":false}),
        );
        mutate(&initial.owner, "sketch_finish", json!({}));
        let receipt = observe(&mut workspace, &fixture);
        let original = fixture.engine.project_snapshot().unwrap();
        let mut thermal = Vec::new();
        ciborium::into_writer(
            &json!({"format":"limo-cad-thermal-studies","version":1,"studies":[]}),
            &mut thermal,
        )
        .unwrap();
        let snapshot = ProjectSnapshot::new(
            original.model_json().to_owned(),
            vec![
                SnapshotIntent {
                    name: "intent/thermal-studies.cbor".into(),
                    payload: thermal,
                },
                SnapshotIntent {
                    name: "intent/future-domain.cbor".into(),
                    payload: vec![0, 255, 7],
                },
            ],
        )
        .unwrap();
        fixture
            .bridge
            .apply_native_snapshot_at(
                &fixture.engine,
                &receipt.owner,
                receipt.revision,
                &snapshot,
                || Ok(()),
            )
            .unwrap();
        let clean = observe(&mut workspace, &fixture);
        let save = workspace
            .prepare_save_guarded(
                &fixture.bridge,
                &fixture.engine,
                &clean,
                (path(&format!("clean-domain-{case}.limo")), false),
                metadata(),
                || Ok(()),
            )
            .unwrap();
        workspace
            .complete_save(&fixture.bridge, save.write())
            .unwrap();
        // Deliberately vary only the saved archive fixture: the live owner,
        // revision, model and archive Arc remain identical across these cases.
        let archive = workspace.tabs[0].archive.clone().unwrap();
        {
            let mut archive = archive.lock().unwrap();
            let saved = archive.snapshot().unwrap();
            let mut intents = saved.intents().to_vec();
            if case == "different" {
                intents
                    .iter_mut()
                    .find(|entry| entry.name == "intent/future-domain.cbor")
                    .unwrap()
                    .payload
                    .push(42);
            } else if case == "missing" {
                intents.retain(|entry| entry.name != "intent/future-domain.cbor");
            }
            let saved = ProjectSnapshot::new(format!("{}\n", saved.model_json()), intents).unwrap();
            *archive = archive.with_snapshot(&saved, metadata()).unwrap();
        }
        workspace
            .begin_clean_sketch(&fixture.bridge, &fixture.engine, &clean)
            .unwrap();
        mutate(&clean.owner, "sketch_edit", json!({"name":"Sketch1"}));
        mutate(&clean.owner, "sketch_finish", json!({}));
        let finished = observe(&mut workspace, &fixture);
        let reconciled = workspace
            .finish_clean_sketch(&fixture.bridge, &fixture.engine, &finished, false)
            .unwrap();
        assert_eq!(reconciled, case == "matching", "{case}");
        assert_eq!(
            workspace
                .summaries(&fixture.bridge, &finished.owner)
                .unwrap()[0]
                .dirty,
            case != "matching",
            "{case}"
        );
        assert_eq!(
            fixture.engine.project_snapshot().unwrap().intents(),
            snapshot.intents()
        );
    }
}
