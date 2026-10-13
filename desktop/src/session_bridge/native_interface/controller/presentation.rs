//! Native adapter for the existing live script runner's presentation protocol.
//! Captions and permits are UI state; the shared inbox remains the only model
//! dispatcher, and document ownership never follows a replaced design.
use super::*;
use bevy::input::ButtonState;
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Command {
    Configure,
    Note,
    Pause,
    Resume,
    Step,
    Stop,
    #[default]
    Status,
    Finish,
    Dismiss,
    Show,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Mode {
    #[default]
    Fast,
    Present,
}

#[derive(Default, Deserialize)]
struct Request {
    #[serde(default)]
    command: Command,
    mode: Option<Mode>,
    speed: Option<f64>,
    duration_ms: Option<u64>,
    text: Option<String>,
    chapter: Option<String>,
    step_index: Option<u64>,
    step_count: Option<u64>,
}

#[derive(Clone, Serialize)]
struct Snapshot {
    active: bool,
    visible: bool,
    mode: Mode,
    speed: f64,
    paused: bool,
    stopped: bool,
    finished: bool,
    text: String,
    chapter: String,
    operation: String,
    step_index: u64,
    step_count: u64,
    highlighted_body_ids: Vec<u64>,
    highlighted_sketch_entity_ids: Vec<u64>,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            active: false,
            visible: false,
            mode: Mode::Fast,
            speed: 1.,
            paused: false,
            stopped: false,
            finished: false,
            text: String::new(),
            chapter: String::new(),
            operation: String::new(),
            step_index: 0,
            step_count: 0,
            highlighted_body_ids: Vec::new(),
            highlighted_sketch_entity_ids: Vec::new(),
        }
    }
}

