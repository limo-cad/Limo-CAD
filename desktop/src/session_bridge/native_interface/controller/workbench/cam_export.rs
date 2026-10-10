//! Native NC review and export over the shared, verified CAM post boundary.
//! A prepared program belongs to one exact document receipt until it is saved.
use super::*;
use crate::session_bridge::native_interface::workspace::DocumentReceipt;
use limo_cad_cam::{
    CamDocumentDto, CamPostConfigDto, CamPostRequestDto, CamPostResultDto, CamUnits,
    PostEventStreamDto,
};
use limo_cad_interface::{ChoiceOption, ControlInput, Field, KeyChord};
use std::{path::PathBuf, sync::mpsc};

mod io;
mod panel;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Open,
    OpenEvents,
    Name,
    ProgramNumber,
    SequenceNumbers,
    ReviewMachine,
    Prepare,
    BackToSettings,
    Page(i32),
    Save,
    Close,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Nc,
    Events,
}
struct Draft {
    receipt: DocumentReceipt,
    kind: Kind,
    setup_id: u64,
    setup_name: String,
    setup_summary: String,
    machine_name: Option<String>,
    post: CamPostConfigDto,
    units: CamUnits,
    program_name: String,
    program_number: String,
    sequence_numbers: bool,
    reviewed_machine: bool,
}
impl Draft {
    fn new(
        receipt: DocumentReceipt,
        document: &CamDocumentDto,
        setup_id: u64,
        kind: Kind,
    ) -> Result<Self, String> {
        let setup = document.setup(setup_id).ok_or("Choose a setup to post")?;
        let post = setup.machine.as_ref().map_or_else(
            || document.post_defaults.clone(),
            |m| m.profile.post.clone(),
        );
        let mut setup_summary = format!(
            "Output units: {}\nWork offset: {:?} · {} consecutive offset(s)\n",
            document.units.length_label(),
            setup.work_offset,
            setup.work_offset_count
        );
        if let Some(machine) = &setup.machine {
            setup_summary.push_str(&format!(
                "Profile: {} revision {}\nController: {}\nChannel: {}\n",
                machine.profile.id,
                machine.profile.revision,
                machine.profile.controller.model,
                machine.channel_id
            ));
        }
        for tool in document.tools.iter().filter(|tool| {
            setup
                .operations
                .iter()
                .any(|op| op.enabled() && op.tool_id() == tool.id)
        }) {
            setup_summary.push_str(&format!(
                "Tool: {} · {} · diameter {} {}\n",
                tool.name,
                tool.number
                    .map_or_else(|| "No tool number".into(), |number| format!("T{number}")),
                document.units.from_mm(tool.diameter),
                document.units.length_label()
            ));
        }
        Ok(Self {
            receipt,
            kind,
            setup_id,
            setup_name: setup.name.clone(),
            setup_summary,
            machine_name: setup.machine.as_ref().map(|m| m.profile.name.clone()),
            program_name: setup.name.clone(),
            program_number: post.program_number.map_or(String::new(), |n| n.to_string()),
            sequence_numbers: post.sequence_numbers,
            units: document.units,
            post,
            reviewed_machine: false,
        })
    }
    fn request(&self) -> Result<(String, Value), String> {
        if !self.reviewed_machine {
            return Err("Review the setup and machine settings before preparing output".into());
        }
        if self.kind == Kind::Events {
            return Ok(("cam_post_events".into(), json!(self.setup_id)));
        }
        if self.machine_name.is_none() {
            return Err("Choose and review a machine in the setup before posting NC".into());
        }
        let mut post = self.post.clone();
        post.program_number = if self.program_number.trim().is_empty() {
            None
        } else {
            Some(
                self.program_number
                    .trim()
                    .parse::<u32>()
                    .map_err(|_| "Program number must be a whole non-negative number")?,
            )
        };
        post.sequence_numbers = self.sequence_numbers;
        let request = CamPostRequestDto {
            setup_id: self.setup_id,
            post: Some(post),
            program_name: (!self.program_name.trim().is_empty())
                .then(|| self.program_name.trim().to_owned()),
        };
        Ok((
            "cam_post".into(),
            serde_json::to_value(request).map_err(|e| e.to_string())?,
        ))
    }
}
#[derive(Clone)]
struct Prepared {
    receipt: DocumentReceipt,
    bytes: Vec<u8>,
    extension: String,
    file_name: String,
    summary: String,
    warnings: Vec<String>,
}
impl Prepared {
    fn from_result(draft: &Draft, value: Value) -> Result<Self, String> {
        let (bytes, extension, summary, warnings) = match draft.kind {
            Kind::Nc => {
                let result: CamPostResultDto =
                    serde_json::from_value(value).map_err(|e| e.to_string())?;
                let summary = format!(
                    "{} operations · {:.1} s · {} NC lines · {:?}",
                    result.program.stats.operation_count,
                    result.program.stats.estimated_seconds,
                    result.nc.lines().count(),
                    result.dialect
                );
                (
                    result.nc.into_bytes(),
                    result.extension.trim_start_matches('.').to_owned(),
                    summary,
                    result.warnings,
                )
            }
            Kind::Events => {
                let result: PostEventStreamDto =
                    serde_json::from_value(value).map_err(|e| e.to_string())?;
                let mut bytes = serde_json::to_vec_pretty(&result).map_err(|e| e.to_string())?;
                bytes.push(b'\n');
                let summary = format!(
                    "{} post events · {} · event format {}",
                    result.events.len(),
                    result.units,
                    result.version
                );
                (bytes, "json".into(), summary, result.warnings)
            }
        };
        if extension.is_empty() || !extension.bytes().all(|c| c.is_ascii_alphanumeric()) {
            return Err("The post returned an invalid filename extension".into());
        }
        let name = if draft.kind == Kind::Events || draft.program_name.trim().is_empty() {
            &draft.setup_name
        } else {
            &draft.program_name
        };
        let suffix = if draft.kind == Kind::Events {
            "-post-events"
        } else {
            ""
        };
        Ok(Self {
            receipt: draft.receipt.clone(),
            bytes,
            file_name: format!("{}{suffix}.{extension}", io::safe_stem(name)),
            extension,
            summary,
            warnings,
        })
    }
}
struct Picker {
    serial: u64,
    prepared: Prepared,
    receive: Mutex<mpsc::Receiver<Option<PathBuf>>>,
}
#[derive(Resource, Default)]
struct State {
    draft: Option<Draft>,
    prepared: Option<Prepared>,
    serial: u64,
    preparing: bool,
    saving: bool,
    picker: Option<Picker>,
    page: usize,
    error: String,
    status: String,
    widgets: Widgets,
}

