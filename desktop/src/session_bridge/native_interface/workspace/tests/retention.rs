use super::super::retention::IDLE_LIMIT;
use super::*;
use std::time::Duration;

#[test]
fn native_retention_preserves_dirty_file_archive_and_undo_history() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut workspace = DocumentWorkspace::default();
    let a = observe(&mut workspace, &fixture);
    let destination = path("retained-dirty.limo");
    let save = workspace
        .prepare_save_guarded(
            &fixture.bridge,
            &fixture.engine,
            &a,
            (destination.clone(), false),
            metadata(),
            || Ok(()),
        )
        .unwrap();
    workspace
        .complete_save(&fixture.bridge, save.write())
        .unwrap();
    let archive = workspace.tabs[0].archive.clone().unwrap();
    fixture.rename(&a.owner, "Unsaved after save").unwrap();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &a.owner,
            "drawing_create_sheet",
            &json!({"name":"Retained sheet","format":"a4","orientation":"landscape"}),
            || Ok(()),
        )
        .unwrap();
    let a = observe(&mut workspace, &fixture);
    let model =
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap();
    let b = workspace
        .new_tab_guarded(&fixture.bridge, &fixture.engine, &a, || Ok(()))
        .unwrap();
    assert_eq!(
        workspace
            .evict_inactive(
                &fixture.bridge,
                &fixture.engine,
                &b,
                MemoryPressure::Critical,
                Instant::now(),
                || Ok(())
            )
            .unwrap(),
        1
    );
    let restored = workspace
        .activate_guarded(&fixture.bridge, &fixture.engine, &b, &a.owner, || Ok(()))
        .unwrap();
    assert_eq!(restored, a);
    assert_eq!(
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap(),
        model
    );
    assert_eq!(workspace.tabs[0].path.as_ref(), Some(&destination));
    assert!(Arc::ptr_eq(
        workspace.tabs[0].archive.as_ref().unwrap(),
        &archive
    ));
    assert!(
        workspace
            .summaries(&fixture.bridge, &restored.owner)
            .unwrap()[0]
            .dirty
    );
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &restored.owner, false, || Ok(()))
        .unwrap();
    let undone = observe(&mut workspace, &fixture);
    assert!(fixture.engine.drawing_snapshot().sheets.is_empty());
    assert_eq!(workspace.tabs[0].path.as_ref(), Some(&destination));
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &undone.owner, true, || Ok(()))
        .unwrap();
    let redone = observe(&mut workspace, &fixture);
    assert_eq!(fixture.engine.drawing_snapshot().sheets.len(), 1);
    let save = workspace
        .prepare_save_guarded(
            &fixture.bridge,
            &fixture.engine,
            &redone,
            (destination.clone(), true),
            metadata(),
            || Ok(()),
        )
        .unwrap();
    workspace
        .complete_save(&fixture.bridge, save.write())
        .unwrap();
    let disk = ProjectArchive::decode(fs::read(destination).unwrap()).unwrap();
    let saved: serde_json::Value = serde_json::from_str(disk.model_json()).unwrap();
    assert_eq!(saved["document"]["name"], "Unsaved after save");
}