#[derive(Resource)]
struct Playback {
    owner: Option<DocumentContext>,
    state: Snapshot,
    remaining_ms: f64,
    clock: Instant,
    credits: u64,
    pace_ms: u64,
    saved: Vec<SavedPlayback>,
    busy_pointer: bool,
    widgets: chrome::Widgets,
}
struct SavedPlayback {
    owner: DocumentContext,
    state: Snapshot,
    remaining_ms: f64,
    clock: Instant,
    credits: u64,
    pace_ms: u64,
}
impl Default for Playback {
    fn default() -> Self {
        Self {
            owner: None,
            state: Snapshot::default(),
            remaining_ms: 0.,
            clock: Instant::now(),
            credits: 0,
            pace_ms: 0,
            saved: Vec::new(),
            busy_pointer: false,
            widgets: default(),
        }
    }
}
impl Playback {
    fn observe(&mut self, owner: &DocumentContext, now: Instant) {
        if self.owner.as_ref() == Some(owner) {
            return;
        }
        self.busy_pointer = false;
        let same_document = |candidate: &DocumentContext| {
            candidate.window_id == owner.window_id && candidate.document_id == owner.document_id
        };
        self.saved
            .retain(|saved| !same_document(&saved.owner) || saved.owner == *owner);
        if let Some(previous) = self
            .owner
            .take()
            .filter(|previous| self.state.active && !same_document(previous))
        {
            self.saved.push(SavedPlayback {
                owner: previous,
                state: std::mem::take(&mut self.state),
                remaining_ms: self.remaining_ms,
                clock: self.clock,
                credits: self.credits,
                pace_ms: self.pace_ms,
            });
        }
        if let Some(index) = self.saved.iter().position(|saved| saved.owner == *owner) {
            let saved = self.saved.swap_remove(index);
            self.state = saved.state;
            self.remaining_ms = saved.remaining_ms;
            self.clock = saved.clock;
            self.credits = saved.credits;
            self.pace_ms = saved.pace_ms;
        } else {
            self.state = Snapshot::default();
            self.remaining_ms = 0.;
            self.credits = 0;
            self.pace_ms = 0;
            self.clock = now;
        }
        self.owner = Some(owner.clone());
    }
    fn advance(&mut self, now: Instant) {
        if !self.state.paused {
            self.remaining_ms = (self.remaining_ms
                - now.saturating_duration_since(self.clock).as_secs_f64()
                    * 1000.
                    * self.state.speed)
                .max(0.);
        }
        self.clock = now;
    }
    fn status(&mut self, now: Instant) -> Value {
        self.advance(now);
        let mut status = serde_json::to_value(&self.state).expect("Presentation state is finite");
        status["wait_ms"] = json!(if self.state.mode == Mode::Fast {
            0
        } else {
            (self.remaining_ms / self.state.speed).ceil() as u64
        });
        status["step_pending"] = json!(self.credits > 0);
        status
    }
    fn control(&mut self, request: Request, now: Instant) -> Result<Value, String> {
        if request
            .speed
            .is_some_and(|speed| !speed.is_finite() || !(0.1..=16.).contains(&speed))
        {
            return Err("Presentation speed must be from 0.1 to 16".into());
        }
        if request
            .duration_ms
            .is_some_and(|duration| duration > 10_000)
        {
            return Err("Presentation duration_ms must be from 0 to 10000".into());
        }
        for (name, text, limit) in [
            ("text", request.text.as_deref(), 4000),
            ("chapter", request.chapter.as_deref(), 200),
        ] {
            if text.is_some_and(|text| {
                text.encode_utf16().count() > limit
                    || text
                        .chars()
                        .any(|c| c.is_control() && c != '\n' && c != '\t')
            }) {
                return Err(format!(
                    "Presentation {name} must contain at most {limit} printable characters"
                ));
            }
        }
        let index = request.step_index.unwrap_or(self.state.step_index);
        let count = request.step_count.unwrap_or(self.state.step_count);
        if index > 9_007_199_254_740_991
            || count > 9_007_199_254_740_991
            || (count > 0 && index > count)
        {
            return Err("Presentation step_index must not exceed step_count".into());
        }
        if self.state.stopped
            && matches!(
                request.command,
                Command::Resume | Command::Step | Command::Finish
            )
        {
            return Err("Playback stopped; configure a new run before continuing".into());
        }
        if request.command == Command::Show && !self.state.active {
            return Err("There is no playback to show".into());
        }
        self.advance(now);
        if request.command == Command::Status {
            return Ok(self.status(now));
        }
        if matches!(request.command, Command::Dismiss | Command::Show) {
            self.state.visible = request.command == Command::Show;
            return Ok(self.status(now));
        }
        if !self.state.active {
            self.state.visible = true;
        }
        self.state.active = true;
        if request.command == Command::Configure {
            self.pace_ms = 0;
        }
        if let Some(mode) = request.mode {
            self.state.mode = mode;
        }
        if let Some(speed) = request.speed {
            self.state.speed = speed;
        }
        if let Some(text) = request.text {
            self.state.text = text;
        }
        if let Some(chapter) = request.chapter {
            self.state.chapter = chapter;
        }
        if request.step_index.is_some() {
            self.state.step_index = index;
        }
        if request.step_count.is_some() {
            self.state.step_count = count;
        }
        match request.command {
            Command::Configure if self.state.finished || self.state.stopped => {
                self.state.visible = true;
                self.state.paused = false;
                self.state.finished = false;
                self.state.stopped = false;
                self.state.operation.clear();
                self.remaining_ms = 0.;
                self.credits = 0;
            }
            Command::Note => self.remaining_ms = request.duration_ms.unwrap_or(0) as f64,
            Command::Pause => {
                self.state.paused = true;
                self.credits = 0;
            }
            Command::Resume => {
                self.state.paused = false;
                self.credits = 0;
            }
            Command::Step => {
                self.state.paused = true;
                self.credits = self.credits.saturating_add(1);
            }
            Command::Finish | Command::Stop => {
                self.state.paused = request.command == Command::Stop;
                self.state.stopped = request.command == Command::Stop;
                self.state.finished = request.command == Command::Finish;
                self.credits = 0;
                self.remaining_ms = 0.;
            }
            _ => {}
        }
        Ok(self.status(now))
    }
}

#[derive(Debug, PartialEq)]
pub(super) enum Gate {
    Ready,
    Waiting,
    Stopped,
}

pub(super) fn gate(world: &mut World, owner: &DocumentContext) -> Gate {
    world.init_resource::<Playback>();
    let mut playback = world.resource_mut::<Playback>();
    let now = Instant::now();
    playback.observe(owner, now);
    playback.advance(now);
    if playback.state.stopped {
        Gate::Stopped
    } else if playback.state.paused {
        if playback.credits > 0 {
            Gate::Ready
        } else {
            Gate::Waiting
        }
    } else if playback.state.mode == Mode::Fast || playback.remaining_ms <= 0. {
        Gate::Ready
    } else {
        Gate::Waiting
    }
}

pub(super) fn observe(world: &mut World, owner: &DocumentContext) {
    world.init_resource::<Playback>();
    world
        .resource_mut::<Playback>()
        .observe(owner, Instant::now());
}

pub(super) fn pace(
    world: &mut World,
    services: &NativeServices,
    owner: &DocumentContext,
    value: &Value,
) -> Result<(), String> {
    let pace = value
        .as_u64()
        .filter(|pace| *pace <= 2000)
        .ok_or("pace_ms must be an integer from 0 to 2000")?;
    services
        .bridge
        .with_native_document_owner(&services.engine, owner, || {
            observe(world, owner);
            let mut playback = world.resource_mut::<Playback>();
            playback.control(
                Request {
                    command: Command::Configure,
                    mode: Some(if pace == 0 { Mode::Fast } else { Mode::Present }),
                    ..default()
                },
                Instant::now(),
            )?;
            playback.pace_ms = pace;
            Ok(())
        })
}

