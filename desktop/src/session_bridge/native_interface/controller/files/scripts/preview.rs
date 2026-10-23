//! One bounded view of the shared, isolated teaching-preview service.
//! Neither preparing nor rendering a preview receives the live CAD document.
use super::*;
use crate::native_viewport::script_preview::{
    Frame, PendingRender, PreviewDescriptor, PreviewService, RenderRequest, RenderedTexture,
    MAX_RENDER_SIZE,
};
#[cfg(test)]
use bevy::{
    asset::RenderAssetUsages,
    image::{CompressedImageFormats, ImageSampler, ImageType},
};
use std::time::{Duration, Instant};

mod annotations;
mod blend;
mod card;
mod feature;
mod hover;
mod images;
mod input;
mod job;
mod lesson;
mod navigation;
mod panel;
mod prepared;
mod scope;
mod steps;
#[cfg(test)]
pub(crate) use feature::paint_hint;
pub(crate) use feature::paint_workbench_hint;
pub(crate) use input::input;
pub(crate) use panel::paint;

// Match the README concept: broad upright, with the base toward the lower left.
const HOME: (f32, f32) = (
    320. * std::f32::consts::PI / 180.,
    32. * std::f32::consts::PI / 180.,
);
const FRAME_DELAY: Duration = Duration::from_millis(1800);
const ANIMATION_STEPS: u8 = 20;
const ANIMATION_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Open,
    Extrude,
    Lesson(limo_cad_mcp::lessons::Id),
    Scenario(limo_cad_mcp::lessons::CutScenario),
    Read(limo_cad_mcp::lessons::Id),
    Index,
    IndexPage(bool),
    Back,
    FullBuild,
    Help,
    HelpBody,
    Demo,
    Close,
    Model,
    Previous,
    Next,
    Step(usize),
    Steps,
    StepsPage(bool),
    Replay,
    Resume,
    Stop,
    Fit,
    Left,
    Right,
    Up,
    Down,
}

