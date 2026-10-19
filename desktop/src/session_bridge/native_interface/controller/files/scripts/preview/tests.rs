use super::*;
use crate::session_bridge::native_interface::tests::Fixture;

fn world() -> World {
    let mut world = World::new();
    initialize(
        &mut world,
        Arc::new(Mutex::new(DocumentWorkspace::default())),
    );
    world.init_resource::<Assets<Image>>();
    world.resource_mut::<Files>().scripts = true;
    world
}
fn select(world: &mut World, id: &str) {
    let example = catalog::examples()
        .iter()
        .find(|example| example.id == id)
        .unwrap();
    let mut loaded = inspected(
        None,
        limo_cad_mcp::inspect_script(json!({"source":example.source})).unwrap(),
    )
    .unwrap();
    loaded.example = Some(example);
    world.resource_mut::<Files>().script.accept(loaded).unwrap();
}
fn frames() -> Vec<Frame> {
    (0..3)
        .map(|index| Frame {
            assembly: None,
            caption: format!("Frame {index}"),
            scene: serde_json::from_value(json!({"bodies":[],"errors":[]})).unwrap(),
        })
        .collect()
}
fn retained(world: &mut World) -> (String, String, Arc<PreviewService>) {
    let state = &mut world.resource_mut::<Files>().script;
    state.editor_open = false;
    let preview = &mut state.preview;
    preview.open = true;
    let retained = Retained::new(preview.service.clone(), frames()).unwrap();
    let result = (
        retained.view.clone(),
        retained.descriptor.preview_id.clone(),
        preview.service.clone(),
    );
    preview.retained = Some(retained);
    result
}
fn png() -> Vec<u8> {
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    crate::native_viewport::screenshot::png_bytes(&Image::new_fill(
        Extent3d {
            width: 2,
            height: 2,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[64, 128, 192, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD,
    ))
    .unwrap()
}

#[test]
fn native_script_preview_uses_catalog_policy_and_never_substitutes_edited_source() {
    let mut world = world();
    let handle = NativeInterfaceHandle::new(|| {});
    select(&mut world, "garden-bench");
    assert!(
        command(&mut world, &handle, Action::Open, &ControlInput::Click)
            .unwrap_err()
            .contains("no miniature preview")
    );
    select(&mut world, "fillet-basics");
    assert_eq!(
        eligible(&world.resource::<Files>().script).unwrap().id,
        "fillet-basics"
    );
    let edited = format!(
        "{}\n// changed draft",
        world.resource::<Files>().script.source
    );
    editor::edit_source(&mut world, &ControlInput::SetValue(edited.clone())).unwrap();
    assert!(
        command(&mut world, &handle, Action::Open, &ControlInput::Click)
            .unwrap_err()
            .contains("original bundled lesson")
    );
    assert_eq!(world.resource::<Files>().script.source, edited);
    assert!(world.resource::<Files>().script.dirty());
    assert!(!world.resource::<Files>().script.preview.building());
    assert!(world.resource::<Files>().lesson.is_none());
}

#[test]
fn native_script_preview_real_preparation_is_isolated_and_close_cannot_spawn_parallel_work() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let before = fixture.engine.engine_call("project_export_model", "");
    let owner = fixture.owner();
    let mut world = world();
    let handle = NativeInterfaceHandle::new(|| {});
    select(&mut world, "fillet-basics");
    let source = world.resource::<Files>().script.source.clone();
    let geometry = prepare(&source).unwrap();
    assert!(geometry.len() >= 2);
    assert!(
        geometry.iter().any(|frame| !frame.scene.bodies.is_empty()),
        "Shared runner must produce real solid geometry"
    );
    let service = Arc::new(PreviewService::default());
    let retained = Retained::new(service, geometry).unwrap();
    assert!(retained
        .descriptor
        .captions
        .iter()
        .all(|caption| !caption.is_empty()));
    drop(retained);
    let (send, receive) = mpsc::channel();
    world.resource_mut::<Files>().script.preview.open = true;
    world.resource_mut::<Files>().script.preview.build = Some(Mutex::new(receive));
    close(&mut world);
    assert!(available(&world).unwrap_err().contains("lesson preview"));
    assert!(command(&mut world, &handle, Action::Open, &ControlInput::Click).is_err());
    send.send(Ok(frames())).unwrap();
    poll(&mut world);
    assert!(!world.resource::<Files>().script.preview.building());
    assert!(world.resource::<Files>().script.preview.retained.is_none());
    assert_eq!(world.resource::<Files>().script.source, source);
    assert_eq!(fixture.owner(), owner);
    assert_eq!(
        fixture.engine.engine_call("project_export_model", ""),
        before
    );
}

#[test]
fn native_script_preview_rejects_stale_pixels_and_releases_images_documents_and_views() {
    let mut world = world();
    let (view, document, service) = retained(&mut world);
    let revision = world.resource::<Files>().script.preview.revision;
    publish(
        &mut world,
        Rendered {
            view: view.clone(),
            revision,
            png: png(),
        },
    )
    .unwrap();
    assert_eq!(world.resource::<Assets<Image>>().len(), 1);
    world
        .resource_mut::<Files>()
        .script
        .preview
        .changed()
        .unwrap();
    publish(
        &mut world,
        Rendered {
            view: view.clone(),
            revision,
            png: vec![],
        },
    )
    .unwrap();
    let revision = world.resource::<Files>().script.preview.revision;
    publish(
        &mut world,
        Rendered {
            view: "retired-view".into(),
            revision,
            png: vec![],
        },
    )
    .unwrap();
    publish(
        &mut world,
        Rendered {
            view: view.clone(),
            revision,
            png: png(),
        },
    )
    .unwrap();
    assert_eq!(
        world.resource::<Assets<Image>>().len(),
        1,
        "Repaint replaces the prior image asset"
    );
    close(&mut world);
    assert_eq!(world.resource::<Assets<Image>>().len(), 0);
    publish(
        &mut world,
        Rendered {
            view: view.clone(),
            revision,
            png: vec![],
        },
    )
    .unwrap();
    let request = RenderRequest {
        preview_id: document,
        view_id: view.clone(),
        revision: revision + 1,
        frame_index: 0,
        width: 300,
        height: 176,
        yaw: HOME.0,
        pitch: HOME.1,
    };
    assert!(service.render(request).unwrap_err().contains("expired"));
    let reopened = Retained::new(service.clone(), frames()).unwrap();
    let request = RenderRequest {
        preview_id: reopened.descriptor.preview_id.clone(),
        view_id: view,
        revision: revision + 1,
        frame_index: 0,
        width: 300,
        height: 176,
        yaw: HOME.0,
        pitch: HOME.1,
    };
    assert!(service.render(request).unwrap_err().contains("closed"));
}

#[test]
fn native_script_preview_keys_and_replay_wait_for_current_frame_and_stop_at_last_frame() {
    let mut world = world();
    retained(&mut world);
    let handle = NativeInterfaceHandle::new(|| {});
    command(
        &mut world,
        &handle,
        Action::Model,
        &ControlInput::Key(limo_cad_interface::KeyChord::plain("ArrowRight")),
    )
    .unwrap();
    assert!((world.resource::<Files>().script.preview.yaw - HOME.0 - 0.15).abs() < 1e-5);
    command(
        &mut world,
        &handle,
        Action::Model,
        &ControlInput::Key(limo_cad_interface::KeyChord::plain("Home")),
    )
    .unwrap();
    assert_eq!(world.resource::<Files>().script.preview.yaw, HOME.0);
    command(&mut world, &handle, Action::Replay, &ControlInput::Click).unwrap();
    let now = Instant::now();
    let preview = &mut world.resource_mut::<Files>().script.preview;
    replay(preview, &handle, now + FRAME_DELAY * 10).unwrap();
    assert_eq!(
        preview.index, 0,
        "Do not skip a frame whose pixels have not arrived"
    );
    assert!(preview.wake.is_none());
    for target in 1..3 {
        preview.rendered = preview.revision;
        let cancelled = Arc::new(AtomicBool::new(false));
        preview.wake = Some(Wake {
            due: now,
            cancelled: cancelled.clone(),
        });
        replay(preview, &handle, now).unwrap();
        assert_eq!(preview.index, target);
        assert!(cancelled.load(Ordering::Acquire));
    }
    assert!(!preview.playing);
    preview.turn(100000., 999.).unwrap();
    assert_eq!(preview.pitch, 1.3);
    assert!(preview.yaw < std::f32::consts::TAU);
    assert!(preview.turn(f32::NAN, 0.).is_err());
}

#[test]
fn native_package_preview_requires_current_source_and_keeps_the_retained_baseline() {
    use limo_cad_project_file::{ProjectArchive, ReplayPackage, SaveMetadata};
    let source =
        json!({"version":2,"name":"Package preview","starting_state":"project","steps":[]})
            .to_string();
    let archive = ProjectArchive::new(
        limo_cad_sketch::SketchManager::new()
            .export_project_model()
            .unwrap(),
        SaveMetadata {
            application_version: "test",
            saved_at: "2026-10-17T00:00:00Z",
        },
    )
    .unwrap()
    .encode()
    .unwrap();
    let package = Arc::new(ReplayPackage::new(source.clone(), archive.clone()).unwrap());
    let identity = package.source_identity(&source).unwrap();
    let artifact = Artifact::Project {
        package: Arc::clone(&package),
        opened: identity.clone(),
        saved: identity,
    };
    let mut world = world();
    world
        .resource_mut::<Files>()
        .script
        .accept(artifact.inspect(None, &source).unwrap())
        .unwrap();
    assert!(offered(&world.resource::<Files>().script));
    assert!(can_open(&world.resource::<Files>().script));
    let selected = job::source(&world.resource::<Files>().script).unwrap();
    world
        .resource_mut::<Files>()
        .script
        .source
        .push_str("\n// newer draft");
    assert!(!can_open(&world.resource::<Files>().script));
    let frames = selected
        .prepare(crate::app_preferences::Locale::En, || Ok(()))
        .unwrap();
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].caption, "Starting project");
    assert!(frames[0].assembly.is_some());
    assert_eq!(package.project_bytes(), archive);
    assert!(world
        .resource::<Files>()
        .script
        .source
        .ends_with("// newer draft"));
    assert!(world.resource::<Files>().lesson.is_none());
}

