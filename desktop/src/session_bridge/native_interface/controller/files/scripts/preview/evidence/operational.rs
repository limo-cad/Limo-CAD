//! Finite operational component recording through the production controller.
//! These are native widget checkpoint captures, not OS pointer/video evidence.
use super::*;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

const BASE: &str = "1624ecc686c3711d8a286e60c0b7e4da6ea70af1";
const MAX_FRAMES: usize = 12;
const MAX_PNG_BYTES: u64 = 2 * 1024 * 1024;
const MAX_RECORDING_BYTES: u64 = 16 * 1024 * 1024;
const RECORDING_LIFETIME: Duration = Duration::from_secs(90);
const PAUSE_WAIT: Duration = Duration::from_millis(650);
const RIBBON_TEST: &str = "session_bridge::native_interface::controller::workbench::ribbon_menu::localization_tests::native_script_preview_tabs_after_extrude_in_the_production_ribbon";

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
#[ignore = "requires native graphics; records two finite operational component clips"]
fn native_show_me_ui_operational_component_recordings() {
    let root = std::env::var("LIMO_CAD_SHOW_ME_PROOF_DIR")
        .expect("Choose a fresh LIMO_CAD_SHOW_ME_PROOF_DIR");
    // Deliberately independent of ALL: this is two representative recordings,
    // not another catalog/localization qualification sweep.
    for id in [lesson::Id::Extrude, lesson::Id::CircularPattern] {
        capture_lesson_mode(
            id,
            lesson::CutScenario::ThroughAll,
            &Path::new(&root).join("operational").join(id.key()),
            false,
            true,
        );
    }
}

struct Recording<'a> {
    app: &'a mut App,
    handle: &'a NativeInterfaceHandle,
    camera: Entity,
    widgets: &'a mut chrome::Widgets,
    target: &'a Handle<Image>,
    id: lesson::Id,
    out: &'a Path,
    wakes: &'a AtomicUsize,
    started: Instant,
    events: Vec<Value>,
    frames: Vec<Value>,
    png_bytes: u64,
    next_order: usize,
}