struct Retained {
    service: Arc<PreviewService>,
    descriptor: PreviewDescriptor,
    view: String,
    geometry_motion: Vec<bool>,
}
impl Retained {
    fn new(service: Arc<PreviewService>, frames: Vec<Frame>) -> Result<Self, String> {
        let geometry_motion = blend::geometry_motion(&frames);
        let descriptor = service.retain(frames)?;
        match service.open_view() {
            Ok(view) => Ok(Self {
                service,
                descriptor,
                view,
                geometry_motion,
            }),
            Err(error) => {
                service.release(&descriptor.preview_id);
                Err(error)
            }
        }
    }
}
impl Drop for Retained {
    fn drop(&mut self) {
        self.service.close_view(&self.view);
        self.service.release(&self.descriptor.preview_id);
    }
}
struct Rendered {
    view: String,
    revision: u64,
    image: images::FrameImage,
}
enum InFlight {
    Gpu {
        view: String,
        revision: u64,
        pending: Mutex<PendingRender>,
    },
    #[cfg(test)]
    Fixture(ResultChannel<Rendered>),
}
impl InFlight {
    fn receive(&self) -> Option<Result<Rendered, String>> {
        match self {
            Self::Gpu {
                view,
                revision,
                pending,
            } => {
                let result = pending
                    .lock()
                    .map_err(|_| "Preview response lock poisoned".to_owned())
                    .and_then(|pending| pending.try_recv());
                match result {
                    Ok(Some(texture)) => Some(Ok(Rendered {
                        view: view.clone(),
                        revision: *revision,
                        image: images::FrameImage::Gpu(texture),
                    })),
                    Ok(None) => None,
                    Err(error) => Some(Err(error)),
                }
            }
            #[cfg(test)]
            Self::Fixture(channel) => Some(match channel.lock() {
                Ok(channel) => match channel.try_recv() {
                    Ok(value) => value,
                    Err(mpsc::TryRecvError::Empty) => return None,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Err("Preview fixture disconnected".into())
                    }
                },
                Err(_) => Err("Preview fixture lock poisoned".into()),
            }),
        }
    }
}
type ResultChannel<T> = Mutex<mpsc::Receiver<Result<T, String>>>;
struct Wake {
    due: Instant,
    cancelled: Arc<AtomicBool>,
}
impl Drop for Wake {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

pub(crate) struct State {
    pub open: bool,
    generation: u64,
    build: Option<ResultChannel<Vec<Frame>>>,
    preparation: Option<job::Preparation>,
    project: bool,
    render: Option<InFlight>,
    render_revision: u64,
    render_started: Option<Instant>,
    #[cfg(test)]
    render_jobs: usize,
    service: Arc<PreviewService>,
    retained: Option<Retained>,
    prepared: Arc<Mutex<prepared::Cache>>,
    index: usize,
    steps_open: bool,
    steps_page: Option<usize>,
    steps_return_focus: bool,
    yaw: f32,
    pitch: f32,
    revision: u64,
    rendered: u64,
    attempted: u64,
    size: [u32; 2],
    image: Option<Handle<Image>>,
    images: images::Cache,
    palette: crate::native_viewport::ViewportPalette,
    image_index: Option<usize>,
    image_frame: Option<usize>,
    animation_progress: u8,
    blend: Option<blend::Blend<Handle<Image>>>,
    error: Option<String>,
    playing: bool,
    wake: Option<Wake>,
    drag: Option<input::Drag>,
    return_to: Option<(bool, bool, bool, bool)>,
    help: bool,
    help_view: feature::HelpView,
    lesson: Option<lesson::Id>,
    scenario: lesson::CutScenario,
    index_open: bool,
    index_page: usize,
    index_focus: Option<lesson::Id>,
    history: navigation::History<lesson::Id, lesson::CutScenario>,
    restore: Option<navigation::Lesson<lesson::Id, lesson::CutScenario>>,
    transition: Option<Retained>,
    focus_owner: Option<DocumentContext>,
    focus_from_menu: Option<&'static str>,
}
pub(in super::super) fn cancel_pointer(world: &mut World) {
    if let Some(mut files) = world.get_resource_mut::<Files>() {
        files.script.preview.drag = None;
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            open: false,
            generation: 0,
            build: None,
            preparation: None,
            project: false,
            render: None,
            render_revision: 0,
            render_started: None,
            #[cfg(test)]
            render_jobs: 0,
            service: Arc::new(PreviewService::default()),
            retained: None,
            prepared: Arc::new(Mutex::new(prepared::Cache::default())),
            index: 0,
            steps_open: false,
            steps_page: None,
            steps_return_focus: false,
            yaw: HOME.0,
            pitch: HOME.1,
            revision: 1,
            rendered: 0,
            attempted: 0,
            size: [300, 176],
            image: None,
            images: images::Cache::default(),
            palette: crate::native_viewport::ViewportPalette::default(),
            image_index: None,
            image_frame: None,
            animation_progress: ANIMATION_STEPS,
            blend: None,
            error: None,
            playing: false,
            wake: None,
            drag: None,
            return_to: None,
            help: false,
            help_view: feature::HelpView::default(),
            lesson: None,
            scenario: lesson::CutScenario::default(),
            index_open: false,
            index_page: 0,
            index_focus: None,
            history: navigation::History::default(),
            restore: None,
            transition: None,
            focus_owner: None,
            focus_from_menu: None,
        }
    }
}
impl State {
    fn location(&self) -> Option<navigation::Location<lesson::Id, lesson::CutScenario>> {
        if !self.open {
            return None;
        }
        if self.index_open {
            return Some(navigation::Location::Index {
                page: self.index_page,
                focus: self.index_focus,
            });
        }
        self.lesson.map(|id| {
            navigation::Location::Lesson(navigation::Lesson {
                id,
                step: self.index,
                scenario: self.scenario,
                instructions: self.help,
                scroll: self.help_view.offset,
                steps_open: self.steps_open,
                steps_page: self.steps_page,
                yaw: self.yaw,
                pitch: self.pitch,
            })
        })
    }
    fn restore_location(&mut self, location: navigation::Lesson<lesson::Id, lesson::CutScenario>) {
        self.index = location.step;
        self.scenario = location.scenario;
        self.help = location.instructions;
        self.help_view.offset = location.scroll;
        self.steps_open = location.steps_open;
        self.steps_page = location.steps_page;
        self.yaw = location.yaw;
        self.pitch = location.pitch;
        // Back returns to a readable, paused step, never a stale partial dissolve.
        self.animation_progress = ANIMATION_STEPS;
        self.pause();
    }
    fn pause(&mut self) {
        self.playing = false;
        self.blend = None;
        self.wake = None;
        self.render_started = None;
    }
    fn image_ready(&self) -> bool {
        self.image.is_some()
            && self.image_index == Some(self.index)
            && self.rendered == self.revision
    }
    fn display_index(&self) -> usize {
        self.image_index
            .filter(|_| self.image.is_some())
            .unwrap_or(self.index)
    }
    fn display_frame_index(&self) -> usize {
        self.image_frame
            .filter(|_| self.image.is_some())
            .unwrap_or_else(|| self.frame_index())
    }
    fn geometry_animation_complete(&self) -> bool {
        self.animation_progress == ANIMATION_STEPS
    }
    fn animation_complete(&self) -> bool {
        self.geometry_animation_complete() && self.blend.is_none()
    }
    fn begin_animation(&mut self) {
        let moving = self
            .retained
            .as_ref()
            .and_then(|r| r.geometry_motion.get(self.frame_index()))
            .copied()
            .unwrap_or(false);
        self.animation_progress = if moving { 0 } else { ANIMATION_STEPS };
        self.blend = None;
        if !moving && self.image_frame != Some(self.frame_index()) {
            let key = images::Key::current(self);
            if let Some(from) = self
                .image
                .as_ref()
                .and_then(|image| self.images.promote(image, key))
            {
                self.blend = Some(blend::Blend::new(from, self.frame_index()));
            }
        }
    }
    fn validate_blend(&mut self, target: &Handle<Image>) {
        let key = images::Key::current(self);
        let valid = self.blend.as_ref().is_some_and(|blend| {
            blend.target == self.frame_index()
                && blend.from != *target
                && self.images.get_handle(&blend.from, key).is_some()
        });
        if !valid {
            self.blend = None;
        }
    }
    pub fn contextual(&self) -> bool {
        self.return_to.is_some()
    }
    pub fn building(&self) -> bool {
        self.build.is_some()
    }
    fn assembly_notices(&self) -> (usize, bool) {
        self.retained.as_ref().map_or((0, false), |retained| {
            let counts = &retained.descriptor.assembly_notice_counts;
            (
                counts.get(self.display_frame_index()).copied().unwrap_or(0),
                counts.iter().any(|count| *count > 0),
            )
        })
    }
    fn count(&self) -> usize {
        if self.cut_choices() {
            return 4;
        }
        self.retained
            .as_ref()
            .map_or(0, |r| r.descriptor.captions.len())
    }
    fn cut_choices(&self) -> bool {
        self.lesson == Some(lesson::Id::Cut)
            && self
                .retained
                .as_ref()
                .is_some_and(|r| r.descriptor.captions.len() == 6)
    }
    fn frame_index(&self) -> usize {
        if self.cut_choices() {
            self.scenario.frame(self.index)
        } else {
            self.index
        }
    }
    fn changed(&mut self) -> Result<(), String> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or("Preview view sequence exhausted")?;
        // Retiring the receipt cancels obsolete work at the worker's next safe
        // boundary; navigation must not wait for that request's deadline.
        self.render = None;
        self.render_started = None;
        self.blend = None;
        self.error = None;
        self.wake = None;
        Ok(())
    }
    fn turn(&mut self, yaw: f32, pitch: f32) -> Result<(), String> {
        if !yaw.is_finite() || !pitch.is_finite() {
            return Err("Invalid preview camera".into());
        }
        self.pause();
        self.yaw = yaw.rem_euclid(std::f32::consts::TAU);
        self.pitch = pitch.clamp(-1.3, 1.3);
        self.changed()
    }
    fn accepts(&self, result: &Rendered) -> bool {
        self.open
            && self.revision == result.revision
            && self
                .retained
                .as_ref()
                .is_some_and(|r| r.view == result.view)
    }
}

