//! Actual controller transitions over immutable bundles; native qualification is separate.
use super::*;

fn fixture() -> (World, NativeInterfaceHandle) {
    let mut world = World::new();
    initialize(
        &mut world,
        Arc::new(Mutex::new(DocumentWorkspace::default())),
    );
    world.init_resource::<Assets<Image>>();
    world.init_resource::<ViewportUiAssets>();
    world.resource_mut::<Files>().script.editor_open = true;
    (world, NativeInterfaceHandle::new(|| {}))
}

fn settle(world: &mut World) {
    // Existing preparation uses the bundled cache, never a live CAD document.
    let channel = world
        .resource_mut::<Files>()
        .script
        .preview
        .build
        .take()
        .unwrap();
    let frames = channel
        .lock()
        .unwrap()
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .unwrap();
    let preview = &mut world.resource_mut::<Files>().script.preview;
    preview.retained = Some(Retained::new(preview.service.clone(), frames).unwrap());
    preview.transition = None;
}

#[test]
fn native_script_preview_back_restores_prerequisite_origin_and_visible_localized_routes() {
    let (mut world, handle) = fixture();
    command(&mut world, &handle, Action::Index, &ControlInput::Click).unwrap();
    world.resource_mut::<Files>().script.preview.index_page = 1;
    command(
        &mut world,
        &handle,
        Action::Lesson(lesson::Id::Cut),
        &ControlInput::Click,
    )
    .unwrap();
    settle(&mut world);
    command(
        &mut world,
        &handle,
        Action::Scenario(lesson::CutScenario::MeasuredDepth),
        &ControlInput::Click,
    )
    .unwrap();
    command(&mut world, &handle, Action::Step(3), &ControlInput::Click).unwrap();
    command(&mut world, &handle, Action::Help, &ControlInput::Click).unwrap();
    {
        let preview = &mut world.resource_mut::<Files>().script.preview;
        preview.help_view.offset = 84.;
        preview.yaw = 0.8;
        preview.pitch = 0.4;
    }
    let origin = world.resource::<Files>().script.preview.location().unwrap();
    let prerequisite = lesson::Id::Cut.prerequisite().unwrap();
    command(
        &mut world,
        &handle,
        Action::Read(prerequisite),
        &ControlInput::Click,
    )
    .unwrap();
    settle(&mut world);
    assert_eq!(
        world.resource::<Files>().script.preview.lesson,
        Some(prerequisite)
    );
    command(&mut world, &handle, Action::Back, &ControlInput::Click).unwrap();
    settle(&mut world);
    let preview = &world.resource::<Files>().script.preview;
    assert_eq!(preview.location(), Some(origin));
    assert_eq!(preview.frame_index(), 5, "Restore the measured Cut outcome");
    assert!(preview.help && !preview.playing && preview.blend.is_none());
    assert!(preview.images.footprint().0 <= 2);
    let camera = world.spawn_empty().id();
    let theme = crate::native_viewport::ui::theme(&world);
    let mut widgets = super::super::super::super::chrome::Widgets::default();
    for locale in crate::app_preferences::locale::SUPPORTED {
        crate::native_viewport::localization::set_locale(&mut world, locale);
        widgets.begin();
        panel::paint(&mut world, camera, &mut widgets, 700., 600., theme).unwrap();
        widgets.finish(&mut world);
        let back = widgets.entity("show-me-demo-back").unwrap();
        let control = world.get::<InterfaceControl>(back).unwrap();
        assert_eq!(control.label, lesson::text(&world, "ui.backToGuides"));
        assert!(!control.disabled && !control.label.is_empty());
        assert_eq!(
            world.get::<NativeCommandBinding>(back).unwrap().command,
            NativeCommand::File(FileCommand::ScriptPreview(Action::Back))
        );
    }
    command(&mut world, &handle, Action::Back, &ControlInput::Click).unwrap();
    let preview = &world.resource::<Files>().script.preview;
    assert!(preview.index_open && preview.retained.is_none());
    assert_eq!(
        (preview.index_page, preview.index_focus),
        (1, Some(lesson::Id::Cut))
    );
    assert!(preview.history.peek().is_none());
    command(&mut world, &handle, Action::Close, &ControlInput::Click).unwrap();
    assert!(!world.resource::<Files>().script.preview.open);
    assert!(world.resource::<Files>().script.editor_open);
    command(&mut world, &handle, Action::Index, &ControlInput::Click).unwrap();
    assert_eq!(world.resource::<Files>().script.preview.index_page, 1);
}

