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

mod operational;

#[derive(Resource, Default)]
struct Capture(Option<Vec<u8>>);

fn process_memory() -> Value {
    let Ok(pid) = sysinfo::get_current_pid() else {
        return Value::Null;
    };
    let mut system = sysinfo::System::new();
    system.refresh_processes_specifics(
        sysinfo::ProcessesToUpdate::Some(&[pid]),
        true,
        sysinfo::ProcessRefreshKind::nothing().with_memory(),
    );
    system.process(pid).map_or(Value::Null, |process| {
        json!({"resident_bytes":process.memory(),
            "virtual_memory_bytes":process.virtual_memory(),
            "virtual_memory_semantics":if cfg!(windows) { "Windows private bytes" } else { "sysinfo virtual memory" },
            "scope":"whole test process; GPU dedicated memory excluded"})
    })
}

fn lifecycle(app: &mut App, handle: &NativeInterfaceHandle, id: lesson::Id) -> Value {
    let prepared = app
        .world()
        .resource::<Files>()
        .script
        .preview
        .prepared
        .clone();
    let before = prepared.lock().unwrap().metrics();
    assert_eq!((before.0, before.1), (0, 1));
    let closed_memory = process_memory();
    let mut timings = vec![];
    let other = if id == lesson::Id::Sketch {
        lesson::Id::Extrude
    } else {
        lesson::Id::Sketch
    };
    for lesson in [id, other, id] {
        let begin = Instant::now();
        command(
            app.world_mut(),
            handle,
            Action::Lesson(lesson),
            &ControlInput::Click,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            update(app);
            poll(app.world_mut());
            let preview = &app.world().resource::<Files>().script.preview;
            assert!(preview.error.is_none(), "{:?}", preview.error);
            if preview.build.is_none()
                && preview.image.is_some()
                && preview.rendered == preview.revision
            {
                break;
            }
            assert!(Instant::now() < deadline, "Controller reopen timed out");
            std::thread::sleep(Duration::from_millis(2));
        }
        timings.push(begin.elapsed().as_secs_f64() * 1000.);
        command(app.world_mut(), handle, Action::Close, &ControlInput::Click).unwrap();
        released_textures(app);
    }
    let after = prepared.lock().unwrap().metrics();
    assert_eq!((after.0, after.1, after.2), (2, 2, 2));
    json!({"reopen_same_lesson_ms":timings[0], "open_other_lesson_ms":timings[1],
        "return_to_first_lesson_ms":timings[2], "renderer":"existing shared CAD worker and device",
        "authored_cache_hits":after.0,"authored_cache_misses":after.1,
        "authored_cache_entries":after.2,"authored_cache_serialized_bytes":after.3,
        "closed_memory_before_reopen":closed_memory,"closed_memory_after_reopen":process_memory(),
        "scope":"actual controller open/close, fresh pixel cache each open, retained current viewport size and palette"})
}

fn update(app: &mut App) {
    app.update();
    assert!(
        app.should_exit().is_none(),
        "Native evidence renderer failed"
    );
    crate::native_viewport::script_preview::shared_fixture_retirement_finished(app.world())
        .expect("Native evidence CAD worker stopped");
    app.world()
        .resource::<RenderDevice>()
        .wgpu_device()
        .poll(PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(5)),
        })
        .unwrap();
}

fn settle(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        update(app);
        poll(app.world_mut());
        let preview = &app.world().resource::<Files>().script.preview;
        assert!(preview.error.is_none(), "{:?}", preview.error);
        let footprint = preview.images.footprint();
        assert!(footprint.0 <= 2 && footprint.1 <= 2 * 1024 * 1024);
        assert!(crate::native_viewport::script_preview::textures::retained_count(app.world()) <= 2);
        assert!(
            crate::native_viewport::script_preview::textures::imported_count(
                app.sub_app(bevy::render::RenderApp).world()
            ) <= 2
        );
        if preview.build.is_none()
            && preview.image.is_some()
            && preview.rendered == preview.revision
        {
            break;
        }
        assert!(Instant::now() < deadline, "Shared preview did not settle");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn released_textures(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        update(app);
        let retained =
            crate::native_viewport::script_preview::textures::retained_count(app.world());
        let imported = crate::native_viewport::script_preview::textures::imported_count(
            app.sub_app(bevy::render::RenderApp).world(),
        );
        let retired =
            crate::native_viewport::script_preview::shared_fixture_retirement_finished(app.world())
                .expect("Close stopped the CAD worker");
        if retained == 0 && imported == 0 && retired {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Close did not finish: textures {retained}/{imported}, worker retired {retired}"
        );
    }
}
fn capture(app: &mut App, target: &Handle<Image>, path: &Path) {
    capture_component(app, target, path, true);
}

