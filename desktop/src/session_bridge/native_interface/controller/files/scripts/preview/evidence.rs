//! Reproducible captures of production Bevy widgets, never an HTML mockup.
use super::super::super::super::chrome;
use super::*;
use bevy::{
    camera::RenderTarget,
    render::{
        pipelined_rendering::PipelinedRenderingPlugin,
        render_resource::{Extent3d, PollType, TextureDimension, TextureFormat, TextureUsages},
        renderer::RenderDevice,
        view::screenshot::{Screenshot, ScreenshotCaptured},
    },
    window::{ExitCondition, WindowPlugin},
};
use std::{path::Path, sync::atomic::AtomicUsize};

#[derive(Resource, Default)]
struct Capture(Option<Vec<u8>>);

fn update(app: &mut App) {
    app.update();
    app.world()
        .resource::<RenderDevice>()
        .wgpu_device()
        .poll(PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(5)),
        })
        .unwrap();
}
fn capture(app: &mut App, target: &Handle<Image>, path: &Path) {
    for _ in 0..8 {
        update(app);
    }
    app.world_mut().resource_mut::<Capture>().0 = None;
    app.world_mut()
        .spawn(Screenshot::image(target.clone()))
        .observe(
            |event: On<ScreenshotCaptured>, mut pixels: ResMut<Capture>| {
                pixels.0 =
                    Some(crate::native_viewport::screenshot::png_bytes(&event.image).unwrap());
            },
        );
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        update(app);
        if let Some(bytes) = app.world_mut().resource_mut::<Capture>().0.take() {
            assert!(
                bytes.len() > 10_000,
                "Native UI capture must contain actual pixels"
            );
            if path.file_name().unwrap() != "entry.png" {
                let decoded = Image::from_buffer(
                    &bytes,
                    ImageType::Extension("png"),
                    CompressedImageFormats::NONE,
                    true,
                    ImageSampler::Default,
                    RenderAssetUsages::MAIN_WORLD,
                )
                .unwrap();
                let ink = crate::native_viewport::ui::theme(app.world())
                    .ink
                    .to_srgba();
                let expected = [ink.red, ink.green, ink.blue].map(|v| (v * 255.).round() as i16);
                let pixels = decoded.data.as_ref().unwrap();
                let (node, transform) = app
                    .world_mut()
                    .query::<(&Text, &ComputedNode, &bevy::ui::UiGlobalTransform)>()
                    .iter(app.world())
                    .find_map(|(text, node, transform)| {
                        text.0
                            .starts_with("Extrude ·")
                            .then_some((*node, *transform))
                    })
                    .expect("Native title is laid out");
                let start = transform.translation - node.size / 2.;
                let size = decoded.texture_descriptor.size;
                let left = start.x.max(0.) as usize;
                let top = start.y.max(0.) as usize;
                let right = (start.x + node.size.x).min(size.width as f32) as usize;
                let bottom = (start.y + node.size.y).min(size.height as f32) as usize;
                let visible = (top..bottom)
                    .flat_map(|y| (left..right).map(move |x| (y * size.width as usize + x) * 4))
                    .filter(|offset| {
                        (0..3).all(|i| (pixels[*offset + i] as i16 - expected[i]).abs() < 45)
                    })
                    .count();
                assert!(
                    visible > 100,
                    "The native card title must survive view transitions"
                );
            }
            std::fs::write(path, bytes).unwrap();
            return;
        }
        assert!(Instant::now() < deadline, "Native UI capture timed out");
    }
}

