//! Controller/asset contracts; these fixtures do not establish GPU pixel proof.
use super::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use std::sync::atomic::AtomicUsize;

fn fixture() -> (World, NativeInterfaceHandle, Arc<AtomicUsize>) {
    let mut world = World::new();
    initialize(
        &mut world,
        Arc::new(Mutex::new(DocumentWorkspace::default())),
    );
    world.init_resource::<Assets<Image>>();
    let wakes = Arc::new(AtomicUsize::new(0));
    let counted = wakes.clone();
    let handle = NativeInterfaceHandle::new(move || {
        counted.fetch_add(1, Ordering::Relaxed);
    });
    world.insert_resource(handle.clone());
    let files = &mut world.resource_mut::<Files>();
    files.scripts = true;
    files.script.editor_open = false;
    let generation = files.script.generation;
    let preview = &mut files.script.preview;
    preview.open = true;
    preview.generation = generation;
    preview.lesson = Some(lesson::Id::Fillet);
    preview.retained = Some(
        Retained::new(
            preview.service.clone(),
            (0..3)
                .map(|index| Frame {
                    caption: format!("Frame {index}"),
                    ..default()
                })
                .collect(),
        )
        .unwrap(),
    );
    (world, handle, wakes)
}

fn image() -> images::FrameImage {
    images::FrameImage::Fixture(Image::new_fill(
        Extent3d {
            width: 512,
            height: 512,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[64, 128, 192, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD,
    ))
}

fn publish_current(world: &mut World) {
    let preview = &world.resource::<Files>().script.preview;
    let view = preview.retained.as_ref().unwrap().view.clone();
    let revision = preview.revision;
    publish(
        world,
        Rendered {
            view,
            revision,
            image: image(),
        },
    )
    .unwrap();
    world.resource_mut::<Files>().script.preview.attempted = revision;
}

fn begin_static(world: &mut World) -> Handle<Image> {
    publish_current(world);
    let preview = &mut world.resource_mut::<Files>().script.preview;
    let source = preview.image.clone().unwrap();
    preview.index = 1;
    preview.playing = true;
    preview.changed().unwrap();
    preview.begin_animation();
    assert!(preview.geometry_animation_complete());
    assert!(!preview.animation_complete());
    assert_eq!(
        preview.display_index(),
        0,
        "Keep source caption while target is pending"
    );
    source
}

#[test]
fn native_preview_pause_finishes_dissolve_without_changing_render_key_revision_or_jobs() {
    let (mut world, handle, wakes) = fixture();
    let source = begin_static(&mut world);
    publish_current(&mut world);
    let now = Instant::now();
    let (key, revision, jobs) = {
        let preview = &mut world.resource_mut::<Files>().script.preview;
        preview.render_started = Some(now - Duration::from_secs(30));
        arm_replay(preview, now);
        assert_eq!(preview.wake.as_ref().unwrap().due, now + ANIMATION_INTERVAL);
        (
            images::Key::current(preview),
            preview.revision,
            preview.render_jobs,
        )
    };
    for tick in 1..=2 {
        let preview = &mut world.resource_mut::<Files>().script.preview;
        replay(preview, &handle, now + ANIMATION_INTERVAL * tick).unwrap();
        assert!(preview.wake.is_none(), "Consume the opacity deadline");
        assert!(preview.blend.as_ref().unwrap().alpha() > 0.);
        arm_replay(preview, now + ANIMATION_INTERVAL * tick);
    }
    command(&mut world, &handle, Action::Stop, &ControlInput::Click).unwrap();
    {
        let preview = &world.resource::<Files>().script.preview;
        assert!(preview.blend.is_none());
        assert!(preview.animation_complete());
        assert_eq!(preview.display_index(), 1);
        assert_eq!(preview.display_frame_index(), 1);
    }
    let count = wakes.load(Ordering::Relaxed);
    for _ in 0..100 {
        replay(
            &mut world.resource_mut::<Files>().script.preview,
            &handle,
            now + Duration::from_secs(90),
        )
        .unwrap();
        poll(&mut world);
    }
    assert_eq!(wakes.load(Ordering::Relaxed), count);
    command(&mut world, &handle, Action::Resume, &ControlInput::Click).unwrap();
    let preview = &mut world.resource_mut::<Files>().script.preview;
    let resumed = now + Duration::from_secs(120);
    arm_replay(preview, resumed);
    assert_eq!(preview.wake.as_ref().unwrap().due, resumed + FRAME_DELAY);
    replay(preview, &handle, resumed + ANIMATION_INTERVAL * 4).unwrap();
    assert_eq!(
        preview.index, 1,
        "Completed target keeps its readable dwell on Resume"
    );
    assert!(preview.animation_complete());
    assert!(images::Key::current(preview) == key);
    assert_eq!(
        (
            preview.revision,
            preview.rendered,
            preview.attempted,
            preview.render_jobs
        ),
        (revision, revision, revision, jobs)
    );
    assert_eq!(preview.images.footprint(), (2, 2 * 1024 * 1024));
    assert!(world.resource::<Assets<Image>>().contains(source.id()));
}

#[test]
fn native_preview_click_without_drag_finishes_dissolve_and_a_real_drag_invalidates_camera() {
    use bevy::input::ButtonState;
    use interface_shell::{PointerButton, PointerPhase};
    for pending in [false, true] {
        let (mut app, handle, model, _) = interface_shell::tests::fixture();
        initialize(
            app.world_mut(),
            Arc::new(Mutex::new(DocumentWorkspace::default())),
        );
        app.world_mut().init_resource::<Assets<Image>>();
        {
            let files = &mut app.world_mut().resource_mut::<Files>();
            files.scripts = true;
            let preview = &mut files.script.preview;
            preview.open = true;
            preview.lesson = Some(lesson::Id::CircularPattern);
            preview.retained = Some(
                Retained::new(
                    preview.service.clone(),
                    (0..3)
                        .map(|index| Frame {
                            caption: format!("Frame {index}"),
                            ..default()
                        })
                        .collect(),
                )
                .unwrap(),
            );
        }
        bind_command(
            app.world_mut(),
            model,
            NativeCommand::File(FileCommand::ScriptPreview(Action::Model)),
        )
        .unwrap();
        app.update();
        begin_static(app.world_mut());
        let receipt = if pending {
            let (send, receive) = mpsc::channel();
            let preview = &mut app.world_mut().resource_mut::<Files>().script.preview;
            preview.render = Some(InFlight::Fixture(Mutex::new(receive)));
            preview.attempted = preview.revision;
            Some(send)
        } else {
            publish_current(app.world_mut());
            let now = Instant::now();
            let preview = &mut app.world_mut().resource_mut::<Files>().script.preview;
            arm_replay(preview, now);
            replay(preview, &handle, now + ANIMATION_INTERVAL).unwrap();
            arm_replay(preview, now + ANIMATION_INTERVAL);
            None
        };
        let bounds = handle
            .read_surface(|_, surface| {
                surface
                    .controls
                    .iter()
                    .find(|c| c.key.0 == model.to_bits())
                    .unwrap()
                    .bounds
            })
            .unwrap();
        let point = Vec2::new(
            (bounds.x + bounds.width / 2.) as f32,
            (bounds.y + bounds.height / 2.) as f32,
        );
        let owner = handle.presented_context().unwrap();
        let event = |state, point| NativeHostInput {
            ui_scale: 1.,
            event: WindowEvent::MouseButtonInput(bevy::input::mouse::MouseButtonInput {
                window: Entity::PLACEHOLDER,
                button: MouseButton::Left,
                state,
            }),
            context: Some(owner.clone()),
            cursor: Some(point),
            consumed: false,
            modifiers: default(),
            actions: vec![],
        };
        let before = {
            let p = &app.world().resource::<Files>().script.preview;
            assert!(p.blend.is_some());
            (
                images::Key::current(p),
                p.revision,
                p.render_jobs,
                p.yaw,
                p.pitch,
            )
        };
        assert!(!input::input(
            app.world_mut(),
            &handle,
            &event(ButtonState::Pressed, point)
        )
        .unwrap());
        assert!(handle
            .pointer(
                PointerPhase::Down,
                point.as_dvec2().to_array(),
                PointerButton::Primary
            )
            .unwrap());
        let preview = &app.world().resource::<Files>().script.preview;
        assert!(
            !preview.playing
                && preview.blend.is_none()
                && preview.wake.is_none()
                && preview.render_started.is_none()
        );
        assert!(images::Key::current(preview) == before.0);
        assert_eq!(
            (
                preview.revision,
                preview.render_jobs,
                preview.yaw,
                preview.pitch
            ),
            (before.1, before.2, before.3, before.4)
        );
        assert!(!input::input(
            app.world_mut(),
            &handle,
            &event(ButtonState::Released, point)
        )
        .unwrap());
        assert!(handle
            .pointer(
                PointerPhase::Up,
                point.as_dvec2().to_array(),
                PointerButton::Primary
            )
            .unwrap());
        assert!(app
            .world()
            .resource::<Files>()
            .script
            .preview
            .drag
            .is_none());
        for action in handle.take_actions().unwrap() {
            assert_eq!(
                action.control.key,
                limo_cad_interface::ControlKey(model.to_bits())
            );
            command(
                app.world_mut(),
                &handle,
                Action::Model,
                &action.control.input,
            )
            .unwrap();
        }
        if let Some(receipt) = receipt {
            let p = &app.world().resource::<Files>().script.preview;
            assert!(
                p.render.is_some(),
                "A click retains the pending target receipt"
            );
            receipt
                .send(Ok(Rendered {
                    view: p.retained.as_ref().unwrap().view.clone(),
                    revision: p.revision,
                    image: image(),
                }))
                .unwrap();
            poll(app.world_mut());
        }
        let preview = &app.world().resource::<Files>().script.preview;
        assert!(preview.image_ready() && preview.blend.is_none() && preview.wake.is_none());
        assert_eq!(preview.display_index(), 1);
        assert_eq!(
            (preview.revision, preview.render_jobs),
            (before.1, before.2)
        );
        input::input(
            app.world_mut(),
            &handle,
            &event(ButtonState::Pressed, point),
        )
        .unwrap();
        let moved = point + Vec2::new(20., 10.);
        let motion = NativeHostInput {
            event: WindowEvent::CursorMoved(bevy::window::CursorMoved {
                window: Entity::PLACEHOLDER,
                position: moved,
                delta: None,
            }),
            cursor: Some(moved),
            ..event(ButtonState::Pressed, point)
        };
        assert!(input::input(app.world_mut(), &handle, &motion).unwrap());
        let preview = &app.world().resource::<Files>().script.preview;
        assert!(preview.revision > before.1 && preview.blend.is_none());
        assert!(images::Key::current(preview) != before.0);
    }
}

#[test]
fn native_preview_cached_static_target_blends_without_connecting_or_scheduling_gpu_work() {
    let (mut world, handle, _) = fixture();
    publish_current(&mut world);
    command(&mut world, &handle, Action::Step(1), &ControlInput::Click).unwrap();
    publish_current(&mut world);
    command(&mut world, &handle, Action::Step(0), &ControlInput::Click).unwrap();
    render(&mut world, &handle).unwrap();
    let preview = &mut world.resource_mut::<Files>().script.preview;
    preview.index = 1;
    preview.playing = true;
    preview.changed().unwrap();
    preview.begin_animation();
    assert!(preview.blend.is_some());
    render(&mut world, &handle).unwrap();
    let preview = &world.resource::<Files>().script.preview;
    assert!(preview.image_ready() && preview.blend.is_some());
    assert_eq!(preview.render_jobs, 0);
    assert!(preview.render.is_none());
    assert_eq!(world.resource::<Assets<Image>>().len(), 2);
}

#[test]
fn native_preview_pause_read_controls_finish_zero_alpha_blends_and_keep_pending_receipts() {
    for action in [Action::Stop, Action::Help, Action::Demo, Action::Steps] {
        for pending in [false, true] {
            let (mut world, handle, _) = fixture();
            let source = begin_static(&mut world);
            let receipt = if pending {
                let (send, receive) = mpsc::channel();
                let preview = &mut world.resource_mut::<Files>().script.preview;
                preview.render = Some(InFlight::Fixture(Mutex::new(receive)));
                preview.render_revision = preview.revision;
                preview.attempted = preview.revision;
                Some(send)
            } else {
                publish_current(&mut world);
                arm_replay(
                    &mut world.resource_mut::<Files>().script.preview,
                    Instant::now(),
                );
                None
            };
            let preview = &world.resource::<Files>().script.preview;
            assert_eq!(preview.blend.as_ref().unwrap().alpha(), 0.);
            let (revision, rendered, attempted, jobs, key) = (
                preview.revision,
                preview.rendered,
                preview.attempted,
                preview.render_jobs,
                images::Key::current(preview),
            );
            command(&mut world, &handle, action, &ControlInput::Click).unwrap();
            let preview = &world.resource::<Files>().script.preview;
            assert!(preview.blend.is_none() && preview.wake.is_none() && !preview.playing);
            assert_eq!(
                (
                    preview.revision,
                    preview.rendered,
                    preview.attempted,
                    preview.render_jobs
                ),
                (revision, rendered, attempted, jobs)
            );
            assert!(images::Key::current(preview) == key);
            if let Some(receipt) = receipt {
                assert!(
                    preview.render.is_some(),
                    "Pause/read must retain the pending target receipt"
                );
                assert_eq!(
                    preview.display_index(),
                    0,
                    "Pending target keeps its source caption"
                );
                assert_eq!(preview.image.as_ref(), Some(&source));
                let view = preview.retained.as_ref().unwrap().view.clone();
                receipt
                    .send(Ok(Rendered {
                        view,
                        revision,
                        image: image(),
                    }))
                    .unwrap();
                poll(&mut world);
            }
            let preview = &world.resource::<Files>().script.preview;
            assert!(preview.image_ready() && preview.animation_complete());
            assert!(preview.blend.is_none() && preview.render.is_none() && preview.wake.is_none());
            assert_eq!(preview.display_index(), 1);
            assert_eq!(preview.display_frame_index(), 1);
            assert_ne!(
                preview.image.as_ref(),
                Some(&source),
                "Published target must match the current caption"
            );
            assert_eq!((preview.revision, preview.render_jobs), (revision, jobs));
            assert!(images::Key::current(preview) == key);
        }
    }
}

#[test]
fn native_preview_paused_terminal_target_exposes_replay_and_cut_alternative() {
    for id in [lesson::Id::Fillet, lesson::Id::Cut] {
        let (mut world, handle, _) = fixture();
        if id == lesson::Id::Cut {
            let preview = &mut world.resource_mut::<Files>().script.preview;
            preview.retained = Some(
                Retained::new(
                    preview.service.clone(),
                    (0..6)
                        .map(|index| Frame {
                            caption: format!("Frame {index}"),
                            ..default()
                        })
                        .collect(),
                )
                .unwrap(),
            );
        }
        world.resource_mut::<Files>().script.preview.lesson = Some(id);
        world.init_resource::<ViewportUiAssets>();
        let camera = world.spawn_empty().id();
        let theme = crate::native_viewport::ui::theme(&world);
        let mut widgets = super::super::super::super::chrome::Widgets::default();
        widgets.begin();
        panel::paint(&mut world, camera, &mut widgets, 432., 600., theme).unwrap();
        widgets.finish(&mut world);
        publish_current(&mut world);
        let source = world
            .resource::<Files>()
            .script
            .preview
            .image
            .clone()
            .unwrap();
        {
            let preview = &mut world.resource_mut::<Files>().script.preview;
            preview.index = preview.count() - 1;
            preview.playing = true;
            preview.changed().unwrap();
            preview.begin_animation();
        }
        publish_current(&mut world);
        widgets.begin();
        panel::paint(&mut world, camera, &mut widgets, 432., 600., theme).unwrap();
        widgets.finish(&mut world);
        let model = widgets.entity("scripts-preview-model").unwrap();
        assert_eq!(world.get::<ImageNode>(model).unwrap().image, source);
        let preview = &world.resource::<Files>().script.preview;
        let (revision, jobs, key, target, caption) = (
            preview.revision,
            preview.render_jobs,
            images::Key::current(preview),
            preview.image.clone().unwrap(),
            preview.retained.as_ref().unwrap().descriptor.captions[preview.frame_index()].clone(),
        );
        command(&mut world, &handle, Action::Stop, &ControlInput::Click).unwrap();
        widgets.begin();
        panel::paint(&mut world, camera, &mut widgets, 432., 600., theme).unwrap();
        widgets.finish(&mut world);
        assert_eq!(world.get::<ImageNode>(model).unwrap().image, target);
        assert!(widgets.entity("scripts-preview-blend").is_none());
        let primary = widgets.entity("scripts-preview-primary").unwrap();
        assert_eq!(
            world.get::<NativeCommandBinding>(primary).unwrap().command,
            NativeCommand::File(FileCommand::ScriptPreview(Action::Replay))
        );
        let text = widgets.entity("scripts-preview-caption").unwrap();
        assert_eq!(world.get::<Text>(text).unwrap().0, caption);
        assert_eq!(
            widgets.entity("cut-scenario-alternative").is_some(),
            id == lesson::Id::Cut
        );
        let preview = &world.resource::<Files>().script.preview;
        assert!(preview.animation_complete() && preview.image_ready());
        assert_eq!((preview.revision, preview.render_jobs), (revision, jobs));
        assert!(images::Key::current(preview) == key);
    }
}

#[test]
fn native_preview_navigation_view_changes_and_owner_retirement_fence_blend_handles() {
    for action in [
        Action::Step(2),
        Action::Next,
        Action::Previous,
        Action::Fit,
        Action::Left,
        Action::Close,
    ] {
        let (mut world, handle, _) = fixture();
        begin_static(&mut world);
        publish_current(&mut world);
        command(&mut world, &handle, action, &ControlInput::Click).unwrap();
        let preview = &world.resource::<Files>().script.preview;
        assert!(preview.blend.is_none() && preview.wake.is_none());
        assert!(!preview.playing);
    }
    for camera in [false, true] {
        let (mut world, _, _) = fixture();
        begin_static(&mut world);
        let preview = &mut world.resource_mut::<Files>().script.preview;
        if camera {
            preview.yaw += 0.1;
        } else {
            preview.size[0] += 1;
        }
        preview.changed().unwrap();
        preview.begin_animation();
        assert!(
            preview.blend.is_none(),
            "Incompatible images cannot be blended"
        );
    }
    let (mut world, _, _) = fixture();
    begin_static(&mut world);
    world.resource_mut::<Files>().script.generation += 1;
    poll(&mut world);
    assert!(!world.resource::<Files>().script.preview.open);
    assert!(world.resource::<Files>().script.preview.blend.is_none());
    assert!(world.resource::<Assets<Image>>().is_empty());
}

#[test]
fn native_preview_blend_layer_preserves_model_input_and_retires_on_completion_and_resize() {
    let (mut world, _, _) = fixture();
    world.init_resource::<ViewportUiAssets>();
    let camera = world.spawn_empty().id();
    let theme = crate::native_viewport::ui::theme(&world);
    let mut widgets = super::super::super::super::chrome::Widgets::default();
    widgets.begin();
    panel::paint(&mut world, camera, &mut widgets, 432., 600., theme).unwrap();
    widgets.finish(&mut world);
    // Painting warms shared UI icons; Close owns only the preview textures.
    let shared_images: std::collections::HashSet<_> =
        world.resource::<Assets<Image>>().ids().collect();
    assert!(!shared_images.is_empty());
    let source = begin_static(&mut world);
    publish_current(&mut world);
    let target = world
        .resource::<Files>()
        .script
        .preview
        .image
        .clone()
        .unwrap();
    widgets.begin();
    panel::paint(&mut world, camera, &mut widgets, 432., 600., theme).unwrap();
    widgets.finish(&mut world);
    let model = widgets.entity("scripts-preview-model").unwrap();
    let overlay = widgets.entity("scripts-preview-blend").unwrap();
    assert_eq!(
        world.get::<Node>(overlay).unwrap().border_radius,
        BorderRadius::all(px(8.))
    );
    assert_eq!(
        world.get::<Node>(overlay).unwrap().border_radius,
        world.get::<Node>(model).unwrap().border_radius
    );
    assert_eq!(world.get::<ImageNode>(model).unwrap().image, source);
    assert_eq!(world.get::<ChildOf>(overlay).unwrap().parent(), model);
    assert!(world.get::<NativeCommandBinding>(overlay).is_none());
    assert!(world.get::<InterfaceControl>(overlay).is_none());
    assert!(world
        .get::<interface_shell::InterfacePointerPassthrough>(overlay)
        .is_some());
    assert!(world
        .get::<interface_shell::InterfaceOccluder>(overlay)
        .is_none());
    assert_eq!(
        world.get::<BackgroundColor>(overlay),
        Some(&BackgroundColor(Color::NONE))
    );
    assert!(matches!(
        world.get::<NativeCommandBinding>(model).unwrap().command,
        NativeCommand::File(FileCommand::ScriptPreview(Action::Model))
    ));
    world.resource_mut::<Files>().script.preview.blend = None;
    widgets.begin();
    panel::paint(&mut world, camera, &mut widgets, 432., 600., theme).unwrap();
    widgets.finish(&mut world);
    assert!(world.get_entity(overlay).is_err());
    assert_eq!(
        world.get::<ImageNode>(model).unwrap().image,
        world
            .resource::<Files>()
            .script
            .preview
            .image
            .clone()
            .unwrap()
    );
    widgets.begin();
    panel::paint(&mut world, camera, &mut widgets, 432., 400., theme).unwrap();
    widgets.finish(&mut world);
    assert!(world.resource::<Files>().script.preview.blend.is_none());
    close(&mut world);
    widgets.begin();
    widgets.finish(&mut world);
    assert!(world.get_entity(model).is_err());
    let images = world.resource::<Assets<Image>>();
    assert!(!images.contains(source.id()));
    assert!(!images.contains(target.id()));
    assert_eq!(
        images.ids().collect::<std::collections::HashSet<_>>(),
        shared_images
    );
    let preview = &world.resource::<Files>().script.preview;
    assert_eq!(preview.images.footprint(), (0, 0));
    assert!(preview.image.is_none() && preview.blend.is_none() && preview.wake.is_none());
}

#[test]
fn native_preview_layout_and_palette_replacement_remove_the_old_opacity_layer() {
    for resize in [true, false] {
        let (mut world, _, _) = fixture();
        world.init_resource::<ViewportUiAssets>();
        let camera = world.spawn_empty().id();
        let theme = crate::native_viewport::ui::theme(&world);
        let mut widgets = super::super::super::super::chrome::Widgets::default();
        widgets.begin();
        panel::paint(&mut world, camera, &mut widgets, 432., 600., theme).unwrap();
        widgets.finish(&mut world);
        begin_static(&mut world);
        publish_current(&mut world);
        widgets.begin();
        panel::paint(&mut world, camera, &mut widgets, 432., 600., theme).unwrap();
        widgets.finish(&mut world);
        let overlay = widgets.entity("scripts-preview-blend").unwrap();
        if !resize {
            let mut palette = crate::native_viewport::ui::palette(&world);
            palette.body[0] += 0.05;
            let theme = ViewportUiTheme::from_palette(&palette);
            world.insert_resource(crate::native_viewport::ui::Appearance {
                palette,
                theme,
                revision: 1,
            });
        }
        let revision = world.resource::<Files>().script.preview.revision;
        let theme = crate::native_viewport::ui::theme(&world);
        widgets.begin();
        panel::paint(
            &mut world,
            camera,
            &mut widgets,
            if resize { 390. } else { 432. },
            600.,
            theme,
        )
        .unwrap();
        widgets.finish(&mut world);
        let preview = &world.resource::<Files>().script.preview;
        assert!(preview.revision > revision);
        assert!(preview.blend.is_none() && preview.wake.is_none());
        assert!(world.get_entity(overlay).is_err());
        assert_eq!(preview.render_jobs, 0);
        assert!(
            preview.images.footprint().0 <= 2 && preview.images.footprint().1 <= 2 * 1024 * 1024
        );
    }
}

#[test]
fn native_preview_single_edge_uses_cached_dissolve_without_geometry_jobs() {
    #[derive(serde::Deserialize)]
    struct Bundle {
        frames: Vec<Frame>,
    }
    let bundle: Bundle = ciborium::de::from_reader(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/show-me/selection_solid.cbor"
        ))
        .as_slice(),
    )
    .unwrap();
    assert_eq!(bundle.frames[2].edges.len(), 1);
    let (mut world, handle, _) = fixture();
    {
        let preview = &mut world.resource_mut::<Files>().script.preview;
        preview.lesson = Some(lesson::Id::SelectionSolid);
        preview.retained = Some(Retained::new(preview.service.clone(), bundle.frames).unwrap());
        preview.index = 1;
        preview.changed().unwrap();
    }
    publish_current(&mut world);
    let source = world
        .resource::<Files>()
        .script
        .preview
        .image
        .clone()
        .unwrap();
    {
        let preview = &mut world.resource_mut::<Files>().script.preview;
        preview.index = 2;
        preview.playing = true;
        preview.changed().unwrap();
        preview.begin_animation();
        assert!(preview.geometry_animation_complete());
        assert!(preview.blend.is_some());
        assert_eq!(preview.display_index(), 1);
    }
    publish_current(&mut world);
    let now = Instant::now();
    let (revision, jobs, key) = {
        let preview = &mut world.resource_mut::<Files>().script.preview;
        arm_replay(preview, now);
        (
            preview.revision,
            preview.render_jobs,
            images::Key::current(preview),
        )
    };
    for tick in 1..=blend::STEPS {
        let preview = &mut world.resource_mut::<Files>().script.preview;
        let at = now + ANIMATION_INTERVAL * u32::from(tick);
        replay(preview, &handle, at).unwrap();
        assert_eq!((preview.revision, preview.render_jobs), (revision, jobs));
        assert!(images::Key::current(preview) == key);
        arm_replay(preview, at);
    }
    let preview = &world.resource::<Files>().script.preview;
    assert!(preview.blend.is_none());
    assert!(preview.animation_complete());
    assert_eq!(preview.display_index(), 2);
    assert_eq!(preview.images.footprint(), (2, 2 * 1024 * 1024));
    assert!(world.resource::<Assets<Image>>().contains(source.id()));
}

#[test]
fn native_preview_multi_edge_fillet_keeps_progressive_highlighting() {
    #[derive(serde::Deserialize)]
    struct Bundle {
        frames: Vec<Frame>,
    }
    let bundle: Bundle = ciborium::de::from_reader(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/show-me/fillet.cbor"
        ))
        .as_slice(),
    )
    .unwrap();
    let index = bundle
        .frames
        .iter()
        .position(|frame| {
            frame.cue == Some(crate::native_viewport::script_preview::PreviewCue::Edges)
        })
        .unwrap();
    assert!(bundle.frames[index].edges.len() > 1);
    assert!(blend::geometry_motion(&bundle.frames)[index]);
}
