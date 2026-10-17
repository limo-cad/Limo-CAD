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
            caption: format!("Frame {index}"),
            scene: serde_json::from_value(json!({"bodies":[],"errors":[]})).unwrap(),
            ..default()
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
fn native_script_preview_cut_choices_share_opening_frames_and_fence_stale_results() {
    let mut world = world();
    let handle = NativeInterfaceHandle::new(|| {});
    retained(&mut world);
    {
        let state = &mut world.resource_mut::<Files>().script.preview;
        let mut choices = frames();
        choices.extend(frames());
        state.retained = Some(Retained::new(state.service.clone(), choices).unwrap());
        state.lesson = Some(lesson::Id::Cut);
    }
    let mut through_keys = vec![];
    for scenario in lesson::CutScenario::ALL {
        command(
            &mut world,
            &handle,
            Action::Scenario(scenario),
            &ControlInput::Click,
        )
        .unwrap();
        assert_eq!(world.resource::<Files>().script.preview.count(), 4);
        for index in 0..4 {
            let (view, revision, key) = {
                let state = &mut world.resource_mut::<Files>().script.preview;
                state.index = index;
                state.changed().unwrap();
                (
                    state.retained.as_ref().unwrap().view.clone(),
                    state.revision,
                    images::Key::current(state),
                )
            };
            if scenario == lesson::CutScenario::ThroughAll {
                through_keys.push(key);
            } else {
                assert_eq!(key == through_keys[index], index < 2);
            }
            publish(
                &mut world,
                Rendered {
                    view,
                    revision,
                    png: png(),
                },
            )
            .unwrap();
        }
    }
    assert_eq!(
        world
            .resource::<Files>()
            .script
            .preview
            .images
            .footprint()
            .0,
        6
    );
    let (view, revision) = {
        let state = &world.resource::<Files>().script.preview;
        (
            state.retained.as_ref().unwrap().view.clone(),
            state.revision,
        )
    };
    command(
        &mut world,
        &handle,
        Action::Scenario(lesson::CutScenario::ThroughAll),
        &ControlInput::Click,
    )
    .unwrap();
    publish(
        &mut world,
        Rendered {
            view,
            revision,
            png: vec![],
        },
    )
    .unwrap();
    assert!(world
        .resource::<Files>()
        .script
        .preview
        .image_index
        .is_none());
    for _ in 0..20 {
        for scenario in lesson::CutScenario::ALL {
            command(
                &mut world,
                &handle,
                Action::Scenario(scenario),
                &ControlInput::Click,
            )
            .unwrap();
            let revision = world.resource::<Files>().script.preview.revision;
            command(
                &mut world,
                &handle,
                Action::Scenario(scenario),
                &ControlInput::Click,
            )
            .unwrap();
            assert_eq!(world.resource::<Files>().script.preview.revision, revision);
            command(&mut world, &handle, Action::Help, &ControlInput::Click).unwrap();
            assert!(command(
                &mut world,
                &handle,
                Action::Scenario(scenario),
                &ControlInput::Click
            )
            .is_err());
            command(&mut world, &handle, Action::Demo, &ControlInput::Click).unwrap();
            assert_eq!(world.resource::<Files>().script.preview.scenario, scenario);
            command(&mut world, &handle, Action::Replay, &ControlInput::Click).unwrap();
            for index in 0..4 {
                if index > 0 {
                    command(&mut world, &handle, Action::Next, &ControlInput::Click).unwrap();
                }
                render(&mut world, &handle).unwrap();
            }
        }
    }
    let state = &world.resource::<Files>().script.preview;
    assert_eq!(state.render_jobs, 0);
    assert!(state.build.is_none());
    close(&mut world);
    assert!(command(
        &mut world,
        &handle,
        Action::Scenario(lesson::CutScenario::ThroughAll),
        &ControlInput::Click
    )
    .is_err());
}