pub(crate) fn modal(world: &World) -> Option<&'static str> {
    world
        .get_resource::<State>()
        .filter(|s| s.draft.is_some())
        .map(|_| "cam-export")
}
pub(crate) fn caption(world: &World) -> Option<String> {
    let state = world.get_resource::<State>()?;
    state.draft.as_ref()?;
    Some(format!(
        "{}\n{}\n{}",
        panel::review_text(state),
        state.error,
        state.status
    ))
}
pub(crate) fn escape(world: &mut World) {
    if let Some(mut state) = world.get_resource_mut::<State>() {
        state.serial = state.serial.wrapping_add(1);
        state.draft = None;
        state.prepared = None;
        state.picker = None;
        state.preparing = false;
        state.saving = false;
    }
}
fn setup_id(document: &CamDocumentDto, selection: Option<cam::Selection>) -> Option<u64> {
    match selection {
        Some(cam::Selection::Setup(id)) => document.setup(id).map(|s| s.id),
        Some(cam::Selection::Operation(id)) => document
            .setups
            .iter()
            .find(|s| s.operations.iter().any(|o| o.id() == id))
            .map(|s| s.id),
        _ => document.active_setup_id,
    }
}
fn current(receipt: &DocumentReceipt, revision: u64) -> Result<(), String> {
    if receipt.revision != revision {
        return Err("The project changed; prepare and review the output again".into());
    }
    Ok(())
}

