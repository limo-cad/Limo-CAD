use super::super::tests::Fixture;
use super::*;
use std::fs;

fn metadata() -> SaveMetadata<'static> {
    SaveMetadata {
        application_version: "test",
        saved_at: "2026-09-13T00:00:00.000Z",
    }
}
fn path(name: &str) -> PathBuf {
    let path = crate::session_bridge::session_root().join("workspace-files");
    fs::create_dir_all(&path).unwrap();
    path.join(name)
}
fn observe(workspace: &mut DocumentWorkspace, fixture: &Fixture) -> DocumentReceipt {
    workspace
        .observe(&fixture.bridge, &fixture.engine, "main")
        .unwrap()
}

#[test]
fn snapshot_history_keeps_the_file_destination_but_retires_save_receipts() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut workspace = DocumentWorkspace::default();
    let initial = observe(&mut workspace, &fixture);
    let destination = path("history-destination.limo");
    let save = workspace
        .prepare_save_guarded(
            &fixture.bridge,
            &fixture.engine,
            &initial,
            (destination.clone(), false),
            metadata(),
            || Ok(()),
        )
        .unwrap();
    workspace
        .complete_save(&fixture.bridge, save.write())
        .unwrap();
    let archive = workspace.tabs[0].archive.clone().unwrap();
    let delayed = workspace
        .prepare_save_guarded(
            &fixture.bridge,
            &fixture.engine,
            &initial,
            (path("old-save-as.limo"), false),
            metadata(),
            || Ok(()),
        )
        .unwrap();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &initial.owner,
            "drawing_create_sheet",
            &json!({"name":"History sheet","format":"a4","orientation":"landscape"}),
            || Ok(()),
        )
        .unwrap();
    for redo in [false, true, false, true] {
        fixture
            .bridge
            .apply_native_history(&fixture.engine, &fixture.owner(), redo, || Ok(()))
            .unwrap();
    }
    let restored = observe(&mut workspace, &fixture);
    assert_ne!(restored.owner, initial.owner);
    let summary = &workspace
        .summaries(&fixture.bridge, &restored.owner)
        .unwrap()[0];
    assert_eq!(summary.path.as_ref(), Some(&destination));
    assert!(summary.dirty);
    assert!(Arc::ptr_eq(
        workspace.tabs[0].archive.as_ref().unwrap(),
        &archive
    ));
    assert!(workspace
        .complete_save(&fixture.bridge, delayed.write())
        .is_err());
    assert_eq!(workspace.tabs[0].path.as_ref(), Some(&destination));
    let save = workspace
        .prepare_save_guarded(
            &fixture.bridge,
            &fixture.engine,
            &restored,
            (destination.clone(), true),
            metadata(),
            || Ok(()),
        )
        .unwrap();
    workspace
        .complete_save(&fixture.bridge, save.write())
        .unwrap();
    assert!(
        !workspace
            .summaries(&fixture.bridge, &restored.owner)
            .unwrap()[0]
            .dirty
    );
    let disk = ProjectArchive::decode(fs::read(&destination).unwrap()).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(disk.model_json()).unwrap(),
        serde_json::from_str::<serde_json::Value>(
            parse_engine_envelope(fixture.engine.engine_call("project_export_model", ""))
                .unwrap()
                .as_str()
                .unwrap()
        )
        .unwrap()
    );
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &restored.owner,
            "cad_load_project_model",
            &json!({"model_json":disk.model_json()}),
            || Ok(()),
        )
        .unwrap();
    let replaced = observe(&mut workspace, &fixture);
    assert_ne!(replaced.owner, restored.owner);
    assert!(workspace.tabs[0].path.is_none());
    assert!(workspace.tabs[0].archive.is_none());
}