#[test]
fn native_preview_close_cancels_preparation_without_releasing_its_worker_slot() {
    let mut world = world();
    let preparation = job::Preparation::new();
    let cancelled = Arc::clone(&preparation.cancelled);
    let deadline = preparation.deadline;
    let (send, receive) = mpsc::channel();
    let preview = &mut world.resource_mut::<Files>().script.preview;
    preview.open = true;
    preview.build = Some(Mutex::new(receive));
    preview.preparation = Some(preparation);
    close(&mut world);
    assert!(job::check(&cancelled, deadline)
        .unwrap_err()
        .contains("cancelled"));
    assert!(world.resource::<Files>().script.preview.building());
    send.send(Ok(frames())).unwrap();
    poll(&mut world);
    assert!(!world.resource::<Files>().script.preview.building());
    assert!(world.resource::<Files>().script.preview.retained.is_none());
    assert!(job::check(
        &AtomicBool::new(false),
        Instant::now() - Duration::from_secs(1)
    )
    .unwrap_err()
    .contains("timed out"));
}

#[test]
fn native_preview_notice_count_follows_frame_without_collapsing_reserved_space() {
    let mut world = world();
    retained(&mut world);
    let preview = &mut world.resource_mut::<Files>().script.preview;
    preview
        .retained
        .as_mut()
        .unwrap()
        .descriptor
        .assembly_notice_counts = vec![0, 2, 0];
    assert_eq!(preview.assembly_notices(), (0, true));
    preview.index = 1;
    assert_eq!(preview.assembly_notices(), (2, true));
    preview.index = 2;
    assert_eq!(preview.assembly_notices(), (0, true));
    close(&mut world);
    assert_eq!(
        world.resource::<Files>().script.preview.assembly_notices(),
        (0, false)
    );
}