#[test]
fn native_script_preview_close_from_index_lesson_releases_the_guide_and_remembers_page() {
    let (mut world, handle) = fixture();
    command(&mut world, &handle, Action::Index, &ControlInput::Click).unwrap();
    world.resource_mut::<Files>().script.preview.index_page = 3;
    command(
        &mut world,
        &handle,
        Action::Lesson(lesson::Id::CircularPattern),
        &ControlInput::Click,
    )
    .unwrap();
    settle(&mut world);
    let retained = world
        .resource::<Files>()
        .script
        .preview
        .retained
        .as_ref()
        .unwrap();
    let (service, view, document) = (
        retained.service.clone(),
        retained.view.clone(),
        retained.descriptor.preview_id.clone(),
    );
    command(&mut world, &handle, Action::Close, &ControlInput::Click).unwrap();
    let preview = &world.resource::<Files>().script.preview;
    assert!(!preview.open && !preview.index_open && !preview.playing);
    assert_eq!(preview.index_page, 3);
    assert!(preview.retained.is_none() && preview.transition.is_none() && preview.render.is_none());
    assert!(preview.history.peek().is_none() && preview.restore.is_none());
    assert!(preview.wake.is_none() && preview.blend.is_none() && preview.drag.is_none());
    assert_eq!(preview.images.footprint().0, 0);
    assert!(world.resource::<Assets<Image>>().is_empty());
    let request = RenderRequest {
        palette: None,
        preview_id: document,
        view_id: view,
        revision: 100,
        frame_index: 0,
        width: 300,
        height: 176,
        yaw: HOME.0,
        pitch: HOME.1,
    };
    assert!(
        service.render(request).is_err(),
        "Closing a guide retires its render authority"
    );
    assert!(preview.return_to.is_none());
    assert!(world.resource::<Files>().script.editor_open);
    command(&mut world, &handle, Action::Index, &ControlInput::Click).unwrap();
    assert_eq!(world.resource::<Files>().script.preview.index_page, 3);
}

#[test]
fn native_script_preview_close_while_preparing_rejects_late_frames_and_keeps_index_memory() {
    let (mut world, handle) = fixture();
    command(&mut world, &handle, Action::Index, &ControlInput::Click).unwrap();
    let (send, receive) = mpsc::channel();
    {
        let preview = &mut world.resource_mut::<Files>().script.preview;
        preview.index_open = false;
        preview.index_page = 4;
        preview.lesson = Some(lesson::Id::SketchPolygon);
        preview.history.push(navigation::Location::Index {
            page: 4,
            focus: Some(lesson::Id::SketchPolygon),
        });
        preview.build = Some(Mutex::new(receive));
    }
    command(&mut world, &handle, Action::Close, &ControlInput::Click).unwrap();
    assert!(
        available(&world).is_err(),
        "Keep the existing single-worker admission fence"
    );
    send.send(Ok(vec![Frame {
        caption: "late".into(),
        ..default()
    }]))
    .unwrap();
    poll(&mut world);
    let preview = &world.resource::<Files>().script.preview;
    assert!(
        !preview.open && !preview.index_open && preview.retained.is_none() && !preview.building()
    );
    assert_eq!(preview.index_page, 4);
    assert!(preview.transition.is_none() && preview.render.is_none());
    assert_eq!(preview.render_jobs, 0);
    assert!(available(&world).is_ok());
}

#[test]
fn native_script_preview_standalone_book_has_no_back_route_and_close_restores_workspace() {
    let (mut world, handle) = fixture();
    world.resource_mut::<Files>().script.source = "unfinished { draft".into();
    command(
        &mut world,
        &handle,
        Action::Lesson(lesson::Id::SketchChamfer),
        &ControlInput::Click,
    )
    .unwrap();
    settle(&mut world);
    assert!(world
        .resource::<Files>()
        .script
        .preview
        .history
        .peek()
        .is_none());
    assert!(command(&mut world, &handle, Action::Back, &ControlInput::Click).is_err());
    let camera = world.spawn_empty().id();
    let theme = crate::native_viewport::ui::theme(&world);
    let mut widgets = super::super::super::super::chrome::Widgets::default();
    widgets.begin();
    panel::paint(&mut world, camera, &mut widgets, 432., 600., theme).unwrap();
    widgets.finish(&mut world);
    assert!(widgets.entity("show-me-demo-back").is_none());
    command(&mut world, &handle, Action::Close, &ControlInput::Click).unwrap();
    let files = world.resource::<Files>();
    assert!(!files.script.preview.open && files.script.editor_open && !files.scripts);
    assert_eq!(files.script.source, "unfinished { draft");
    assert!(files.script.preview.retained.is_none() && files.script.preview.transition.is_none());
    assert_eq!(files.script.preview.images.footprint().0, 0);
}