pub(super) fn offered(state: &super::State) -> bool {
    state
        .loaded
        .as_ref()
        .is_some_and(|loaded| loaded.artifact.is_project())
        || state.example.is_some_and(|example| example.preview)
}
pub(super) fn can_open(state: &super::State) -> bool {
    job::source(state).is_ok()
}

fn eligible(state: &super::State) -> Result<&'static catalog::Example, String> {
    let example = state
        .example
        .filter(|example| example.preview)
        .ok_or("This example has no miniature preview; Run in new design to inspect it")?;
    if state.source != example.source || state.library.pending().is_some() {
        return Err("Miniature previews show the original bundled lesson. Save edited source and Run in new design to inspect changes".into());
    }
    Ok(example)
}

#[cfg(test)]
fn prepare(source: &str) -> Result<Vec<Frame>, String> {
    prepare_lesson(source, None)
}
fn prepare_lesson(source: &str, lesson: Option<lesson::Id>) -> Result<Vec<Frame>, String> {
    let mut report = limo_cad_mcp::preview_script(source)?;
    let frames = match lesson {
        Some(lesson) => report["exports"]["lessons"][lesson.key()].take(),
        None => report["exports"]["preview_frames"].take(),
    };
    let mut frames: Vec<Frame> = serde_json::from_value(frames)
        .map_err(|error| format!("Preview frames are unavailable: {error}"))?;
    if lesson == Some(lesson::Id::Cut) {
        let measured: Vec<Frame> =
            serde_json::from_value(report["exports"]["lessons"]["cut_measured"].take())
                .map_err(|error| format!("Measured cut frames are unavailable: {error}"))?;
        if frames.len() != 4 || measured.len() != 4 {
            return Err("Cut needs two four-frame scenarios".into());
        }
        frames.extend(measured.into_iter().skip(2));
    }
    for frame in &mut frames {
        if let Some(profile) = &mut frame.profile {
            // Face sketches carry passive snapping aids. The isolated viewer
            // draws authored entities against the owned solid, without snapping.
            profile.projected_edges.clear();
            profile.reference_midpoints.clear();
        }
    }
    Ok(frames)
}

