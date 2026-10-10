use super::*;
use crate::native_viewport::interface_shell::fields;
use crate::session_bridge::native_interface::{controller, tests::Fixture};

fn setup(fixture: &Fixture) -> (App, NativeServices, NativeInterfaceHandle, Controller) {
    let (mut app, services, handle) = super::super::super::tests::setup(fixture);
    let path = super::super::super::tests::path("script-exit.limo.jsonc");
    std::fs::write(
        &path,
        r#"{"version":1,"name":"Source exit","steps":[{"note":"Review only"}]}"#,
    )
    .unwrap();
    app.world_mut()
        .resource_mut::<Files>()
        .script
        .accept(inspect(path).unwrap())
        .unwrap();
    let mut state = Controller::new("main".into(), None, Arc::new(AtomicBool::new(false)));
    state.workspace = app.world().resource::<Files>().workspace.clone();
    (app, services, handle, state)
}

#[test]
fn native_script_exit_both_commands_preserve_dirty_source_before_cad_close_confirmation() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, handle, mut state) = setup(&fixture);
    let owner = fixture.owner();
    let model = fixture.engine.engine_call("project_export_model", "");
    editor::edit_source(
        app.world_mut(),
        &ControlInput::SetValue("{ unsaved draft".into()),
    )
    .unwrap();
    for ui in [
        json!({"action":"file","command":"exit"}),
        json!({"action":"window","mode":"close"}),
    ] {
        let result = controller::apply_control(
            app.world_mut(),
            &handle,
            &services,
            &mut state,
            &owner,
            &json!({"expires_ms":now_ms()+30_000,"ui":ui}),
        );
        assert!(result.unwrap_err().contains("Unsaved script source"));
        assert!(!state.exit_after_receipt);
        assert!(
            !state.close_pending,
            "Source decision must stay reachable before the CAD close modal"
        );
        assert!(app.world().resource::<Files>().scripts);
        assert!(app.world().resource::<Files>().script.editor_open);
        assert_eq!(
            app.world().resource::<Files>().script.source,
            "{ unsaved draft"
        );
    }
    assert_eq!(fixture.owner(), owner);
    assert_eq!(
        fixture.engine.engine_call("project_export_model", ""),
        model
    );
    editor::discard(app.world_mut()).unwrap();
    controller::request_close(
        app.world_mut(),
        &mut state,
        &fixture.bridge,
        &fixture.engine,
    )
    .unwrap();
    assert!(
        state.exit_after_receipt,
        "A clean design may close after explicit source discard"
    );
}

#[test]
fn native_script_exit_title_bar_checks_uncommitted_text_and_rejected_edits_without_resetting_them()
{
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, _, _, mut state) = setup(&fixture);
    let baseline = app.world().resource::<Files>().script.source.clone();
    let native_draft = format!("{baseline}\n// typed but not blurred");
    let entity = app
        .world_mut()
        .spawn(EditableText::new(native_draft.clone()))
        .id();
    app.world_mut().resource_mut::<Files>().script.source_entity = Some(entity);
    assert!(!app.world().resource::<Files>().script.dirty());
    assert!(controller::close_from_window_event(
        app.world_mut(),
        &mut state,
        &fixture.bridge,
        &fixture.engine,
        None
    )
    .unwrap_err()
    .contains("Unsaved script source"));
    assert!(!state.exit_after_receipt);
    assert!(!state.close_pending);
    assert_eq!(
        app.world().resource::<Files>().script.source,
        baseline,
        "A blocked close must not publish and reset the active field's Undo history"
    );
    assert_eq!(
        app.world()
            .get::<EditableText>(entity)
            .unwrap()
            .value()
            .to_string(),
        native_draft
    );
    app.world_mut()
        .get_mut::<EditableText>(entity)
        .unwrap()
        .editor
        .set_text(&baseline);
    fields::limits::enable(app.world_mut(), entity, baseline.len());
    app.world_mut()
        .get_mut::<fields::limits::ByteLimit>(entity)
        .unwrap()
        .rejected = Some("Rejected paste".into());
    assert!(guard_exit(app.world_mut()).is_err());
    fields::limits::clear(app.world_mut(), entity);
    assert!(guard_exit(app.world_mut()).is_ok());
}

#[test]
fn native_script_exit_waits_for_actual_source_save_and_allows_a_saved_invalid_draft() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, _, handle, mut state) = setup(&fixture);
    let model = fixture.engine.engine_call("project_export_model", "");
    let draft = "{ saved unfinished source";
    editor::edit_source(app.world_mut(), &ControlInput::SetValue(draft.into())).unwrap();
    let path = super::super::super::tests::path("saved-exit-draft.limo.jsonc");
    let generation = app.world().resource::<Files>().script.generation;
    editor::save(app.world_mut(), &handle, generation, path.clone()).unwrap();
    assert!(controller::request_close(
        app.world_mut(),
        &mut state,
        &fixture.bridge,
        &fixture.engine
    )
    .unwrap_err()
    .contains("script file operation"));
    assert!(!state.exit_after_receipt);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while app.world().resource::<Files>().script.loading() {
        editor::poll(app.world_mut());
        assert!(
            std::time::Instant::now() < deadline,
            "Source save timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(std::fs::read_to_string(path).unwrap(), draft);
    assert!(!app.world().resource::<Files>().script.dirty());
    controller::request_close(
        app.world_mut(),
        &mut state,
        &fixture.bridge,
        &fixture.engine,
    )
    .unwrap();
    assert!(state.exit_after_receipt);
    assert_eq!(
        fixture.engine.engine_call("project_export_model", ""),
        model
    );
}

#[test]
fn native_script_exit_picker_and_recipe_delivery_are_fenced_until_resolved() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let (mut app, services, _, _) = setup(&fixture);
    let receipt = services
        .bridge
        .native_document_receipt(&services.engine, &fixture.owner())
        .unwrap();
    for kind in [PickerKind::Script, PickerKind::ScriptSave(1)] {
        let (_send, receive) = mpsc::channel();
        app.world_mut().resource_mut::<Files>().picker = Some(Picker {
            receipt: receipt.clone(),
            kind,
            result: Mutex::new(receive),
        });
        assert!(guard_exit(app.world_mut())
            .unwrap_err()
            .contains("script file operation"));
        app.world_mut().resource_mut::<Files>().picker = None;
    }
    catalog::open_recipe(app.world_mut(), "garden-bench").unwrap();
    assert!(guard_exit(app.world_mut())
        .unwrap_err()
        .contains("queued example"));
    let token = app
        .world()
        .resource::<Files>()
        .script
        .library
        .pending()
        .unwrap()
        .token;
    catalog::cancel_open(app.world_mut(), token).unwrap();
    assert!(guard_exit(app.world_mut()).is_ok());
}