pub(crate) fn reduce(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    engine: &AppState,
    bridge: &SessionBridgeState,
    action: &NativeInterfaceAction,
    command: &Command,
) -> Result<Value, String> {
    let receipt = bridge.native_document_receipt(engine, &action.context)?;
    bridge
        .with_native_document_owner(engine, &action.context, || handle.validate_action(action))?;
    if workspace(world) != Workspace::Cam {
        return Err("Open the CAM workspace".into());
    }
    if matches!(command, Command::Open | Command::OpenEvents) {
        if !super::super::super::is_activation(&action.control.input) {
            return Err("Activate Post NC or Post events".into());
        }
        if cam::editing_dirty(world) {
            return Err("Apply or reset the CAM edits before posting".into());
        }
        if worker::busy(world) {
            return Err("Wait for the current operation to finish".into());
        }
        if files::modal(world).is_some() || history::modal(world).is_some() {
            return Err("Finish the current dialog first".into());
        }
        let document = bridge.with_native_document_receipt(engine, &receipt.owner, |revision| {
            current(&receipt, revision)?;
            Ok(engine.cam_document_snapshot())
        })?;
        let id = setup_id(&document, cam::selected(world)).ok_or("Choose a CAM setup")?;
        let draft = Draft::new(
            receipt,
            &document,
            id,
            if *command == Command::OpenEvents {
                Kind::Events
            } else {
                Kind::Nc
            },
        )?;
        world.init_resource::<State>();
        let mut state = world.resource_mut::<State>();
        state.serial = state.serial.wrapping_add(1);
        state.draft = Some(draft);
        state.prepared = None;
        state.picker = None;
        state.preparing = false;
        state.saving = false;
        state.page = 0;
        state.error.clear();
        state.status.clear();
        return Ok(json!({"opened":true}));
    }
    let mut state = world
        .remove_resource::<State>()
        .ok_or("Open Post NC first")?;
    let result = (|| {
        let draft = state.draft.as_mut().ok_or("Open Post NC first")?;
        if draft.receipt != receipt {
            return Err("The project changed; reopen Post NC".into());
        }
        if *command == Command::Close {
            state.draft = None;
            state.prepared = None;
            state.picker = None;
            state.serial = state.serial.wrapping_add(1);
            return Ok(json!({"closed":true}));
        }
        if state.preparing || state.saving || state.picker.is_some() {
            return Err("Wait for the current export step to finish".into());
        }
        if matches!(command, Command::Name | Command::ProgramNumber) {
            if state.prepared.is_some() {
                return Err("Return to settings to change prepared program settings".into());
            }
            match &action.control.input {
                ControlInput::SetValue(value) => {
                    if *command == Command::Name {
                        draft.program_name = value.clone();
                    } else {
                        draft.program_number = value.clone();
                    }
                    draft.reviewed_machine = false;
                    state.error.clear();
                    return Ok(json!({"changed":true}));
                }
                ControlInput::Key(_) => return Ok(json!({"handled":true})),
                input if super::super::super::is_activation(input) => {
                    return Ok(json!({"focused":true}))
                }
                _ => return Err("Edit the program field with text".into()),
            }
        }
        if *command == Command::SequenceNumbers {
            if state.prepared.is_some() {
                return Err("Return to settings to change prepared program settings".into());
            }
            draft.sequence_numbers = match &action.control.input {
                ControlInput::SetValue(value) => {
                    value.parse::<bool>().map_err(|_| "Choose Yes or No")?
                }
                input if super::super::super::is_activation(input) => !draft.sequence_numbers,
                ControlInput::Key(key)
                    if [
                        "ArrowUp",
                        "ArrowDown",
                        "ArrowLeft",
                        "ArrowRight",
                        "Home",
                        "End",
                    ]
                    .contains(&key.key.as_str()) =>
                {
                    match key.key.as_str() {
                        "Home" => true,
                        "End" => false,
                        _ => !draft.sequence_numbers,
                    }
                }
                _ => return Err("Choose Yes or No".into()),
            };
            draft.reviewed_machine = false;
            state.error.clear();
            return Ok(json!({"changed":true}));
        }
        if !super::super::super::is_activation(&action.control.input) {
            return Err("Activate a posting control".into());
        }
        match command {
            Command::ReviewMachine => {
                draft.reviewed_machine = !draft.reviewed_machine;
                Ok(json!({"reviewed":draft.reviewed_machine}))
            }
            Command::BackToSettings => back_to_settings(&mut state),
            Command::Prepare => prepare(world, &mut state),
            Command::Save => io::choose(world, handle, &mut state),
            Command::Page(delta) => {
                state.page = state.page.saturating_add_signed(*delta as isize);
                Ok(json!({"handled":true}))
            }
            _ => Err("Open Post NC first".into()),
        }
    })();
    if let Err(error) = &result {
        state.error = error.clone();
    }
    world.insert_resource(state);
    result
}