fn open(world: &mut World, handle: &NativeInterfaceHandle) -> Result<(), String> {
    available(world)?;
    editor::source_ready(world)?;
    let state = &world.resource::<Files>().script;
    let source = job::source(state)?;
    let generation = state.generation;
    start_job(world, handle, source, generation)
}

fn open_extrude(world: &mut World, handle: &NativeInterfaceHandle) -> Result<(), String> {
    open_lesson(world, handle, Some(lesson::Id::Extrude), false)
}
fn open_lesson(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    id: Option<lesson::Id>,
    help: bool,
) -> Result<(), String> {
    let preview = &world.resource::<Files>().script.preview;
    let mut history = preview.history;
    if let Some(mut origin) = preview.location() {
        if let navigation::Location::Index { focus, .. } = &mut origin {
            *focus = id.or(*focus);
        }
        // Opening the same guide's instructions is not another navigation level.
        if preview.lesson != id || preview.index_open != id.is_none() {
            history.push(origin);
        }
    }
    let destination = id.map_or(
        navigation::Location::Index {
            page: preview.index_page,
            focus: preview.index_focus,
        },
        |id| {
            navigation::Location::Lesson(navigation::Lesson {
                id,
                step: 0,
                scenario: lesson::CutScenario::default(),
                instructions: help,
                scroll: 0.,
                steps_open: false,
                steps_page: None,
                yaw: HOME.0,
                pitch: HOME.1,
            })
        },
    );
    open_location(world, handle, destination, history, false)
}

fn show_index(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    page: usize,
    focus: Option<lesson::Id>,
    history: navigation::History<lesson::Id, lesson::CutScenario>,
) {
    let files = world.resource::<Files>();
    let previous = files.script.preview.return_to.unwrap_or((
        files.scripts,
        files.script.editor_open,
        files.script.library.open,
        files.script.chapters.open,
    ));
    let generation = files.script.generation;
    // Index navigation owns no view or texture. A pending preparation receipt is
    // still drained by poll, keeping the existing one-worker admission bound.
    close(world);
    let files = &mut world.resource_mut::<Files>();
    files.script.editor_open = false;
    files.script.library.open = false;
    files.script.chapters.open = false;
    let preview = &mut files.script.preview;
    preview.open = true;
    preview.help = true;
    preview.generation = generation;
    preview.return_to = Some(previous);
    preview.index_open = true;
    preview.index_page = page;
    preview.index_focus = focus;
    preview.history = history;
    preview.focus_owner = handle.presented_context();
}

fn open_location(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    destination: navigation::Location<lesson::Id, lesson::CutScenario>,
    history: navigation::History<lesson::Id, lesson::CutScenario>,
    restoring: bool,
) -> Result<(), String> {
    available(world)?;
    editor::retain_source_error(world);
    let navigation::Location::Lesson(location) = destination else {
        let navigation::Location::Index { page, focus } = destination else {
            unreachable!()
        };
        show_index(world, handle, page, focus, history);
        let menu = feature::dismiss_hint_menu(world);
        world.resource_mut::<Files>().script.preview.focus_from_menu = menu;
        return Ok(());
    };
    let example = catalog::examples()
        .iter()
        .find(|example| example.id == location.id.recipe())
        .ok_or("The guided example is unavailable")?;
    let files = world.resource::<Files>();
    let previous = files.script.preview.return_to.unwrap_or((
        files.scripts,
        files.script.editor_open,
        files.script.library.open,
        files.script.chapters.open,
    ));
    let generation = files.script.generation;
    let transition = world.resource_mut::<Files>().script.preview.retained.take();
    start_lesson(
        world,
        handle,
        example.source.clone(),
        generation,
        Some(location.id),
    )?;
    let files = &mut world.resource_mut::<Files>();
    files.script.editor_open = false;
    files.script.library.open = false;
    files.script.chapters.open = false;
    let preview = &mut files.script.preview;
    preview.return_to = Some(previous);
    preview.help_view = feature::HelpView::default();
    preview.restore_location(location);
    preview.playing = !restoring && !location.instructions;
    preview.animation_progress = if preview.playing { 0 } else { ANIMATION_STEPS };
    preview.steps_return_focus = false;
    preview.lesson = Some(location.id);
    preview.index_open = false;
    preview.transition = transition;
    preview.history = history;
    preview.restore = restoring.then_some(location);
    preview.focus_owner = handle.presented_context();
    let menu = feature::dismiss_hint_menu(world);
    world.resource_mut::<Files>().script.preview.focus_from_menu = menu;
    Ok(())
}

