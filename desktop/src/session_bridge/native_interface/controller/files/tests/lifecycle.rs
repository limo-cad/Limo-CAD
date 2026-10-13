//! Native project ownership, save-on-exit and application-exit regressions.
//! These exercise the real File controller, ordered worker and project archives.
//! Only the OS picker's reply is supplied through its existing channel boundary.

use super::*;
use limo_cad_project_file::ProjectArchive;
use std::{fs, path::Path};

fn model(fixture: &Fixture) -> String {
    crate::session_bridge::parse_engine_envelope(
        fixture.engine.engine_call("project_export_model", ""),
    )
    .unwrap()
    .as_str()
    .unwrap()
    .to_owned()
}

fn saved_model(path: &Path) -> String {
    ProjectArchive::decode(fs::read(path).unwrap())
        .unwrap()
        .model_json()
        .trim_end_matches('\n')
        .to_owned()
}

fn save_to(
    app: &mut App,
    services: &NativeServices,
    handle: &NativeInterfaceHandle,
    owner: &DocumentContext,
    destination: &Path,
) {
    request(
        app.world_mut(),
        handle,
        services,
        owner,
        &json!({"command":"save","path":destination}),
    )
    .unwrap();
    assert_eq!(drain(app.world_mut(), services).unwrap()["saved"], true);
}

fn replace_with_same_model(fixture: &Fixture) -> DocumentContext {
    let before = model(fixture);
    let owner = fixture.owner();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner,
            "cad_load_project_model",
            &json!({"model_json":before}),
            || Ok(()),
        )
        .unwrap();
    let replacement = fixture.owner();
    assert_eq!(replacement.document_id, owner.document_id);
    assert_ne!(replacement.epoch, owner.epoch);
    assert_eq!(model(fixture), before, "Only document ownership changed");
    replacement
}

#[test]
fn file_open_receipt_is_compact_and_preserves_native_geometry_and_owner_guards() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    for (operation, arguments) in [
        ("sketch_begin", json!({"type":"origin_plane","plane":"xy"})),
        (
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":10.,"y":6.},"ctrl_held":false}),
        ),
        ("sketch_finish", json!({})),
        (
            "solid_extrude",
            json!({"sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":3.},"taper_angle_deg":0.,"flip":false,"target_body_ids":[]}),
        ),
    ] {
        fixture
            .bridge
            .apply_native_mutation(&fixture.engine, &owner, operation, &arguments, || Ok(()))
            .unwrap();
    }
    let expected_model = model(&fixture);
    let expected_scene = fixture.engine.solid_scene_snapshot();
    assert_eq!(expected_scene.bodies.len(), 1);
    assert!(!expected_scene.bodies[0].mesh.indices.is_empty());
    assert!(serde_json::to_vec(&*expected_scene).unwrap().len() > 1024);
    let (mut app, services, handle) = setup(&fixture);
    let source = path("compact-open.limo");
    save_to(&mut app, &services, &handle, &owner, &source);
    let original_bytes = fs::read(&source).unwrap();
    fixture
        .rename(&owner, "Unsaved work must be guarded")
        .unwrap();
    let edited = model(&fixture);
    assert!(
        request(
            app.world_mut(),
            &handle,
            &services,
            &owner,
            &json!({"command":"open","path":source})
        )
        .unwrap()["awaiting_input"]
            == true
    );
    assert!(!worker::busy(app.world()));
    assert_eq!(model(&fixture), edited);
    assert_eq!(fixture.owner(), owner);
    let token = app
        .world()
        .resource::<Files>()
        .dialog
        .as_ref()
        .unwrap()
        .token;
    execute(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        FileCommand::Cancel(token),
    )
    .unwrap();

    assert!(!worker::busy(app.world()));
    assert_eq!(model(&fixture), edited);
    assert_eq!(fixture.owner(), owner);
    assert_eq!(fs::read(&source).unwrap(), original_bytes);

    request(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        &json!({"command":"open","path":source,"discard_changes":true}),
    )
    .unwrap();
    let receipt = drain(app.world_mut(), &services).unwrap();
    assert_eq!(receipt["operation"], "open_project");
    assert_eq!(
        receipt["result"],
        json!({"opened":true,"body_count":1,
        "errors":[],"display_warnings":[]})
    );
    assert!(receipt["render_error"].is_null(), "{receipt}");
    assert!(receipt["publication_error"].is_null(), "{receipt}");
    assert!(
        serde_json::to_vec(&receipt).unwrap().len() < 4096,
        "{receipt}"
    );
    let opened = fixture.owner();
    assert_ne!(opened, owner);
    assert_eq!(receipt["document_id"], opened.document_id);
    assert_eq!(receipt["document_epoch"], opened.epoch);
    assert_eq!(model(&fixture), expected_model);
    assert_eq!(
        serde_json::to_value(&*fixture.engine.solid_scene_snapshot()).unwrap(),
        serde_json::to_value(&*expected_scene).unwrap()
    );
    assert_eq!(fs::read(&source).unwrap(), original_bytes);
    let current = tabs(app.world(), &services, &opened).unwrap();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].path.as_ref(), Some(&source));
    assert!(!current[0].dirty);
    assert_eq!(
        app.world().resource::<NativeRenderedDocument>().owner,
        opened
    );
    assert!(services
        .bridge
        .with_native_document_owner(&services.engine, &owner, || Ok(()))
        .is_err());
}

