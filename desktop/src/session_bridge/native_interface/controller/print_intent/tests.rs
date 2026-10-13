use super::*;
use crate::session_bridge::native_interface::tests::Fixture;
use limo_cad_core::{BodyId, PartPrintIntentDto};

#[test]
fn inherited_counts_and_percentages_preserve_explicit_zero_and_invalid_drafts() {
    let mut settings = PrintSettingsDto::default();
    for field in [Field::Walls, Field::Density, Field::Top, Field::Bottom] {
        edit_settings(&mut settings, field, "0").unwrap();
        assert_eq!(settings_text(&settings, field).parse::<f64>().unwrap(), 0.);
        for invalid in ["-1", "1001", "NaN", "inf", "1.2.3"] {
            let before = settings.clone();
            assert!(edit_settings(&mut settings, field, invalid).is_err());
            assert_eq!(
                settings, before,
                "Invalid values must retain the last valid request"
            );
        }
        edit_settings(&mut settings, field, " ").unwrap();
        assert_eq!(settings_text(&settings, field), "");
    }
    edit_settings(&mut settings, Field::Density, "30.5").unwrap();
    assert_eq!(settings.infill_density_percent, Some(30.5));
    assert!(edit_settings(&mut settings, Field::Walls, "1.5").is_err());
    edit_settings(&mut settings, Field::Pattern, "gyroid").unwrap();
    assert_eq!(settings_text(&settings, Field::Pattern), "gyroid");
    assert!(edit_settings(&mut settings, Field::Pattern, "unverified_vendor_pattern").is_err());
    edit_settings(&mut settings, Field::Pattern, "Inherit").unwrap();
    assert!(settings.infill_pattern.is_none());
}

#[test]
fn draft_survives_unrelated_metadata_but_conflicting_saved_requests_keep_old_precondition() {
    let mut state = State {
        body: 7,
        ..default()
    };
    let mut document = PrintIntentDocumentDto::default();
    state.accept(document.clone(), "first".into(), json!({}), 1);
    state.draft.wall_count = Some(6);
    document.presets.push(PrintIntentPresetDto {
        name: "Fixture".into(),
        settings: Default::default(),
    });
    state.accept(document.clone(), "preset".into(), json!({}), 2);
    assert_eq!(state.expected_model, "preset");
    assert_eq!(state.draft.wall_count, Some(6));
    document.parts.push(PartPrintIntentDto {
        body_id: BodyId(7),
        settings: PrintSettingsDto {
            wall_count: Some(8),
            ..Default::default()
        },
    });
    state.accept(document, "external edit".into(), json!({}), 3);
    assert_eq!(state.draft.wall_count, Some(6));
    assert_eq!(state.expected_model, "preset");
    assert!(state
        .error
        .as_deref()
        .unwrap()
        .contains("Saved settings changed"));
    let mut world = World::new();
    state.visible = true;
    world.insert_resource(state);
    assert!(ensure_clean(&world).is_err());
}