fn start_lesson(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    source: String,
    generation: u64,
    lesson: Option<lesson::Id>,
) -> Result<(), String> {
    start_job(
        world,
        handle,
        job::Source::Lesson { source, lesson },
        generation,
    )
}

fn start_job(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    source: job::Source,
    generation: u64,
) -> Result<(), String> {
    let revision = world
        .resource::<Files>()
        .script
        .preview
        .revision
        .checked_add(1)
        .ok_or("Preview view sequence exhausted")?;
    let project = source.is_project();
    let locale = crate::native_viewport::localization::locale(world);
    close(world);
    let (send, receive) = mpsc::channel();
    let wake = handle.clone();
    let prepared = world.resource::<Files>().script.preview.prepared.clone();
    let preparation = job::Preparation::new();
    let cancelled = Arc::clone(&preparation.cancelled);
    let deadline = preparation.deadline;
    std::thread::Builder::new()
        .name("cad-preview-prepare".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                source.prepare(locale, &prepared, || job::check(&cancelled, deadline))
            }))
            .unwrap_or_else(|_| Err("Lesson preview preparation stopped unexpectedly".into()));
            let result = result.map_err(|error| artifact::message(locale, &error));
            let _ = send.send(result);
            wake.request_redraw();
        })
        .map_err(|error| format!("Cannot prepare lesson preview: {error}"))?;
    let files = &mut world.resource_mut::<Files>();
    files.script.editor_open = false;
    files.script.library.open = false;
    let preview = &mut files.script.preview;
    preview.open = true;
    preview.generation = generation;
    preview.index = 0;
    preview.yaw = HOME.0;
    preview.pitch = HOME.1;
    preview.revision = revision;
    preview.error = None;
    preview.help = false;
    preview.build = Some(Mutex::new(receive));
    preview.preparation = Some(preparation);
    preview.project = project;
    Ok(())
}

fn close(world: &mut World) {
    world.resource_mut::<Files>().script.preview.return_to = None;
    let images = {
        let preview = &mut world.resource_mut::<Files>().script.preview;
        preview.open = false;
        preview.preparation = None;
        preview.project = false;
        preview.render = None;
        preview.render_started = None;
        preview.steps_open = false;
        preview.steps_return_focus = false;
        preview.help = false;
        preview.focus_owner = None;
        preview.focus_from_menu = None;
        preview.retained = None;
        preview.transition = None;
        preview.lesson = None;
        preview.index_open = false;
        preview.history = navigation::History::default();
        preview.restore = None;
        preview.playing = false;
        preview.animation_progress = ANIMATION_STEPS;
        preview.blend = None;
        preview.wake = None;
        preview.drag = None;
        preview.image_index = None;
        preview.image_frame = None;
        preview.image = None;
        preview.images.drain()
    };
    for image in images {
        crate::native_viewport::script_preview::textures::remove(world, image.id());
    }
}

fn close_to_origin(world: &mut World) {
    let previous = world.resource::<Files>().script.preview.return_to;
    close(world);
    if let Some((scripts, editor, library, chapters)) = previous {
        let files = &mut world.resource_mut::<Files>();
        files.scripts = scripts;
        files.script.editor_open = editor;
        files.script.library.open = library;
        files.script.chapters.open = chapters;
    } else {
        world.resource_mut::<Files>().script.editor_open = true;
    }
}

