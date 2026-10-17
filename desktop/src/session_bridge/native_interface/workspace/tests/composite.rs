use super::*;
use limo_cad_project_file::{ProjectSnapshot, SnapshotIntent};

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

fn replay_archive(fixture: &Fixture) -> ProjectArchive {
    let snapshot = fixture.engine.project_snapshot().unwrap();
    let mut archive = ProjectArchive::new(snapshot.model_json().to_owned(), metadata()).unwrap();
    archive
        .set_intent("intent/replay-future.cbor", Some(vec![0, 255, 7, 42]))
        .unwrap();
    archive
}

#[test]
fn replay_baseline_stays_dirty_and_pathless_until_save_as() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut workspace = DocumentWorkspace::default();
    let initial = observe(&mut workspace, &fixture);
    let archive = replay_archive(&fixture);
    let retained = archive.encode().unwrap();
    let loaded = workspace
        .load_replay_baseline_guarded(&fixture.bridge, &fixture.engine, &initial, archive, || {
            Ok(())
        })
        .unwrap();
    assert_ne!(loaded.context, initial.owner);
    assert_eq!(loaded.value["replay_baseline_loaded"], true);
    assert!(loaded.value.get("opened").is_none());
    let receipt = observe(&mut workspace, &fixture);
    let tab = &workspace.tabs[0];
    assert!(tab.path.is_none());
    assert!(tab.saved.is_none());
    assert_eq!(
        tab.archive
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .encode()
            .unwrap(),
        retained
    );
    assert!(
        workspace
            .summaries(&fixture.bridge, &receipt.owner)
            .unwrap()[0]
            .dirty
    );
    assert_eq!(
        fixture.engine.project_snapshot().unwrap().intents()[0].payload,
        [0, 255, 7, 42]
    );
    let destination = path("replay-baseline-save-as.limo");
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
    assert_eq!(workspace.tabs[0].path.as_ref(), Some(&destination));
    let saved = ProjectArchive::decode(fs::read(&destination).unwrap()).unwrap();
    assert_eq!(
        saved.snapshot().unwrap().intents(),
        fixture.engine.project_snapshot().unwrap().intents()
    );
    assert!(
        !workspace
            .summaries(&fixture.bridge, &receipt.owner)
            .unwrap()[0]
            .dirty
    );
}

#[test]
fn rejected_replay_preserves_owner_file_state_and_model() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    for case in [
        "cancelled",
        "invalid-domain",
        "saving",
        "dirty",
        "saved-nonblank",
    ] {
        let fixture = Fixture::new();
        let mut workspace = DocumentWorkspace::default();
        let initial = observe(&mut workspace, &fixture);
        let mut archive = replay_archive(&fixture);
        if case == "invalid-domain" {
            archive
                .set_intent("intent/thermal-studies.cbor", Some(vec![0xff]))
                .unwrap();
        }
        if case == "dirty" {
            fixture.rename(&initial.owner, "Unsaved name").unwrap();
        }
        if case == "saved-nonblank" {
            fixture
                .bridge
                .apply_native_mutation(
                    &fixture.engine,
                    &initial.owner,
                    "drawing_create_sheet",
                    &json!({"name":"Retained","format":"a4","orientation":"landscape"}),
                    || Ok(()),
                )
                .unwrap();
        }
        let receipt = observe(&mut workspace, &fixture);
        let mut pending = None;
        if case == "saving" || case == "saved-nonblank" {
            let save = workspace
                .prepare_save_guarded(
                    &fixture.bridge,
                    &fixture.engine,
                    &receipt,
                    (path(&format!("replay-rejected-{case}.limo")), false),
                    metadata(),
                    || Ok(()),
                )
                .unwrap();
            if case == "saving" {
                pending = Some(save);
            } else {
                workspace
                    .complete_save(&fixture.bridge, save.write())
                    .unwrap();
            }
        }
        let before = fixture.engine.project_snapshot().unwrap();
        let prior_path = workspace.tabs[0].path.clone();
        let prior_saved = workspace.tabs[0].saved.clone();
        let prior_archive = workspace.tabs[0].archive.clone();
        assert!(
            workspace
                .load_replay_baseline_guarded(
                    &fixture.bridge,
                    &fixture.engine,
                    &receipt,
                    archive,
                    || {
                        if case == "cancelled" {
                            Err("cancelled".into())
                        } else {
                            Ok(())
                        }
                    }
                )
                .is_err(),
            "{case}"
        );
        assert_eq!(observe(&mut workspace, &fixture), receipt, "{case}");
        assert_eq!(fixture.engine.project_snapshot().unwrap(), before, "{case}");
        assert_eq!(workspace.tabs[0].path, prior_path);
        assert_eq!(workspace.tabs[0].saved, prior_saved);
        match (&workspace.tabs[0].archive, prior_archive) {
            (Some(actual), Some(prior)) => assert!(Arc::ptr_eq(actual, &prior)),
            (None, None) => {}
            _ => panic!("archive changed on rejection: {case}"),
        }
        drop(pending);
    }
}
