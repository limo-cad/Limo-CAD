use super::super::*;
use super::*;
use crate::session_bridge::{native_interface::tests::Fixture, parse_engine_envelope};

fn export(f: &Fixture) -> serde_json::Value {
    parse_engine_envelope(f.engine.engine_call("project_export_model", "")).unwrap()
}
fn seed(f: &Fixture) {
    f.bridge
        .apply_native_mutation(
            &f.engine,
            &f.owner(),
            "drawing_set_document",
            &serde_json::to_value(document()).unwrap(),
            || Ok(()),
        )
        .unwrap();
}

#[test]
fn sheet_editor_without_paper_shares_engine_snapshot_and_preserves_unapplied_draft() {
    crate::session_bridge::tests::with_isolated_session_test(|| {
        let f = Fixture::new();
        seed(&f);
        let owner = f.owner();
        let services = NativeServices {
            engine: f.engine.clone(),
            bridge: f.bridge.clone(),
        };
        let shared = f.engine.shared_drawing_snapshot();
        let mut app = native_viewport::interface_scene_fixture();
        let world = app.world_mut();
        let camera = world.spawn(InterfaceCamera).id();
        let state = Workbench::default();
        synchronize(world, camera, &services, &owner, (860., 248.), true, &state).unwrap();
        assert!(Arc::ptr_eq(&shared, &world.resource::<Editor>().document));
        edit(
            world.resource_mut::<Editor>().draft.as_mut().unwrap(),
            "/name",
            "Unapplied name",
        );
        synchronize(world, camera, &services, &owner, (860., 248.), true, &state).unwrap();
        assert!(Arc::ptr_eq(&shared, &world.resource::<Editor>().document));
        assert!(Arc::ptr_eq(&shared, &f.engine.shared_drawing_snapshot()));
        assert!(world.resource::<Editor>().draft.as_ref().unwrap().dirty());
        retire_document(world, &owner, false);
        assert!(
            world.resource::<Editor>().draft.as_ref().unwrap().dirty(),
            "Pressure must retain required edit/cancel ownership"
        );
        retire_document(world, &owner, true);
        assert!(world.get_resource::<Editor>().is_none());
        assert!(
            Arc::ptr_eq(&shared, &f.engine.shared_drawing_snapshot()),
            "Editor retirement must not retire the engine's live drawing"
        );
    });
}

#[test]
fn native_view_draft_uses_shared_update_and_restores_aligned_history() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    for (operation, args) in [
        (
            "sketch_begin",
            json!({"plane":{"type":"origin_plane","plane":"xy"}}),
        ),
        (
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":40.,"y":30.},"ctrl_held":true}),
        ),
        ("sketch_finish", json!({})),
        (
            "solid_extrude",
            json!({"sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":6.}}),
        ),
        (
            "drawing_create_sheet",
            json!({"name":"Fixture","format":"a4","orientation":"landscape"}),
        ),
        (
            "drawing_add_view",
            json!({"sheet_id":1,"view":{"name":"Top","kind":"top","direction":[0.,0.,1.],"up":[0.,1.,0.],"position":[80.,65.],"scale":1.}}),
        ),
        (
            "drawing_add_view",
            json!({"sheet_id":1,"view":{"name":"Front","kind":"front","direction":[0.,-1.,0.],"up":[0.,0.,1.],"position":[80.,140.],"scale":1.,"parent_view_id":1,"alignment":"vertical"}}),
        ),
    ] {
        f.bridge
            .apply_native_mutation(&f.engine, &f.owner(), operation, &args, || Ok(()))
            .unwrap();
    }
    let before = export(&f);
    let drawing = f.engine.drawing_snapshot();
    let mut draft = Draft::new(&drawing, Selection::View(1)).unwrap();
    edit(&mut draft, "/scale", "2");
    edit(&mut draft, "/position/0", "110");
    let expected = draft.apply(&drawing).unwrap();
    let receipt = f
        .bridge
        .native_document_receipt(&f.engine, &f.owner())
        .unwrap();
    f.bridge
        .apply_native_mutation_at(
            &f.engine,
            &receipt.owner,
            receipt.revision,
            "drawing_update_view",
            &serde_json::to_value(draft.view_edit(&drawing).unwrap()).unwrap(),
            || Ok(()),
        )
        .unwrap();
    assert_eq!(f.engine.drawing_snapshot(), expected);
    assert_eq!(expected.sheets[0].views[1].position, [110., 140.]);
    assert_eq!(expected.sheets[0].views[1].scale, 2.);
    let after = export(&f);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
        .unwrap();
    assert_eq!(export(&f), before);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
        .unwrap();
    assert_eq!(export(&f), after);
}