pub(crate) fn command(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    mut action: Action,
    input: &ControlInput,
) -> Result<Value, String> {
    if let (Action::HelpBody, ControlInput::Key(key)) = (action, input) {
        feature::scroll_help_key(world, key)?;
        handle.request_redraw();
        return Ok(
            json!({"help_scroll":world.resource::<Files>().script.preview.help_view.offset}),
        );
    }
    if let (Action::Model, ControlInput::Key(key)) = (action, input) {
        if key.alt || key.ctrl || key.meta || key.shift {
            return Err("Use unmodified arrow keys or Home to turn the preview".into());
        }
        action = match key.key.as_str() {
            "ArrowLeft" => Action::Left,
            "ArrowRight" => Action::Right,
            "ArrowUp" => Action::Up,
            "ArrowDown" => Action::Down,
            "Home" => Action::Fit,
            _ => return Ok(json!({"focused":true})),
        };
    } else if !super::super::super::super::is_activation(input) {
        return Err("Preview command requires activation".into());
    }
    if action != Action::Model && world.resource::<Files>().script.preview.drag.is_some() {
        crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
        cancel_pointer(world);
    }
    if action == Action::Open {
        open(world, handle)?;
    } else if action == Action::Demo && world.resource::<Files>().script.preview.retained.is_none()
    {
        let id = world.resource::<Files>().script.preview.lesson;
        open_lesson(world, handle, id, false)?;
    } else if let Action::Lesson(id) | Action::Read(id) = action {
        open_lesson(world, handle, Some(id), matches!(action, Action::Read(_)))?;
    } else if action == Action::Index {
        open_lesson(world, handle, None, true)?;
    } else if action == Action::Back {
        let mut history = world.resource::<Files>().script.preview.history;
        let destination = history
            .pop()
            .ok_or("There is no earlier guide to return to")?;
        open_location(world, handle, destination, history, true)?;
    } else if action == Action::FullBuild {
        let recipe = world
            .resource::<Files>()
            .script
            .preview
            .lesson
            .map_or(lesson::RECIPE, lesson::Id::recipe);
        close_to_origin(world);
        super::catalog::open_recipe(world, recipe)?;
    } else if action == Action::Extrude {
        open_extrude(world, handle)?;
    } else if action == Action::Close {
        close_to_origin(world);
    } else {
        let preview = &mut world.resource_mut::<Files>().script.preview;
        if !preview.open {
            return Err("Open the lesson preview first".into());
        }
        match action {
            Action::IndexPage(next) => {
                if !preview.index_open {
                    return Err("Open the guide list before changing its page".into());
                }
                preview.index_page = if next {
                    preview.index_page.saturating_add(1)
                } else {
                    preview.index_page.saturating_sub(1)
                };
                preview.focus_owner = handle.presented_context();
            }
            Action::Help | Action::Demo => {
                preview.steps_open = false;
                preview.steps_return_focus = false;
                preview.help = action == Action::Help && !preview.help;
                preview.focus_owner = handle.presented_context();
                preview.pause();
                preview.help_view = feature::HelpView::default();
                preview.help_view.return_focus = !preview.help;
            }
            Action::HelpBody => {}
            Action::Steps => {
                if preview.lesson.is_none() || preview.count() == 0 {
                    return Err("Open a prepared lesson before viewing its steps".into());
                }
                preview.steps_open = !preview.steps_open;
                preview.steps_page = None;
                preview.steps_return_focus = !preview.steps_open;
                preview.pause();
                preview.focus_owner = handle.presented_context();
            }
            Action::StepsPage(next) => {
                if !preview.steps_open {
                    return Err("Open the step list before changing its page".into());
                }
                let page = preview.steps_page.unwrap_or(0);
                preview.steps_page = Some(if next {
                    page.saturating_add(1)
                } else {
                    page.saturating_sub(1)
                });
            }
            Action::Step(index) => {
                if index >= preview.count() {
                    return Err("Choose a step in the current lesson".into());
                }
                preview.index = index;
                preview.steps_open = false;
                preview.steps_return_focus = false;
                preview.playing = false;
                preview.animation_progress = ANIMATION_STEPS;
                preview.focus_owner = handle.presented_context();
                preview.changed()?;
            }
            Action::Scenario(scenario) => {
                if !preview.cut_choices() {
                    return Err("Open the Cut example before choosing its outcome".into());
                }
                if preview.scenario != scenario {
                    preview.steps_open = false;
                    preview.scenario = scenario;
                    preview.index = 0;
                    preview.playing = !preview.help;
                    preview.changed()?;
                    preview.begin_animation();
                    if preview.help {
                        preview.animation_progress = ANIMATION_STEPS;
                        preview.blend = None;
                    }
                }
            }
            Action::Model => {}
            Action::Previous | Action::Next => {
                preview.steps_open = false;
                preview.playing = false;
                preview.index = if action == Action::Previous {
                    preview.index.saturating_sub(1)
                } else {
                    (preview.index + 1).min(preview.count().saturating_sub(1))
                };
                preview.animation_progress = ANIMATION_STEPS;
                preview.changed()?;
            }
            Action::Replay => {
                preview.steps_open = false;
                preview.index = 0;
                preview.playing = preview.count() > 1;
                preview.help = false;
                preview.changed()?;
                preview.begin_animation();
            }
            Action::Resume => {
                if preview.retained.is_none() || preview.count() == 0 {
                    return Err("Wait for the guide to finish preparing".into());
                }
                preview.help = false;
                preview.steps_open = false;
                preview.playing = true;
                preview.wake = None;
                preview.render_started = None;
            }
            Action::Stop => {
                preview.pause();
            }
            Action::Fit => preview.turn(HOME.0, HOME.1)?,
            Action::Left => preview.turn(preview.yaw - 0.15, preview.pitch)?,
            Action::Right => preview.turn(preview.yaw + 0.15, preview.pitch)?,
            Action::Up => preview.turn(preview.yaw, preview.pitch + 0.15)?,
            Action::Down => preview.turn(preview.yaw, preview.pitch - 0.15)?,
            _ => unreachable!(),
        }
    }
    handle.request_redraw();
    Ok(json!({"preview_open":world.resource::<Files>().script.preview.open}))
}

