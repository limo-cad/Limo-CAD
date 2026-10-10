use super::*;
use crate::session_bridge::native_interface::tests::Fixture;

#[test]
fn print_metadata_receipts_preserve_unapplied_layout_preview_and_its_export_guard() {
    let mut world = World::new();
    let owner = DocumentContext {
        window_id: "main".into(),
        document_id: "owned-document".into(),
        epoch: 7,
    };
    let draft = view();
    world.insert_resource(State {
        owner: Some(owner.clone()),
        revision: Some(8),
        visible: true,
        previewing: true,
        draft: Some(draft.clone()),
        ..default()
    });
    advance_metadata(&mut world, &owner, 9);
    let state = world.resource::<State>();
    assert_eq!(state.revision, Some(9));
    assert_eq!(state.draft.as_ref(), Some(&draft));
    assert!(state.previewing);
    assert!(ensure_exportable(&world).is_err());
    advance_metadata(&mut world, &DocumentContext { epoch: 8, ..owner }, 10);
    assert_eq!(
        world.resource::<State>().revision,
        Some(9),
        "Another document incarnation cannot advance this preview"
    );
}

#[test]
fn saved_named_view_edits_use_the_shared_snapshot_undo_stack() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut view = solid_and_view(&fixture);
    let model =
        || parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap();
    let original = model();
    view.part_offsets.push(ViewPartOffsetDto {
        body_id: view.visible_body_ids[0],
        translation: [21., 0., 0.],
    });
    for (operation, args) in [
        ("upsert_named_view", serde_json::to_value(&view).unwrap()),
        (
            "rename_named_view",
            json!({"name":view.name,"new_name":"Renamed fixture"}),
        ),
        ("delete_named_view", json!({"name":"Renamed fixture"})),
    ] {
        fixture
            .bridge
            .apply_native_mutation(&fixture.engine, &fixture.owner(), operation, &args, || {
                Ok(())
            })
            .unwrap();
    }
    let deleted = model();
    for _ in 0..3 {
        fixture
            .bridge
            .apply_native_history(&fixture.engine, &fixture.owner(), false, || Ok(()))
            .unwrap();
    }
    assert_eq!(
        model(),
        original,
        "Undo must restore view placement and name without changing source definitions"
    );
    for _ in 0..3 {
        fixture
            .bridge
            .apply_native_history(&fixture.engine, &fixture.owner(), true, || Ok(()))
            .unwrap();
    }
    assert_eq!(model(), deleted);
}

fn view() -> NamedViewConfigurationDto {
    NamedViewConfigurationDto {
        id: None,
        name: "Fixture view".into(),
        camera: ViewCameraDto {
            position: [100., -100., 100.],
            target: [0.; 3],
            up: [0., 0., 1.],
        },
        visible_body_ids: vec![7],
        part_offsets: vec![],
        occurrence_offsets: vec![ViewOccurrenceOffsetDto {
            occurrence_id: limo_cad_sketch::OccurrenceId(42),
            translation: [10., 20., 30.],
            rotation: [
                0.,
                0.,
                std::f64::consts::FRAC_1_SQRT_2,
                std::f64::consts::FRAC_1_SQRT_2,
            ],
        }],
        print_layout: true,
        print_bed: PrintBedDto::default(),
    }
}

fn solid_and_view(fixture: &Fixture) -> NamedViewConfigurationDto {
    for (operation, arguments) in [
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
            .apply_native_mutation(
                &fixture.engine,
                &fixture.owner(),
                operation,
                &arguments,
                || Ok(()),
            )
            .unwrap();
    }
    let assembly = read_assembly(&fixture.engine).unwrap();
    let mut saved = view();
    saved.visible_body_ids = fixture
        .engine
        .viewport_snapshot()
        .2
        .bodies
        .iter()
        .map(|b| b.id.0)
        .collect();
    saved.occurrence_offsets[0].occurrence_id = assembly.component_structure.occurrences[0].id;
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "upsert_named_view",
            &serde_json::to_value(&saved).unwrap(),
            || Ok(()),
        )
        .unwrap();
    saved
}