#[test]
fn native_retention_uses_inactive_lru_idle_time_and_document_receipts() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut workspace = DocumentWorkspace::default();
    let a = observe(&mut workspace, &fixture);
    let b = workspace
        .new_tab_guarded(&fixture.bridge, &fixture.engine, &a, || Ok(()))
        .unwrap();
    let c = workspace
        .new_tab_guarded(&fixture.bridge, &fixture.engine, &b, || Ok(()))
        .unwrap();
    let now = Instant::now();
    workspace.tabs[0].last_used = now - Duration::from_secs(120);
    workspace.tabs[1].last_used = now - Duration::from_secs(60);
    assert!(workspace
        .evict_inactive(
            &fixture.bridge,
            &fixture.engine,
            &c,
            MemoryPressure::Critical,
            now,
            || Err("cancelled".into())
        )
        .is_err());
    assert!(fixture.engine.cold_project_sessions().is_empty());
    assert_eq!(
        workspace
            .evict_inactive(
                &fixture.bridge,
                &fixture.engine,
                &c,
                MemoryPressure::Normal,
                now,
                || Ok(())
            )
            .unwrap(),
        0
    );
    assert_eq!(
        workspace
            .evict_inactive(
                &fixture.bridge,
                &fixture.engine,
                &c,
                MemoryPressure::Constrained,
                now,
                || Ok(())
            )
            .unwrap(),
        1
    );
    assert_eq!(
        fixture.engine.cold_project_sessions(),
        std::slice::from_ref(&a.owner.document_id)
    );
    workspace.tabs[1].last_used = now - IDLE_LIMIT;
    assert_eq!(
        workspace
            .evict_inactive(
                &fixture.bridge,
                &fixture.engine,
                &c,
                MemoryPressure::Normal,
                now,
                || Ok(())
            )
            .unwrap(),
        1
    );
    assert_eq!(fixture.engine.cold_project_sessions().len(), 2);
    fixture.rename(&c.owner, "Current changes").unwrap();
    assert!(workspace
        .evict_inactive(
            &fixture.bridge,
            &fixture.engine,
            &c,
            MemoryPressure::Critical,
            now,
            || Ok(())
        )
        .is_err());
    assert_eq!(
        fixture.engine.active_project_session_id(),
        c.owner.document_id
    );
}

#[test]
fn native_retention_protects_incomplete_sketches_and_in_flight_saves() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut workspace = DocumentWorkspace::default();
    let a = observe(&mut workspace, &fixture);
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &a.owner,
            "sketch_begin",
            &json!({"type":"origin_plane","plane":"xy"}),
            || Ok(()),
        )
        .unwrap();
    let a = observe(&mut workspace, &fixture);
    let sketch = parse_engine_envelope(fixture.engine.engine_call("active_sketch", "")).unwrap();
    let b = workspace
        .new_tab_guarded(&fixture.bridge, &fixture.engine, &a, || Ok(()))
        .unwrap();
    let saving = workspace
        .prepare_save_guarded(
            &fixture.bridge,
            &fixture.engine,
            &b,
            (path("retention-save.limo"), false),
            metadata(),
            || Ok(()),
        )
        .unwrap();
    let c = workspace
        .new_tab_guarded(&fixture.bridge, &fixture.engine, &b, || Ok(()))
        .unwrap();
    assert_eq!(
        workspace
            .evict_inactive(
                &fixture.bridge,
                &fixture.engine,
                &c,
                MemoryPressure::Critical,
                Instant::now(),
                || Ok(())
            )
            .unwrap(),
        0
    );
    drop(saving);
    assert_eq!(
        workspace
            .evict_inactive(
                &fixture.bridge,
                &fixture.engine,
                &c,
                MemoryPressure::Critical,
                Instant::now(),
                || Ok(())
            )
            .unwrap(),
        1
    );
    assert_eq!(
        fixture.engine.cold_project_sessions(),
        [b.owner.document_id]
    );
    workspace
        .activate_guarded(&fixture.bridge, &fixture.engine, &c, &a.owner, || Ok(()))
        .unwrap();
    assert_eq!(
        parse_engine_envelope(fixture.engine.engine_call("active_sketch", "")).unwrap(),
        sketch
    );
}

#[test]
fn native_retention_pressure_uses_conservative_physical_memory_thresholds() {
    let gib = 1024_u64 * 1024 * 1024;
    assert_eq!(
        MemoryPressure::from_physical_memory(0, 0),
        MemoryPressure::Normal
    );
    assert_eq!(
        MemoryPressure::from_physical_memory(4 * gib, gib / 2),
        MemoryPressure::Critical
    );
    assert_eq!(
        MemoryPressure::from_physical_memory(4 * gib, gib),
        MemoryPressure::Constrained
    );
    assert_eq!(
        MemoryPressure::from_physical_memory(4 * gib, gib + 1),
        MemoryPressure::Normal
    );
    assert_eq!(
        MemoryPressure::from_physical_memory(32 * gib, 32 * gib / 20),
        MemoryPressure::Critical
    );
    assert_eq!(
        MemoryPressure::from_physical_memory(32 * gib, 32 * gib / 20 + 1),
        MemoryPressure::Constrained
    );
    assert_eq!(
        MemoryPressure::from_physical_memory(32 * gib, 32 * gib / 10 + 1),
        MemoryPressure::Normal
    );
}