fn receive<T>(channel: &Option<ResultChannel<T>>) -> Option<Result<T, String>> {
    let channel = channel.as_ref()?;
    Some(match channel.lock() {
        Ok(channel) => match channel.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => Err("Preview worker disconnected".into()),
        },
        Err(_) => Err("Preview result could not be read".into()),
    })
}

fn publish(world: &mut World, rendered: Rendered) -> Result<(), String> {
    if !world.resource::<Files>().script.preview.accepts(&rendered) {
        return Ok(());
    }
    world.resource_scope(|world, mut files: Mut<Files>| {
        let preview = &mut files.script.preview;
        let key = images::Key::current(preview);
        // The source must be the newest resident before the incoming target is
        // stored: get() is a probe, and this cache has only two physical images.
        if let Some(blend) = &preview.blend {
            preview.images.promote(&blend.from, key);
        }
        let handle = preview.images.store(world, key, rendered.image)?;
        preview.validate_blend(&handle);
        preview.image = Some(handle);
        preview.image_index = Some(preview.index);
        preview.image_frame = Some(preview.frame_index());
        preview.rendered = rendered.revision;
        preview.error = None;
        Ok(())
    })
}

fn render(world: &mut World, _handle: &NativeInterfaceHandle) -> Result<(), String> {
    let service = world.resource::<Files>().script.preview.service.clone();
    // Host supplies its existing device/queue and completion wake callback.
    // Connecting is cheap and idempotent; no secondary adapter/device is created.
    // Cached frames and test fixtures need no connection at all.
    let needs_connection = {
        let preview = &world.resource::<Files>().script.preview;
        preview.open
            && preview.attempted != preview.revision
            && preview.render.is_none()
            && preview.retained.is_some()
            && preview.images.get(images::Key::current(preview)).is_none()
    };
    if needs_connection {
        world.resource_scope(|world, mut files: Mut<Files>| {
            let preview = &mut files.script.preview;
            // Make room before the GPU request, retaining the visible source.
            // The pending target occupies the other slot, not a third cached image.
            preview.images.reserve(world, preview.image.as_ref());
        });
        crate::native_viewport::script_preview::connect(world, &service)?;
    }
    let preview = &mut world.resource_mut::<Files>().script.preview;
    if !preview.open || preview.attempted == preview.revision {
        return Ok(());
    }
    let key = images::Key::current(preview);
    if let Some(image) = preview.images.get(key) {
        if let Some(blend) = &preview.blend {
            preview.images.promote(&blend.from, key);
        }
        preview.images.promote(&image, key);
        preview.validate_blend(&image);
        preview.render_started = None;
        preview.image = Some(image);
        preview.image_index = Some(preview.index);
        preview.image_frame = Some(preview.frame_index());
        preview.rendered = preview.revision;
        preview.attempted = preview.revision;
        return Ok(());
    }
    if preview.render.is_some() {
        return Ok(());
    }
    let Some(retained) = &preview.retained else {
        return Ok(());
    };
    let revision = preview.revision;
    preview.render_started = Some(Instant::now());
    let view = retained.view.clone();
    let request = RenderRequest {
        palette: Some(preview.palette),
        preview_id: retained.descriptor.preview_id.clone(),
        view_id: view.clone(),
        revision,
        frame_index: preview.frame_index(),
        width: preview.size[0],
        height: preview.size[1],
        yaw: preview.yaw,
        pitch: preview.pitch,
    };
    let pending = if preview.geometry_animation_complete() {
        retained.service.render(request)?
    } else {
        retained.service.render_animated(
            request,
            f32::from(preview.animation_progress) / f32::from(ANIMATION_STEPS),
        )?
    };
    preview.attempted = revision;
    #[cfg(test)]
    {
        preview.render_jobs += 1;
    }
    preview.render = Some(InFlight::Gpu {
        view,
        revision,
        pending: Mutex::new(pending),
    });
    preview.render_revision = revision;
    Ok(())
}

