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
