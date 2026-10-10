use super::*;

fn check_presentation_history(operation: &str, arguments: Value) {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = part(&fixture);
    if operation != "construction_set_visibility" {
        edit(
            &fixture,
            &owner,
            "upsert_named_view",
            json!({"name":"Hidden bodies","camera":{
                "position":[100.,-100.,100.],"target":[0.,0.,0.],"up":[0.,0.,1.]},
                "visible_body_ids":[]}),
        );
        if operation == "clear_named_view" {
            edit(
                &fixture,
                &owner,
                "recall_named_view",
                json!({"name":"Hidden bodies"}),
            );
        }
    }
    let original = model(&fixture);
    let extrude = json!({"feature_id":original["extrudes"][0]["feature_id"],"extrude":{
        "sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body",
        "extent":{"type":"distance","distance":25.},"taper_angle_deg":0.,
        "flip":false,"target_body_ids":[]}});
    edit(&fixture, &owner, "solid_edit_extrude", extrude);
    edit(&fixture, &owner, operation, arguments.clone());
    let visibility = model(&fixture)["visibility"].clone();
    let undone = fixture
        .bridge
        .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
        .unwrap();
    let restored = model(&fixture);
    assert_eq!(
        fixture.engine.document_snapshot().features.len(),
        2,
        "{operation}"
    );
    assert_eq!(restored["extrudes"], original["extrudes"], "{operation}");
    assert_eq!(restored["visibility"], visibility, "{operation}");

    // A second presentation command must also retain the newly created Redo.
    edit(&fixture, &undone.context, operation, arguments.clone());
    let before_rejection = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &undone.context)
        .unwrap();
    let unchanged = model(&fixture);
    assert!(fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &undone.context,
            operation,
            &arguments,
            || { Err("cancelled presentation command".into()) }
        )
        .is_err());
    let invalid = match operation {
        "construction_set_visibility" => Some(json!({"visible":false,"sketch_names":["Missing"]})),
        "recall_named_view" => Some(json!({"name":"Missing"})),
        _ => None,
    };
    if let Some(invalid) = invalid {
        assert!(fixture
            .bridge
            .apply_native_mutation(
                &fixture.engine,
                &undone.context,
                operation,
                &invalid,
                || Ok(())
            )
            .is_err());
    }
    assert_eq!(model(&fixture), unchanged);
    assert_eq!(
        fixture
            .bridge
            .native_document_receipt(&fixture.engine, &undone.context)
            .unwrap()
            .revision,
        before_rejection.revision
    );
    assert!(
        fixture
            .bridge
            .native_history_available(&fixture.engine, &undone.context)
            .unwrap()
            .1,
        "{operation}"
    );
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &undone.context, true, || Ok(()))
        .unwrap();
    let restored = model(&fixture);
    assert_eq!(restored["extrudes"][0]["extent"]["distance"], 25.0);
    assert_eq!(
        restored["visibility"], unchanged["visibility"],
        "{operation}"
    );
}

#[test]
fn construction_visibility_preserves_edit_undo_and_redo() {
    check_presentation_history(
        "construction_set_visibility",
        json!({"visible":false,"sketch_names":["Sketch1"]}),
    );
}

#[test]
fn named_view_recall_preserves_edit_undo_and_redo() {
    check_presentation_history("recall_named_view", json!({"name":"Hidden bodies"}));
}

#[test]
fn named_view_reset_preserves_edit_undo_and_redo() {
    check_presentation_history("clear_named_view", json!({}));
}