#[test]
fn native_script_preview_cached_frames_skip_gpu_work_and_fence_late_errors() {
    let mut world = world();
    let (view, _, _) = retained(&mut world);
    let handle = NativeInterfaceHandle::new(|| {});
    let revision = world.resource::<Files>().script.preview.revision;
    publish(
        &mut world,
        Rendered {
            view,
            revision,
            png: png(),
        },
    )
    .unwrap();
    let original = world.resource::<Files>().script.preview.image.clone();
    let (send, receive) = mpsc::channel();
    {
        let preview = &mut world.resource_mut::<Files>().script.preview;
        preview.render = Some(Mutex::new(receive));
        preview.render_revision = revision;
        preview.changed().unwrap();
    }
    render(&mut world, &handle).unwrap();
    assert_eq!(world.resource::<Files>().script.preview.image, original);
    assert_eq!(world.resource::<Files>().script.preview.render_jobs, 0);
    send.send(Err("retired render failure".into())).unwrap();
    poll(&mut world);
    let preview = &mut world.resource_mut::<Files>().script.preview;
    assert!(preview.error.is_none());
    assert_eq!(preview.rendered, preview.revision);
    preview.turn(HOME.0 + 0.1, HOME.1).unwrap();
    assert!(preview.images.get(images::Key::current(preview)).is_none());
    preview.turn(HOME.0, HOME.1).unwrap();
    preview.size[0] += 1;
    assert!(preview.images.get(images::Key::current(preview)).is_none());
    preview.size[0] -= 1;
    preview.palette.body[0] += 0.05;
    assert!(preview.images.get(images::Key::current(preview)).is_none());
    close(&mut world);
    assert!(world.resource::<Assets<Image>>().is_empty());
    assert_eq!(
        world.resource::<Files>().script.preview.images.footprint(),
        (0, 0)
    );
}

#[test]
fn native_script_preview_closed_and_paused_poll_do_not_mutate_or_wake() {
    use std::sync::atomic::AtomicUsize;
    let mut world = world();
    let wakes = Arc::new(AtomicUsize::new(0));
    let counted = wakes.clone();
    world.insert_resource(NativeInterfaceHandle::new(move || {
        counted.fetch_add(1, Ordering::Relaxed);
    }));
    for open in [false, true] {
        {
            let preview = &mut world.resource_mut::<Files>().script.preview;
            preview.open = open;
            preview.rendered = preview.revision;
        }
        world.clear_trackers();
        for _ in 0..10_000 {
            poll(&mut world);
        }
        assert!(!world.get_resource_ref::<Files>().unwrap().is_changed());
        assert_eq!(wakes.load(Ordering::Relaxed), 0);
        assert_eq!(world.resource::<Files>().script.preview.render_jobs, 0);
    }
}

#[test]
fn native_script_preview_high_dpi_layout_respects_the_bounded_renderer() {
    let mut world = world();
    world.init_resource::<ViewportUiAssets>();
    let camera = world.spawn_empty().id();
    let mut widgets = super::super::super::super::chrome::Widgets::default();
    let theme = crate::native_viewport::ui::theme(&world);
    for scale in [1., 2., 3.5] {
        world.insert_resource(bevy::ui::UiScale(scale));
        widgets.begin();
        panel::paint(&mut world, camera, &mut widgets, 432., 600., theme).unwrap();
        widgets.finish(&mut world);
        let size = world.resource::<Files>().script.preview.size;
        assert!((64..=MAX_RENDER_SIZE[0]).contains(&size[0]));
        assert!((64..=MAX_RENDER_SIZE[1]).contains(&size[1]));
    }
}

#[test]
fn native_script_preview_all_connected_stages_retain_independent_profiles() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let source = &catalog::examples()
        .iter()
        .find(|example| example.id == lesson::RECIPE)
        .unwrap()
        .source;
    for id in lesson::ALL {
        let frames = prepare_lesson(source, Some(id)).unwrap();
        assert_eq!(
            frames.len(),
            match id {
                lesson::Id::Cut => 6,
                lesson::Id::Fillet => 4,
                _ => 5,
            }
        );
        for frame in &frames {
            if let Some(profile) = &frame.profile {
                assert!(
                    profile.projected_edges.is_empty() && profile.reference_midpoints.is_empty()
                );
            }
        }
        let retained = Retained::new(Arc::new(PreviewService::default()), frames).unwrap();
        assert!(!retained.descriptor.captions.is_empty());
    }
}