#[test]
fn released_sheet_editor_commit_undo_redo_preserve_exact_release_history() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    let mut saved = document();
    release(&mut saved);
    f.bridge
        .apply_native_mutation(
            &f.engine,
            &f.owner(),
            "drawing_set_document",
            &serde_json::to_value(&saved).unwrap(),
            || Ok(()),
        )
        .unwrap();
    let before = export(&f);
    let drawing = f.engine.drawing_snapshot();
    let receipt = f
        .bridge
        .native_document_receipt(&f.engine, &f.owner())
        .unwrap();
    let mut draft = Draft::new(&drawing, Selection::Sheet(7)).unwrap();
    edit(&mut draft, "/title_block/title", "Changed after release");
    let next = draft.apply(&drawing).unwrap();
    let mut expected = drawing.clone();
    expected.sheets[6].title_block.title = "Changed after release".into();
    expected.sheets[6].release.status = DrawingReleaseStatus::Draft;
    assert_eq!(next, expected);
    f.bridge
        .apply_native_mutation_at(
            &f.engine,
            &receipt.owner,
            receipt.revision,
            "drawing_set_document",
            &serde_json::to_value(&next).unwrap(),
            || Ok(()),
        )
        .unwrap();
    assert_eq!(f.engine.drawing_snapshot(), expected);
    let after = export(&f);
    assert_ne!(before, after);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
        .unwrap();
    assert_eq!(export(&f), before);
    assert_eq!(f.engine.drawing_snapshot(), drawing);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
        .unwrap();
    assert_eq!(export(&f), after);
    assert_eq!(f.engine.drawing_snapshot(), expected);
}

#[test]
fn drawing_editor_commits_use_existing_document_command_and_exact_history() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    seed(&f);
    let before = export(&f);
    let receipt = f
        .bridge
        .native_document_receipt(&f.engine, &f.owner())
        .unwrap();
    let drawing = f.engine.drawing_snapshot();
    let mut draft = Draft::new(&drawing, Selection::Sheet(7)).unwrap();
    edit(&mut draft, "/title_block/title", "Native drawing title");
    let next = draft.apply(&drawing).unwrap();
    f.bridge
        .apply_native_mutation_at(
            &f.engine,
            &receipt.owner,
            receipt.revision,
            "drawing_set_document",
            &serde_json::to_value(&next).unwrap(),
            || Ok(()),
        )
        .unwrap();
    let after = export(&f);
    assert_ne!(before, after);
    assert_eq!(f.engine.drawing_snapshot(), next);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
        .unwrap();
    assert_eq!(export(&f), before);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
        .unwrap();
    assert_eq!(export(&f), after);
    let current = f
        .bridge
        .native_document_receipt(&f.engine, &f.owner())
        .unwrap();
    let mut invalid = next.clone();
    invalid.next_sheet_id = 0;
    assert!(f
        .bridge
        .apply_native_mutation_at(
            &f.engine,
            &current.owner,
            current.revision,
            "drawing_set_document",
            &serde_json::to_value(invalid).unwrap(),
            || Ok(()),
        )
        .is_err());
    assert_eq!(export(&f), after);
    assert_eq!(
        f.bridge
            .native_document_receipt(&f.engine, &f.owner())
            .unwrap()
            .revision,
        current.revision
    );
    let error = f
        .bridge
        .apply_native_mutation_at(
            &f.engine,
            &current.owner,
            current.revision,
            "drawing_unregistered_operation",
            &json!({}),
            || Ok(()),
        )
        .unwrap_err();
    assert!(error.contains("unsupported inbox mutate"));
    assert_eq!(export(&f), after);
    assert_eq!(
        f.bridge
            .native_document_receipt(&f.engine, &f.owner())
            .unwrap()
            .revision,
        current.revision
    );
    assert!(f
        .bridge
        .apply_native_mutation_at(
            &f.engine,
            &receipt.owner,
            receipt.revision,
            "drawing_set_document",
            &serde_json::to_value(&drawing).unwrap(),
            || Ok(())
        )
        .is_err());
    assert_eq!(export(&f), after);
    parse_engine_envelope(
        f.bridge
            .with_project_session_transition("main", &f.engine, || {
                f.engine.create_project_session("drawing-tab-b")
            }),
    )
    .unwrap();
    assert!(f
        .bridge
        .apply_native_mutation_at(
            &f.engine,
            &receipt.owner,
            receipt.revision,
            "drawing_set_document",
            &serde_json::to_value(&drawing).unwrap(),
            || Ok(())
        )
        .is_err());
    assert!(f.engine.drawing_snapshot().sheets.is_empty());
}

