use super::*;

fn world() -> World {
    let mut world = World::new();
    initialize(
        &mut world,
        Arc::new(Mutex::new(DocumentWorkspace::default())),
    );
    world.insert_resource(NativeInterfaceHandle::new(|| {}));
    world
}

fn drain(world: &mut World) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        super::super::poll(world);
        let state = &world.resource::<Files>().script;
        if !state.loading() && state.library.pending().is_none() {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Recipe inspection timed out: {:?}",
            state.status
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

#[test]
fn native_script_catalog_is_shared_and_source_only_including_flagship_recipes() {
    let shared = limo_cad_mcp::script_examples();
    assert_eq!(examples().len(), shared.as_array().unwrap().len());
    assert_eq!(examples()[0].id, "fillet-basics");
    for entry in examples() {
        let loaded = inspect_example(entry).unwrap();
        assert_eq!(loaded.authored(), entry.source);
        assert_eq!(loaded.name, entry.name);
        assert!(
            loaded.path.is_none(),
            "Bundled recipes must not invent filesystem provenance"
        );
        assert_eq!(loaded.example.unwrap().id, entry.id);
    }
    assert!(examples()
        .iter()
        .any(|entry| entry.kind == "flagship-candidate"));
    let mut world = world();
    let before = world.resource::<Files>().script.generation;
    assert!(open_recipe(&mut world, "not-installed").is_err());
    assert_eq!(world.resource::<Files>().script.generation, before);
    let queued = open_recipe(&mut world, "garden-bench").unwrap();
    assert_eq!(queued["recipe"]["status"], "queued");
    assert!(world.resource::<Files>().script.loaded.is_none());
    drain(&mut world);
    let files = world.resource::<Files>();
    assert!(
        files.lesson.is_none(),
        "Opening a recipe must never start playback"
    );
    assert!(files.script.source_path.is_none());
    assert!(files.script.path.is_empty());
    assert!(files.script.editor_open);
    assert_eq!(files.script.source, example("garden-bench").unwrap().source);
    assert!(!files.script.dirty());
    assert!(files.script.selected(files.script.generation).is_ok());
}

#[test]
fn native_script_catalog_queue_preserves_dirty_drafts_until_discard_or_cancel() {
    let mut world = world();
    world
        .resource_mut::<Files>()
        .script
        .accept(inspect_example(example("fillet-basics").unwrap()).unwrap())
        .unwrap();
    let edited = format!(
        "{}\n// unsaved edit",
        world.resource::<Files>().script.source
    );
    editor::edit_source(&mut world, &ControlInput::SetValue(edited.clone())).unwrap();
    open_recipe(&mut world, "garden-bench").unwrap();
    open_recipe(&mut world, "garden-bench").unwrap();
    assert_eq!(world.resource::<Files>().script.library.requests.len(), 1);
    poll(&mut world);
    let state = &world.resource::<Files>().script;
    assert_eq!(state.source, edited);
    assert!(!state.loading());
    assert!(state.editor_open);
    let token = state.library.pending().unwrap().token;
    assert!(cancel_open(&mut world, token + 1).is_err());
    cancel_open(&mut world, token).unwrap();
    assert_eq!(world.resource::<Files>().script.source, edited);
    assert!(world.resource::<Files>().script.dirty());
    assert!(world.resource::<Files>().script.library.pending().is_none());
    open_recipe(&mut world, "garden-bench").unwrap();
    editor::discard(&mut world).unwrap();
    drain(&mut world);
    assert_eq!(
        world.resource::<Files>().script.source,
        example("garden-bench").unwrap().source
    );
    assert!(world.resource::<Files>().lesson.is_none());
}

#[test]
fn native_script_catalog_waits_for_source_work_and_retains_user_document() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = crate::session_bridge::native_interface::tests::Fixture::new();
    fixture
        .rename(&fixture.owner(), "Retain recipe review document")
        .unwrap();
    let model = fixture.engine.engine_call("project_export_model", "");
    let owner = fixture.owner();
    let (mut app, _, _) = super::super::super::tests::setup(&fixture);
    let (send, receive) = mpsc::channel();
    app.world_mut().resource_mut::<Files>().script.loading = Some(Mutex::new(receive));
    open_recipe(app.world_mut(), "fillet-basics").unwrap();
    poll(app.world_mut());
    assert!(app
        .world()
        .resource::<Files>()
        .script
        .library
        .pending()
        .is_some());
    send.send(Err("First source load rejected".into())).unwrap();
    drain(app.world_mut());
    assert_eq!(fixture.owner(), owner);
    assert_eq!(
        fixture.engine.engine_call("project_export_model", ""),
        model
    );
    assert!(app.world().resource::<Files>().lesson.is_none());
}

#[test]
fn native_script_catalog_bounded_queue_and_received_catalog_validation() {
    let mut library = Library::default();
    for index in 0..16 {
        let entry = Box::leak(Box::new(Example {
            id: format!("test-{index}"),
            name: "Test".into(),
            summary: String::new(),
            kind: "lesson".into(),
            source: String::new(),
            preview: false,
        }));
        library.queue(entry).unwrap();
        library.queue(entry).unwrap();
    }
    assert_eq!(library.requests.len(), 16);
    assert!(library.queue(example("fillet-basics").unwrap()).is_err());
    assert!(example("limo-cad://recipe/fillet-basics").is_err());
    assert!(example("../fillet-basics").is_err());
    assert!(example("fillet-basics?run=true").is_err());
}

#[test]
fn native_script_catalog_cold_start_enters_the_same_source_queue() {
    let mut world = World::new();
    world.insert_resource(Controller::new(
        "main".into(),
        None,
        Arc::new(AtomicBool::new(false)),
    ));
    world.insert_resource(NativeInterfaceHandle::new(|| {}));
    super::super::super::super::open_startup_recipe(&mut world, "fillet-basics");
    assert!(world.resource::<Files>().scripts);
    assert_eq!(
        world
            .resource::<Files>()
            .script
            .library
            .pending()
            .unwrap()
            .example
            .id,
        "fillet-basics"
    );
    assert!(world.resource::<Files>().script.loaded.is_none());
    drain(&mut world);
    assert!(world.resource::<Files>().script.editor_open);
    assert!(world.resource::<Files>().lesson.is_none());
}

#[test]
fn os_recipe_url_double_enters_the_same_source_queue() {
    let mut world = World::new();
    world.insert_resource(Controller::new(
        "main".into(),
        None,
        Arc::new(AtomicBool::new(false)),
    ));
    world.insert_resource(NativeInterfaceHandle::new(|| {}));
    RecipeUrlDouble::new("limo-cad://recipe/fillet-basics").deliver(&mut world);
    assert!(world.resource::<Controller>().status.is_empty());
    assert_eq!(
        world
            .resource::<Files>()
            .script
            .library
            .pending()
            .unwrap()
            .example
            .id,
        "fillet-basics"
    );
    drain(&mut world);
    assert!(world.resource::<Files>().script.editor_open);
    assert!(world.resource::<Files>().lesson.is_none());

    let mut rejected = World::new();
    rejected.insert_resource(Controller::new(
        "main".into(),
        None,
        Arc::new(AtomicBool::new(false)),
    ));
    RecipeUrlDouble::new("limo-cad://recipe/not-installed").deliver(&mut rejected);
    assert!(!rejected.resource::<Controller>().status.is_empty());
    assert!(rejected.get_resource::<Files>().is_none());
}