#[test]
fn applied_inbox_named_view_recall_updates_camera_without_moving_source_geometry() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let saved = solid_and_view(&fixture);
    let source = serde_json::to_value(fixture.engine.viewport_snapshot().2).unwrap();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "project_set_visibility",
            &json!({"hidden_body_ids":saved.visible_body_ids}),
            || Ok(()),
        )
        .unwrap();
    let mut app = native_viewport::interface_scene_fixture();
    let bodies = refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    let (document, mut camera, mut presentation, _) =
        native_viewport::interface_view_snapshot(app.world());
    camera.position = [14., -20., 60.];
    camera.target = [1., 2., 3.];
    camera.up = [0., 1., 0.];
    presentation.selected_surface_point = Some(limo_cad_solid::Point3Dto {
        x: 3.,
        y: 2.,
        z: 3.,
    });
    native_viewport::apply_interface_view(
        app.world_mut(),
        &document,
        Some(camera),
        Some(presentation),
    )
    .unwrap();
    fixture
        .bridge
        .publish_native_document(&fixture.engine, &fixture.owner(), "solid")
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
    let owner = fixture.owner();
    app.world_mut().insert_resource(NativeRenderedDocument {
        owner: owner.clone(),
        revision,
        bodies,
    });
    let inbox = crate::session_bridge::session_root()
        .join(&session)
        .join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();
    std::fs::write(
        inbox.join("1.json"),
        json!({"name":"recall_named_view","arguments":{"name":saved.name},
            "base_generation":revision,"session_id":session,"window_id":"main",
            "document_id":owner.document_id})
        .to_string(),
    )
    .unwrap();
    let applied = crate::session_bridge::apply_or_reject_one_inbox_op_with_presentation_guard(
        &fixture.bridge,
        "main",
        &fixture.engine,
        None,
        Some((&owner.document_id, &session)),
        false,
    )
    .unwrap();
    assert_eq!(applied["applied"], true);
    assert_eq!(applied["model_changed"], true);
    let recalled: limo_cad_sketch::RecallNamedViewDto =
        serde_json::from_value(applied["result"].clone()).unwrap();
    let mut rejected = applied.clone();
    rejected["applied"] = json!(false);
    after_mutation(app.world_mut(), "inbox", &rejected).unwrap();
    let (_, unchanged_camera, unchanged_presentation, _) =
        native_viewport::interface_view_snapshot(app.world());
    assert_eq!(unchanged_camera.position, camera.position);
    assert_eq!(unchanged_camera.target, camera.target);
    assert_eq!(unchanged_camera.up, camera.up);
    assert!(unchanged_presentation.selected_surface_point.is_some());
    let mutation = NativeMutationResult {
        context: owner,
        engine_revision: applied["engine_revision"].as_u64().unwrap(),
        value: applied.clone(),
    };
    let prepared =
        prepare_native_presentation(&fixture.engine, &fixture.bridge, &mutation, "inbox");
    app.world_mut().insert_resource(prepared);
    let outcome = finish_mutation(
        &fixture.engine,
        &fixture.bridge,
        app.world_mut(),
        "inbox",
        mutation,
    );
    assert!(outcome["render_error"].is_null(), "{outcome}");
    assert_eq!(outcome["operation"], "inbox");
    assert_eq!(
        outcome["result"], applied,
        "Keep the existing inbox receipt shape"
    );
    let (_, camera, presentation, _) = native_viewport::interface_view_snapshot(app.world());
    assert_eq!(camera.position, saved.camera.position.map(|v| v as f32));
    assert_eq!(camera.target, saved.camera.target.map(|v| v as f32));
    assert_eq!(camera.up, saved.camera.up.map(|v| v as f32));
    assert_eq!(
        presentation.hidden_body_ids,
        recalled.visibility.hidden_body_ids
    );
    assert_eq!(*presentation.body_poses, recalled.solution.body_poses);
    assert_eq!(
        *presentation.instance_body_poses,
        recalled.solution.instance_body_poses
    );
    assert!(presentation.selected_surface_point.is_none());
    assert_eq!(
        serde_json::to_value(fixture.engine.viewport_snapshot().2).unwrap(),
        source
    );
}

#[test]
fn translated_and_rotated_saved_view_resets_before_source_forms_without_losing_ids() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let saved = solid_and_view(&fixture);
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "recall_named_view",
            &json!({"name":saved.name}),
            || Ok(()),
        )
        .unwrap();
    let mut app = native_viewport::interface_scene_fixture();
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    let (document, _, mut presentation, _) = native_viewport::interface_view_snapshot(app.world());
    presentation.selected_body_ids = saved.visible_body_ids.clone();
    presentation.selected_occurrence_id = Some(saved.occurrence_offsets[0].occurrence_id.0);
    presentation.selected_face_ids = vec![17];
    presentation.selected_surface_point = Some(limo_cad_solid::Point3Dto {
        x: 100.,
        y: 20.,
        z: 30.,
    });
    native_viewport::apply_interface_view(app.world_mut(), &document, None, Some(presentation))
        .unwrap();
    ensure_source_ready(
        app.world_mut(),
        &fixture.engine,
        &fixture.bridge,
        &fixture.owner(),
    )
    .unwrap();
    assert!(read_views(&fixture.engine).unwrap().active.is_none());
    let presentation = native_viewport::interface_view_snapshot(app.world()).2;
    assert!(presentation.selected_surface_point.is_none());
    assert_eq!(presentation.selected_body_ids, saved.visible_body_ids);
    assert_eq!(
        presentation.selected_occurrence_id,
        Some(saved.occurrence_offsets[0].occurrence_id.0)
    );
    assert_eq!(presentation.selected_face_ids, vec![17]);
    let current: AssemblySolutionDto = serde_json::from_value(
        parse_engine_envelope(fixture.engine.engine_call("assembly_solution", "")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        *presentation.instance_body_poses,
        current.instance_body_poses
    );
}