#[test]
fn save_completion_stays_with_its_source_tab_and_keeps_later_edits_dirty() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut workspace = DocumentWorkspace::default();
    let first = observe(&mut workspace, &fixture);
    fixture.rename(&first.owner, "Source A").unwrap();
    let capture = observe(&mut workspace, &fixture);
    let saved_path = path("source-a.limo");
    let save = workspace
        .prepare_save_guarded(
            &fixture.bridge,
            &fixture.engine,
            &capture,
            (saved_path.clone(), false),
            metadata(),
            || Ok(()),
        )
        .unwrap();
    fixture
        .rename(&capture.owner, "A changed during Save")
        .unwrap();
    let changed = observe(&mut workspace, &fixture);
    let second = workspace
        .new_tab_guarded(&fixture.bridge, &fixture.engine, &changed, || Ok(()))
        .unwrap();
    fixture.rename(&second.owner, "Source B").unwrap();
    let second = observe(&mut workspace, &fixture);
    workspace
        .complete_save(&fixture.bridge, save.write())
        .unwrap();
    assert_eq!(fixture.engine.document_snapshot().name, "Source B");
    let summaries = workspace.summaries(&fixture.bridge, &second.owner).unwrap();
    let a = summaries
        .iter()
        .find(|tab| tab.owner == capture.owner)
        .unwrap();
    let b = summaries
        .iter()
        .find(|tab| tab.owner == second.owner)
        .unwrap();
    assert_eq!(a.path.as_ref(), Some(&saved_path));
    assert!(a.dirty);
    assert!(!a.active);
    assert!(b.path.is_none());
    assert!(b.dirty);
    assert!(b.active);
    let archive = ProjectArchive::decode(fs::read(saved_path).unwrap()).unwrap();
    let saved: serde_json::Value = serde_json::from_str(archive.model_json()).unwrap();
    assert_eq!(saved["document"]["name"], "Source A");
}

#[test]
fn failed_or_cancelled_save_preserves_destination_metadata_and_releases_its_lease() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut workspace = DocumentWorkspace::default();
    let owner = observe(&mut workspace, &fixture);
    fixture.rename(&owner.owner, "Unsaved work").unwrap();
    let owner = observe(&mut workspace, &fixture);
    let folder = path("cannot-replace.limo");
    fs::create_dir(&folder).unwrap();
    let cancelled = workspace
        .prepare_save_guarded(
            &fixture.bridge,
            &fixture.engine,
            &owner,
            (folder.clone(), true),
            metadata(),
            || Ok(()),
        )
        .unwrap();
    assert!(workspace
        .prepare_save_guarded(
            &fixture.bridge,
            &fixture.engine,
            &owner,
            (path("second.limo"), false),
            metadata(),
            || Ok(())
        )
        .is_err());
    drop(cancelled);
    let work = workspace
        .prepare_save_guarded(
            &fixture.bridge,
            &fixture.engine,
            &owner,
            (folder, true),
            metadata(),
            || Ok(()),
        )
        .unwrap();
    assert!(workspace
        .complete_save(&fixture.bridge, work.write())
        .is_err());
    let tab = &workspace.summaries(&fixture.bridge, &owner.owner).unwrap()[0];
    assert!(tab.path.is_none());
    assert!(tab.dirty);
    assert!(!tab.saving);
    let work = workspace
        .prepare_save_guarded(
            &fixture.bridge,
            &fixture.engine,
            &owner,
            (path("good.limo"), false),
            metadata(),
            || Ok(()),
        )
        .unwrap();
    workspace
        .complete_save(&fixture.bridge, work.write())
        .unwrap();
    assert!(!workspace.summaries(&fixture.bridge, &owner.owner).unwrap()[0].dirty);
}

#[test]
fn delayed_save_cannot_adopt_a_path_or_clean_a_same_tab_replacement() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut workspace = DocumentWorkspace::default();
    let owner = observe(&mut workspace, &fixture);
    let work = workspace
        .prepare_save_guarded(
            &fixture.bridge,
            &fixture.engine,
            &owner,
            (path("old-document.limo"), false),
            metadata(),
            || Ok(()),
        )
        .unwrap();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner.owner,
            "cad_new_project",
            &json!({}),
            || Ok(()),
        )
        .unwrap();
    assert!(workspace
        .complete_save(&fixture.bridge, work.write())
        .is_err());
    let replacement = observe(&mut workspace, &fixture);
    let tab = &workspace
        .summaries(&fixture.bridge, &replacement.owner)
        .unwrap()[0];
    assert!(tab.path.is_none());
    assert!(tab.dirty);
}

#[test]
fn create_only_save_never_overwrites_a_file_that_appears_after_preparation() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut workspace = DocumentWorkspace::default();
    let receipt = observe(&mut workspace, &fixture);
    let destination = path("create-only.limo");
    let work = workspace
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
        .complete_save(&fixture.bridge, work.write())
        .unwrap();
    fixture.rename(&receipt.owner, "New unsaved edits").unwrap();
    let changed = observe(&mut workspace, &fixture);
    fs::remove_file(&destination).unwrap();
    let work = workspace
        .prepare_save_guarded(
            &fixture.bridge,
            &fixture.engine,
            &changed,
            (destination.clone(), false),
            metadata(),
            || Ok(()),
        )
        .unwrap();
    fs::write(&destination, b"Created by someone else while Save waited").unwrap();
    assert!(workspace
        .complete_save(&fixture.bridge, work.write())
        .is_err());
    assert_eq!(
        fs::read(&destination).unwrap(),
        b"Created by someone else while Save waited"
    );
    assert!(
        workspace
            .summaries(&fixture.bridge, &changed.owner)
            .unwrap()[0]
            .dirty
    );
}