#[test]
fn drawing_editor_panel_exposes_all_sheets_and_preserves_dirty_draft_until_history_refresh() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    seed(&f);
    let before = export(&f);
    let services = NativeServices {
        engine: f.engine.clone(),
        bridge: f.bridge.clone(),
    };
    let mut app = native_viewport::interface_scene_fixture();
    let world = app.world_mut();
    world.init_resource::<Assets<Image>>();
    world.init_resource::<ViewportUiAssets>();
    world.insert_resource(Workbench {
        workspace: Workspace::Drawing,
        ..default()
    });
    let camera = world.spawn(InterfaceCamera).id();
    for height in [600., 860.] {
        synchronize(
            world,
            camera,
            &services,
            &f.owner(),
            (height, 248.),
            true,
            &Workbench::default(),
        )
        .unwrap();
        let sheet = world
            .query::<&InterfaceControl>()
            .iter(world)
            .find(|c| c.label == "Sheet")
            .unwrap();
        let limo_cad_interface::Field::Choice { options, value } = &sheet.field else {
            panic!("Sheet selector must expose all shared choices")
        };
        assert_eq!(options.len(), 8);
        assert_eq!(options[7].value, "8");
        assert_eq!(value, "1");
        assert_eq!(
            choose(
                options,
                value,
                &ControlInput::Key(limo_cad_interface::KeyChord::plain("End"))
            )
            .unwrap(),
            "8"
        );
        assert!(world
            .query::<&InterfaceControl>()
            .iter(world)
            .any(|c| c.label == "Sheet name" && c.role == "textbox"));
    }
    assert_eq!(
        export(&f),
        before,
        "Painting forms must not write the drawing"
    );
    {
        let mut editor = world.resource_mut::<Editor>();
        edit(editor.draft.as_mut().unwrap(), "/name", "Unapplied text");
    }
    synchronize(
        world,
        camera,
        &services,
        &f.owner(),
        (860., 248.),
        true,
        &Workbench::default(),
    )
    .unwrap();
    assert!(world.resource::<Editor>().draft.as_ref().unwrap().dirty());
    for operation in [
        "drawing_create_sheet",
        "drawing_delete_sheet",
        "drawing_select_sheet",
        "drawing_add_view",
        "drawing_add_note",
    ] {
        assert!(
            guard_ribbon_edit(world, operation).is_err(),
            "Unapplied draft allowed {operation}"
        );
    }
    assert!(guard_ribbon_edit(world, "solid_extrude").is_ok());
    assert_eq!(export(&f), before);
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), false, || Ok(()))
        .unwrap();
    synchronize(
        world,
        camera,
        &services,
        &f.owner(),
        (860., 248.),
        true,
        &Workbench::default(),
    )
    .unwrap();
    assert!(world.resource::<Editor>().draft.is_none());
    f.bridge
        .apply_native_history(&f.engine, &f.owner(), true, || Ok(()))
        .unwrap();
    synchronize(
        world,
        camera,
        &services,
        &f.owner(),
        (860., 248.),
        true,
        &Workbench::default(),
    )
    .unwrap();
    assert!(!world.resource::<Editor>().draft.as_ref().unwrap().dirty());
}