#[test]
fn failed_save_as_preserves_the_previous_target_and_explicit_document_name() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, handle) = setup(&fixture);
    let owner = fixture.owner();
    let original = path("original.limo");
    save_to(&mut app, &services, &handle, &owner, &original);
    let original_bytes = fs::read(&original).unwrap();

    request(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        &json!({"command":"rename","name":"Explicit design name"}),
    )
    .unwrap();
    drain(app.world_mut(), &services).unwrap();
    let edited = model(&fixture);
    let destination = path("missing-folder/failed-copy.limo");
    request(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        &json!({"command":"save","path":destination}),
    )
    .unwrap();
    let error = drain(app.world_mut(), &services).unwrap_err();
    assert!(
        error.contains("could not create temporary save file"),
        "{error}"
    );

    let all = tabs(app.world(), &services, &owner).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].path.as_ref(), Some(&original));
    assert!(all[0].dirty);
    assert!(!all[0].saving);
    assert_eq!(model(&fixture), edited);
    assert_eq!(fs::read(&original).unwrap(), original_bytes);
    assert!(!destination.exists());

    execute(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        FileCommand::Save,
    )
    .unwrap();
    assert_eq!(drain(app.world_mut(), &services).unwrap()["saved"], true);
    assert_eq!(saved_model(&original), edited);
    assert_eq!(
        fixture.engine.document_snapshot().name,
        "Explicit design name"
    );
    assert!(!tabs(app.world(), &services, &owner).unwrap()[0].dirty);
}

#[test]
fn cancelled_or_disconnected_save_picker_keeps_the_dirty_close_confirmation() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    for disconnected in [false, true] {
        let fixture = Fixture::new();
        let (mut app, services, handle) = setup(&fixture);
        let owner = fixture.owner();
        fixture.rename(&owner, "Unsaved design").unwrap();
        let edited = model(&fixture);
        execute(
            app.world_mut(),
            &handle,
            &services,
            &owner,
            FileCommand::Close,
        )
        .unwrap();
        let dialog = app.world().resource::<Files>().dialog.clone().unwrap();
        let (send, receive) = mpsc::channel();
        app.world_mut().resource_mut::<Files>().picker = Some(Picker {
            receipt: dialog.receipt.clone(),
            kind: PickerKind::Project {
                save: true,
                continuation: Some(Intent::Close),
            },
            result: Mutex::new(receive),
        });
        if !disconnected {
            send.send(None).unwrap();
        }
        drop(send);
        assert_eq!(poll(app.world_mut(), &services).is_err(), disconnected);
        assert!(!worker::busy(app.world()));
        assert_eq!(fixture.owner(), owner);
        assert_eq!(model(&fixture), edited);
        assert_eq!(modal(app.world()), Some("file-dialog"));
        let all = tabs(app.world(), &services, &owner).unwrap();
        assert_eq!(all.len(), 1);
        assert!(all[0].dirty && !all[0].saving && all[0].path.is_none());

        execute(
            app.world_mut(),
            &handle,
            &services,
            &owner,
            FileCommand::Cancel(dialog.token),
        )
        .unwrap();
        assert!(!awaiting(app.world()));
        let destination = path("saved-after-cancel.limo");
        save_to(&mut app, &services, &handle, &owner, &destination);
        assert_eq!(saved_model(&destination), edited);
        assert_eq!(
            fixture.owner(),
            owner,
            "Cancelled close must not resume after Save"
        );
    }
}

