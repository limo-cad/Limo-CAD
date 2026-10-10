use super::*;
use limo_cad_project_file::ProjectArchive;

fn sketch(fixture: &Fixture, app: &mut App) -> DocumentContext {
    let owner = fixture.owner();
    for (operation, arguments) in [
        ("sketch_begin", json!({"type":"origin_plane","plane":"xy"})),
        (
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":20.,"y":10.},"ctrl_held":true}),
        ),
    ] {
        fixture
            .bridge
            .apply_native_mutation(&fixture.engine, &owner, operation, &arguments, || Ok(()))
            .unwrap();
    }
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    owner
}

#[test]
fn close_during_a_sketch_prompts_and_cancel_preserves_the_exact_sketch_and_preview() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, handle) = setup(&fixture);
    let owner = sketch(&fixture, &mut app);
    let original = fixture.engine.engine_call("active_sketch", "");
    native_viewport::apply_interface_preview(
        app.world_mut(),
        &owner.document_id,
        native_viewport::ViewportPreview {
            lines: vec![native_viewport::ViewportLineLayer {
                segments: vec![20., 10., 0., 32.3, 15., 0.].into(),
                ..default()
            }],
            ..default()
        },
    )
    .unwrap();
    let unfinished = native_viewport::interface_preview_snapshot(app.world());
    let preview_revision = native_viewport::interface_preview_revision(app.world());
    let result = request(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        &json!({"command":"close"}),
    )
    .unwrap();
    assert_eq!(result["awaiting_input"], true);
    assert!(!worker::busy(app.world()));
    let dialog = app.world().resource::<Files>().dialog.clone().unwrap();
    execute(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        FileCommand::Cancel(dialog.token),
    )
    .unwrap();
    assert_eq!(fixture.owner(), owner);
    assert_eq!(fixture.engine.engine_call("active_sketch", ""), original);
    assert_eq!(
        native_viewport::interface_preview_revision(app.world()),
        preview_revision
    );
    assert!(Arc::ptr_eq(
        &unfinished,
        &native_viewport::interface_preview_snapshot(app.world())
    ));
    assert_eq!(
        native_viewport::interface_view_snapshot(app.world()).2.mode,
        native_viewport::ViewportMode::Sketch
    );
    assert!(!awaiting(app.world()));
}

#[test]
fn discarding_an_active_sketch_closes_only_its_owned_tab() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, handle) = setup(&fixture);
    let owner = sketch(&fixture, &mut app);
    execute(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        FileCommand::CloseTab(owner.clone()),
    )
    .unwrap();
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
        FileCommand::Discard(token),
    )
    .unwrap();
    drain(app.world_mut(), &services).unwrap();
    let next = fixture.owner();
    assert_ne!(next.document_id, owner.document_id);
    assert!(crate::native_editor::active(&fixture.engine)
        .unwrap()
        .is_none());
    let tabs = tabs(app.world(), &services, &next).unwrap();
    assert_eq!(tabs.len(), 1);
    assert!(!tabs[0].dirty);
}

#[test]
fn save_and_close_finishes_the_sketch_then_saves_before_retiring_the_tab() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, handle) = setup(&fixture);
    let owner = fixture.owner();
    let destination = path("close-sketch.limo");
    request(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        &json!({"command":"save","path":destination}),
    )
    .unwrap();
    drain(app.world_mut(), &services).unwrap();
    let owner = sketch(&fixture, &mut app);
    let original = crate::native_editor::active(&fixture.engine)
        .unwrap()
        .unwrap();
    execute(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        FileCommand::CloseTab(owner.clone()),
    )
    .unwrap();
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
        FileCommand::SaveContinue(token),
    )
    .unwrap();
    drain(app.world_mut(), &services).unwrap();
    let next = fixture.owner();
    assert_ne!(next.document_id, owner.document_id);
    assert!(!awaiting(app.world()));
    let archive = ProjectArchive::decode(std::fs::read(&destination).unwrap()).unwrap();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &next,
            "cad_load_project_model",
            &json!({"model_json":archive.model_json()}),
            || Ok(()),
        )
        .unwrap();
    let frame = fixture.engine.viewport_frame();
    assert!(frame.document.active_sketch.is_none());
    assert_eq!(frame.document.finished_sketches.len(), 1);
    assert_eq!(
        frame.document.finished_sketches[0].entities,
        original.entities
    );
}

#[test]
fn cancelling_the_save_destination_keeps_the_sketch_and_close_prompt() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, handle) = setup(&fixture);
    let owner = sketch(&fixture, &mut app);
    let original = fixture.engine.engine_call("active_sketch", "");
    execute(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        FileCommand::Close,
    )
    .unwrap();
    let receipt = app
        .world()
        .resource::<Files>()
        .dialog
        .as_ref()
        .unwrap()
        .receipt
        .clone();
    let (send, receive) = mpsc::channel();
    app.world_mut().resource_mut::<Files>().picker = Some(Picker {
        receipt,
        kind: PickerKind::Project {
            save: true,
            continuation: Some(Intent::Close),
        },
        result: Mutex::new(receive),
    });
    send.send(None).unwrap();
    poll(app.world_mut(), &services).unwrap();
    assert_eq!(fixture.owner(), owner);
    assert_eq!(fixture.engine.engine_call("active_sketch", ""), original);
    assert_eq!(modal(app.world()), Some("file-dialog"));
    assert!(!worker::busy(app.world()));
}

#[test]
fn a_failed_sketch_save_keeps_the_tab_and_an_owned_close_prompt_for_retry() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, handle) = setup(&fixture);
    let owner = fixture.owner();
    let parent = path("destination");
    std::fs::create_dir(&parent).unwrap();
    let destination = parent.join("close-sketch.limo");
    request(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        &json!({"command":"save","path":destination}),
    )
    .unwrap();
    drain(app.world_mut(), &services).unwrap();
    let owner = sketch(&fixture, &mut app);
    std::fs::rename(&parent, path("original-destination")).unwrap();
    std::fs::write(&parent, "Save cannot write through a regular file").unwrap();
    execute(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        FileCommand::Close,
    )
    .unwrap();
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
        FileCommand::SaveContinue(token),
    )
    .unwrap();
    assert!(drain(app.world_mut(), &services).is_err());
    assert_eq!(fixture.owner(), owner);
    let receipt = services
        .bridge
        .native_document_receipt(&services.engine, &owner)
        .unwrap();
    assert_eq!(
        app.world()
            .resource::<Files>()
            .dialog
            .as_ref()
            .unwrap()
            .receipt,
        receipt
    );
    assert!(tabs(app.world(), &services, &owner).unwrap()[0].dirty);
    execute(
        app.world_mut(),
        &handle,
        &services,
        &owner,
        FileCommand::Cancel(token),
    )
    .unwrap();
    assert!(!awaiting(app.world()));
}
