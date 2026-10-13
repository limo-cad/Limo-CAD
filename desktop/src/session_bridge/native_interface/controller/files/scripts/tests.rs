use super::*;
use crate::session_bridge::native_interface::tests::Fixture;

fn source(name: &str) -> String {
    json!({"version":1,"name":name,"steps":[{"note":"Inspected without running"}]}).to_string()
}

#[test]
fn inspected_file_retains_authored_provenance_and_freezes_expanded_includes() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let path = super::super::tests::path("opened-script.limo.jsonc");
    let fragment = path.with_file_name("fragment.collection.jsonc");
    let authored = r#"// Retain this comment and include for source provenance.
{"version":1,"name":"Opened file","includes":["fragment.collection.jsonc"],"steps":[{"note":"Root chapter"}]}"#;
    std::fs::write(&path, authored).unwrap();
    std::fs::write(&fragment, r#"{"steps":[{"note":"Frozen chapter"}]}"#).unwrap();
    let before = fixture.engine.engine_call("project_export_model", "");
    let loaded = inspect(path.clone()).unwrap();
    assert_eq!(loaded.path, Some(path.clone()));
    assert_eq!((loaded.steps, loaded.checks), (2, 0));
    assert_eq!(loaded.inspection["authored_source"], authored);
    let snapshot = loaded.source().to_owned();
    assert!(snapshot.contains("Frozen chapter"));
    assert!(!snapshot.contains("includes"));
    std::fs::write(&fragment, "changed after inspection").unwrap();
    std::fs::write(&path, "changed after inspection").unwrap();
    assert_eq!(loaded.source(), snapshot);
    assert!(limo_cad_mcp::inspect_script(json!({"source":loaded.source()})).is_ok());
    assert!(inspect(path).is_err());
    assert_eq!(
        fixture.engine.engine_call("project_export_model", ""),
        before
    );
}

#[test]
fn inspection_uses_shared_path_size_include_and_operation_preflight() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let before = fixture.engine.engine_call("project_export_model", "");
    assert!(inspect(PathBuf::from("relative.limo.jsonc")).is_err());
    let wrong_suffix = super::super::tests::path("not-a-command-file.json");
    std::fs::write(&wrong_suffix, source("Wrong suffix")).unwrap();
    assert!(inspect(wrong_suffix).is_err());
    let path = super::super::tests::path("rejected.limo.jsonc");
    for source in [
        "{}".to_owned(),
        r#"{"version":1,"name":"Escape","includes":["../outside.collection.jsonc"],"steps":[{"note":"Do not execute"}]}"#.into(),
        r#"{"version":1,"name":"Transport","steps":[{"call":{"group":"document/session","operation":"cad_interface","arguments":{}}}]}"#.into(),
        r#"{"version":1,"name":"Wrong group","steps":[{"call":{"group":"solid/inspect","operation":"solid_scene","arguments":{}}}]}"#.into(),
        " ".repeat(16 * 1024 * 1024 + 1),
    ] {
        std::fs::write(&path, source).unwrap();
        assert!(inspect(path.clone()).is_err());
    }
    assert_eq!(
        fixture.engine.engine_call("project_export_model", ""),
        before
    );
}

#[test]
fn failed_load_preserves_previous_source_and_replacement_retires_old_run_control() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let _fixture = Fixture::new();
    let path = super::super::tests::path("retained-script.limo.jsonc");
    std::fs::write(&path, source("First source")).unwrap();
    let mut world = World::new();
    initialize(
        &mut world,
        Arc::new(Mutex::new(DocumentWorkspace::default())),
    );
    world
        .resource_mut::<Files>()
        .script
        .accept(inspect(path.clone()).unwrap())
        .unwrap();
    let original_generation = world.resource::<Files>().script.generation;
    let original = world
        .resource::<Files>()
        .script
        .selected(original_generation)
        .unwrap();
    let (send, receive) = mpsc::channel();
    world.resource_mut::<Files>().script.loading = Some(Mutex::new(receive));
    assert!(world
        .resource::<Files>()
        .script
        .selected(original_generation)
        .is_err());
    send.send(Err("Invalid replacement".into())).unwrap();
    poll(&mut world);
    let state = &world.resource::<Files>().script;
    assert!(Arc::ptr_eq(
        &state.selected(original_generation).unwrap(),
        &original
    ));
    assert_eq!(
        state.status.as_deref(),
        Some("Script not loaded: Invalid replacement")
    );
    std::fs::write(&path, source("Second source")).unwrap();
    world
        .resource_mut::<Files>()
        .script
        .accept(inspect(path).unwrap())
        .unwrap();
    let state = &world.resource::<Files>().script;
    assert!(state.selected(original_generation).is_err());
    assert_eq!(
        state.selected(state.generation).unwrap().name,
        "Second source"
    );
    assert_eq!(original.name, "First source");
}