#[test]
fn delayed_save_picker_rejects_a_byte_identical_same_tab_replacement() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, _) = setup(&fixture);
    fixture
        .rename(&fixture.owner(), "Same contents, new owner")
        .unwrap();
    let receipt = current(app.world(), &services, &fixture.owner()).unwrap();
    let (send, receive) = mpsc::channel();
    app.world_mut().resource_mut::<Files>().picker = Some(Picker {
        receipt,
        kind: PickerKind::Project {
            save: true,
            continuation: None,
        },
        result: Mutex::new(receive),
    });
    let replacement = replace_with_same_model(&fixture);
    let destination = path("obsolete-picker.limo");
    send.send(Some(destination.clone())).unwrap();
    poll(app.world_mut(), &services).unwrap();
    let error = drain(app.world_mut(), &services).unwrap_err();
    assert!(error.contains("replaced"), "{error}");
    assert!(
        !destination.exists(),
        "An obsolete picker cannot authorize a write"
    );
    assert!(!awaiting(app.world()));
    assert_eq!(fixture.owner(), replacement);
    current(app.world(), &services, &replacement).unwrap();
    let all = tabs(app.world(), &services, &replacement).unwrap();
    assert_eq!(all.len(), 1);
    assert!(all[0].dirty && !all[0].saving && all[0].path.is_none());
}

#[test]
fn queued_save_and_close_cannot_write_or_close_a_byte_identical_replacement() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, handle) = setup(&fixture);
    let owner = fixture.owner();
    let destination = path("before-pending-save.limo");
    save_to(&mut app, &services, &handle, &owner, &destination);
    let original_bytes = fs::read(&destination).unwrap();
    fixture
        .rename(&owner, "Edit awaiting Save and close")
        .unwrap();
    execute(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        FileCommand::Close,
    )
    .unwrap();
    let dialog = app.world().resource::<Files>().dialog.clone().unwrap();

    let workspace = app.world().resource::<Files>().workspace.clone();
    let (locked, ready) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let blocker = std::thread::spawn(move || {
        let _held = workspace.lock().unwrap();
        locked.send(()).unwrap();
        wait.recv_timeout(Duration::from_secs(5)).is_ok()
    });
    ready.recv_timeout(Duration::from_secs(5)).unwrap();
    save(
        app.world_mut(),
        dialog.receipt,
        destination.clone(),
        true,
        Some(Intent::Close),
    )
    .unwrap();
    let replacement = replace_with_same_model(&fixture);
    let released = release.send(());
    assert!(
        blocker.join().unwrap(),
        "Save blocked the caller waiting for the workspace"
    );
    released.unwrap();
    let error = drain(app.world_mut(), &services).unwrap_err();
    assert!(error.contains("replaced"), "{error}");
    assert_eq!(fs::read(&destination).unwrap(), original_bytes);
    assert_eq!(fixture.owner(), replacement);
    assert_eq!(
        app.world()
            .resource::<Files>()
            .dialog
            .as_ref()
            .unwrap()
            .token,
        dialog.token
    );
    current(app.world(), &services, &replacement).unwrap();
    let all = tabs(app.world(), &services, &replacement).unwrap();
    assert_eq!(
        all.len(),
        1,
        "The obsolete save continuation cannot close the tab"
    );
    assert!(all[0].dirty && !all[0].saving && all[0].path.is_none());
}