fn replay(preview: &mut State, handle: &NativeInterfaceHandle, now: Instant) -> Result<(), String> {
    if !preview.open || !preview.playing || preview.rendered != preview.revision {
        return Ok(());
    }
    if preview.wake.as_ref().is_some_and(|wake| now >= wake.due) {
        // Consume the old deadline before changing only UI opacity. Otherwise
        // an expired render_started deadline could cause a 1 ms wake loop.
        preview.wake = None;
        if let Some(blend) = &mut preview.blend {
            if blend.advance() {
                preview.blend = None;
            }
            handle.request_redraw();
        } else if !preview.geometry_animation_complete() {
            preview.animation_progress += 1;
            preview.changed()?;
        } else if preview.index + 1 < preview.count() {
            preview.index += 1;
            preview.changed()?;
            preview.begin_animation();
        } else {
            preview.playing = false;
            preview.wake = None;
        }
    } else {
        arm_replay(preview, now);
    }
    Ok(())
}

// Rendering may synchronously publish a cached sample. This second phase only
// arms a deadline; it must never advance after the update's render opportunity.
fn arm_replay(preview: &mut State, now: Instant) {
    if preview.open
        && preview.playing
        && preview.rendered == preview.revision
        && preview.wake.is_none()
    {
        let delay = if preview.blend.is_some() {
            ANIMATION_INTERVAL
        } else if preview.animation_complete() {
            preview
                .retained
                .as_ref()
                .and_then(|r| r.descriptor.durations_ms.get(preview.frame_index()))
                .copied()
                .map(Duration::from_millis)
                .unwrap_or(FRAME_DELAY)
        } else {
            preview
                .render_started
                .map_or(ANIMATION_INTERVAL, |started| {
                    (started + ANIMATION_INTERVAL)
                        .saturating_duration_since(now)
                        .max(Duration::from_millis(1))
                })
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        preview.wake = Some(Wake {
            due: now + delay,
            cancelled,
        });
    }
}

pub(super) fn poll(world: &mut World) {
    let files = world.resource::<Files>();
    let preview = &files.script.preview;
    if preview.open
        && ((!files.scripts && !preview.contextual())
            || files.script.library.open
            || files.script.editor_open
            || files.script.generation != preview.generation)
    {
        close(world);
    }
    let preview = &world.resource::<Files>().script.preview;
    if preview.build.is_none()
        && preview.render.is_none()
        && (!preview.open || (!preview.playing && preview.rendered == preview.revision))
    {
        return;
    }
    if let Some(result) = receive(&world.resource::<Files>().script.preview.build) {
        let preview = &mut world.resource_mut::<Files>().script.preview;
        preview.build = None;
        preview.preparation = None;
        if preview.open && !preview.index_open {
            match result.and_then(|frames| Retained::new(preview.service.clone(), frames)) {
                Ok(retained) => {
                    preview.retained = Some(retained);
                    if let Some(location) = preview.restore.take() {
                        preview.index = preview.index.min(preview.count().saturating_sub(1));
                        preview.help_view.restore_offset = Some(location.scroll);
                    } else if preview.playing {
                        preview.begin_animation();
                    }
                }
                Err(error) => {
                    preview.error = Some(error);
                    preview.playing = false;
                    preview.wake = None;
                    preview.render_started = None;
                }
            }
            preview.transition = None;
        }
    }
    let completed = world
        .resource::<Files>()
        .script
        .preview
        .render
        .as_ref()
        .and_then(InFlight::receive);
    if let Some(result) = completed {
        world.resource_mut::<Files>().script.preview.render = None;
        if let Err(error) = result.and_then(|result| publish(world, result)) {
            let preview = &mut world.resource_mut::<Files>().script.preview;
            if preview.open && preview.render_revision == preview.revision {
                preview.error = Some(error);
                preview.playing = false;
                preview.wake = None;
            }
        }
    }
    let Some(handle) = world.get_resource::<NativeInterfaceHandle>().cloned() else {
        return;
    };
    let outcome = replay(
        &mut world.resource_mut::<Files>().script.preview,
        &handle,
        Instant::now(),
    )
    .and_then(|_| render(world, &handle))
    // Cached pixels arrive synchronously: arm their dwell timer in this update.
    .map(|_| {
        arm_replay(
            &mut world.resource_mut::<Files>().script.preview,
            Instant::now(),
        );
    });
    if let Err(error) = outcome {
        let preview = &mut world.resource_mut::<Files>().script.preview;
        preview.error = Some(error);
        preview.playing = false;
        preview.wake = None;
    } else {
        let next = world
            .resource::<Files>()
            .script
            .preview
            .wake
            .as_ref()
            .map(|wake| (wake.due, wake.cancelled.clone()));
        if let Some((due, token)) = next {
            if let Err(error) = hover::arm(world, handle, due, token) {
                let preview = &mut world.resource_mut::<Files>().script.preview;
                preview.error = Some(error);
                preview.playing = false;
                preview.wake = None;
            }
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod transition_tests;

#[cfg(test)]
mod navigation_tests;

#[cfg(test)]
mod evidence;