fn back_to_settings(state: &mut State) -> Result<Value, String> {
    if state.prepared.is_none() {
        return Err("Prepare output first".into());
    }
    let draft = state.draft.as_mut().ok_or("Open Post NC first")?;
    draft.reviewed_machine = false;
    state.prepared = None;
    state.serial = state.serial.wrapping_add(1);
    state.page = 0;
    state.error.clear();
    state.status.clear();
    Ok(json!({"settings":true}))
}

fn prepare(world: &mut World, state: &mut State) -> Result<Value, String> {
    let draft = state.draft.as_ref().ok_or("Open Post NC first")?;
    let (operation, arguments) = draft.request()?;
    let receipt = draft.receipt.clone();
    let serial = state.serial;
    state.error.clear();
    state.status.clear();
    state.prepared = None;
    state.page = 0;
    let result = worker::enqueue_query(
        world,
        receipt.owner.clone(),
        receipt.revision,
        operation,
        arguments,
        move |world, services, result| {
            let Some(mut state) = world.remove_resource::<State>() else {
                return Ok(json!({"discarded":true}));
            };
            let output = (|| {
                if state.serial != serial
                    || state.draft.as_ref().is_none_or(|d| d.receipt != receipt)
                {
                    return Ok(json!({"discarded":true}));
                }
                state.preparing = false;
                let result = services.bridge.with_native_document_receipt(
                    &services.engine,
                    &receipt.owner,
                    |revision| {
                        current(&receipt, revision)?;
                        Prepared::from_result(state.draft.as_ref().unwrap(), result?.value)
                    },
                );
                match result {
                    Ok(prepared) => {
                        state.prepared = Some(prepared);
                        Ok(json!({"prepared":true}))
                    }
                    Err(error) => {
                        state.error = error.clone();
                        Err(error)
                    }
                }
            })();
            world.insert_resource(state);
            output
        },
    );
    state.preparing = result.is_ok();
    result
}

pub(crate) fn synchronize(
    world: &mut World,
    camera: Entity,
    services: &NativeServices,
    owner: &DocumentContext,
    (width, height, side): (f32, f32, f32),
    active: bool,
) -> Result<(), String> {
    let mut state = world.remove_resource::<State>().unwrap_or_default();
    state.widgets.begin();
    let result = (|| {
        if active {
            let disabled =
                worker::busy(world) || cam::editing_dirty(world) || state.draft.is_some();
            let x = (width - 250.).max(side + 12.);
            let y = if width >= 790. { 36. } else { 98. };
            for (index, (label, command)) in [
                ("Post NC", Command::Open),
                ("Post events", Command::OpenEvents),
            ]
            .into_iter()
            .enumerate()
            {
                let mut control = InterfaceControl::button("cam/view", label);
                control.disabled = disabled;
                state.widgets.button(
                    world,
                    camera,
                    &format!("cam-post-open-{index}"),
                    control,
                    Some(label),
                    NativeCommand::Workbench(super::Command::CamExport(command)),
                    rect(x + index as f32 * 122., y, 116., 28.),
                    None,
                    46,
                )?;
            }
        }
        if let Some(draft) = &state.draft {
            let valid = active
                && &draft.receipt.owner == owner
                && services
                    .bridge
                    .native_document_receipt(&services.engine, owner)
                    .is_ok_and(|r| r == draft.receipt);
            if !valid {
                state.draft = None;
                state.prepared = None;
                state.picker = None;
                state.serial = state.serial.wrapping_add(1);
                state.preparing = false;
                state.saving = false;
            }
        }
        if state.draft.is_none() {
            return Ok(());
        }
        if let Err(error) = io::poll(world, &mut state) {
            state.error = error;
        }
        panel::paint(world, camera, &mut state, width, height, side)
    })();
    state.widgets.finish(world);
    world.insert_resource(state);
    result
}