#[test]
fn sheet_editor_shares_paper_and_retires_hidden_replaced_document() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    seed(&f);
    let owner = f.owner();
    let services = NativeServices {
        engine: f.engine.clone(),
        bridge: f.bridge.clone(),
    };
    let receipt = f.bridge.native_document_receipt(&f.engine, &owner).unwrap();
    let document = Arc::new(f.engine.drawing_snapshot());
    let retired = Arc::downgrade(&document);
    let mut state = Workbench {
        paper_document: Some((receipt, document)),
        ..default()
    };
    let mut app = native_viewport::interface_scene_fixture();
    let world = app.world_mut();
    let camera = world.spawn(InterfaceCamera).id();
    synchronize(world, camera, &services, &owner, (860., 248.), true, &state).unwrap();
    assert!(Arc::ptr_eq(
        &world.resource::<Editor>().document,
        &state.paper_document.as_ref().unwrap().1
    ));
    edit(
        world.resource_mut::<Editor>().draft.as_mut().unwrap(),
        "/name",
        "Unapplied sheet name",
    );
    synchronize(
        world,
        camera,
        &services,
        &owner,
        (860., 248.),
        false,
        &state,
    )
    .unwrap();
    assert!(guard_sheet_edit(world).is_err());
    let mut foreign = owner.clone();
    foreign.window_id.push_str("-other");
    retire_document(world, &foreign, true);
    assert!(guard_sheet_edit(world).is_err());
    retire_document(world, &owner, false);
    assert!(
        guard_sheet_edit(world).is_err(),
        "Pressure must preserve the unapplied draft"
    );
    assert!(guard_document_switch(world, &owner).is_err());
    assert!(guard_document_switch(world, &foreign).is_ok());
    let mut other_tab = owner.clone();
    other_tab.document_id.push_str("-another-live-tab");
    synchronize(
        world,
        camera,
        &services,
        &other_tab,
        (860., 248.),
        false,
        &state,
    )
    .unwrap();
    assert!(world.resource::<Editor>().draft.as_ref().unwrap().dirty());
    assert_eq!(world.resource::<Editor>().owner.as_ref(), Some(&owner));
    state.paper_document = None;
    let mut replacement = owner;
    replacement.epoch += 1;
    synchronize(
        world,
        camera,
        &services,
        &replacement,
        (860., 248.),
        false,
        &state,
    )
    .unwrap();
    assert!(retired.upgrade().is_none());
    assert!(guard_sheet_edit(world).is_ok());
}

#[test]
fn unapplied_drawing_draft_blocks_tab_and_window_close_without_losing_text() {
    use crate::session_bridge::native_interface::{controller, workspace::DocumentWorkspace};
    use std::sync::{atomic::AtomicBool, Mutex};

    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let f = Fixture::new();
    seed(&f);
    let owner = f.owner();
    let services = NativeServices {
        engine: f.engine.clone(),
        bridge: f.bridge.clone(),
    };
    let mut app = native_viewport::interface_scene_fixture();
    let world = app.world_mut();
    let workspace = Arc::new(Mutex::new(DocumentWorkspace::default()));
    controller::files::initialize(world, Arc::clone(&workspace));
    let camera = world.spawn(InterfaceCamera).id();
    synchronize(
        world,
        camera,
        &services,
        &owner,
        (860., 248.),
        true,
        &Workbench::default(),
    )
    .unwrap();
    edit(
        world.resource_mut::<Editor>().draft.as_mut().unwrap(),
        "/name",
        "Unapplied close test",
    );
    let before = export(&f);
    let handle = NativeInterfaceHandle::new(|| {});
    let error = controller::files::request(
        world,
        &handle,
        &services,
        &owner,
        &json!({"command":"close"}),
    )
    .unwrap_err();
    assert!(error.contains("Apply or reset the drawing edit"), "{error}");
    assert!(!controller::files::awaiting(world));

    let mut state =
        controller::Controller::new("main".into(), None, Arc::new(AtomicBool::new(false)));
    state.workspace = workspace;
    let error = controller::request_close(world, &mut state, &f.bridge, &f.engine).unwrap_err();
    assert!(error.contains("Apply or reset the drawing edit"), "{error}");
    assert!(!state.close_pending && !state.exit_after_receipt);
    assert_eq!(export(&f), before);
    assert_eq!(f.owner(), owner);
    assert!(world.resource::<Editor>().draft.as_ref().unwrap().dirty());

    // Reset remains the explicit way to discard an unapplied editor draft.
    let editor = world.resource_mut::<Editor>().into_inner();
    let selection = editor.draft.as_ref().unwrap().selection;
    editor.draft = Some(Draft::new(&editor.document, selection).unwrap());
    assert!(guard_document_switch(world, &owner).is_ok());
    controller::request_close(world, &mut state, &f.bridge, &f.engine).unwrap();
    assert!(state.close_pending || state.exit_after_receipt);
}
