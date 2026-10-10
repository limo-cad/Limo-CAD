use super::super::tests::Fixture;
use super::*;

mod attached_reads;
mod presentation;

#[test]
fn metadata_undo_retains_reserved_occurrence_ids_without_resetting_presentation() {
    let _lock = super::super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut current = model(&fixture);
    let target = current.clone();
    current["assembly"]["component_structure"]["next_occurrence_id"] = json!(500);
    current["print_intent"]["source_document_id"] = json!("83117445-4c07-4f27-bcbb-81077efce39c");
    let (restored, presentation) = prepare_history_restore(
        &fixture.engine,
        &current.to_string(),
        &target.to_string(),
        false,
    )
    .unwrap();
    let restored: Value = serde_json::from_str(&restored).unwrap();
    assert_eq!(
        restored["assembly"]["component_structure"]["next_occurrence_id"],
        json!(500)
    );
    assert!(
        presentation.is_some(),
        "A counter reserved by a metadata handoff must not reset the recalled scene on Undo"
    );
    let mut newer = target;
    newer["assembly"]["component_structure"]["next_occurrence_id"] = json!(700);
    let (restored, _) = prepare_history_restore(
        &fixture.engine,
        &restored.to_string(),
        &newer.to_string(),
        false,
    )
    .unwrap();
    let restored: Value = serde_json::from_str(&restored).unwrap();
    assert_eq!(
        restored["assembly"]["component_structure"]["next_occurrence_id"],
        json!(700)
    );
}

#[test]
fn owned_geometry_history_keeps_namespace_and_rejects_foreign_snapshot_identity() {
    let _lock = super::super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut current = model(&fixture);
    let mut target = current.clone();
    current["print_intent"]["source_document_id"] = json!("83117445-4c07-4f27-bcbb-81077efce39c");
    target["document"]["name"] = json!("Earlier geometry history");
    let (restored, presentation) = prepare_history_restore(
        &fixture.engine,
        &current.to_string(),
        &target.to_string(),
        false,
    )
    .unwrap();
    let restored: Value = serde_json::from_str(&restored).unwrap();
    assert_eq!(
        restored["print_intent"]["source_document_id"],
        current["print_intent"]["source_document_id"]
    );
    assert_eq!(restored["document"]["name"], target["document"]["name"]);
    assert!(
        presentation.is_none(),
        "Source model changes must retain ordinary replacement handling"
    );
    target["print_intent"]["source_document_id"] = json!("f53a7155-aa4f-4080-990d-d3cae2ed8e77");
    let before = model(&fixture);
    assert!(prepare_history_restore(
        &fixture.engine,
        &current.to_string(),
        &target.to_string(),
        false
    )
    .is_err());
    assert_eq!(
        model(&fixture),
        before,
        "Foreign history identity rejection must not mutate the model"
    );
}

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

fn hide_sketch(fixture: &Fixture, owner: &DocumentContext, operation: &str) {
    let arguments = match operation {
        "construction_set_visibility" => json!({"visible":false,"sketch_names":["Sketch1"]}),
        "project_set_visibility" => {
            let mut visibility = model(fixture)["visibility"].clone();
            visibility["hidden_sketch_names"] = json!(["Sketch1"]);
            visibility
        }
        _ => panic!("Unexpected visibility operation: {operation}"),
    };
    edit(fixture, owner, operation, arguments);
}

#[test]
fn browser_visibility_keeps_feature_edit_undo_and_current_visibility() {
    let _lock = super::super::super::tests::TEST_LOCK.lock().unwrap();
    assert_visibility_keeps_feature_edit_undo("project_set_visibility");
}

#[test]
fn construction_visibility_keeps_feature_edit_undo_and_current_visibility() {
    let _lock = super::super::super::tests::TEST_LOCK.lock().unwrap();
    assert_visibility_keeps_feature_edit_undo("construction_set_visibility");
}