#[test]
fn save_all_failure_preserves_remaining_edits_and_only_successful_retry_allows_exit() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, handle) = setup(&fixture);
    let first = fixture.owner();
    let first_path = path("inactive-design.limo");
    save_to(&mut app, &services, &handle, &first, &first_path);
    fixture.rename(&first, "Inactive unsaved edit").unwrap();
    let first_model = model(&fixture);
    execute(
        app.world_mut(),
        &handle,
        &services,
        &first,
        FileCommand::New,
    )
    .unwrap();
    drain(app.world_mut(), &services).unwrap();
    let second = fixture.owner();
    let second_path = path("active-design.limo");
    save_to(&mut app, &services, &handle, &second, &second_path);
    fixture.rename(&second, "Active unsaved edit").unwrap();
    let second_model = model(&fixture);

    fs::remove_file(&first_path).unwrap();
    fs::create_dir(&first_path).unwrap();
    let mut controller = Controller::new("main".into(), None, Arc::new(AtomicBool::new(false)));
    controller.workspace = app.world().resource::<Files>().workspace.clone();
    super::super::super::request_close(
        app.world_mut(),
        &mut controller,
        &services.bridge,
        &services.engine,
    )
    .unwrap();
    assert!(controller.close_pending && !controller.exit_after_receipt);
    let start = execute(
        app.world_mut(),
        &handle,
        &services,
        &second,
        FileCommand::SaveAllAndExit,
    )
    .unwrap();
    super::super::super::apply_host_result(&mut controller, &start);
    assert_eq!(start["saving_before_exit"], true);
    let error = drain(app.world_mut(), &services).unwrap_err();
    assert!(error.contains("could not replace save file"), "{error}");
    assert_eq!(fixture.owner(), first);
    assert_eq!(model(&fixture), first_model);
    assert_eq!(saved_model(&second_path), second_model);
    let all = tabs(app.world(), &services, &first).unwrap();
    assert_eq!(all.len(), 2, "A partial Save all cannot close either tab");
    let failed = all.iter().find(|tab| tab.owner == first).unwrap();
    let saved = all.iter().find(|tab| tab.owner == second).unwrap();
    assert!(failed.dirty && !failed.saving);
    assert!(!saved.dirty && !saved.saving);
    assert_eq!(failed.path.as_ref(), Some(&first_path));
    assert_eq!(saved.path.as_ref(), Some(&second_path));
    super::super::super::request_close(
        app.world_mut(),
        &mut controller,
        &services.bridge,
        &services.engine,
    )
    .unwrap();
    assert!(controller.close_pending && !controller.exit_after_receipt);

    fs::remove_dir(&first_path).unwrap();
    let already_saved = fs::read(&second_path).unwrap();
    let retry = execute(
        app.world_mut(),
        &handle,
        &services,
        &first,
        FileCommand::SaveAllAndExit,
    )
    .unwrap();
    super::super::super::apply_host_result(&mut controller, &retry);
    let result = drain(app.world_mut(), &services).unwrap();
    assert_eq!(result["request_exit"], true);
    assert_eq!(saved_model(&first_path), first_model);
    assert_eq!(
        fs::read(&second_path).unwrap(),
        already_saved,
        "Retry need not rewrite a clean tab"
    );
    assert!(tabs(app.world(), &services, &first)
        .unwrap()
        .iter()
        .all(|tab| !tab.dirty));
    super::super::super::request_close(
        app.world_mut(),
        &mut controller,
        &services.bridge,
        &services.engine,
    )
    .unwrap();
    assert!(controller.exit_after_receipt && !controller.close_pending);
}
