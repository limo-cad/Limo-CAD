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
mod card;
mod feature;
mod hover;
mod images;
mod input;
mod lesson;
mod panel;
mod prepared;
mod steps;
pub(crate) use feature::paint_hint;
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
}
impl Retained {
    fn new(service: Arc<PreviewService>, frames: Vec<Frame>) -> Result<Self, String> {
        let descriptor = service.retain(frames)?;
        match service.open_view() {
            Ok(view) => Ok(Self {
                service,
                descriptor,
                view,
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
    transition: Option<Retained>,
    focus_owner: Option<DocumentContext>,
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
            transition: None,
            focus_owner: None,
        }
    }
}
impl State {
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
    fn animation_complete(&self) -> bool {
        self.animation_progress == ANIMATION_STEPS
    }
    fn begin_animation(&mut self) {
        use crate::native_viewport::script_preview::PreviewCue;
        let moving = self
            .retained
            .as_ref()
            .and_then(|r| r.descriptor.cues.get(self.frame_index()))
            .is_some_and(|cue| {
                matches!(
                    cue,
                    Some(
                        PreviewCue::Plane
                            | PreviewCue::Profile
                            | PreviewCue::Extrude
                            | PreviewCue::Depth
                            | PreviewCue::Cut
                            | PreviewCue::Edges
                            | PreviewCue::Radius
                    )
                )
            });
        self.animation_progress = if moving { 0 } else { ANIMATION_STEPS };
    }
    pub fn contextual(&self) -> bool {
        self.return_to.is_some()
    }
    pub fn building(&self) -> bool {
        self.build.is_some()
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
        self.error = None;
        self.wake = None;
        Ok(())
    }
    fn turn(&mut self, yaw: f32, pitch: f32) -> Result<(), String> {
        if !yaw.is_finite() || !pitch.is_finite() {
            return Err("Invalid preview camera".into());
        }
        self.playing = false;
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
    let example = eligible(state)?;
    let generation = state.generation;
    start(world, handle, example.source.clone(), generation)
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
    available(world)?;
    editor::retain_source_error(world);
    if world.resource::<Files>().script.preview.building() {
        return Err("Wait for the current example to finish preparing".into());
    }
    let example = catalog::examples()
        .iter()
        .find(|example| example.id == id.map_or(lesson::RECIPE, lesson::Id::recipe))
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
    if id.is_none() {
        close(world);
        let preview = &mut world.resource_mut::<Files>().script.preview;
        preview.open = true;
        preview.help = true;
        preview.generation = generation;
    } else {
        start_lesson(world, handle, example.source.clone(), generation, id)?;
    }
    let files = &mut world.resource_mut::<Files>();
    files.script.editor_open = false;
    files.script.library.open = false;
    files.script.chapters.open = false;
    files.script.preview.return_to = Some(previous);
    files.script.preview.help = help;
    files.script.preview.playing = id.is_some() && !help;
    files.script.preview.animation_progress = if help { ANIMATION_STEPS } else { 0 };
    files.script.preview.steps_open = false;
    files.script.preview.steps_return_focus = false;
    files.script.preview.lesson = id;
    files.script.preview.scenario = lesson::CutScenario::default();
    files.script.preview.index_open = id.is_none();
    files.script.preview.index_page = 0;
    files.script.preview.transition = if id.is_none() { None } else { transition };
    files.script.preview.help_view = feature::HelpView::default();
    files.script.preview.focus_owner = handle.presented_context();
    Ok(())
}

fn start(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    source: String,
    generation: u64,
) -> Result<(), String> {
    start_lesson(world, handle, source, generation, None)
}
fn start_lesson(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    source: String,
    generation: u64,
    lesson: Option<lesson::Id>,
) -> Result<(), String> {
    let state = &world.resource::<Files>().script;
    let revision = state
        .preview
        .revision
        .checked_add(1)
        .ok_or("Preview view sequence exhausted")?;
    close(world);
    let (send, receive) = mpsc::channel();
    let wake = handle.clone();
    let prepared = world.resource::<Files>().script.preview.prepared.clone();
    std::thread::Builder::new()
        .name("cad-preview-prepare".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                prepared::prepare(&prepared, &source, lesson)
            }))
            .unwrap_or_else(|_| Err("Lesson preview preparation stopped unexpectedly".into()));
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
    Ok(())
}

fn close(world: &mut World) {
    world.resource_mut::<Files>().script.preview.return_to = None;
    let images = {
        let preview = &mut world.resource_mut::<Files>().script.preview;
        preview.open = false;
        preview.render = None;
        preview.render_started = None;
        preview.steps_open = false;
        preview.steps_return_focus = false;
        preview.help = false;
        preview.focus_owner = None;
        preview.retained = None;
        preview.transition = None;
        preview.lesson = None;
        preview.index_open = false;
        preview.playing = false;
        preview.animation_progress = ANIMATION_STEPS;
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
    } else if action == Action::FullBuild {
        let recipe = world
            .resource::<Files>()
            .script
            .preview
            .lesson
            .map_or(lesson::RECIPE, lesson::Id::recipe);
        let _ = command(world, handle, Action::Close, &ControlInput::Click)?;
        super::catalog::open_recipe(world, recipe)?;
    } else if action == Action::Extrude {
        open_extrude(world, handle)?;
    } else if action == Action::Close {
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
                preview.playing = false;
                preview.wake = None;
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
                preview.playing = false;
                preview.wake = None;
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
                    preview.begin_animation();
                    if preview.help {
                        preview.animation_progress = ANIMATION_STEPS;
                    }
                    preview.changed()?;
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
                preview.begin_animation();
                preview.changed()?;
            }
            Action::Resume => {
                if preview.retained.is_none() || preview.count() == 0 {
                    return Err("Wait for the guide to finish preparing".into());
                }
                preview.help = false;
                preview.steps_open = false;
                preview.playing = true;
                preview.wake = None;
            }
            Action::Stop => {
                preview.playing = false;
                preview.wake = None;
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
        let handle = preview.images.store(world, key, rendered.image)?;
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
        crate::native_viewport::script_preview::connect(world, &service)?;
    }
    let preview = &mut world.resource_mut::<Files>().script.preview;
    if !preview.open || preview.attempted == preview.revision {
        return Ok(());
    }
    if let Some(image) = preview.images.get(images::Key::current(preview)) {
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
    let pending = if preview.animation_complete() {
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

fn replay(
    preview: &mut State,
    _handle: &NativeInterfaceHandle,
    now: Instant,
) -> Result<(), String> {
    if !preview.open || !preview.playing || preview.rendered != preview.revision {
        return Ok(());
    }
    if preview.wake.as_ref().is_some_and(|wake| now >= wake.due) {
        if !preview.animation_complete() {
            preview.animation_progress += 1;
            preview.changed()?;
        } else if preview.index + 1 < preview.count() {
            preview.index += 1;
            preview.begin_animation();
            preview.changed()?;
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
        let delay = if preview.animation_complete() {
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
        if preview.open {
            match result.and_then(|frames| Retained::new(preview.service.clone(), frames)) {
                Ok(retained) => {
                    preview.retained = Some(retained);
                    if preview.playing {
                        preview.begin_animation();
                    }
                }
                Err(error) => preview.error = Some(error),
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
mod evidence;
