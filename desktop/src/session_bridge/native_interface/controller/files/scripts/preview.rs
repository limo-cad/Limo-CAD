//! One bounded view of the shared, isolated teaching-preview service.
//! Neither preparing nor rendering a preview receives the live CAD document.
use super::*;
use crate::native_viewport::script_preview::{
    Frame, PreviewDescriptor, PreviewService, RenderRequest,
};
use bevy::{
    asset::RenderAssetUsages,
    image::{CompressedImageFormats, ImageSampler, ImageType},
};
use std::time::{Duration, Instant};

mod input;
mod panel;
pub(crate) use input::input;
pub(crate) use panel::paint;

const HOME: (f32, f32) = (std::f32::consts::FRAC_PI_4, std::f32::consts::FRAC_PI_6);
const FRAME_DELAY: Duration = Duration::from_millis(1800);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Open,
    Close,
    Model,
    Previous,
    Next,
    Replay,
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
    png: Vec<u8>,
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
    render: Option<ResultChannel<Rendered>>,
    service: Arc<PreviewService>,
    retained: Option<Retained>,
    index: usize,
    yaw: f32,
    pitch: f32,
    revision: u64,
    rendered: u64,
    attempted: u64,
    size: [u32; 2],
    image: Option<Handle<Image>>,
    image_index: Option<usize>,
    error: Option<String>,
    playing: bool,
    wake: Option<Wake>,
    drag: Option<input::Drag>,
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
            service: Arc::new(PreviewService::default()),
            retained: None,
            index: 0,
            yaw: HOME.0,
            pitch: HOME.1,
            revision: 1,
            rendered: 0,
            attempted: 0,
            size: [300, 176],
            image: None,
            image_index: None,
            error: None,
            playing: false,
            wake: None,
            drag: None,
        }
    }
}
impl State {
    pub fn building(&self) -> bool {
        self.build.is_some()
    }
    fn count(&self) -> usize {
        self.retained
            .as_ref()
            .map_or(0, |r| r.descriptor.captions.len())
    }
    fn changed(&mut self) -> Result<(), String> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or("Preview view sequence exhausted")?;
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

fn prepare(source: &str) -> Result<Vec<Frame>, String> {
    let mut report = limo_cad_mcp::preview_script(source)?;
    serde_json::from_value(report["exports"]["preview_frames"].take())
        .map_err(|error| format!("Preview frames are unavailable: {error}"))
}

fn open(world: &mut World, handle: &NativeInterfaceHandle) -> Result<(), String> {
    available(world)?;
    editor::source_ready(world)?;
    let state = &world.resource::<Files>().script;
    let example = eligible(state)?;
    let generation = state.generation;
    let revision = state
        .preview
        .revision
        .checked_add(1)
        .ok_or("Preview view sequence exhausted")?;
    close(world);
    let (send, receive) = mpsc::channel();
    let wake = handle.clone();
    std::thread::Builder::new()
        .name("cad-preview-prepare".into())
        .spawn(move || {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| prepare(&example.source)))
                    .unwrap_or_else(|_| {
                        Err("Lesson preview preparation stopped unexpectedly".into())
                    });
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
    preview.build = Some(Mutex::new(receive));
    Ok(())
}

fn close(world: &mut World) {
    let image = {
        let preview = &mut world.resource_mut::<Files>().script.preview;
        preview.open = false;
        preview.retained = None;
        preview.playing = false;
        preview.wake = None;
        preview.drag = None;
        preview.image_index = None;
        preview.image.take()
    };
    if let Some(image) = image {
        if let Some(mut images) = world.get_resource_mut::<Assets<Image>>() {
            images.remove(image.id());
        }
    }
}

pub(crate) fn command(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    mut action: Action,
    input: &ControlInput,
) -> Result<Value, String> {
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
    if action == Action::Open {
        open(world, handle)?;
    } else if action == Action::Close {
        close(world);
        world.resource_mut::<Files>().script.editor_open = true;
    } else {
        let preview = &mut world.resource_mut::<Files>().script.preview;
        if !preview.open {
            return Err("Open the lesson preview first".into());
        }
        match action {
            Action::Model => {}
            Action::Previous | Action::Next => {
                preview.playing = false;
                preview.index = if action == Action::Previous {
                    preview.index.saturating_sub(1)
                } else {
                    (preview.index + 1).min(preview.count().saturating_sub(1))
                };
                preview.changed()?;
            }
            Action::Replay => {
                preview.index = 0;
                preview.playing = preview.count() > 1;
                preview.changed()?;
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
    let image = Image::from_buffer(
        &rendered.png,
        ImageType::Extension("png"),
        CompressedImageFormats::NONE,
        true,
        ImageSampler::Default,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
    .map_err(|error| format!("Cannot display lesson preview: {error}"))?;
    let old = world.resource_mut::<Files>().script.preview.image.take();
    let handle = {
        let mut images = world.resource_mut::<Assets<Image>>();
        if let Some(old) = old {
            images.remove(old.id());
        }
        images.add(image)
    };
    let preview = &mut world.resource_mut::<Files>().script.preview;
    preview.image = Some(handle);
    preview.image_index = Some(preview.index);
    preview.rendered = rendered.revision;
    preview.error = None;
    Ok(())
}

fn render(world: &mut World, handle: &NativeInterfaceHandle) -> Result<(), String> {
    let preview = &mut world.resource_mut::<Files>().script.preview;
    if !preview.open || preview.render.is_some() || preview.attempted == preview.revision {
        return Ok(());
    }
    let Some(retained) = &preview.retained else {
        return Ok(());
    };
    let revision = preview.revision;
    preview.attempted = revision;
    let view = retained.view.clone();
    let request = RenderRequest {
        preview_id: retained.descriptor.preview_id.clone(),
        view_id: view.clone(),
        revision,
        frame_index: preview.index,
        width: preview.size[0],
        height: preview.size[1],
        yaw: preview.yaw,
        pitch: preview.pitch,
    };
    let pending = retained.service.render(request)?;
    let (send, receive) = mpsc::channel();
    let wake = handle.clone();
    std::thread::Builder::new()
        .name("cad-preview-pixels".into())
        .spawn(move || {
            let result = pending.wait().map(|png| Rendered {
                view,
                revision,
                png,
            });
            let _ = send.send(result);
            wake.request_redraw();
        })
        .map_err(|error| format!("Cannot wait for preview pixels: {error}"))?;
    preview.render = Some(Mutex::new(receive));
    Ok(())
}

fn replay(preview: &mut State, handle: &NativeInterfaceHandle, now: Instant) -> Result<(), String> {
    if !preview.open || !preview.playing || preview.rendered != preview.revision {
        return Ok(());
    }
    if preview.wake.as_ref().is_some_and(|wake| now >= wake.due) {
        preview.index = (preview.index + 1).min(preview.count().saturating_sub(1));
        preview.playing = preview.index + 1 < preview.count();
        preview.changed()?;
    } else if preview.wake.is_none() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let token = cancelled.clone();
        let handle = handle.clone();
        std::thread::Builder::new()
            .name("cad-preview-replay".into())
            .spawn(move || {
                std::thread::sleep(FRAME_DELAY);
                if !token.load(Ordering::Acquire) {
                    handle.request_redraw();
                }
            })
            .map_err(|error| format!("Cannot schedule preview replay: {error}"))?;
        preview.wake = Some(Wake {
            due: now + FRAME_DELAY,
            cancelled,
        });
    }
    Ok(())
}

pub(super) fn poll(world: &mut World) {
    let files = world.resource::<Files>();
    let preview = &files.script.preview;
    if preview.open
        && (!files.scripts
            || files.script.library.open
            || files.script.editor_open
            || files.script.generation != preview.generation)
    {
        close(world);
    }
    if let Some(result) = receive(&world.resource::<Files>().script.preview.build) {
        let preview = &mut world.resource_mut::<Files>().script.preview;
        preview.build = None;
        if preview.open {
            match result.and_then(|frames| Retained::new(preview.service.clone(), frames)) {
                Ok(retained) => preview.retained = Some(retained),
                Err(error) => preview.error = Some(error),
            }
        }
    }
    if let Some(result) = receive(&world.resource::<Files>().script.preview.render) {
        world.resource_mut::<Files>().script.preview.render = None;
        if let Err(error) = result.and_then(|result| publish(world, result)) {
            let preview = &mut world.resource_mut::<Files>().script.preview;
            if preview.open && preview.attempted == preview.revision {
                preview.error = Some(error);
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
    .and_then(|_| render(world, &handle));
    if let Err(error) = outcome {
        world.resource_mut::<Files>().script.preview.error = Some(error);
    }
}

#[cfg(test)]
mod tests;