pub(super) fn motion_duration(world: &World, owner: &DocumentContext, milliseconds: u64) -> u64 {
    let Some(playback) = world
        .get_resource::<Playback>()
        .filter(|playback| playback.owner.as_ref() == Some(owner))
    else {
        return milliseconds;
    };
    if !playback.state.active || playback.state.finished || playback.state.stopped {
        return milliseconds;
    }
    if playback.state.mode == Mode::Fast {
        0
    } else {
        (milliseconds as f64 / playback.state.speed).min(10_000.) as u64
    }
}

pub(super) fn request(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    owner: &DocumentContext,
    arguments: &Value,
) -> Result<Value, String> {
    let request = serde_json::from_value(arguments.clone())
        .map_err(|error| format!("Invalid presentation request: {error}"))?;
    services
        .bridge
        .with_native_document_owner(&services.engine, owner, || {
            world.init_resource::<Playback>();
            let mut playback = world.resource_mut::<Playback>();
            let now = Instant::now();
            playback.observe(owner, now);
            let status = playback.control(request, now)?;
            handle.invalidate_presentation();
            Ok(json!({"presentation": status}))
        })
}

pub(crate) fn reduce(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    engine: &AppState,
    bridge: &SessionBridgeState,
    action: &NativeInterfaceAction,
    command: &Command,
) -> Result<Value, String> {
    if !super::super::is_activation(&action.control.input) {
        return Err("Playback control requires activation".into());
    }
    bridge.with_native_document_owner(engine, &action.context, || {
        handle.validate_action(action)?;
        world.init_resource::<Playback>();
        let mut playback = world.resource_mut::<Playback>();
        let now = Instant::now();
        playback.observe(&action.context, now);
        let status = playback.control(
            Request {
                command: *command,
                ..default()
            },
            now,
        )?;
        handle.invalidate_presentation();
        Ok(json!({"presentation": status}))
    })
}

pub(super) fn applied(world: &mut World, owner: &DocumentContext, result: &Value) {
    let Some(mut playback) = world.get_resource_mut::<Playback>() else {
        return;
    };
    if playback.owner.as_ref() != Some(owner) || result["applied"] != true {
        return;
    }
    playback.advance(Instant::now());
    if playback.state.paused && playback.credits > 0 {
        playback.credits -= 1;
    }
    if playback.state.mode == Mode::Present {
        playback.remaining_ms = playback.remaining_ms.max(playback.pace_ms as f64);
    }
    if !playback.state.active || playback.state.finished || playback.state.stopped {
        return;
    }
    playback.state.operation = result["name"]
        .as_str()
        .unwrap_or("")
        .trim_start_matches("sketch_")
        .trim_start_matches("solid_")
        .trim_start_matches("assembly_")
        .replace('_', " ");
    let progress = &result["script_progress"];
    if let (Some(index), Some(count)) = (
        progress["steps_completed"].as_u64(),
        progress["step_count"].as_u64(),
    ) {
        if count == playback.state.step_count
            && index >= playback.state.step_index
            && index <= count
        {
            playback.state.step_index = index;
        }
    }
}

pub(super) fn caption(world: &World) -> Option<String> {
    let playback = world.get_resource::<Playback>()?;
    let state = &playback.state;
    if !state.active || !state.visible {
        return None;
    }
    let status = if state.stopped {
        "Playback stopped"
    } else if state.finished {
        "Playback complete"
    } else if state.paused {
        "Paused"
    } else {
        "Running"
    };
    let caption = if !state.text.is_empty() {
        &state.text
    } else {
        &state.operation
    };
    Some(format!(
        "{status} · {}/{}{}{}",
        state.step_index,
        state.step_count,
        if state.chapter.is_empty() {
            String::new()
        } else {
            format!(" · {}", state.chapter)
        },
        if caption.is_empty() {
            String::new()
        } else {
            format!(" · {caption}")
        }
    ))
}

pub(super) fn available_while_busy(world: &World, entity: Entity) -> bool {
    world
        .get::<NativeCommandBinding>(entity)
        .is_some_and(|binding| {
            matches!(
                binding.command,
                NativeCommand::Presentation(Command::Pause | Command::Stop)
            )
        })
}

pub(super) fn busy_status(world: &World) -> Option<&'static str> {
    let playback = world.get_resource::<Playback>()?;
    if playback.state.stopped {
        Some("Playback stopped. Finishing the current modeling operation…")
    } else if playback.state.paused {
        Some("Playback paused. Finishing the current modeling operation…")
    } else {
        None
    }
}