#[test]
fn native_script_preview_help_entry_is_localized_dormant_and_restores_the_draft() {
    let mut world = world();
    world.init_resource::<ViewportUiAssets>();
    world.resource_mut::<Files>().scripts = false;
    world.resource_mut::<Files>().script.editor_open = true;
    let handle = NativeInterfaceHandle::new(|| {});
    command(&mut world, &handle, Action::Index, &ControlInput::Click).unwrap();
    let camera = world.spawn_empty().id();
    let mut widgets = super::super::super::super::chrome::Widgets::default();
    let theme = crate::native_viewport::ui::theme(&world);
    for locale in crate::app_preferences::locale::SUPPORTED {
        crate::native_viewport::localization::set_locale(&mut world, locale);
        widgets.begin();
        panel::paint(&mut world, camera, &mut widgets, 700., 440., theme).unwrap();
        widgets.finish(&mut world);
        for id in lesson::ALL {
            assert!(widgets.entity(&format!("help-{}", id.key())).is_some());
        }
        for id in lesson::ALL {
            command(&mut world, &handle, Action::Read(id), &ControlInput::Click).unwrap();
            widgets.begin();
            panel::paint(&mut world, camera, &mut widgets, 700., 440., theme).unwrap();
            widgets.finish(&mut world);
            let text = widgets.entity("extrude-help-text").unwrap();
            assert_eq!(
                world.get::<Text>(text).unwrap().0,
                feature::help(id, lesson::language(&world))
            );
            let preview = &world.resource::<Files>().script.preview;
            assert!(
                preview.build.is_none() && preview.render.is_none() && preview.retained.is_none()
            );
            assert_eq!(preview.render_jobs, 0);
        }
        command(&mut world, &handle, Action::Index, &ControlInput::Click).unwrap();
    }
    command(&mut world, &handle, Action::Close, &ControlInput::Click).unwrap();
    let files = world.resource::<Files>();
    assert!(!files.scripts && files.script.editor_open);
}

#[test]
fn native_script_preview_cached_replay_arms_every_frame_without_gpu_completion_wakes() {
    let mut world = world();
    let (view, _, _) = retained(&mut world);
    let handle = NativeInterfaceHandle::new(|| {});
    world.insert_resource(handle.clone());
    for index in 0..3 {
        let preview = &mut world.resource_mut::<Files>().script.preview;
        preview.index = index;
        preview.changed().unwrap();
        let revision = preview.revision;
        publish(
            &mut world,
            Rendered {
                view: view.clone(),
                revision,
                png: png(),
            },
        )
        .unwrap();
    }
    command(&mut world, &handle, Action::Replay, &ControlInput::Click).unwrap();
    poll(&mut world);
    for index in 0..2 {
        let preview = &mut world.resource_mut::<Files>().script.preview;
        assert_eq!(preview.index, index);
        preview
            .wake
            .as_mut()
            .expect("Cached frame needs its own next wake")
            .due = Instant::now();
        poll(&mut world);
    }
    let preview = &world.resource::<Files>().script.preview;
    assert_eq!(preview.index, 2);
    assert_eq!(preview.render_jobs, 0);
    assert!(!preview.playing);
    assert!(preview.wake.is_none());
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
fn native_script_preview_extrude_preserves_dirty_source_and_help_does_not_advance_geometry() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let before = fixture.engine.engine_call("project_export_model", "");
    let mut world = world();
    select(&mut world, "fillet-basics");
    editor::edit_source(
        &mut world,
        &ControlInput::SetValue("unfinished { draft".into()),
    )
    .unwrap();
    world.resource_mut::<Files>().script.editor_open = true;
    world.resource_mut::<Files>().scripts = false;
    let handle = NativeInterfaceHandle::new(|| {});
    command(&mut world, &handle, Action::Extrude, &ControlInput::Click).unwrap();
    assert!(
        !world.resource::<Files>().scripts,
        "A contextual demo must not select the Scripts workspace"
    );
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
    assert_eq!(frames.len(), 5);
    let preview = &mut world.resource_mut::<Files>().script.preview;
    preview.retained = Some(Retained::new(preview.service.clone(), frames).unwrap());
    assert!(preview.playing);
    command(&mut world, &handle, Action::Help, &ControlInput::Click).unwrap();
    assert!(command(&mut world, &handle, Action::Next, &ControlInput::Click).is_err());
    let preview = &world.resource::<Files>().script.preview;
    assert_eq!(preview.index, 0);
    assert!(!preview.playing);
    command(&mut world, &handle, Action::Demo, &ControlInput::Click).unwrap();
    command(&mut world, &handle, Action::Close, &ControlInput::Click).unwrap();
    let files = world.resource::<Files>();
    assert!(!files.scripts && files.script.editor_open && files.script.dirty());
    assert_eq!(files.script.source, "unfinished { draft");
    assert_eq!(files.script.example.unwrap().id, "fillet-basics");
    assert!(files.script.preview.retained.is_none());
    assert_eq!(
        fixture.engine.engine_call("project_export_model", ""),
        before
    );
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
        palette: None,
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
        palette: None,
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