#[test]
fn print_metadata_has_bounded_snapshot_undo_without_changing_geometry_or_recalled_layout() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    for (operation, args) in [
        (
            "sketch_begin",
            json!({"plane":{"type":"origin_plane","plane":"xy"}}),
        ),
        (
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":10.,"y":6.},"ctrl_held":true}),
        ),
        ("sketch_finish", json!({})),
        (
            "solid_extrude",
            json!({"sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":3.}}),
        ),
    ] {
        fixture
            .bridge
            .apply_native_mutation(&fixture.engine, &owner, operation, &args, || Ok(()))
            .unwrap();
    }
    let read = |op: &str| parse_engine_envelope(fixture.engine.engine_call(op, "")).unwrap();
    let body = fixture.engine.viewport_snapshot().2.bodies[0].id.0;
    fixture.bridge.apply_native_mutation(&fixture.engine,&owner,"upsert_named_view",&json!({
        "name":"Print metadata fixture","camera":{"position":[100.,-100.,100.],"target":[0.,0.,0.],"up":[0.,0.,1.]},
        "visible_body_ids":[body],"part_offsets":[{"body_id":body,"translation":[40.,0.,0.]}],"occurrence_offsets":[],"print_layout":true
    }),||Ok(())).unwrap();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner,
            "recall_named_view",
            &json!({"name":"Print metadata fixture"}),
            || Ok(()),
        )
        .unwrap();
    let before = read("project_export_model");
    let loaded = load_document(&fixture.engine, body, "portable", None).unwrap();
    assert_eq!(loaded["model"], before);
    assert_eq!(loaded["document"], read("print_intent_get"));
    assert!(loaded["document"]["source_document_id"].is_null());
    assert_eq!(loaded["effective"]["parts"][0]["body_id"], body);
    let mut app = native_viewport::interface_scene_fixture();
    let bodies = refresh_native_model(&fixture.engine, app.world_mut(), false).unwrap();
    let revision = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &owner)
        .unwrap()
        .revision;
    app.insert_resource(NativeRenderedDocument {
        owner: owner.clone(),
        revision,
        bodies,
    });
    let geometry_revision = native_viewport::interface_model_revision(app.world());
    let (_, _, mut presentation, _) = native_viewport::interface_view_snapshot(app.world());
    presentation.selected_body_ids = vec![body];
    native_viewport::apply_interface_view(
        app.world_mut(),
        &owner.document_id,
        None,
        Some(presentation.clone()),
    )
    .unwrap();
    let result=fixture.bridge.apply_native_mutation(&fixture.engine,&owner,"print_intent_set_part",&json!({"body_id":body,"settings":{"wall_count":0,"infill_density_percent":30.,"infill_pattern":"gyroid","top_shell_layers":6,"bottom_shell_layers":6},"expected_model_json":before}),||Ok(())).unwrap();
    let prepared = prepare_native_presentation(
        &fixture.engine,
        &fixture.bridge,
        &result,
        "print_intent_set_part",
    );
    assert!(matches!(
        prepared.scene,
        Ok(PreparedNativeScene::Unchanged { .. })
    ));
    app.insert_resource(prepared);
    let receipt = finish_mutation(
        &fixture.engine,
        &fixture.bridge,
        app.world_mut(),
        "print_intent_set_part",
        result,
    );
    assert!(receipt["render_error"].is_null(), "{receipt}");
    assert_eq!(
        native_viewport::interface_model_revision(app.world()),
        geometry_revision
    );
    assert_eq!(
        native_viewport::interface_view_snapshot(app.world())
            .2
            .selected_body_ids,
        presentation.selected_body_ids
    );
    assert_eq!(
        native_viewport::interface_view_snapshot(app.world())
            .2
            .body_poses,
        presentation.body_poses
    );
    assert_eq!(
        read("print_intent_get")["parts"][0]["settings"]["wall_count"],
        0
    );
    let namespace = read("print_intent_get")["source_document_id"].clone();
    let current = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &owner)
        .unwrap();
    fixture
        .bridge
        .apply_native_history_at(&fixture.engine, &owner, current.revision, false, || Ok(()))
        .unwrap();
    let mut restored_before: Value = serde_json::from_str(before.as_str().unwrap()).unwrap();
    restored_before["print_intent"]["source_document_id"] = namespace.clone();
    let actual: Value =
        serde_json::from_str(read("project_export_model").as_str().unwrap()).unwrap();
    assert_eq!(
        actual, restored_before,
        "Metadata Undo restores requests and retains once-assigned identity"
    );
    assert_eq!(read("named_views")["active"], "Print metadata fixture");
    let owner = fixture.owner();
    let current = fixture
        .bridge
        .native_document_receipt(&fixture.engine, &owner)
        .unwrap();
    fixture
        .bridge
        .apply_native_history_at(&fixture.engine, &owner, current.revision, true, || Ok(()))
        .unwrap();
    assert_eq!(
        read("print_intent_get")["parts"][0]["settings"]["wall_count"],
        0
    );
    assert_eq!(read("print_intent_get")["source_document_id"], namespace);
    assert_eq!(read("named_views")["active"], "Print metadata fixture");
}