impl Recording<'_> {
    fn elapsed_ms(&self) -> u64 {
        self.started.elapsed().as_millis().try_into().unwrap()
    }

    fn state(&self) -> Value {
        let p = &self.app.world().resource::<Files>().script.preview;
        let footprint = p.images.footprint();
        json!({"open":p.open,"playing":p.playing,"instructions":p.help,
            "step":p.index,"displayed_step":p.display_index(),"frame":p.frame_index(),
            "geometry_sample":p.animation_progress,"geometry_samples":ANIMATION_STEPS,
            "blend_alpha":p.blend.as_ref().map(|blend| blend.alpha()),
            "revision":p.revision,"rendered_revision":p.rendered,"attempted_revision":p.attempted,
            "render_jobs":p.render_jobs,"wakes":self.wakes.load(Ordering::Relaxed),
            "wake_armed":p.wake.is_some(),"build_pending":p.build.is_some(),"render_pending":p.render.is_some(),
            "cached_textures":footprint.0,"cached_texture_bytes":footprint.1,
            "retained_textures":crate::native_viewport::script_preview::textures::retained_count(self.app.world()),
            "imported_textures":crate::native_viewport::script_preview::textures::imported_count(
                self.app.sub_app(bevy::render::RenderApp).world()),
            "preparation_cache_metrics":p.prepared.lock().unwrap().metrics(),"error":p.error})
    }

    fn event(&mut self, action: &str, details: Value) {
        let order = self.next_order;
        self.next_order += 1;
        self.events
            .push(json!({"order":order,"at_ms":self.elapsed_ms(),
            "action":action,"details":details,"state":self.state()}));
    }

    fn pump(&mut self) {
        assert!(
            self.started.elapsed() < RECORDING_LIFETIME,
            "Operational recording timed out"
        );
        update(self.app);
        // This is the actual production controller. No sample, step, playing,
        // revision or timer state is assigned by this recording module.
        poll(self.app.world_mut());
        if self.app.world().resource::<Files>().script.preview.open {
            let theme = crate::native_viewport::ui::theme(self.app.world());
            self.widgets.begin();
            panel::paint(
                self.app.world_mut(),
                self.camera,
                self.widgets,
                432.,
                600.,
                theme,
            )
            .unwrap();
            self.widgets.finish(self.app.world_mut());
        }
        update(self.app);
        let p = &self.app.world().resource::<Files>().script.preview;
        assert!(p.error.is_none(), "{:?}", p.error);
        assert!(p.images.footprint().0 <= 2 && p.images.footprint().1 <= 2 * 1024 * 1024);
        assert!(
            crate::native_viewport::script_preview::textures::retained_count(self.app.world()) <= 2
        );
        assert!(
            crate::native_viewport::script_preview::textures::imported_count(
                self.app.sub_app(bevy::render::RenderApp).world()
            ) <= 2
        );
    }

    fn wait(&mut self, phase: &str, ready: impl Fn(&State) -> bool) {
        loop {
            if ready(&self.app.world().resource::<Files>().script.preview) {
                break;
            }
            self.pump();
            assert!(
                self.started.elapsed() < RECORDING_LIFETIME,
                "Waiting for {phase}"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        self.event(phase, Value::Null);
    }

    fn activate(&mut self, key: &str, expected: Action) {
        let entity = self
            .widgets
            .entity(key)
            .unwrap_or_else(|| panic!("Missing displayed control {key}"));
        let control = self.app.world().get::<InterfaceControl>(entity).unwrap();
        assert!(
            control.visible && !control.disabled,
            "Displayed control must be enabled: {key}"
        );
        assert_eq!(
            self.app
                .world()
                .get::<NativeCommandBinding>(entity)
                .unwrap()
                .command,
            NativeCommand::File(FileCommand::ScriptPreview(expected))
        );
        let action = self
            .handle
            .resolve_retained(limo_cad_interface::ControlKey(entity.to_bits()))
            .unwrap();
        self.handle.prepare_activation(&action).unwrap();
        self.event(
            "activate_control",
            json!({"widget":key,"command":format!("{expected:?}"),
            "input":"component retained-control activation; no physical pointer claimed"}),
        );
        let p = &self.app.world().resource::<Files>().script.preview;
        let (revision, jobs, render_key) = (p.revision, p.render_jobs, images::Key::current(p));
        command(
            self.app.world_mut(),
            self.handle,
            expected,
            &ControlInput::Click,
        )
        .unwrap();
        if matches!(
            expected,
            Action::Stop | Action::Help | Action::Demo | Action::Steps
        ) {
            let p = &self.app.world().resource::<Files>().script.preview;
            assert!(p.blend.is_none() && p.wake.is_none() && !p.playing);
            assert_eq!((p.revision, p.render_jobs), (revision, jobs));
            assert!(images::Key::current(p) == render_key);
            self.event("read_control_kept_current_render", json!({"command":format!("{expected:?}"),
                "blend_cleared":true,"revision_delta":0,"render_job_delta":0,"render_key_unchanged":true}));
        }
        self.pump();
    }

    fn assert_current_presentation(&self) {
        let p = &self.app.world().resource::<Files>().script.preview;
        assert!(p.image_ready() && p.blend.is_none());
        assert_eq!(p.display_index(), p.index);
        assert_eq!(p.display_frame_index(), p.frame_index());
        let model = self.widgets.entity("scripts-preview-model").unwrap();
        assert_eq!(
            self.app.world().get::<ImageNode>(model).unwrap().image,
            p.image.clone().unwrap()
        );
        assert!(self.widgets.entity("scripts-preview-blend").is_none());
        let caption = self.widgets.entity("scripts-preview-caption").unwrap();
        assert_eq!(
            self.app.world().get::<Text>(caption).unwrap().0,
            limo_cad_mcp::lessons::caption(
                lesson::language(self.app.world()),
                &p.retained.as_ref().unwrap().descriptor.captions[p.frame_index()]
            )
        );
    }

    fn capture(&mut self, label: &str, card_visible: bool) -> PathBuf {
        assert!(
            self.frames.len() < MAX_FRAMES,
            "Operational frame cap exceeded"
        );
        let file = format!("{:02}-{label}.png", self.frames.len());
        let path = self.out.join(&file);
        assert!(
            !path.exists(),
            "Retain prior capture; choose a fresh output directory"
        );
        let before = self.state();
        let begin = self.elapsed_ms();
        let order = self.next_order;
        self.next_order += 1;
        capture_component(self.app, self.target, &path, card_visible);
        let after = self.state();
        // Capturing the already presented checkpoint must not drive playback.
        for field in [
            "step",
            "geometry_sample",
            "blend_alpha",
            "revision",
            "render_jobs",
            "playing",
            "instructions",
        ] {
            assert_eq!(
                before[field], after[field],
                "Screenshot changed controller {field}"
            );
        }
        let bytes = std::fs::read(&path).unwrap();
        assert!(
            bytes.len() as u64 <= MAX_PNG_BYTES,
            "Operational PNG exceeds 2MiB"
        );
        self.png_bytes += bytes.len() as u64;
        assert!(
            self.png_bytes <= MAX_RECORDING_BYTES,
            "Operational clip exceeds 16MiB"
        );
        self.frames.push(json!({"order":order,"file":file,"label":label,
            "capture_start_ms":begin,"capture_end_ms":self.elapsed_ms(),"sha256":sha256(&bytes),
            "bytes":bytes.len(),"card_visible":card_visible,"state_before":before,"state_after":after}));
        path
    }

    fn model(&self, path: &Path) -> Vec<u8> {
        model_pixels(
            self.app,
            self.widgets.entity("scripts-preview-model").unwrap(),
            path,
        )
    }

    fn quiet_wait(&mut self, duration: Duration) {
        let baseline = self.state();
        assert_eq!(baseline["wake_armed"], false);
        assert_eq!(baseline["build_pending"], false);
        assert_eq!(baseline["render_pending"], false);
        assert_eq!(baseline["playing"], false);
        let began = Instant::now();
        let mut samples = 0;
        while began.elapsed() < duration {
            self.pump();
            assert_eq!(
                self.state(),
                baseline,
                "Paused/closed controller submitted work, woke or changed state"
            );
            samples += 1;
            std::thread::sleep(Duration::from_millis(20));
        }
        self.event(
            "quiet_wait_verified",
            json!({"requested_ms":duration.as_millis(),
            "observed_ms":began.elapsed().as_millis(),"samples":samples,
            "wake_delta":0,"render_job_delta":0,"step_delta":0}),
        );
    }

    fn finish(mut self, source_revision: String) {
        let compiled = limo_cad_build_info::build_info();
        assert_eq!(compiled.revision, source_revision);
        let finished = self.elapsed_ms();
        assert!(self.started.elapsed() < RECORDING_LIFETIME);
        for i in 0..self.frames.len() {
            let next = self.frames.get(i + 1).map_or(finished, |frame| {
                frame["capture_start_ms"].as_u64().unwrap()
            });
            let start = self.frames[i]["capture_start_ms"].as_u64().unwrap();
            self.frames[i]["delay_to_next_ms"] = json!(next.saturating_sub(start));
        }
        let source = &catalog::examples()
            .iter()
            .find(|example| example.id == self.id.recipe())
            .unwrap()
            .source;
        let asset: &[u8] = match self.id {
            lesson::Id::Extrude => include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/assets/show-me/extrude.cbor"
            )),
            lesson::Id::CircularPattern => include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/assets/show-me/circular_pattern.cbor"
            )),
            _ => unreachable!("finite representative recording"),
        };
        let report = json!({"version":1,"lesson":self.id,"implementation_base":BASE,
            "executed_source_revision":source_revision,"revision_attestation":"compiled build identity; runtime revision values checked",
            "compiled_modified":compiled.modified,"compiled_version":compiled.version,"compiled_channel":compiled.channel,
            "recipe_source_sha256_lf":sha256(source.replace("\r\n", "\n").as_bytes()),"embedded_asset_sha256":sha256(asset),
            "crate_version":env!("CARGO_PKG_VERSION"),"debug_assertions":cfg!(debug_assertions),
            "locale":lesson::language(self.app.world()),
            "os":std::env::consts::OS,"arch":std::env::consts::ARCH,
            "scope":"windowless production native widgets and actual controller; checkpoint PNGs, not continuous OS video",
            "os_pointer_recording":false,"simulated_pointer_overlay":false,"manual_animation_or_step_assignment":false,
            "operational_sequence_complete":true,"elapsed_ms":finished,"frames":self.frames,"actions":self.events,
            "png_bytes":self.png_bytes,"limits":{"frames":MAX_FRAMES,"per_png_bytes":MAX_PNG_BYTES,
                "recording_png_bytes":MAX_RECORDING_BYTES,"lifetime_seconds":RECORDING_LIFETIME.as_secs()},
            "gif_assembly":{"frame_order":"frames.order","timing":"delay_to_next_ms is the actual checkpoint capture interval",
                "pixels":"use the unchanged native PNGs; no pointer overlays","gif_encoded_by_this_test":false},
            "keyboard_companion":{"required_test_filter":RIBBON_TEST,"executed_by_this_recording":false,
                "scope":"existing production ribbon synchronization: Extrude/Show me/Cut/Revolve neighbors and Fillet reverse navigation; no full-ribbon traversal claim"},
            "final_state":self.state()});
        let bytes = serde_json::to_vec_pretty(&report).unwrap();
        assert!(bytes.len() <= 128 * 1024);
        std::fs::write(self.out.join("recording.json"), bytes).unwrap();
    }
}

