use super::super::tests::Fixture;
use super::*;

mod attached_reads;

fn edit(fixture: &Fixture, owner: &DocumentContext, operation: &str, arguments: Value) {
    fixture
        .bridge
        .apply_native_mutation(&fixture.engine, owner, operation, &arguments, || Ok(()))
        .unwrap();
}

fn part(fixture: &Fixture) -> DocumentContext {
    let owner = fixture.owner();
    edit(
        fixture,
        &owner,
        "sketch_begin",
        json!({"type":"origin_plane","plane":"xy"}),
    );
    edit(
        fixture,
        &owner,
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":-10.,"y":-10.},"p2":{"x":10.,"y":10.},"ctrl_held":false}),
    );
    edit(fixture, &owner, "sketch_finish", json!({}));
    edit(
        fixture,
        &owner,
        "solid_extrude",
        json!({"sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":10.},"taper_angle_deg":0.,"flip":false,"target_body_ids":[]}),
    );
    owner
}

fn model(fixture: &Fixture) -> Value {
    let raw =
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap();
    serde_json::from_str(raw.as_str().unwrap()).unwrap()
}

#[test]
fn queued_history_requires_its_original_revision_and_does_not_undo_an_intervening_edit() {
    let _lock = super::super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = part(&fixture);
    let revision = fixture
        .bridge
        .engine_revision_for_window("main")
        .unwrap()
        .unwrap();
    fixture.rename(&owner, "Intervening edit").unwrap();
    let current = model(&fixture);
    let mut validated = false;
    assert!(fixture
        .bridge
        .apply_native_history_at(&fixture.engine, &owner, revision, false, || {
            validated = true;
            Ok(())
        })
        .unwrap_err()
        .contains("design changed"));
    assert!(
        !validated,
        "The stale revision is rejected before dispatch authorization"
    );
    assert_eq!(model(&fixture), current);
    assert_eq!(fixture.engine.document_snapshot().features.len(), 2);
    let revision = fixture
        .bridge
        .engine_revision_for_window("main")
        .unwrap()
        .unwrap();
    fixture
        .bridge
        .apply_native_history_at(&fixture.engine, &owner, revision, false, || Ok(()))
        .unwrap();
    assert_eq!(fixture.engine.document_snapshot().features.len(), 1);
}

#[test]
fn latest_undo_removes_features_and_redo_rebuilds_the_exact_editable_model_with_new_ownership() {
    let _lock = super::super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = part(&fixture);
    let original = model(&fixture);
    assert_eq!(fixture.engine.document_snapshot().features.len(), 2);
    assert_eq!(
        fixture
            .bridge
            .native_history_available(&fixture.engine, &owner)
            .unwrap(),
        (true, false)
    );

    let deleted = fixture
        .bridge
        .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
        .unwrap();
    assert_eq!(deleted.context, owner);
    assert_eq!(
        fixture.engine.document_snapshot().features.len(),
        1,
        "latest Undo must delete, not merely hide by rollback"
    );
    assert!(fixture.engine.viewport_snapshot().2.bodies.is_empty());
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
        .unwrap();
    assert!(fixture.engine.document_snapshot().features.is_empty());
    assert_eq!(
        fixture
            .bridge
            .native_history_available(&fixture.engine, &owner)
            .unwrap(),
        (false, true)
    );

    let first = fixture
        .bridge
        .apply_native_history(&fixture.engine, &owner, true, || Ok(()))
        .unwrap();
    assert_ne!(first.context.epoch, owner.epoch);
    assert_eq!(fixture.engine.document_snapshot().features.len(), 1);
    assert!(
        fixture
            .bridge
            .apply_native_history(&fixture.engine, &owner, true, || Ok(()))
            .is_err(),
        "old queued controls cannot follow Redo replacement"
    );
    let second = fixture
        .bridge
        .apply_native_history(&fixture.engine, &first.context, true, || Ok(()))
        .unwrap();
    assert_ne!(first.context.epoch, second.context.epoch);
    assert_eq!(model(&fixture), original);
    assert_eq!(fixture.engine.viewport_snapshot().2.bodies.len(), 1);
    assert_eq!(
        fixture
            .bridge
            .native_history_available(&fixture.engine, &second.context)
            .unwrap(),
        (true, false)
    );
}