#[test]
fn script_new_design_retains_current_work_and_rejects_stale_or_rejected_handoffs() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    fixture
        .rename(&fixture.owner(), "Retain this authored design")
        .unwrap();
    fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &fixture.owner(),
            "drawing_create_sheet",
            &json!({"name":"Keep this sheet","format":"a4","orientation":"landscape"}),
            || Ok(()),
        )
        .unwrap();
    assert!(!fixture.engine.is_blank_for_script());
    let (app, services, _) = super::super::tests::setup(&fixture);
    let workspace = app.world().resource::<Files>().workspace.clone();
    let original = fixture.owner();
    let original_model = fixture.engine.engine_call("project_export_model", "");
    let receipt = services
        .bridge
        .native_document_receipt(&services.engine, &original)
        .unwrap();
    assert!(new_document(&services, &workspace, &receipt, || Err(
        "Retired control".into()
    ))
    .is_err());
    assert_eq!(fixture.owner(), original);
    let new = new_document(&services, &workspace, &receipt, || Ok(())).unwrap();
    assert_ne!(new.context.document_id, original.document_id);
    assert!(services.engine.is_blank_for_script());
    assert!(new_document(&services, &workspace, &receipt, || Ok(())).is_err());
    let current = services
        .bridge
        .native_document_receipt(&services.engine, &new.context)
        .unwrap();
    workspace
        .lock()
        .unwrap()
        .activate_guarded(
            &services.bridge,
            &services.engine,
            &current,
            &original,
            || Ok(()),
        )
        .unwrap();
    assert_eq!(
        fixture.engine.engine_call("project_export_model", ""),
        original_model
    );
    assert_eq!(
        workspace
            .lock()
            .unwrap()
            .summaries(&services.bridge, &original)
            .unwrap()
            .len(),
        2
    );
}

fn settle_picker(world: &mut World, services: &NativeServices) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        super::super::poll(world, services).unwrap();
        let files = world.resource::<Files>();
        if files.picker.is_none() && !files.script.loading() {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "script chooser did not finish: {:?}",
            files.script.status
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

#[test]
fn os_script_chooser_cancel_open_and_save_use_the_prepared_choice() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let path = super::super::tests::path("chooser.limo.jsonc");
    let saved = super::super::tests::path("chooser-saved.limo.jsonc");
    std::fs::write(&path, source("Chooser")).unwrap();
    let workspace = Arc::new(Mutex::new(DocumentWorkspace::default()));
    let receipt = workspace
        .lock()
        .unwrap()
        .observe(&fixture.bridge, &fixture.engine, "main")
        .unwrap();
    let mut world = World::new();
    initialize(&mut world, workspace);
    let handle = NativeInterfaceHandle::new(|| {});
    world.insert_resource(handle.clone());
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };

    super::dialog::prepare(None);
    choose(&mut world, &handle, receipt.clone()).unwrap();
    settle_picker(&mut world, &services);
    assert!(world.resource::<Files>().script.loaded.is_none());

    super::dialog::prepare(Some(path.clone()));
    choose(&mut world, &handle, receipt.clone()).unwrap();
    settle_picker(&mut world, &services);
    assert_eq!(
        world
            .resource::<Files>()
            .script
            .loaded
            .as_ref()
            .and_then(|loaded| loaded.path.clone()),
        Some(path)
    );

    let edited = source("Chooser saved");
    edit_source(&mut world, &ControlInput::SetValue(edited.clone())).unwrap();
    super::dialog::prepare(Some(saved.clone()));
    save_as(&mut world, &handle, receipt).unwrap();
    settle_picker(&mut world, &services);
    assert_eq!(std::fs::read_to_string(&saved).unwrap(), edited);
    assert_eq!(
        world.resource::<Files>().script.source_path.as_ref(),
        Some(&saved)
    );
}