#[test]
fn draft_preview_rejects_hole_open_and_preserves_draft_while_assembled_picks_survive() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let saved = solid_and_view(&fixture);
    let mut app = native_viewport::interface_scene_fixture();
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    let (document, _, mut presentation, _) = native_viewport::interface_view_snapshot(app.world());
    presentation.selected_surface_point = Some(limo_cad_solid::Point3Dto {
        x: 3.,
        y: 2.,
        z: 3.,
    });
    native_viewport::apply_interface_view(app.world_mut(), &document, None, Some(presentation))
        .unwrap();
    ensure_source_ready(
        app.world_mut(),
        &fixture.engine,
        &fixture.bridge,
        &fixture.owner(),
    )
    .unwrap();
    assert!(native_viewport::interface_view_snapshot(app.world())
        .2
        .selected_surface_point
        .is_some());
    app.world_mut().insert_resource(State {
        draft: Some(saved.clone()),
        previewing: true,
        ..default()
    });
    let error = feature::reduce(
        &fixture.engine,
        &fixture.bridge,
        app.world_mut(),
        &fixture.owner(),
        &feature::FeatureCommand::Open {
            kind: feature::SolidFormKind::Hole,
            feature_id: None,
        },
        &ControlInput::Click,
        || Ok(()),
    )
    .unwrap_err();
    assert!(error.contains("Reset to assembled placement"), "{error}");
    assert!(feature::panel(app.world()).is_none());
    assert_eq!(app.world().resource::<State>().draft.as_ref(), Some(&saved));
    assert!(app.world().resource::<State>().previewing);
}

#[test]
fn corrections_translate_existing_groups_without_replacing_their_rotation() {
    let mut draft = view();
    let rotation = draft.occurrence_offsets[0].rotation;
    apply_corrections(
        &mut draft,
        &json!({"proposal_fits":true,"proposed_translations":[
            {"occurrence_id":42,"translation":[-4.,5.,-30.]},
            {"occurrence_id":99,"translation":[200.,0.,0.]}
        ]}),
    )
    .unwrap();
    assert_eq!(draft.occurrence_offsets[0].translation, [6., 25., 0.]);
    assert_eq!(draft.occurrence_offsets[0].rotation, rotation);
    assert_eq!(draft.occurrence_offsets[1].rotation, [0., 0., 0., 1.]);
    let before = draft.clone();
    assert!(apply_corrections(&mut draft, &json!({"proposal_fits":false})).is_err());
    assert_eq!(draft, before);
}

#[test]
fn occurrence_measurements_use_existing_cad_units_and_leave_other_instances_untouched() {
    let mut state = State::default();
    select(&mut state, view(), true);
    state.target = "occurrence:42".into();
    state.placement = Some(assembly::TransformDraft::new(
        target_pose(state.draft.as_ref().unwrap(), &state.target),
        UnitSystem::In,
    ));
    state.placement.as_mut().unwrap().translation[0].set_text("2 in".into());
    state.placement_dirty = true;
    apply_placement(&mut state, UnitSystem::In).unwrap();
    let draft = state.draft.as_ref().unwrap();
    assert_eq!(draft.occurrence_offsets.len(), 1);
    assert_eq!(draft.occurrence_offsets[0].occurrence_id.0, 42);
    assert_eq!(draft.occurrence_offsets[0].translation, [50.8, 20., 30.]);
    assert_eq!(
        draft.occurrence_offsets[0].rotation,
        view().occurrence_offsets[0].rotation
    );
    let before = draft.clone();
    state.placement.as_mut().unwrap().translation[0].set_text("NaN".into());
    state.placement_dirty = true;
    assert!(apply_placement(&mut state, UnitSystem::In).is_err());
    assert_eq!(state.draft.as_ref(), Some(&before));
}

#[test]
fn reset_removes_stale_world_coordinates_but_retains_occurrence_and_face_identity() {
    let mut presentation = native_viewport::ViewportPresentation {
        selected_body_ids: vec![7],
        selected_occurrence_id: Some(42),
        selected_face_ids: vec![17],
        selected_surface_point: Some(limo_cad_solid::Point3Dto {
            x: 100.,
            y: 0.,
            z: 0.,
        }),
        ..Default::default()
    };
    clear_pick_coordinates(&mut presentation);
    assert!(presentation.selected_surface_point.is_none());
    assert_eq!(presentation.selected_body_ids, vec![7]);
    assert_eq!(presentation.selected_occurrence_id, Some(42));
    assert_eq!(presentation.selected_face_ids, vec![17]);
}