fn assert_visibility_keeps_feature_edit_undo(operation: &str) {
    let fixture = Fixture::new();
    let owner = part(&fixture);
    let original = model(&fixture);
    edit(
        &fixture,
        &owner,
        "solid_edit_extrude",
        json!({"feature_id":original["extrudes"][0]["feature_id"],"extrude":{
            "sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body",
            "extent":{"type":"distance","distance":25.},"taper_angle_deg":0.,
            "flip":false,"target_body_ids":[]}}),
    );
    hide_sketch(&fixture, &owner, operation);
    let undone = fixture
        .bridge
        .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
        .unwrap();
    assert_eq!(fixture.engine.document_snapshot().features.len(), 2);
    assert_eq!(fixture.engine.viewport_snapshot().2.bodies.len(), 1);
    assert!(fixture.engine.viewport_snapshot().2.errors.is_empty());
    let restored = model(&fixture);
    assert_eq!(restored["extrudes"][0]["extent"]["distance"], 10.0);
    assert_eq!(
        restored["visibility"]["hidden_sketch_names"],
        json!(["Sketch1"])
    );
    let redone = fixture
        .bridge
        .apply_native_history(&fixture.engine, &undone.context, true, || Ok(()))
        .unwrap();
    assert_ne!(redone.context.epoch, undone.context.epoch);
    assert_eq!(fixture.engine.document_snapshot().features.len(), 2);
    let restored = model(&fixture);
    assert_eq!(restored["extrudes"][0]["extent"]["distance"], 25.0);
    assert_eq!(
        restored["visibility"]["hidden_sketch_names"],
        json!(["Sketch1"])
    );
}

#[test]
fn browser_visibility_keeps_deleted_feature_redo_and_current_visibility() {
    let _lock = super::super::super::tests::TEST_LOCK.lock().unwrap();
    assert_visibility_keeps_deleted_feature_redo("project_set_visibility");
}

#[test]
fn construction_visibility_keeps_deleted_feature_redo_and_current_visibility() {
    let _lock = super::super::super::tests::TEST_LOCK.lock().unwrap();
    assert_visibility_keeps_deleted_feature_redo("construction_set_visibility");
}

fn assert_visibility_keeps_deleted_feature_redo(operation: &str) {
    let fixture = Fixture::new();
    let owner = part(&fixture);
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
        .unwrap();
    assert_eq!(fixture.engine.document_snapshot().features.len(), 1);
    hide_sketch(&fixture, &owner, operation);
    assert!(
        fixture
            .bridge
            .native_history_available(&fixture.engine, &owner)
            .unwrap()
            .1
    );
    let redone = fixture
        .bridge
        .apply_native_history(&fixture.engine, &owner, true, || Ok(()))
        .unwrap();
    assert_ne!(redone.context.epoch, owner.epoch);
    assert_eq!(fixture.engine.document_snapshot().features.len(), 2);
    assert_eq!(fixture.engine.viewport_snapshot().2.bodies.len(), 1);
    let restored = model(&fixture);
    assert_eq!(restored["extrudes"][0]["extent"]["distance"], 10.0);
    assert_eq!(
        restored["visibility"]["hidden_sketch_names"],
        json!(["Sketch1"])
    );
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

#[test]
fn owning_history_keeps_legacy_view_identity_through_first_rename_and_removes_new_views() {
    let _lock = super::super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let before = model(&fixture);
    let view = serde_json::to_value(limo_cad_sketch::NamedViewConfigurationDto {
        id: None,
        name: "Legacy layout".into(),
        camera: limo_cad_sketch::ViewCameraDto {
            position: [100., 100., 100.],
            target: [0., 0., 0.],
            up: [0., 0., 1.],
        },
        visible_body_ids: vec![],
        part_offsets: vec![],
        occurrence_offsets: vec![],
        print_layout: false,
        print_bed: Default::default(),
    })
    .unwrap();
    let mut old = before.clone();
    old["views"] = json!([view]);
    let mut current = old.clone();
    current["views"][0]["id"] = json!("01234567-89ab-4cde-8123-456789abcdef");
    current["views"][0]["name"] = json!("Renamed layout");
    let (restored, _) = prepare_history_restore(
        &fixture.engine,
        &current.to_string(),
        &old.to_string(),
        false,
    )
    .unwrap();
    let restored: Value = serde_json::from_str(&restored).unwrap();
    assert_eq!(restored["views"][0]["id"], current["views"][0]["id"]);
    assert_eq!(restored["views"][0]["name"], "Legacy layout");
    let (absent, _) = prepare_history_restore(
        &fixture.engine,
        &current.to_string(),
        &before.to_string(),
        false,
    )
    .unwrap();
    let absent: Value = serde_json::from_str(&absent).unwrap();
    assert!(absent["views"].as_array().unwrap().is_empty());
    let mut foreign = old;
    foreign["views"][0]["id"] = json!("01234567-89ab-4cde-8123-456789abcdee");
    foreign["views"][0]["name"] = json!("Renamed layout");
    assert!(prepare_history_restore(
        &fixture.engine,
        &current.to_string(),
        &foreign.to_string(),
        false
    )
    .is_err());
    assert_eq!(model(&fixture), before);
}