#[test]
fn metadata_history_reowns_only_clean_editor_for_the_same_document() {
    let previous = DocumentContext {
        window_id: "main".into(),
        document_id: "owned".into(),
        epoch: 3,
    };
    let replacement = DocumentContext {
        epoch: 4,
        ..previous.clone()
    };
    let mut world = World::new();
    world.insert_resource(State {
        visible: true,
        owner: Some(previous.clone()),
        loaded_revision: Some(10),
        document: Some(Default::default()),
        ..default()
    });
    after_history(&mut world, &replacement);
    assert_eq!(world.resource::<State>().owner.as_ref(), Some(&replacement));
    assert!(world.resource::<State>().visible);
    assert!(world.resource::<State>().document.is_none());
    world.resource_mut::<State>().draft.wall_count = Some(6);
    after_history(
        &mut world,
        &DocumentContext {
            epoch: 5,
            ..replacement.clone()
        },
    );
    assert_eq!(world.resource::<State>().owner.as_ref(), Some(&replacement));
    world.resource_mut::<State>().draft = Default::default();
    after_history(
        &mut world,
        &DocumentContext {
            document_id: "other".into(),
            epoch: 5,
            ..previous
        },
    );
    assert_eq!(world.resource::<State>().owner.as_ref(), Some(&replacement));
}

#[test]
fn inbox_model_mutations_preserve_print_drafts_and_keep_snapshot_guards_after_cancel() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    fixture
        .bridge
        .publish_native_document(&fixture.engine, &owner, "solid")
        .unwrap();
    let session = fixture
        .bridge
        .session_id_for_window("main")
        .unwrap()
        .unwrap();
    let revision = fixture
        .bridge
        .engine_revision_for_window("main")
        .unwrap()
        .unwrap();
    let model =
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap();
    let mut world = World::new();
    world.insert_resource(State {
        visible: true,
        owner: Some(owner.clone()),
        draft: PrintSettingsDto {
            wall_count: Some(6),
            ..Default::default()
        },
        ..default()
    });
    let root = crate::session_bridge::session_root()
        .join(&session)
        .join("inbox");
    std::fs::create_dir_all(&root).unwrap();
    for (seq, name, arguments) in [
        (1, "cad_new_project", json!({})),
        (2, "cad_load_project_model", json!({"model_json":model})),
        (
            3,
            "print_intent_set_document",
            json!({"document":PrintIntentDocumentDto::default(), "expected_model_json":"{}"}),
        ),
    ] {
        std::fs::write(
            root.join(format!("{seq}.json")),
            json!({
                "name":name,"arguments":arguments,"base_generation":revision,
                "session_id":session,"window_id":"main","document_id":owner.document_id
            })
            .to_string(),
        )
        .unwrap();
        let reason = ensure_clean(&world).unwrap_err();
        let result = crate::session_bridge::apply_or_reject_one_inbox_op_with_editor_guards(
            &fixture.bridge,
            "main",
            &fixture.engine,
            None,
            Some((&owner.document_id, &session)),
            false,
            Some(&reason),
        )
        .unwrap();
        assert_eq!(result["dead_lettered"], true, "{result}");
        assert_eq!(result["reason"], "document_editor_draft");
        assert_eq!(result["error"], reason);
        assert_eq!(fixture.owner(), owner);
        assert_eq!(
            fixture.bridge.engine_revision_for_window("main").unwrap(),
            Some(revision)
        );
        assert_eq!(
            parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap(),
            model
        );
        assert!(world.resource::<State>().visible);
        assert_eq!(world.resource::<State>().draft.wall_count, Some(6));
    }
    // Cancelling the local draft permits dispatch, but it must not bypass the
    // engine's independent optimistic model precondition.
    cancel(&mut world, &fixture.engine, &fixture.bridge, &owner).unwrap();
    assert!(ensure_clean(&world).is_ok());
    std::fs::write(
        root.join("4.json"),
        json!({"name":"print_intent_set_document","base_generation":revision,
            "arguments":{"document":PrintIntentDocumentDto::default(),"expected_model_json":"{}"},
            "session_id":session,"window_id":"main","document_id":owner.document_id})
        .to_string(),
    )
    .unwrap();
    let result = crate::session_bridge::apply_or_reject_one_inbox_op_with_editor_guards(
        &fixture.bridge,
        "main",
        &fixture.engine,
        None,
        Some((&owner.document_id, &session)),
        false,
        None,
    )
    .unwrap();
    assert_eq!(result["dead_lettered"], true, "{result}");
    assert_ne!(result["reason"], "document_editor_draft", "{result}");
    assert!(
        result["error"]
            .as_str()
            .unwrap()
            .contains("document changed"),
        "{result}"
    );
    assert_eq!(fixture.owner(), owner);
    assert_eq!(
        fixture.bridge.engine_revision_for_window("main").unwrap(),
        Some(revision)
    );
    assert_eq!(
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap(),
        model
    );
}