#[test]
fn draft_preview_requires_an_explicit_save_recall_or_reset_before_export() {
    let mut world = World::new();
    world.insert_resource(State {
        previewing: true,
        ..default()
    });
    assert!(ensure_exportable(&world).is_err());
    world.resource_mut::<State>().previewing = false;
    assert!(ensure_exportable(&world).is_ok());
}

#[test]
fn capturing_an_active_view_copies_offsets_and_current_camera_without_mutating_the_document() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let saved = solid_and_view(&fixture);
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "recall_named_view",
            &json!({"name":saved.name}),
            || Ok(()),
        )
        .unwrap();
    let mut app = native_viewport::interface_scene_fixture();
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    let original =
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap();
    let current = native_viewport::ViewportCamera {
        position: [10., 20., 30.],
        target: [1., 2., 3.],
        ..default()
    };
    native_viewport::apply_interface_view(
        app.world_mut(),
        &fixture.owner().document_id,
        Some(current),
        None,
    )
    .unwrap();
    let captured = capture(app.world(), &fixture.engine, "New view".into()).unwrap();
    assert_eq!(captured.camera.position, [10., 20., 30.]);
    assert_eq!(captured.camera.target, [1., 2., 3.]);
    assert_eq!(captured.occurrence_offsets, saved.occurrence_offsets);
    assert_eq!(
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap(),
        original
    );
}

#[test]
fn capture_rejects_a_viewport_from_a_previous_document_without_creating_a_view() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    solid_and_view(&fixture);
    let mut app = native_viewport::interface_scene_fixture();
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    parse_engine_envelope(fixture.bridge.with_project_session_transition(
        "main",
        &fixture.engine,
        || fixture.engine.create_project_session("new-document"),
    ))
    .unwrap();
    let before = read_views(&fixture.engine).unwrap();
    assert!(
        capture(app.world(), &fixture.engine, "Stale capture".into())
            .unwrap_err()
            .contains("changing documents")
    );
    assert_eq!(read_views(&fixture.engine).unwrap(), before);
}

#[test]
fn named_views_cannot_take_over_picking_from_an_open_source_hole_form() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    solid_and_view(&fixture);
    let mut app = native_viewport::interface_scene_fixture();
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    feature::reduce(
        &fixture.engine,
        &fixture.bridge,
        app.world_mut(),
        &fixture.owner(),
        &feature::FeatureCommand::Open {
            kind: feature::SolidFormKind::Hole,
            feature_id: None,
        },
        &ControlInput::Click,
        || Ok(()),
    )
    .unwrap();
    let original = feature::panel(app.world()).unwrap().form_id;
    let before =
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap();
    assert!(
        open(app.world_mut(), &fixture.engine, &fixture.owner(), None)
            .unwrap_err()
            .contains("cancel the source feature")
    );
    assert_eq!(feature::panel(app.world()).unwrap().form_id, original);
    assert!(!active(app.world()));
    assert_eq!(
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap(),
        before
    );
    fixture
        .bridge
        .publish_native_document(&fixture.engine, &fixture.owner(), "solid")
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
    let owner = fixture.owner();
    let root = crate::session_bridge::session_root()
        .join(&session)
        .join("inbox");
    std::fs::create_dir_all(&root).unwrap();
    {
        let (seq, name, arguments) = (1, "recall_named_view", json!({"name":"Fixture view"}));
        std::fs::write(root.join(format!("{seq}.json")), json!({"name":name,"arguments":arguments,
            "base_generation":revision,"session_id":session,"window_id":"main","document_id":owner.document_id}).to_string()).unwrap();
        let result = crate::session_bridge::apply_or_reject_one_inbox_op_with_presentation_guard(
            &fixture.bridge,
            "main",
            &fixture.engine,
            None,
            Some((&owner.document_id, &session)),
            presentation_locked(app.world()),
        )
        .unwrap();
        assert_eq!(result["dead_lettered"], true);
        assert_eq!(result["reason"], "presentation_editor_active");
        assert_eq!(
            fixture.bridge.engine_revision_for_window("main").unwrap(),
            Some(revision)
        );
    }
    assert!(limo_cad_mcp_mutate::is_live_engine_query("named_views"));
    assert_eq!(read_views(&fixture.engine).unwrap().views.len(), 1);
    assert!(read_views(&fixture.engine).unwrap().active.is_none());
    assert_eq!(feature::panel(app.world()).unwrap().form_id, original);
}