// Operational entry/Close have no card. Existing captures retain every card/font/ink gate.
fn capture_component(app: &mut App, target: &Handle<Image>, path: &Path, card_visible: bool) {
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
            for (text, block) in app
                .world_mut()
                .query::<(&Text, &bevy::text::ComputedTextBlock)>()
                .iter(app.world())
            {
                assert!(
                    block
                        .buffer()
                        .lines()
                        .flat_map(|line| line.runs())
                        .all(|run| run
                            .clusters()
                            .flat_map(|cluster| cluster.glyphs())
                            .all(|glyph| glyph.id != 0)),
                    "Missing native glyph in {}",
                    text.0
                );
            }
            assert!(
                bytes.len() > 10_000,
                "Native UI capture must contain actual pixels"
            );
            if card_visible && path.file_name().unwrap() != "entry.png" {
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
                for (text, node, transform) in app.world_mut()
                    .query_filtered::<(&Text, &ComputedNode, &bevy::ui::UiGlobalTransform), With<annotations::DimensionLabel>>()
                    .iter(app.world())
                {
                    let size = decoded.texture_descriptor.size;
                    let half = node.size / 2.;
                    let corners = [-half, Vec2::new(half.x, -half.y), half, Vec2::new(-half.x, half.y)]
                        .map(|point| transform.transform_point2(point));
                    let min = corners.into_iter().fold(Vec2::splat(f32::INFINITY), Vec2::min).max(Vec2::ZERO);
                    let max = corners.into_iter().fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max)
                        .min(Vec2::new(size.width as f32, size.height as f32));
                    let inverse = transform.try_inverse().unwrap();
                    let color = crate::native_viewport::ui::palette(app.world()).dimension.map(|value| (value * 255.).round() as i16);
                    let mut ink = 0;
                    for y in min.y as usize..max.y as usize {
                        for x in min.x as usize..max.x as usize {
                            let local = inverse.transform_point2(Vec2::new(x as f32 + 0.5, y as f32 + 0.5));
                            if local.x.abs() >= half.x - 2. || local.y.abs() >= half.y - 2. { continue; }
                            let offset = (y * size.width as usize + x) * 4;
                            if (0..3).all(|axis| (pixels[offset + axis] as i16 - color[axis]).abs() < 45) { ink += 1; }
                        }
                    }
                    // Completed dimension frames keep the existing ink gate.
                    // Intermediate labels intentionally fade with their image.
                    if app.world().resource::<Files>().script.preview.animation_complete() {
                        assert!(ink > 8, "Dimension {} must rasterize inside its own label box", text.0);
                    }
                }
                if app.world().resource::<Files>().script.preview.lesson == Some(lesson::Id::Sketch)
                    && path.file_name().unwrap() == "frame-0.png"
                {
                    let (node, transform) = app
                        .world_mut()
                        .query::<(
                            &NativeCommandBinding,
                            &ComputedNode,
                            &bevy::ui::UiGlobalTransform,
                        )>()
                        .iter(app.world())
                        .find_map(|(binding, node, transform)| {
                            (binding.command
                                == NativeCommand::File(FileCommand::ScriptPreview(Action::Model)))
                            .then_some((*node, *transform))
                        })
                        .expect("Sketch model stage is laid out");
                    let size = decoded.texture_descriptor.size;
                    let start = transform.translation - node.size / 2.;
                    // Exclude focus/border pixels: only actual plane ink counts.
                    let left = (start.x + 12.).max(0.) as usize;
                    let top = (start.y + 12.).max(0.) as usize;
                    let right = (start.x + node.size.x - 12.).min(size.width as f32) as usize;
                    let bottom = (start.y + node.size.y - 12.).min(size.height as f32) as usize;
                    let reference = (top * size.width as usize + left) * 4;
                    let visible = (top..bottom)
                        .flat_map(|y| (left..right).map(move |x| (y * size.width as usize + x) * 4))
                        .filter(|offset| {
                            (0..3).any(|i| {
                                (pixels[*offset + i] as i16 - pixels[reference + i] as i16).abs()
                                    > 20
                            })
                        })
                        .count();
                    assert!(
                        visible > 100,
                        "Initial Sketch must show its plane, not a blank stage"
                    );
                }
                let (node, transform) = app
                    .world_mut()
                    .query::<(&Text, &ComputedNode, &bevy::ui::UiGlobalTransform)>()
                    .iter(app.world())
                    .find_map(|(text, node, transform)| {
                        (lesson::ALL.into_iter().any(|id| {
                            text.0 == lesson::text(app.world(), &format!("{}.example", id.key()))
                        }) || text.0 == lesson::text(app.world(), "ui.guidedExamples"))
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
#[ignore = "requires a native graphics adapter; captures the affected Sketch lesson"]
fn native_show_me_sketch_plane_evidence() {
    let root = std::env::var("LIMO_CAD_SHOW_ME_PROOF_DIR")
        .expect("Choose a fresh LIMO_CAD_SHOW_ME_PROOF_DIR");
    capture_lesson(
        lesson::Id::Sketch,
        lesson::CutScenario::ThroughAll,
        &Path::new(&root).join("sketch"),
        true,
    );
}

#[test]
#[ignore = "requires a native graphics adapter; writes native UI screenshots and performance evidence"]
fn native_show_me_ui_evidence_and_performance() {
    let root = std::env::var("LIMO_CAD_SHOW_ME_PROOF_DIR")
        .expect("Choose a fresh LIMO_CAD_SHOW_ME_PROOF_DIR");
    for (guide_index, id) in lesson::ALL.into_iter().enumerate() {
        capture_lesson(
            id,
            lesson::CutScenario::ThroughAll,
            &Path::new(&root).join(id.key()),
            guide_index == 0,
        );
    }
    capture_lesson(
        lesson::Id::Cut,
        lesson::CutScenario::MeasuredDepth,
        &Path::new(&root).join("cut-measured"),
        false,
    );
}
/// Compare only pixels inside the model, excluding captions, focus rings and UI
/// metadata. A changing step number or PNG encoding cannot qualify motion.
fn model_pixels(app: &App, model: Entity, path: &Path) -> Vec<u8> {
    let decoded = Image::from_buffer(
        &std::fs::read(path).unwrap(),
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::Default,
        RenderAssetUsages::MAIN_WORLD,
    )
    .unwrap();
    let node = app.world().get::<ComputedNode>(model).unwrap();
    let transform = app
        .world()
        .get::<bevy::ui::UiGlobalTransform>(model)
        .unwrap();
    let start = transform.translation - node.size / 2.;
    let size = decoded.texture_descriptor.size;
    let left = (start.x + 12.).max(0.) as usize;
    let top = (start.y + 12.).max(0.) as usize;
    let right = (start.x + node.size.x - 12.).min(size.width as f32) as usize;
    let bottom = (start.y + node.size.y - 12.).min(size.height as f32) as usize;
    assert!(right > left && bottom > top);
    let data = decoded.data.as_ref().unwrap();
    (top..bottom)
        .flat_map(|y| {
            (left..right).flat_map(move |x| {
                let offset = (y * size.width as usize + x) * 4;
                data[offset..offset + 3].iter().copied()
            })
        })
        .collect()
}

fn changed_pixels(a: &[u8], b: &[u8]) -> usize {
    assert_eq!(
        a.len(),
        b.len(),
        "Motion samples need identical native model bounds"
    );
    a.as_chunks::<3>()
        .0
        .iter()
        .zip(b.as_chunks::<3>().0)
        .filter(|(a, b)| a != b)
        .count()
}

fn visible_motion(samples: &[Vec<u8>], id: lesson::Id) -> usize {
    assert!(samples.len() >= 3);
    let distinct_samples = samples
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    assert!(
        distinct_samples >= 3,
        "{id:?} must show visible progress, not repeated endpoint images"
    );
    assert!(
        samples[1..samples.len() - 1].iter().any(|sample| {
            changed_pixels(&samples[0], sample) > 16
                && changed_pixels(sample, samples.last().unwrap()) > 16
        }),
        "{id:?} needs actual intermediate model pixels distinct from both endpoints"
    );
    distinct_samples
}

fn paint_anchor(
    world: &mut World,
    camera: Entity,
    widgets: &mut chrome::Widgets,
    id: lesson::Id,
) -> Entity {
    let anchor_caption = id.title("en");
    let anchor_command =
        feature::command_for_guide(id).expect("Every guide has a real entry command");
    let anchor_group = match &anchor_command {
        NativeCommand::Sketch(crate::native_editor::EditorCommand::Interaction(command)) => {
            match command {
                crate::native_editor::InteractionCommand::Form(kind) => kind.group(),
                crate::native_editor::InteractionCommand::Relation(_) => "sketch/constrain",
                crate::native_editor::InteractionCommand::Dimension => "sketch/dimension",
                _ => "sketch/edit",
            }
        }
        NativeCommand::Sketch(_) => "sketch/draw",
        _ => "solid/build",
    };
    widgets
        .button(
            world,
            camera,
            "fixture-operation",
            InterfaceControl::button(anchor_group, anchor_caption),
            Some(anchor_caption),
            anchor_command,
            chrome::rect(24., 4., 128., 32.),
            None,
            50,
        )
        .unwrap()
}

fn capture_lesson(id: lesson::Id, scenario: lesson::CutScenario, out: &Path, capture_index: bool) {
    capture_lesson_mode(id, scenario, out, capture_index, false);
}

fn capture_lesson_mode(
    id: lesson::Id,
    scenario: lesson::CutScenario,
    out: &Path,
    capture_index: bool,
    operational_only: bool,
) {
    use crate::native_viewport::interface_shell::{InterfaceCamera, InterfaceFrame};
    use limo_cad_interface::{DocumentContext, Rect, Surface};
    std::fs::create_dir_all(out).unwrap();
    assert!(
        !out.join("performance.json").exists(),
        "Retain previous evidence; choose a fresh path"
    );
    let mut app = App::new();
    app.add_plugins(
        crate::native_viewport::script_preview::shared_fixture_plugins()
            .set(WindowPlugin {
                primary_window: Some(Window::default()),
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
    crate::native_viewport::script_preview::install_shared_fixture(&mut app);
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
    let anchor = paint_anchor(app.world_mut(), camera, &mut widgets, id);
    widgets.finish(app.world_mut());
    for _ in 0..8 {
        update(&mut app);
    }
    let action = handle
        .resolve_retained(limo_cad_interface::ControlKey(anchor.to_bits()))
        .unwrap();
    handle.prepare_activation(&action).unwrap();
    feature::paint_hint(app.world_mut(), camera, None, &mut widgets, 432.).unwrap();
    assert!(
        widgets
            .entity(&format!(
                "{}-show-me",
                if id == lesson::Id::Cut {
                    "cut"
                } else {
                    id.key()
                }
            ))
            .is_some(),
        "The native hint must teach its actual operation"
    );
    widgets.finish(app.world_mut());
    if operational_only {
        operational::record(
            &mut app,
            &handle,
            camera,
            &mut widgets,
            &target,
            id,
            out,
            &wakes,
        );
        return;
    }
    capture(&mut app, &target, &out.join("entry.png"));
    // Entry, endpoints and locales remain per guide; walk the complete index once.
    if capture_index {
        command(
            app.world_mut(),
            &handle,
            Action::Index,
            &ControlInput::Click,
        )
        .unwrap();
        widgets.begin();
        panel::paint(app.world_mut(), camera, &mut widgets, 432., 600., theme).unwrap();
        widgets.finish(app.world_mut());
        capture(&mut app, &target, &out.join("help-index.png"));
        let mut index_page = 1;
        while widgets.entity("help-index-next").is_some_and(|entity| {
            app.world()
                .get::<InterfaceControl>(entity)
                .is_some_and(|control| !control.disabled)
        }) {
            command(
                app.world_mut(),
                &handle,
                Action::IndexPage(true),
                &ControlInput::Click,
            )
            .unwrap();
            widgets.begin();
            panel::paint(app.world_mut(), camera, &mut widgets, 432., 600., theme).unwrap();
            widgets.finish(app.world_mut());
            for _ in 0..8 {
                update(&mut app);
            }
            capture(
                &mut app,
                &target,
                &out.join(format!("help-index-page-{index_page}.png")),
            );
            index_page += 1;
            assert!(index_page <= lesson::ALL.len());
        }

        assert_eq!(
            app.world().resource::<Files>().script.preview.render_jobs,
            0
        );
        command(
            app.world_mut(),
            &handle,
            Action::Close,
            &ControlInput::Click,
        )
        .unwrap();
    }
    let memory_before = process_memory();
    let controller_open = Instant::now();
    command(
        app.world_mut(),
        &handle,
        Action::Lesson(id),
        &ControlInput::Click,
    )
    .unwrap();
    app.world_mut()
        .resource_mut::<Files>()
        .script
        .preview
        .scenario = scenario;
    widgets.begin();
    panel::paint(app.world_mut(), camera, &mut widgets, 432., 600., theme).unwrap();
    widgets.finish(app.world_mut());
    let deadline = Instant::now() + Duration::from_secs(45);
    while app
        .world()
        .resource::<Files>()
        .script
        .preview
        .build
        .is_some()
    {
        update(&mut app);
        poll(app.world_mut());
        assert!(
            Instant::now() < deadline,
            "Controller preparation timed out"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let prepare_ms = controller_open.elapsed().as_secs_f64() * 1000.;
    let preview = &app.world().resource::<Files>().script.preview;
    assert!(preview.error.is_none(), "{:?}", preview.error);
    let retained = preview
        .retained
        .as_ref()
        .expect("Controller retains lesson");
    let durations: Vec<_> = if id == lesson::Id::Cut {
        (0..4)
            .map(|index| retained.descriptor.durations_ms[scenario.frame(index)])
            .collect()
    } else {
        retained.descriptor.durations_ms.clone()
    };
    let mut first_open_ms = 0.;
    let mut cold_ms = vec![];
    for index in 0..durations.len() {
        {
            let preview = &mut app.world_mut().resource_mut::<Files>().script.preview;
            preview.index = index;
            preview.playing = false;
            preview.animation_progress = ANIMATION_STEPS;
            preview.changed().unwrap();
        }
        widgets.begin();
        panel::paint(app.world_mut(), camera, &mut widgets, 432., 600., theme).unwrap();
        widgets.finish(app.world_mut());
        let begin = Instant::now();
        let deadline = begin + Duration::from_secs(45);
        loop {
            update(&mut app);
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
        if index == 0 {
            first_open_ms = controller_open.elapsed().as_secs_f64() * 1000.;
        }
        app.world_mut()
            .resource_mut::<Files>()
            .script
            .preview
            .playing = false;
        widgets.begin();
        panel::paint(app.world_mut(), camera, &mut widgets, 432., 600., theme).unwrap();
        widgets.finish(app.world_mut());
        let primary = widgets.entity("scripts-preview-primary").unwrap();
        let all_steps = widgets.entity("lesson-all-steps").unwrap();
        assert_eq!(
            app.world()
                .get::<InterfaceControl>(all_steps)
                .unwrap()
                .expanded,
            Some(false)
        );
        assert_eq!(
            app.world().get::<Node>(all_steps).unwrap().border,
            UiRect::all(px(1.))
        );
        assert_eq!(
            app.world()
                .get::<NativeCommandBinding>(primary)
                .unwrap()
                .command,
            NativeCommand::File(FileCommand::ScriptPreview(if index + 1 < durations.len() {
                Action::Resume
            } else {
                Action::Replay
            }))
        );
        assert_eq!(
            widgets.entity("cut-scenario-alternative").is_some(),
            id == lesson::Id::Cut && index + 1 == durations.len(),
            "Alternative outcomes appear only after the first result"
        );
        capture(&mut app, &target, &out.join(format!("frame-{index}.png")));
        if id == lesson::Id::Sketch {
            let dimension_step = app
                .world()
                .resource::<Files>()
                .script
                .preview
                .retained
                .as_ref()
                .unwrap()
                .descriptor
                .cues[index]
                == Some(crate::native_viewport::script_preview::PreviewCue::Dimension);
            assert_eq!(
                widgets.entity("lesson-dimension-label-0").is_some(),
                dimension_step,
                "Dimension labels appear only on their authored step"
            );
            if dimension_step {
                let mut labels = Vec::new();
                for label in 0..32 {
                    let Some(entity) = widgets.entity(&format!("lesson-dimension-label-{label}"))
                    else {
                        break;
                    };
                    labels.push(app.world().get::<Text>(entity).unwrap().0.clone());
                    assert!(
                        app.world().get::<InterfaceControl>(entity).is_none(),
                        "Teaching labels cannot edit the live sketch"
                    );
                    let block = app
                        .world()
                        .get::<bevy::text::ComputedTextBlock>(entity)
                        .unwrap();
                    assert!(
                        block
                            .buffer()
                            .lines()
                            .flat_map(|line| line.runs())
                            .any(|run| run
                                .clusters()
                                .any(|cluster| cluster.glyphs().next().is_some())),
                        "Dimension text must shape into visible glyphs"
                    );
                }
                for expected in ["32", "44", "8"] {
                    assert!(
                        labels.iter().any(|label| label.starts_with(expected)),
                        "Missing authored {expected} mm dimension: {labels:?}"
                    );
                }
            }
        }
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
    // The two most recent textures support immediate back/forward without work.
    let before_adjacent = app.world().resource::<Files>().script.preview.render_jobs;
    for _ in 0..100 {
        for index in [durations.len() - 2, durations.len() - 1] {
            command(
                app.world_mut(),
                &handle,
                Action::Step(index),
                &ControlInput::Click,
            )
            .unwrap();
            render(app.world_mut(), &handle).unwrap();
        }
    }
    assert_eq!(
        app.world().resource::<Files>().script.preview.render_jobs,
        before_adjacent
    );
    // A complete replay deliberately renders evicted frames instead of retaining
    // every frame's GPU texture. Measure this separate, memory-bounded tradeoff.
    let begin = Instant::now();
    let before_replay = app.world().resource::<Files>().script.preview.render_jobs;
    for _ in 0..3 {
        for index in 0..durations.len() {
            command(
                app.world_mut(),
                &handle,
                Action::Step(index),
                &ControlInput::Click,
            )
            .unwrap();
            settle(&mut app);
        }
    }
    let warm_ms = begin.elapsed().as_secs_f64() * 1000. / 3.;
    let warm_jobs = app.world().resource::<Files>().script.preview.render_jobs - before_replay;
    let mut scenario_cached_frames = None;
    let mut scenario_jobs = None;
    if id == lesson::Id::Cut {
        let before_scenarios = app.world().resource::<Files>().script.preview.render_jobs;
        for option in lesson::CutScenario::ALL {
            command(
                app.world_mut(),
                &handle,
                Action::Scenario(option),
                &ControlInput::Click,
            )
            .unwrap();
            for index in 0..4 {
                command(
                    app.world_mut(),
                    &handle,
                    Action::Step(index),
                    &ControlInput::Click,
                )
                .unwrap();
                settle(&mut app);
            }
        }
        let count = app
            .world()
            .resource::<Files>()
            .script
            .preview
            .images
            .footprint()
            .0;
        assert_eq!(count, 2);
        scenario_cached_frames = Some(count);
        scenario_jobs =
            Some(app.world().resource::<Files>().script.preview.render_jobs - before_scenarios);
        command(
            app.world_mut(),
            &handle,
            Action::Scenario(scenario),
            &ControlInput::Click,
        )
        .unwrap();
        command(
            app.world_mut(),
            &handle,
            Action::Step(3),
            &ControlInput::Click,
        )
        .unwrap();
        settle(&mut app);
    }
    let jobs_before_steps = app.world().resource::<Files>().script.preview.render_jobs;
    command(
        app.world_mut(),
        &handle,
        Action::Steps,
        &ControlInput::Click,
    )
    .unwrap();
    widgets.begin();
    panel::paint(app.world_mut(), camera, &mut widgets, 432., 600., theme).unwrap();
    widgets.finish(app.world_mut());
    assert!(widgets.entity("scripts-preview-model").is_none());
    assert!(widgets.entity("scripts-preview-primary").is_none());
    assert!(widgets.entity("scripts-preview-caption").is_none());
    let all_steps = widgets.entity("lesson-all-steps").unwrap();
    assert_eq!(
        app.world()
            .get::<InterfaceControl>(all_steps)
            .unwrap()
            .expanded,
        Some(true)
    );
    for index in 0..durations.len() {
        let step = widgets.entity(&format!("lesson-step-{index}")).unwrap();
        assert!(
            matches!(app.world().get::<Node>(step).unwrap().height, Val::Px(height) if height >= 32.)
        );
    }
    capture(&mut app, &target, &out.join("all-steps.png"));
    assert_eq!(
        app.world().resource::<Files>().script.preview.render_jobs,
        jobs_before_steps,
        "Expanding navigation must not submit work"
    );
    for index in 0..durations.len() {
        command(
            app.world_mut(),
            &handle,
            Action::Step(index),
            &ControlInput::Click,
        )
        .unwrap();
        settle(&mut app);
    }
    command(app.world_mut(), &handle, Action::Help, &ControlInput::Click).unwrap();
    widgets.begin();
    panel::paint(app.world_mut(), camera, &mut widgets, 432., 600., theme).unwrap();
    widgets.finish(app.world_mut());
    capture(&mut app, &target, &out.join("help.png"));
    command(app.world_mut(), &handle, Action::Demo, &ControlInput::Click).unwrap();
    // Retain geometric motion where the renderer supports the actual payload.
    // Otherwise qualify an authored before/after pair via the production UI blend.
    let geometry_motion = app
        .world()
        .resource::<Files>()
        .script
        .preview
        .retained
        .as_ref()
        .unwrap()
        .geometry_motion
        .clone();
    let frame_for_step = |step| {
        if id == lesson::Id::Cut {
            scenario.frame(step)
        } else {
            step
        }
    };
    let preferred = if id == lesson::Id::Sketch { 1 } else { 2 };
    // Do not qualify a static operation with only its animated setup plane.
    let geometry_step =
        (geometry_motion.get(frame_for_step(preferred)) == Some(&true)).then_some(preferred);
    let model = widgets.entity("scripts-preview-model").unwrap();
    let mut motion_samples = Vec::new();
    let motion_step;
    if let Some(step) = geometry_step {
        motion_step = step;
        for progress in [0, 4, 8, 12, 16, 20] {
            command(
                app.world_mut(),
                &handle,
                Action::Step(motion_step),
                &ControlInput::Click,
            )
            .unwrap();
            {
                let preview = &mut app.world_mut().resource_mut::<Files>().script.preview;
                preview.animation_progress = progress;
                preview.changed().unwrap();
            }
            settle(&mut app);
            widgets.begin();
            panel::paint(app.world_mut(), camera, &mut widgets, 432., 600., theme).unwrap();
            widgets.finish(app.world_mut());
            let path = out.join(format!("motion-{progress:02}.png"));
            capture(&mut app, &target, &path);
            motion_samples.push(model_pixels(&app, model, &path));
        }
    } else {
        // Endpoints were already captured for every step. Select a visibly
        // different static pair without rerendering every candidate pair.
        motion_step = std::iter::once(preferred)
            .chain(1..durations.len())
            .find(|step| {
                if *step >= durations.len() || geometry_motion[frame_for_step(*step)] {
                    return false;
                }
                let before =
                    model_pixels(&app, model, &out.join(format!("frame-{}.png", step - 1)));
                let after = model_pixels(&app, model, &out.join(format!("frame-{step}.png")));
                changed_pixels(&before, &after) > 16
            })
            .unwrap_or_else(|| panic!("{id:?} has no visible authored before/after model pair"));
        command(
            app.world_mut(),
            &handle,
            Action::Step(motion_step - 1),
            &ControlInput::Click,
        )
        .unwrap();
        settle(&mut app);
        {
            let preview = &mut app.world_mut().resource_mut::<Files>().script.preview;
            preview.index = motion_step;
            preview.changed().unwrap();
            preview.begin_animation();
        }
        settle(&mut app);
        let preview = &app.world().resource::<Files>().script.preview;
        assert!(
            preview.blend.is_some(),
            "A resident source must blend into its static target"
        );
        let (revision, jobs, key) = (
            preview.revision,
            preview.render_jobs,
            images::Key::current(preview),
        );
        for progress in 0..=blend::STEPS {
            if progress != 0 {
                let preview = &mut app.world_mut().resource_mut::<Files>().script.preview;
                if preview.blend.as_mut().unwrap().advance() {
                    preview.blend = None;
                }
            }
            widgets.begin();
            panel::paint(app.world_mut(), camera, &mut widgets, 432., 600., theme).unwrap();
            widgets.finish(app.world_mut());
            let path = out.join(format!("motion-{progress:02}.png"));
            capture(&mut app, &target, &path);
            motion_samples.push(model_pixels(&app, model, &path));
            let preview = &app.world().resource::<Files>().script.preview;
            assert_eq!((preview.revision, preview.render_jobs), (revision, jobs));
            assert!(images::Key::current(preview) == key);
            assert!(
                preview.images.footprint().0 <= 2
                    && preview.images.footprint().1 <= 2 * 1024 * 1024
            );
        }
    }
    let distinct_samples = visible_motion(&motion_samples, id);
    let mut autoplay_report = Value::Null;
    let static_autoplay = matches!(id.key(), "sketch_line" | "circular_pattern");
    if id == lesson::Id::Sketch || static_autoplay {
        command(
            app.world_mut(),
            &handle,
            Action::Replay,
            &ControlInput::Click,
        )
        .unwrap();
        let begin = Instant::now();
        let jobs = app.world().resource::<Files>().script.preview.render_jobs;
        let mut autoplay_samples = Vec::new();
        let mut autoplay_sample_jobs = None;
        loop {
            update(&mut app);
            poll(app.world_mut());
            widgets.begin();
            panel::paint(app.world_mut(), camera, &mut widgets, 432., 600., theme).unwrap();
            widgets.finish(app.world_mut());
            let preview = &app.world().resource::<Files>().script.preview;
            assert!(preview.error.is_none(), "{:?}", preview.error);
            let model = widgets.entity("scripts-preview-model").unwrap();
            assert!(
                app.world().get::<ImageNode>(model).is_some(),
                "Autoplay must retain its visible model while a replacement is pending"
            );
            assert!(preview.images.footprint().1 <= 2 * 1024 * 1024);
            let playing = preview.playing;
            let stage = if static_autoplay && preview.index == motion_step && preview.image_ready()
            {
                match &preview.blend {
                    Some(blend) if blend.alpha() == 0. && autoplay_samples.is_empty() => {
                        Some("before")
                    }
                    Some(blend) if blend.alpha() > 0. && autoplay_samples.len() == 1 => {
                        Some("intermediate")
                    }
                    None if autoplay_samples.len() == 2 => Some("after"),
                    _ => None,
                }
            } else {
                None
            };
            if let Some(stage) = stage {
                let jobs = app.world().resource::<Files>().script.preview.render_jobs;
                assert_eq!(
                    *autoplay_sample_jobs.get_or_insert(jobs),
                    jobs,
                    "Opacity samples must not enqueue CAD render work"
                );
                let path = out.join(format!("autoplay-static-{stage}.png"));
                capture(&mut app, &target, &path);
                autoplay_samples.push(model_pixels(&app, model, &path));
            }
            if !playing {
                break;
            }
            assert!(
                begin.elapsed() < Duration::from_secs(45),
                "Automatic guide did not complete"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        let preview = &app.world().resource::<Files>().script.preview;
        assert_eq!(preview.index + 1, preview.count());
        assert!(preview.animation_complete());
        assert!(
            preview.render_jobs - jobs <= 3 * (usize::from(ANIMATION_STEPS) + 1) + 2,
            "Static cues must not generate animation samples"
        );
        if static_autoplay {
            assert!(!geometry_motion[frame_for_step(motion_step)]);
            assert_eq!(
                autoplay_samples.len(),
                3,
                "Autoplay needs real before/intermediate/after captures"
            );
            let distinct = visible_motion(&autoplay_samples, id);
            let job_limit: usize = (0..durations.len())
                .map(|step| {
                    if geometry_motion[frame_for_step(step)] {
                        usize::from(ANIMATION_STEPS) + 1
                    } else {
                        1
                    }
                })
                .sum();
            assert!(preview.render_jobs - jobs <= job_limit,
                "Static endpoints render at most once; supported setup geometry keeps its bounded samples");
            autoplay_report = json!({"actual_controller_autoplay":true,"distinct_model_samples":distinct,
                "total_render_jobs":preview.render_jobs-jobs,"render_job_limit":job_limit,
                "static_target_step":motion_step,"opacity_render_jobs":0});
        }
    }
    command(
        app.world_mut(),
        &handle,
        Action::Step(durations.len() - 1),
        &ControlInput::Click,
    )
    .unwrap();
    settle(&mut app);
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
        update(&mut app);
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
    for locale in crate::app_preferences::locale::SUPPORTED {
        crate::native_viewport::localization::set_locale(app.world_mut(), locale);
        app.world_mut()
            .resource_mut::<Files>()
            .script
            .preview
            .help_view = feature::HelpView::default();
        let language = crate::app_preferences::locale::code(locale);
        command(app.world_mut(), &handle, Action::Demo, &ControlInput::Click).unwrap();
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
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            update(&mut app);
            poll(app.world_mut());
            let preview = &app.world().resource::<Files>().script.preview;
            assert!(preview.error.is_none(), "{:?}", preview.error);
            if preview.rendered == preview.revision {
                break;
            }
            assert!(Instant::now() < deadline, "Localized preview timed out");
            std::thread::sleep(Duration::from_millis(2));
        }
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
        capture(
            &mut app,
            &target,
            &out.join(format!("demo-175-{language}.png")),
        );
        // Measure every instruction, not just the short completion caption.
        for index in 0..durations.len() {
            app.world_mut().resource_mut::<Files>().script.preview.index = index;
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
            for _ in 0..3 {
                update(&mut app);
            }
            let entity = widgets.entity("scripts-preview-caption").unwrap();
            let node = app.world().get::<ComputedNode>(entity).unwrap();
            let text = app
                .world()
                .get::<bevy::text::TextLayoutInfo>(entity)
                .unwrap();
            assert!(!text.glyphs.is_empty());
            assert!(
                text.size.y <= node.size.y + 1.,
                "Instruction clipped: {language}/{index}: {} > {}",
                text.size.y,
                node.size.y
            );
        }
        app.world_mut().resource_mut::<Files>().script.preview.index = durations.len() - 1;
        command(
            app.world_mut(),
            &handle,
            Action::Steps,
            &ControlInput::Click,
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
        capture(
            &mut app,
            &target,
            &out.join(format!("steps-175-{language}.png")),
        );
        command(app.world_mut(), &handle, Action::Help, &ControlInput::Click).unwrap();
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
        capture(
            &mut app,
            &target,
            &out.join(format!("help-175-{language}-top.png")),
        );
        let clip = widgets.entity("extrude-help-body").unwrap();
        assert!(
            widgets.entity("lesson-play").is_none()
                && widgets.entity("extrude-help-back").is_none(),
            "Instructions must not expose duplicate guide destinations"
        );
        assert!(
            widgets.entity("scripts-preview-model").is_some(),
            "Instructions retain the model"
        );
        let back = widgets.entity("scripts-preview-primary").unwrap();
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
            clip_center.x + clip_node.size.x / 2. <= back_center.x - back_node.size.x / 2.
                || clip_center.x - clip_node.size.x / 2. >= back_center.x + back_node.size.x / 2.
                || clip_center.y + clip_node.size.y / 2. <= back_center.y - back_node.size.y / 2.
                || clip_center.y - clip_node.size.y / 2. >= back_center.y + back_node.size.y / 2.,
            "{language}: Instructions and playback controls must not overlap"
        );
        let text = widgets.entity("extrude-help-text").unwrap();
        let actual_clip = app
            .world()
            .get::<bevy::ui::CalculatedClip>(text)
            .expect("Help text must inherit a real clipping region")
            .rects()
            .expect("Help is visible")
            .iter()
            .map(|clip| {
                clip.world_to_clip_local
                    .inverse()
                    .transform_point2(clip.rect.max)
                    .y
            })
            .fold(f32::INFINITY, f32::min);
        assert!(actual_clip <= clip_center.y + clip_node.size.y / 2. + 1.);
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
        capture(
            &mut app,
            &target,
            &out.join(format!("help-175-{language}-bottom.png")),
        );
        let text_node = app.world().get::<ComputedNode>(text).unwrap();
        let text_center = app
            .world()
            .get::<bevy::ui::UiGlobalTransform>(text)
            .unwrap()
            .translation;
        assert!(
            text_center.y + text_node.size.y / 2. <= actual_clip + 1.,
            "{language}: End reveals the final instruction"
        );
    }
    let mut idle_ns = vec![];
    for open in [true, false] {
        if !open {
            close(app.world_mut());
            released_textures(&mut app);
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
    let report = json!({"lesson":id,"scope":"windowless production native UI; debug test profile; controller timings, not whole-app CPU/GPU utilization", "prepare_ms":prepare_ms,"cold_frame_ms":cold_ms,"cold_gpu_jobs":jobs,"warm_replay_ms":warm_ms,"warm_gpu_jobs":warm_jobs,"cached_adjacent_gpu_jobs":0,"cached_frames":footprint.0,"cached_gpu_texture_bytes":footprint.1,"texture_budget_bytes":2*1024*1024,"paused_poll_ns":idle_ns[0],"closed_poll_ns":idle_ns[1],"idle_poll_samples":100_000,"idle_preview_wakes":0,"closed_cached_bytes":0,"durations_ms":durations});
    let mut report = report;
    report["controller_first_open_ms"] = json!(first_open_ms);
    report["memory_before_controller_open"] = memory_before;
    report["lifecycle"] = lifecycle(&mut app, &handle, id);
    report["scenario"] = json!(scenario);
    report["scenario_cached_frames"] = json!(scenario_cached_frames);
    report["scenario_gpu_jobs"] = json!(scenario_jobs);
    report["motion"] = json!({"kind":if geometry_step.is_some() {"geometry"} else {"cached_image_dissolve"},
        "step":motion_step,"distinct_model_samples":distinct_samples,"dissolve_ui_steps":blend::STEPS,
        "dissolve_duration_ms":ANIMATION_INTERVAL.as_millis()*u128::from(blend::STEPS)});
    report["static_autoplay"] = autoplay_report;
    std::fs::write(
        out.join("performance.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    println!("{report}");
}