#[test]
fn new_tabs_retain_independent_engines_and_close_requires_the_exact_dirty_owner() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut workspace = DocumentWorkspace::default();
    let a = observe(&mut workspace, &fixture);
    fixture.rename(&a.owner, "Retained A").unwrap();
    let a = observe(&mut workspace, &fixture);
    let b = workspace
        .new_tab_guarded(&fixture.bridge, &fixture.engine, &a, || Ok(()))
        .unwrap();
    fixture.rename(&b.owner, "Retained B").unwrap();
    let b = observe(&mut workspace, &fixture);
    let a = workspace
        .activate_guarded(&fixture.bridge, &fixture.engine, &b, &a.owner, || Ok(()))
        .unwrap();
    assert_eq!(fixture.engine.document_snapshot().name, "Retained A");
    assert!(workspace
        .close_active_guarded(&fixture.bridge, &fixture.engine, &a, false, || Ok(()))
        .is_err());
    fixture
        .rename(&a.owner, "A changed after confirmation")
        .unwrap();
    assert!(workspace
        .close_active_guarded(&fixture.bridge, &fixture.engine, &a, true, || Ok(()))
        .is_err());
    let a = observe(&mut workspace, &fixture);
    let b = workspace
        .close_active_guarded(&fixture.bridge, &fixture.engine, &a, true, || Ok(()))
        .unwrap();
    assert_eq!(fixture.engine.document_snapshot().name, "Retained B");
    let blank = workspace
        .close_active_guarded(&fixture.bridge, &fixture.engine, &b, true, || Ok(()))
        .unwrap();
    assert_eq!(workspace.tabs.len(), 1);
    assert_eq!(fixture.engine.document_snapshot().name, "Untitled");
    assert!(!workspace.summaries(&fixture.bridge, &blank.owner).unwrap()[0].dirty);
}

#[test]
fn rejected_open_keeps_current_model_path_and_incarnation_then_valid_open_replaces_them() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut workspace = DocumentWorkspace::default();
    let owner = observe(&mut workspace, &fixture);
    let good_path = path("source.limo");
    let work = workspace
        .prepare_save_guarded(
            &fixture.bridge,
            &fixture.engine,
            &owner,
            (good_path.clone(), false),
            metadata(),
            || Ok(()),
        )
        .unwrap();
    workspace
        .complete_save(&fixture.bridge, work.write())
        .unwrap();
    fixture.rename(&owner.owner, "Valuable local work").unwrap();
    let changed = observe(&mut workspace, &fixture);
    assert!(workspace
        .open_guarded(
            &fixture.bridge,
            &fixture.engine,
            &changed,
            good_path.clone(),
            false,
            || Ok(())
        )
        .is_err());
    let mut unsupported: serde_json::Value = serde_json::from_str(
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", ""))
            .unwrap()
            .as_str()
            .unwrap(),
    )
    .unwrap();
    unsupported["schema_version"] = json!(9999);
    let invalid = ProjectArchive::new(unsupported.to_string(), metadata()).unwrap();
    let invalid_path = path("unsupported.limo");
    fs::write(&invalid_path, invalid.encode().unwrap()).unwrap();
    assert!(workspace
        .open_guarded(
            &fixture.bridge,
            &fixture.engine,
            &changed,
            invalid_path,
            true,
            || Ok(())
        )
        .is_err());
    assert_eq!(fixture.owner(), changed.owner);
    assert_eq!(
        fixture.engine.document_snapshot().name,
        "Valuable local work"
    );
    assert_eq!(workspace.tabs[0].path.as_ref(), Some(&good_path));
    let opened = workspace
        .open_guarded(
            &fixture.bridge,
            &fixture.engine,
            &changed,
            good_path,
            true,
            || Ok(()),
        )
        .unwrap();
    assert_ne!(opened.context, changed.owner);
    assert_eq!(fixture.engine.document_snapshot().name, "Untitled");
    assert!(
        !workspace
            .summaries(&fixture.bridge, &opened.context)
            .unwrap()[0]
            .dirty
    );
}

mod retention;