/// Pause and Stop affect only this owner's UI playback state. They can be
/// recorded while the kernel owns the model lock; the current operation ends
/// normally and the inbox gate observes this state before starting another.
pub(super) fn busy_input(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    input: &NativeHostInput,
) -> Result<bool, String> {
    use crate::native_viewport::interface_shell::{PointerButton, PointerPhase};
    let Some(playback) = world.get_resource::<Playback>() else {
        return Ok(false);
    };
    if input.context != playback.owner || input.context != handle.presented_context() {
        return Ok(false);
    }
    let captured = playback.busy_pointer && handle.has_capture();
    if playback.busy_pointer && !captured {
        world.resource_mut::<Playback>().busy_pointer = false;
    }
    if matches!(
        input.event,
        WindowEvent::WindowFocused(bevy::window::WindowFocused { focused: false, .. })
            | WindowEvent::CursorLeft(_)
    ) {
        if captured {
            handle.cancel_pointer();
        }
        world.resource_mut::<Playback>().busy_pointer = false;
        return Ok(false);
    }
    let Some(cursor) = input.cursor else {
        return Ok(false);
    };
    let point = cursor.as_dvec2().to_array();
    let phase = match &input.event {
        WindowEvent::MouseButtonInput(button) if button.button == MouseButton::Left => {
            if button.state == ButtonState::Pressed {
                let Some(key) = handle.hit_key(point) else {
                    return Ok(false);
                };
                if !available_while_busy(world, Entity::from_bits(key.0)) {
                    return Ok(false);
                }
                world.resource_mut::<Playback>().busy_pointer = true;
                PointerPhase::Down
            } else if captured {
                world.resource_mut::<Playback>().busy_pointer = false;
                PointerPhase::Up
            } else {
                return Ok(false);
            }
        }
        WindowEvent::CursorMoved(_) if captured => PointerPhase::Move,
        _ => return Ok(false),
    };
    handle.pointer(phase, point, PointerButton::Primary)?;
    for action in handle.take_actions()? {
        let entity = Entity::from_bits(action.control.key.0);
        let Some(binding) = world.get::<NativeCommandBinding>(entity) else {
            continue;
        };
        let NativeCommand::Presentation(command @ (Command::Pause | Command::Stop)) =
            binding.command
        else {
            continue;
        };
        let Some(control) = world.get::<InterfaceControl>(entity) else {
            continue;
        };
        if control.disabled
            || !control.visible
            || control.binding != binding.generation
            || control.binding != action.control.binding()
            || !super::super::is_activation(&action.control.input)
        {
            continue;
        }
        handle.validate_action(&action)?;
        let mut playback = world.resource_mut::<Playback>();
        if playback.owner.as_ref() == Some(&action.context) {
            playback.control(
                Request {
                    command,
                    ..default()
                },
                Instant::now(),
            )?;
            handle.invalidate_presentation();
        }
    }
    Ok(true)
}

pub(super) fn synchronize(
    world: &mut World,
    owner: &DocumentContext,
    camera: Entity,
    width: f32,
    height: f32,
) -> Result<(), String> {
    let mut playback = world.remove_resource::<Playback>().unwrap_or_default();
    playback.observe(owner, Instant::now());
    playback.widgets.begin();
    let result = (|| {
        if playback.state.active {
            let mut controls = Vec::new();
            if !playback.state.visible {
                controls.push(("Show playback", Command::Show));
            } else {
                if !playback.state.finished && !playback.state.stopped {
                    controls.push(if playback.state.paused {
                        ("Resume", Command::Resume)
                    } else {
                        ("Pause", Command::Pause)
                    });
                    controls.push(("Step", Command::Step));
                    controls.push(("Stop", Command::Stop));
                }
                controls.push(("Hide playback", Command::Dismiss));
            }
            let button_width = |command: &Command| {
                if matches!(command, Command::Show | Command::Dismiss) {
                    120.
                } else {
                    80.
                }
            };
            let total: f32 = controls
                .iter()
                .map(|(_, command)| button_width(command) + 4.)
                .sum();
            let mut left = (width - total - 8.).max(0.);
            for (index, (label, command)) in controls.into_iter().enumerate() {
                let control_width = button_width(&command);
                playback.widgets.button(
                    world,
                    camera,
                    &format!("playback-{index}"),
                    InterfaceControl::button("document/presentation", label),
                    None,
                    NativeCommand::Presentation(command),
                    chrome::rect(left, height - 76., control_width, 24.),
                    None,
                    24,
                )?;
                left += control_width + 4.;
            }
        }
        playback.widgets.finish(world);
        Ok(())
    })();
    world.insert_resource(playback);
    result
}

#[cfg(test)]
mod tests;
