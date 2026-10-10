use super::*;
fn bound(body: u64) -> PrintHeightBindingDto {
    serde_json::from_value(json!({"layout":{"kind":"assembly"},"occurrences":[{"body_id":body,"occurrence_id":3,"root_occurrence_id":3,"pose":{"translation_mm":[0,0,0],"rotation":[0,0,0,1]},"min_z_mm":0,"max_z_mm":20}],"groups":[{"root_occurrence_id":3,"members":[{"body_id":body,"occurrence_id":3}],"min_z_mm":0,"max_z_mm":20}]})).unwrap()
}
fn state(profile: bool) -> State {
    let mut state = State {
        body: 7,
        height_scope: true,
        units: limo_cad_core::UnitSystem::Mm,
        ..default()
    };
    state.height_editor.profile = profile;
    state.height_editor.context = json!({"binding":bound(7),"views":{"views":[]}});
    state.document = Some(Default::default());
    create(&mut state).unwrap();
    state
}
#[test]
fn height_fields_keep_inheritance_explicit_zero_and_units_without_reinterpreting_print_axis() {
    let mut state = state(false);
    edit(&mut state, Field::Min, "0.1 in").unwrap();
    edit(&mut state, Field::Max, "12 mm").unwrap();
    edit(&mut state, Field::Speed(0), "0").unwrap();
    state.draft.wall_count = Some(0);
    let (_, args) = write(&state, &super::super::Command::Apply).unwrap();
    assert!((args["range"]["min_z_mm"].as_f64().unwrap() - 2.54).abs() < 1e-9);
    assert_eq!(args["range"]["settings"]["wall_count"], 0);
    assert!(
        args["range"]["id"].is_null(),
        "First create must omit id so the owning engine assigns identity"
    );
    assert_eq!(args["range"]["speeds"]["outer_wall_mm_s"], 0.);
    assert!(args["range"]["speeds"]["inner_wall_mm_s"].is_null());
    let before = state.height_editor.draft.clone();
    assert_eq!(
        edit(&mut state, Field::Speed(0), "NaN").unwrap()["valid"],
        false
    );
    assert_eq!(state.height_editor.draft, before);
    assert!(!state.errors.is_empty());
    edit(&mut state, Field::Speed(0), "").unwrap();
    assert!(state.errors.is_empty());
    edit(&mut state, Field::Max, "2 mm").unwrap();
    assert!(write(&state, &super::super::Command::Apply).is_err());
    assert!(state.dirty());
}
#[test]
fn explicit_variable_profiles_keep_endpoints_and_require_a_review_for_new_layout() {
    let mut state = state(true);
    assert!(write(&state, &super::super::Command::Inherit).is_err());
    edit_points(&mut state, Command::AddPoint).unwrap();
    assert_eq!(
        state.height_editor.draft.as_ref().unwrap()["points"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    edit_points(&mut state, Command::RemovePoint).unwrap();
    assert_eq!(
        state.height_editor.draft.as_ref().unwrap()["points"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    state.height_editor.point = 0;
    assert!(edit_points(&mut state, Command::RemovePoint).is_err());
    edit(&mut state, Field::PointHeight, "0.15 mm").unwrap();
    assert!(write(&state, &super::super::Command::Apply).is_ok());
    state.height_editor.layout = "01234567-89ab-4cde-8123-456789abcdef".into();
    assert!(write(&state, &super::super::Command::Apply)
        .unwrap_err()
        .contains("Rebind"));
    assert!(write(&state, &super::super::Command::Height(Command::Rebind)).is_err());
}
#[test]
fn saved_height_selection_rejects_cross_body_and_refresh_preserves_review_target() {
    let state0 = state(false);
    let own: PrintHeightRangeDto = serde_json::from_value(record(&state0, false).unwrap()).unwrap();
    let mut other = own.clone();
    other.id = uuid::Uuid::new_v4().to_string();
    other.body_id = limo_cad_core::BodyId(8);
    other.binding = bound(8);
    let document = PrintIntentDocumentDto {
        height_ranges: vec![own.clone(), other.clone()],
        ..default()
    };
    let mut state = State {
        body: 7,
        height_scope: true,
        ..default()
    };
    state.accept(document.clone(), "before".into(), json!({}), 1);
    assert!(edit(&mut state, Field::Saved, &other.id).is_err());
    assert_eq!(state.height_editor.selection, own.id);
    assert!(!state.dirty());
    state.height_editor.layout = "01234567-89ab-4cde-8123-456789abcdef".into();
    state.accept(document, "refresh".into(), json!({}), 2);
    assert_eq!(
        state.height_editor.layout,
        "01234567-89ab-4cde-8123-456789abcdef"
    );
}
#[test]
fn reviewed_changes_are_bound_to_exact_source_record_model_and_target_layout() {
    let mut state = state(false);
    state.expected_model = "owned snapshot".into();
    let record = record(&state, false).unwrap();
    state.height_editor.review = Some(Review {
        kind: Command::ReviewRebind,
        model: state.expected_model.clone(),
        record: record.clone(),
        layout: layout(&state),
        value: json!({"binding":bound(7)}),
    });
    assert!(write(&state, &super::super::Command::Height(Command::Rebind)).is_ok());
    state.draft.wall_count = Some(6);
    assert!(write(&state, &super::super::Command::Height(Command::Rebind)).is_err());
    state.draft = Default::default();
    state.expected_model = "new snapshot".into();
    assert!(write(&state, &super::super::Command::Height(Command::Rebind)).is_err());
}

#[test]
fn reviewed_group_copy_expands_intentional_shared_definitions_atomically_and_undo_keeps_geometry() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = crate::session_bridge::native_interface::tests::Fixture::new();
    let owner = fixture.owner();
    let apply = |op: &str, args: Value| {
        fixture
            .bridge
            .apply_native_mutation(&fixture.engine, &owner, op, &args, || Ok(()))
            .unwrap()
            .value
    };
    for index in 1..=3 {
        apply(
            "sketch_begin",
            json!({"plane":{"type":"origin_plane","plane":"xy"}}),
        );
        apply(
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":index as f64*20.,"y":0.},"p2":{"x":index as f64*20.+10.,"y":10.},"ctrl_held":true}),
        );
        apply("sketch_finish", json!({}));
        apply(
            "solid_extrude",
            json!({"sketch_name":format!("Sketch{index}"),"profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":20.}}),
        );
    }
    let bodies: Vec<_> = fixture
        .engine
        .viewport_snapshot()
        .2
        .bodies
        .iter()
        .map(|b| b.id.0)
        .collect();
    assert_eq!(bodies.len(), 3);
    let repeated = apply(
        "assembly_create_component",
        json!({"name":"Repeated B definition","body_ids":[bodies[1]],"absorb_promoted_bodies":true}),
    );
    for (name, body) in [("Group AB", bodies[0]), ("Group BC", bodies[2])] {
        let component = apply(
            "assembly_create_component",
            json!({"name":name,"body_ids":[body],"absorb_promoted_bodies":true}),
        );
        let assembly =
            parse_engine_envelope(fixture.engine.engine_call("assembly_document", "")).unwrap();
        let root = assembly["component_structure"]["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["component_id"] == component["id"])
            .unwrap()["id"]
            .clone();
        apply(
            "assembly_create_occurrence",
            json!({"component_id":repeated["id"],"parent_occurrence_id":root,"name":"Intentional shared B","local_pose":{"translation":[0.,0.,0.],"rotation":[0.,0.,0.,1.]}}),
        );
    }
    let model = || {
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", ""))
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned()
    };
    let binding =
        capture_binding(&fixture.engine, bodies[0], &PrintHeightLayoutDto::Assembly).unwrap();
    let mut state = State {
        body: bodies[0],
        height_scope: true,
        document: Some(Default::default()),
        ..default()
    };
    state.height_editor.context = json!({"binding":binding});
    create(&mut state).unwrap();
    edit(&mut state, Field::Name, "Reviewed group band").unwrap();
    edit(&mut state, Field::Min, "3 mm").unwrap();
    edit(&mut state, Field::Max, "12 mm").unwrap();
    state.draft.wall_count = Some(6);
    let (operation, mut args) = write(&state, &super::super::Command::Apply).unwrap();
    assert!(args["range"]["id"].is_null());
    args["expected_model_json"] = json!(model());
    let saved = apply(operation, args);
    select_created(&mut state, &saved, &[]);
    let source: PrintHeightRangeDto =
        serde_json::from_value(saved["height_ranges"][0].clone()).unwrap();
    assert_eq!(state.height_editor.selection, source.id);
    let before = model();
    let document: PrintIntentDocumentDto = serde_json::from_value(
        parse_engine_envelope(fixture.engine.engine_call("print_intent_get", "")).unwrap(),
    )
    .unwrap();
    let review = review_group_document(
        &fixture.engine,
        document,
        json!(source),
        false,
        &PrintHeightLayoutDto::Assembly,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(
        review["body_ids"].as_array().unwrap().len(),
        3,
        "Shared definition B must expand review to group BC and C"
    );
    assert_eq!(review["copies"].as_array().unwrap().len(), 2);
    assert_eq!(review["document"]["height_ranges"][0]["id"], source.id);
    let stale = fixture.bridge.apply_native_mutation(
        &fixture.engine,
        &owner,
        "print_intent_set_document",
        &json!({"document":review["document"],"expected_model_json":"stale"}),
        || Ok(()),
    );
    assert!(stale.is_err());
    assert_eq!(model(), before);
    apply(
        "print_intent_set_document",
        json!({"document":review["document"],"expected_model_json":before}),
    );
    let after = model();
    let mut geometry_before: Value = serde_json::from_str(&before).unwrap();
    let mut geometry_after: Value = serde_json::from_str(&after).unwrap();
    geometry_before
        .as_object_mut()
        .unwrap()
        .remove("print_intent");
    geometry_after
        .as_object_mut()
        .unwrap()
        .remove("print_intent");
    assert_eq!(geometry_before, geometry_after);
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
        .unwrap();
    assert_eq!(model(), before);
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &fixture.owner(), true, || Ok(()))
        .unwrap();
    assert_eq!(model(), after);
    let document: PrintIntentDocumentDto =
        serde_json::from_value(review["document"].clone()).unwrap();
    assert!(
        review_group_document(
            &fixture.engine,
            document.clone(),
            json!(source),
            false,
            &PrintHeightLayoutDto::Assembly,
            &Default::default()
        )
        .is_err(),
        "Conflicting existing target records need explicit replacement"
    );
    let replacements = review["copies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_str().unwrap().to_string())
        .collect();
    let replacement = review_group_document(
        &fixture.engine,
        document,
        json!(source),
        false,
        &PrintHeightLayoutDto::Assembly,
        &replacements,
    )
    .unwrap();
    assert_eq!(replacement["replaced_ids"].as_array().unwrap().len(), 2);
    assert_eq!(
        replacement["document"]["height_ranges"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(replacement["document"]["height_ranges"][0]["id"], source.id);
}

#[test]
fn height_display_uses_document_units_while_speed_remains_explicit_mm_per_second() {
    let mut state = state(false);
    state.units = limo_cad_core::UnitSystem::In;
    edit(&mut state, Field::Max, "0.5").unwrap();
    assert_eq!(text(&state, Field::Max), "0.5");
    let (_, value) = write(&state, &super::super::Command::Apply).unwrap();
    assert!((value["range"]["max_z_mm"].as_f64().unwrap() - 12.7).abs() < 1e-9);
    edit(&mut state, Field::Speed(0), "12").unwrap();
    assert_eq!(text(&state, Field::Speed(0)), "12.0");
}

#[test]
fn automatic_saved_selection_refreshes_capture_before_creating_another_request() {
    let mut initial = state(false);
    let mut saved: PrintHeightRangeDto =
        serde_json::from_value(record(&initial, false).unwrap()).unwrap();
    let id = "01234567-89ab-4cde-8123-456789abcdef";
    saved.binding.layout = PrintHeightLayoutDto::NamedLayout { id: id.into() };
    initial.height_editor = Default::default();
    initial.document = None;
    initial.height_editor.context = json!({"binding":bound(7)});
    let document = PrintIntentDocumentDto {
        height_ranges: vec![saved.clone()],
        ..default()
    };
    initial.accept(document.clone(), "before".into(), json!({}), 1);
    assert_eq!(initial.height_editor.layout, id);
    assert_eq!(initial.loaded_revision, None);
    assert!(create(&mut initial).is_err());
    set_context(&mut initial, json!({"binding":saved.binding}));
    initial.accept(document, "current".into(), json!({}), 2);
    assert_eq!(initial.loaded_revision, Some(2));
    assert!(create(&mut initial).is_ok());
    assert_eq!(
        initial.height_editor.draft.as_ref().unwrap()["binding"]["layout"]["id"],
        id
    );
}

#[test]
fn current_height_settings_borrow_the_selected_body_and_keep_profile_inheritance() {
    let mut own: PrintHeightRangeDto =
        serde_json::from_value(record(&state(false), false).unwrap()).unwrap();
    own.settings.wall_count = Some(0);
    let mut foreign = own.clone();
    foreign.body_id = limo_cad_core::BodyId(8);
    foreign.settings.wall_count = Some(7);
    let mut profile: PrintLayerHeightProfileDto =
        serde_json::from_value(record(&state(true), false).unwrap()).unwrap();
    profile.id = own.id.clone();
    let document = PrintIntentDocumentDto {
        height_ranges: vec![foreign, own.clone()],
        layer_height_profiles: vec![profile],
        ..default()
    };
    let mut selected = state(false);
    selected.height_editor.selection = own.id;
    assert_eq!(selected.current(&document).wall_count, Some(0));
    selected.body = 9;
    assert_eq!(selected.current(&document), PrintSettingsDto::default());
    selected.body = 7;
    selected.height_editor.profile = true;
    assert_eq!(selected.current(&document), PrintSettingsDto::default());
    assert_eq!(records(&document, true).len(), 1);
    assert_eq!(records(&document, false).len(), 2);
}