#[test]
fn sketch_history_and_rollback_markers_use_their_real_engine_paths() {
    let _lock = super::super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    edit(
        &fixture,
        &owner,
        "sketch_begin",
        json!({"type":"origin_plane","plane":"xy"}),
    );
    edit(
        &fixture,
        &owner,
        "sketch_add_rectangle",
        json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":24.,"y":16.},"ctrl_held":true}),
    );
    let before = parse_engine_envelope(fixture.engine.engine_call("active_sketch", "")).unwrap();
    assert!(
        fixture
            .bridge
            .native_history_available(&fixture.engine, &owner)
            .unwrap()
            .0
    );
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
        .unwrap();
    let undone = parse_engine_envelope(fixture.engine.engine_call("active_sketch", "")).unwrap();
    assert!(
        undone["entities"].as_array().unwrap().len() < before["entities"].as_array().unwrap().len()
    );
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &owner, true, || Ok(()))
        .unwrap();
    let redone = parse_engine_envelope(fixture.engine.engine_call("active_sketch", "")).unwrap();
    assert_eq!(redone["entities"], before["entities"]);
    assert_eq!(fixture.owner(), owner);

    edit(&fixture, &owner, "sketch_finish", json!({}));
    edit(
        &fixture,
        &owner,
        "solid_extrude",
        json!({"sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":10.},"taper_angle_deg":0.,"flip":false,"target_body_ids":[]}),
    );
    edit(
        &fixture,
        &owner,
        "solid_set_rollback",
        json!({"rollback_index":1}),
    );
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
        .unwrap();
    assert_eq!(fixture.engine.document_snapshot().rollback_index, 0);
    assert_eq!(fixture.engine.document_snapshot().features.len(), 2);
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &owner, true, || Ok(()))
        .unwrap();
    assert_eq!(fixture.engine.document_snapshot().rollback_index, 1);
    assert_eq!(fixture.engine.document_snapshot().features.len(), 2);
    assert_eq!(
        fixture.owner(),
        owner,
        "rollback cursor moves do not replace the document"
    );
}

#[test]
fn unchanged_load_failure_retains_history_and_cancel_or_branch_edits_cannot_consume_it() {
    let _lock = super::super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = part(&fixture);
    let original = model(&fixture);
    {
        // Inject one invalid snapshot to exercise the real unchanged-rejection
        // marker, not a mocked loader or a JSON substring classification.
        let mut publishers = fixture.bridge.publishers.lock().unwrap();
        let project = publishers.get_mut(&owner.window_id).unwrap().active_mut();
        let current = state(&owner, project);
        let previous = HistoryState {
            context: owner.clone(),
            engine_revision: current.engine_revision - 1,
        };
        let ticket = project
            .native_history
            .prepare_undo(&previous, "invalid serialized model".into())
            .unwrap();
        project.native_history.commit_undo(ticket, current).unwrap();
    }
    assert!(fixture
        .bridge
        .apply_native_history(&fixture.engine, &owner, true, || Ok(()))
        .is_err());
    assert_eq!(fixture.owner(), owner);
    assert_eq!(model(&fixture), original);
    assert!(
        fixture
            .bridge
            .native_history_available(&fixture.engine, &owner)
            .unwrap()
            .1
    );
    assert!(fixture
        .bridge
        .apply_native_history(&fixture.engine, &owner, false, || Err(
            "Cancelled stale control".into()
        ))
        .is_err());
    assert_eq!(model(&fixture), original);
    assert!(
        fixture
            .bridge
            .native_history_available(&fixture.engine, &owner)
            .unwrap()
            .1
    );
    fixture.rename(&owner, "New branch").unwrap();
    assert!(
        !fixture
            .bridge
            .native_history_available(&fixture.engine, &owner)
            .unwrap()
            .1
    );
}
