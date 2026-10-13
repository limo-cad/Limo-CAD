use super::*;

fn command(
    fixture: &Fixture,
    app: &mut App,
    services: &NativeServices,
    handle: &NativeInterfaceHandle,
    command: FileCommand,
) {
    execute(app.world_mut(), handle, services, &fixture.owner(), command).unwrap();
    if worker::busy(app.world()) {
        let value = drain(app.world_mut(), services).unwrap();
        assert!(
            value["render_error"].is_null() && value["presentation_error"].is_null(),
            "{value}"
        );
    }
}

#[test]
fn file_tabs_restore_drawing_and_active_sheet_after_switch_and_successful_close() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, handle) = setup(&fixture);
    let first = fixture.owner();
    let drawing: limo_cad_sketch::DrawingDocumentDto = serde_json::from_value(json!({
        "sheets":[
            {"id":1,"name":"First sheet","format":"a4","orientation":"landscape"},
            {"id":2,"name":"Selected sheet","format":"a4","orientation":"landscape"}
        ],"active_sheet_id":2,"next_sheet_id":3
    }))
    .unwrap();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &first,
            "drawing_set_document",
            &serde_json::to_value(&drawing).unwrap(),
            || Ok(()),
        )
        .unwrap();
    let before = fixture.engine.engine_call("project_export_model", "");
    refresh_native_model(&fixture.engine, app.world_mut(), true).unwrap();
    workbench::observe_document(app.world_mut(), &first);
    workbench::execute(
        app.world_mut(),
        &workbench::Command::Workspace(workbench::Workspace::Drawing),
    )
    .unwrap();

    command(&fixture, &mut app, &services, &handle, FileCommand::New);
    let second = fixture.owner();
    assert_eq!(
        workbench::workspace(app.world()),
        workbench::Workspace::Solid
    );
    workbench::execute(
        app.world_mut(),
        &workbench::Command::Workspace(workbench::Workspace::Cam),
    )
    .unwrap();
    command(
        &fixture,
        &mut app,
        &services,
        &handle,
        FileCommand::Activate(first.clone()),
    );
    assert_eq!(
        workbench::workspace(app.world()),
        workbench::Workspace::Drawing
    );
    assert_eq!(fixture.engine.drawing_snapshot(), drawing);
    assert_eq!(
        fixture.engine.engine_call("project_export_model", ""),
        before
    );
    command(
        &fixture,
        &mut app,
        &services,
        &handle,
        FileCommand::Activate(second.clone()),
    );
    assert_eq!(workbench::workspace(app.world()), workbench::Workspace::Cam);

    fixture.rename(&second, "Dirty second").unwrap();
    command(&fixture, &mut app, &services, &handle, FileCommand::Close);
    let token = app
        .world()
        .resource::<Files>()
        .dialog
        .as_ref()
        .unwrap()
        .token;
    command(
        &fixture,
        &mut app,
        &services,
        &handle,
        FileCommand::Cancel(token),
    );
    assert_eq!(workbench::workspace(app.world()), workbench::Workspace::Cam);
    let stale = current(app.world_mut(), &services, &second).unwrap();
    fixture.rename(&second, "Changed before close").unwrap();
    transition(app.world_mut(), stale, None, Some(true)).unwrap();
    assert!(drain(app.world_mut(), &services)
        .unwrap_err()
        .contains("document changed"));
    assert_eq!(workbench::workspace(app.world()), workbench::Workspace::Cam);

    command(&fixture, &mut app, &services, &handle, FileCommand::Close);
    let token = app
        .world()
        .resource::<Files>()
        .dialog
        .as_ref()
        .unwrap()
        .token;
    command(
        &fixture,
        &mut app,
        &services,
        &handle,
        FileCommand::Discard(token),
    );
    assert_eq!(fixture.owner(), first);
    assert_eq!(
        workbench::workspace(app.world()),
        workbench::Workspace::Drawing
    );
    assert_eq!(fixture.engine.drawing_snapshot(), drawing);
    assert_eq!(
        fixture.engine.engine_call("project_export_model", ""),
        before
    );
}