#[test]
#[ignore = "requires a native graphics adapter; writes native UI screenshots and performance evidence"]
fn native_show_me_ui_evidence_and_performance() {
    use crate::native_viewport::interface_shell::{InterfaceCamera, InterfaceFrame};
    use limo_cad_interface::{DocumentContext, Rect, Surface};
    let out = std::env::var("LIMO_CAD_SHOW_ME_PROOF_DIR")
        .expect("Choose a fresh LIMO_CAD_SHOW_ME_PROOF_DIR");
    let out = Path::new(&out);
    std::fs::create_dir_all(out).unwrap();
    assert!(
        !out.join("performance.json").exists(),
        "Retain previous evidence; choose a fresh path"
    );
    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .build()
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                close_when_requested: false,
                ..default()
            })
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<PipelinedRenderingPlugin>(),
    );
    let wakes = Arc::new(AtomicUsize::new(0));
    let count = wakes.clone();
    let handle = NativeInterfaceHandle::new(move || {
        count.fetch_add(1, Ordering::Relaxed);
    });
    interface_shell::install(&mut app, handle.clone(), |_, _| {});
    app.add_systems(Startup, crate::native_viewport::ui::load_system_font);
    app.init_resource::<Capture>();
    app.finish();
    app.cleanup();
    initialize(
        app.world_mut(),
        Arc::new(Mutex::new(DocumentWorkspace::default())),
    );
    let theme = crate::native_viewport::ui::theme(app.world());
    app.insert_resource(ClearColor(theme.viewport));
    app.insert_resource(bevy::ui::UiScale(2.));
    update(&mut app);
    let mut texture = Image::new_fill(
        Extent3d {
            width: 864,
            height: 1200,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    texture.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC;
    let target = app.world_mut().resource_mut::<Assets<Image>>().add(texture);
    let camera = app
        .world_mut()
        .query_filtered::<Entity, With<InterfaceCamera>>()
        .single(app.world())
        .unwrap();
    app.world_mut()
        .entity_mut(camera)
        .insert(RenderTarget::Image(target.clone().into()));
    app.world_mut()
        .get_mut::<Camera>(camera)
        .unwrap()
        .clear_color = bevy::camera::ClearColorConfig::Custom(theme.viewport);
    let bounds = Rect {
        x: 0.,
        y: 0.,
        width: 432.,
        height: 600.,
    };
    handle
        .present(InterfaceFrame {
            context: DocumentContext {
                window_id: "show-me-evidence".into(),
                document_id: "isolated-fixture".into(),
                epoch: 1,
            },
            client: bounds,
            surface: bounds,
            canvases: vec![],
            surfaces: vec![Surface {
                name: "document/scripts".into(),
                text: None,
            }],
            modal_stack: vec![],
            document_visible: true,
        })
        .unwrap();
    let mut widgets = chrome::Widgets::default();
    widgets.begin();
    let anchor = widgets
        .button(
            app.world_mut(),
            camera,
            "fixture-extrude",
            InterfaceControl::button("solid/build", "Extrude"),
            Some("Extrude"),
            NativeCommand::File(FileCommand::ScriptPreview(Action::Model)),
            chrome::rect(24., 4., 92., 32.),
            Some(interface_shell::ribbon::Icon::Extrude),
            50,
        )
        .unwrap();
    widgets.finish(app.world_mut());
    for _ in 0..8 {
        update(&mut app);
    }
    let action = handle
        .resolve_retained(limo_cad_interface::ControlKey(anchor.to_bits()))
        .unwrap();
    handle.prepare_activation(&action).unwrap();
    feature::paint_hint(app.world_mut(), camera, Some(anchor), &mut widgets, 432.).unwrap();
    widgets.finish(app.world_mut());
    capture(&mut app, &target, &out.join("entry.png"));
    let prepared = Instant::now();
    let frames = prepare(
        &catalog::examples()
            .iter()
            .find(|e| e.id == lesson::EXTRUDE.recipe)
            .unwrap()
            .source,
    )
    .unwrap();
    let prepare_ms = prepared.elapsed().as_secs_f64() * 1000.;
    let service = app
        .world()
        .resource::<Files>()
        .script
        .preview
        .service
        .clone();
    let retained = Retained::new(service, frames).unwrap();
    let durations = retained.descriptor.durations_ms.clone();
    {
        let files = &mut app.world_mut().resource_mut::<Files>();
        files.scripts = true;
        files.script.preview.open = true;
        files.script.preview.return_to = Some((false, false, false, false));
        files.script.preview.retained = Some(retained);
    }
    let mut cold_ms = vec![];
    for index in 0..durations.len() {
        {
            let preview = &mut app.world_mut().resource_mut::<Files>().script.preview;
            preview.index = index;
            preview.changed().unwrap();
        }
        widgets.begin();
        panel::paint(app.world_mut(), camera, &mut widgets, 432., 600., theme).unwrap();
        widgets.finish(app.world_mut());
        let begin = Instant::now();
        let deadline = begin + Duration::from_secs(45);
        loop {
            poll(app.world_mut());
            let preview = &app.world().resource::<Files>().script.preview;
            assert!(preview.error.is_none(), "{:?}", preview.error);
            if preview.rendered == preview.revision {
                break;
            }
            assert!(Instant::now() < deadline, "Preview frame timed out");
            std::thread::sleep(Duration::from_millis(2));
        }
        cold_ms.push(begin.elapsed().as_secs_f64() * 1000.);
        app.world_mut()
            .resource_mut::<Files>()
            .script
            .preview
            .playing = index + 1 < durations.len();
        widgets.begin();
        panel::paint(app.world_mut(), camera, &mut widgets, 432., 600., theme).unwrap();
        widgets.finish(app.world_mut());
        capture(&mut app, &target, &out.join(format!("frame-{index}.png")));
        app.world_mut()
            .resource_mut::<Files>()
            .script
            .preview
            .playing = false;
    }
    let jobs = app.world().resource::<Files>().script.preview.render_jobs;
    let footprint = app
        .world()
        .resource::<Files>()
        .script
        .preview
        .images
        .footprint();
    let begin = Instant::now();
    for _ in 0..100 {
        command(
            app.world_mut(),
            &handle,
            Action::Replay,
            &ControlInput::Click,
        )
        .unwrap();
        for index in 0..durations.len() {
            if index > 0 {
                command(app.world_mut(), &handle, Action::Next, &ControlInput::Click).unwrap();
            }
            render(app.world_mut(), &handle).unwrap();
        }
    }
    let warm_ms = begin.elapsed().as_secs_f64() * 1000. / 100.;
    assert_eq!(
        app.world().resource::<Files>().script.preview.render_jobs,
        jobs,
        "Warm replay must not submit GPU jobs"
    );
    command(app.world_mut(), &handle, Action::Help, &ControlInput::Click).unwrap();
    widgets.begin();
    panel::paint(app.world_mut(), camera, &mut widgets, 432., 600., theme).unwrap();
    widgets.finish(app.world_mut());
    capture(&mut app, &target, &out.join("help.png"));
    command(app.world_mut(), &handle, Action::Demo, &ControlInput::Click).unwrap();
    // Use the same appearance transition as the application, including the 3D thumbnail.
    let palette = *crate::app_preferences::palette::viewport_palette(
        crate::app_preferences::ResolvedTheme::Light,
    );
    let light = ViewportUiTheme::from_palette(&palette);
    app.insert_resource(crate::native_viewport::ui::Appearance {
        palette,
        theme: light,
        revision: 1,
    });
    app.insert_resource(ClearColor(light.viewport));
    app.world_mut()
        .get_mut::<Camera>(camera)
        .unwrap()
        .clear_color = bevy::camera::ClearColorConfig::Custom(light.viewport);
    interface_shell::refresh_theme(app.world_mut(), light);
    widgets.begin();
    panel::paint(app.world_mut(), camera, &mut widgets, 432., 600., light).unwrap();
    widgets.finish(app.world_mut());
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        poll(app.world_mut());
        let preview = &app.world().resource::<Files>().script.preview;
        if preview.rendered == preview.revision {
            break;
        }
        assert!(Instant::now() < deadline, "Light preview timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
    widgets.begin();
    panel::paint(app.world_mut(), camera, &mut widgets, 432., 600., light).unwrap();
    widgets.finish(app.world_mut());
    capture(&mut app, &target, &out.join("light.png"));
    // Jack's reproduction: the supported 1200×760 window at 175% UI scale.
    command(app.world_mut(), &handle, Action::Help, &ControlInput::Click).unwrap();
    app.insert_resource(bevy::ui::UiScale(1.75));
    let mut texture = Image::new_fill(
        Extent3d {
            width: 1200,
            height: 760,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0, 0, 0, 255],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    texture.texture_descriptor.usage |= TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC;
    app.world_mut()
        .resource_mut::<Assets<Image>>()
        .insert(target.id(), texture)
        .unwrap();
    for _ in 0..3 {
        widgets.begin();
        panel::paint(
            app.world_mut(),
            camera,
            &mut widgets,
            1200. / 1.75,
            760. / 1.75,
            light,
        )
        .unwrap();
        widgets.finish(app.world_mut());
        update(&mut app);
    }
    capture(&mut app, &target, &out.join("help-175-top.png"));
    let clip = widgets.entity("extrude-help-body").unwrap();
    let back = widgets.entity("extrude-help-back").unwrap();
    let clip_node = app.world().get::<ComputedNode>(clip).unwrap();
    let clip_center = app
        .world()
        .get::<bevy::ui::UiGlobalTransform>(clip)
        .unwrap()
        .translation;
    let back_node = app.world().get::<ComputedNode>(back).unwrap();
    let back_center = app
        .world()
        .get::<bevy::ui::UiGlobalTransform>(back)
        .unwrap()
        .translation;
    assert!(
        clip_center.y + clip_node.size.y / 2. + 8. <= back_center.y - back_node.size.y / 2.,
        "Help has a real clipped reading region above its fixed return action"
    );
    command(
        app.world_mut(),
        &handle,
        Action::HelpBody,
        &ControlInput::Key(limo_cad_interface::KeyChord::plain("End")),
    )
    .unwrap();
    widgets.begin();
    panel::paint(
        app.world_mut(),
        camera,
        &mut widgets,
        1200. / 1.75,
        760. / 1.75,
        light,
    )
    .unwrap();
    widgets.finish(app.world_mut());
    capture(&mut app, &target, &out.join("help-175-bottom.png"));
    let mut idle_ns = vec![];
    for open in [true, false] {
        if !open {
            close(app.world_mut());
        }
        app.world_mut()
            .resource_mut::<Files>()
            .script
            .preview
            .playing = false;
        let before = wakes.load(Ordering::Relaxed);
        app.world_mut().clear_trackers();
        let begin = Instant::now();
        for _ in 0..100_000 {
            poll(app.world_mut());
        }
        idle_ns.push(begin.elapsed().as_nanos() as f64 / 100_000.);
        assert_eq!(wakes.load(Ordering::Relaxed), before);
        assert!(!app
            .world()
            .get_resource_ref::<Files>()
            .unwrap()
            .is_changed());
    }
    assert_eq!(
        app.world()
            .resource::<Files>()
            .script
            .preview
            .images
            .footprint(),
        (0, 0)
    );
    let report = json!({"scope":"windowless production native UI; debug test profile; controller timings, not whole-app CPU/GPU utilization", "prepare_ms":prepare_ms,"cold_frame_ms":cold_ms,"cold_gpu_jobs":jobs,"warm_replay_ms":warm_ms,"warm_gpu_jobs":0,"cached_frames":footprint.0,"cached_rgba_bytes":footprint.1,"texture_budget_bytes":16*1024*1024,"paused_poll_ns":idle_ns[0],"closed_poll_ns":idle_ns[1],"idle_poll_samples":100_000,"idle_preview_wakes":0,"closed_cached_bytes":0,"durations_ms":durations});
    std::fs::write(
        out.join("performance.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("{report}");
}
