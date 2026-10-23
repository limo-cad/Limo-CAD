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
fn native_preview_dissolve_pause_resume_ticks_do_not_change_render_key_revision_or_jobs() {
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
    let alpha = world
        .resource::<Files>()
        .script
        .preview
        .blend
        .as_ref()
        .unwrap()
        .alpha();
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
    assert_eq!(
        world
            .resource::<Files>()
            .script
            .preview
            .blend
            .as_ref()
            .unwrap()
            .alpha(),
        alpha
    );
    command(&mut world, &handle, Action::Resume, &ControlInput::Click).unwrap();
    let preview = &mut world.resource_mut::<Files>().script.preview;
    let resumed = now + Duration::from_secs(120);
    arm_replay(preview, resumed);
    assert_eq!(
        preview.wake.as_ref().unwrap().due,
        resumed + ANIMATION_INTERVAL
    );
    for tick in 1..=4 {
        replay(preview, &handle, resumed + ANIMATION_INTERVAL * tick).unwrap();
        arm_replay(preview, resumed + ANIMATION_INTERVAL * tick);
    }
    assert!(preview.animation_complete());
    assert_eq!(
        preview.wake.as_ref().unwrap().due,
        resumed + ANIMATION_INTERVAL * 4 + FRAME_DELAY
    );
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
    for action in [Action::Help, Action::Steps] {
        let (mut world, handle, _) = fixture();
        begin_static(&mut world);
        publish_current(&mut world);
        command(&mut world, &handle, action, &ControlInput::Click).unwrap();
        let preview = &world.resource::<Files>().script.preview;
        assert!(!preview.playing && preview.wake.is_none());
        assert!(
            preview.blend.is_some(),
            "Reading parks the current transition"
        );
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
    let source = begin_static(&mut world);
    publish_current(&mut world);
    widgets.begin();
    panel::paint(&mut world, camera, &mut widgets, 432., 600., theme).unwrap();
    widgets.finish(&mut world);
    let model = widgets.entity("scripts-preview-model").unwrap();
    let overlay = widgets.entity("scripts-preview-blend").unwrap();
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
    assert!(world.resource::<Assets<Image>>().is_empty());
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