// Read the actual published geometry/dissolve phase; never manufacture a sample.
fn motion_progress(p: &State) -> Option<f32> {
    if !p.image_ready() || p.index != 2 {
        return None;
    }
    p.blend.as_ref().map(|blend| blend.alpha()).or_else(|| {
        (!p.geometry_animation_complete())
            .then(|| f32::from(p.animation_progress) / f32::from(ANIMATION_STEPS))
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn record(
    app: &mut App,
    handle: &NativeInterfaceHandle,
    camera: Entity,
    widgets: &mut chrome::Widgets,
    target: &Handle<Image>,
    id: lesson::Id,
    out: &Path,
    wakes: &AtomicUsize,
) {
    assert_eq!(
        std::fs::read_dir(out).unwrap().count(),
        0,
        "Choose a fresh operational output directory"
    );
    let source_revision = limo_cad_build_info::build_info().revision.to_owned();
    assert!(source_revision.len() == 40 && source_revision.bytes().all(|b| b.is_ascii_hexdigit()));
    for variable in ["LIMO_CAD_BUILD_REVISION", "GITHUB_SHA"] {
        if let Ok(declared) = std::env::var(variable) {
            assert_eq!(
                declared, source_revision,
                "{variable} conflicts with the compiled source revision"
            );
        }
    }
    let mut r = Recording {
        app,
        handle,
        camera,
        widgets,
        target,
        id,
        out,
        wakes,
        started: Instant::now(),
        events: Vec::new(),
        frames: Vec::new(),
        png_bytes: 0,
        next_order: 0,
    };
    r.event("guidance_entry_presented", json!({"keyboard_scope":"component anchor only; use the separate real ribbon companion test"}));
    r.capture("guidance-entry", false);
    r.activate(&format!("{}-show-me", id.key()), Action::Lesson(id));
    r.wait("automatic_opening_ready", |p| {
        p.playing && p.image_ready() && p.index == 0
    });
    r.capture("autoplay-opening", true);
    r.wait("automatic_first_step_complete", |p| {
        p.index == 1 && p.image_ready() && p.animation_complete()
    });
    r.capture("autoplay-step-1", true);
    r.wait("automatic_target_before", |p| {
        motion_progress(p) == Some(0.)
    });
    let p = &r.app.world().resource::<Files>().script.preview;
    if id == lesson::Id::CircularPattern {
        assert!(
            p.blend.is_some(),
            "Circular Pattern must prove the static-frame dissolve path"
        );
    }
    let dissolve = p
        .blend
        .as_ref()
        .map(|_| (p.revision, p.render_jobs, images::Key::current(p)));
    let path = r.capture("autoplay-before", true);
    let before = r.model(&path);
    r.wait("automatic_visible_intermediate", |p| {
        motion_progress(p).is_some_and(|v| (0.25..=0.75).contains(&v))
    });
    let path = r.capture("autoplay-intermediate", true);
    let intermediate = r.model(&path);
    assert!(changed_pixels(&before, &intermediate) > 16);
    if let Some((revision, jobs, key)) = dissolve {
        let p = &r.app.world().resource::<Files>().script.preview;
        assert_eq!((p.revision, p.render_jobs), (revision, jobs));
        assert!(images::Key::current(p) == key);
        r.event(
            "visible_dissolve_without_3d_work",
            json!({"revision_delta":0,"render_job_delta":0,
            "render_key_unchanged":true}),
        );
    }
    r.activate("scripts-preview-primary", Action::Stop);
    r.assert_current_presentation();
    let paused = r.capture("paused", true);
    let paused_pixels = r.model(&paused);
    r.quiet_wait(PAUSE_WAIT);
    let path = r.capture("paused-after-wait", true);
    assert_eq!(
        r.model(&path),
        paused_pixels,
        "Pause must freeze native model pixels"
    );
    r.activate("extrude-open-help", Action::Help);
    r.wait("inline_instructions_ready", |p| {
        p.help && !p.playing && p.image_ready()
    });
    r.assert_current_presentation();
    assert!(r.widgets.entity("extrude-help-body").is_some());
    r.capture("inline-instructions", true);
    r.quiet_wait(PAUSE_WAIT);
    r.activate("scripts-preview-primary", Action::Resume);
    r.wait("automatic_playback_resumed", |p| {
        p.playing && !p.help && p.image_ready()
    });
    r.capture("resumed-autoplay", true);
    r.wait("automatic_target_complete", |p| {
        p.index == 2 && p.image_ready() && p.animation_complete()
    });
    let path = r.capture("autoplay-after", true);
    let after = r.model(&path);
    visible_motion(&[before, intermediate, after], id);
    r.wait("automatic_guide_completed", |p| {
        !p.playing && p.index + 1 == p.count() && p.image_ready() && p.animation_complete()
    });
    r.capture("autoplay-complete", true);
    r.activate("show-me-demo-close", Action::Close);
    // Restore the same native guidance entry after Close. Keeping its camera
    // active clears the card instead of retaining an inactive render target.
    r.widgets.begin();
    let anchor = paint_anchor(r.app.world_mut(), r.camera, r.widgets, id);
    r.widgets.finish(r.app.world_mut());
    for _ in 0..8 {
        update(r.app);
    }
    let action = r
        .handle
        .resolve_retained(limo_cad_interface::ControlKey(anchor.to_bits()))
        .unwrap();
    r.handle.prepare_activation(&action).unwrap();
    r.widgets.begin();
    paint_anchor(r.app.world_mut(), r.camera, r.widgets, id);
    feature::paint_hint(r.app.world_mut(), r.camera, None, r.widgets, 432.).unwrap();
    r.widgets.finish(r.app.world_mut());
    released_textures(r.app);
    assert!(r.widgets.entity("scripts-preview-model").is_none());
    assert!(r.widgets.entity("show-me-demo-card").is_none());
    let closed = r.state();
    for field in [
        "cached_textures",
        "cached_texture_bytes",
        "retained_textures",
        "imported_textures",
    ] {
        assert_eq!(closed[field], 0, "Close retained {field}");
    }
    assert_eq!(closed["open"], false);
    r.event(
        "closed_guidance_restored",
        json!({"native_guidance_entry":true}),
    );
    r.capture("closed-textures-released", false);
    r.quiet_wait(Duration::from_millis(150));
    r.finish(source_revision);
}
