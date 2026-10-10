//! Explicit central/project tool synchronization over the existing library.
//! The library's path/revision and the project's owner/revision are independent
//! receipts. Neither grants permission to replace the other implicitly.
use super::*;
use crate::session_bridge::native_interface::workspace::DocumentReceipt;
use limo_cad_cam::CamToolDto;
use std::sync::Arc;

mod io;
mod model;
mod panel;
mod project;
mod storage;
#[cfg(test)]
mod tests;
pub(super) use io::create_project_tool;
pub(super) use project::{copy_tool, create_copy};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Open,
    Close,
    Refresh,
    Select(u64),
    New,
    Duplicate,
    Delete,
    Apply,
    Reset,
    Edit(usize),
    Preset(presets::Command),
    Page(i32),
    Fields(i32),
    Import,
    Publish,
    Storage(storage::Command),
}

#[derive(Resource, Default)]
struct State {
    receipt: Option<DocumentReceipt>,
    serial: u64,
    snapshot: Option<Arc<model::Snapshot>>,
    project: CamDocumentDto,
    project_tool: Option<u64>,
    draft: Option<Draft>,
    creating: bool,
    copied_from: Option<u64>,
    page: usize,
    list_size: usize,
    field_page: usize,
    status: String,
    error: String,
    widgets: Widgets,
    storage: Option<storage::State>,
}

pub(super) fn modal(world: &World) -> Option<&'static str> {
    world
        .get_resource::<State>()
        .filter(|state| state.receipt.is_some())
        .map(|_| "cam-library")
}

pub(super) fn caption(world: &World) -> Option<String> {
    let state = world.get_resource::<State>()?;
    state.receipt.as_ref()?;
    Some(format!(
        "Central tool library\nLibrary path: {}\n{}\n{}\n{}",
        state
            .snapshot
            .as_ref()
            .map_or("", |snapshot| snapshot.path.as_str()),
        state.status,
        state.error,
        storage::caption(state)
    ))
}

pub(super) fn escape(world: &mut World) {
    if worker::busy(world) || awaiting(world) {
        return;
    }
    if let Some(mut state) = world.get_resource_mut::<State>() {
        if dirty(&state) {
            state.error = "Apply or reset the current library edit first".into();
            return;
        }
        close(&mut state);
    }
}

fn close(state: &mut State) {
    state.serial = state.serial.wrapping_add(1);
    state.receipt = None;
    state.draft = None;
    state.creating = false;
    state.copied_from = None;
    if !storage::awaiting(state) {
        state.storage = None;
    }
}

pub(super) fn awaiting(world: &World) -> bool {
    world.get_resource::<State>().is_some_and(storage::awaiting)
}

fn select(state: &mut State, id: Option<u64>) -> Result<(), String> {
    let snapshot = state
        .snapshot
        .as_ref()
        .ok_or("Load the central library first")?;
    let id = id
        .filter(|id| snapshot.tools.iter().any(|tool| tool.id == *id))
        .or_else(|| snapshot.tools.first().map(|tool| tool.id));
    state.draft = id
        .map(|id| {
            Draft::new(
                &snapshot.form_document(state.project.units),
                Selection::Tool(id),
            )
        })
        .transpose()?;
    state.creating = false;
    state.copied_from = None;
    state.field_page = 0;
    if let Some(id) = id {
        state.page = snapshot
            .tools
            .iter()
            .position(|tool| tool.id == id)
            .unwrap()
            / if state.list_size == 0 {
                5
            } else {
                state.list_size
            };
    }
    Ok(())
}

fn selected_id(state: &State) -> Option<u64> {
    match state.draft.as_ref()?.selection {
        Selection::Tool(id) if id != 0 => Some(id),
        _ => None,
    }
}

fn dirty(state: &State) -> bool {
    state.creating || state.draft.as_ref().is_some_and(Draft::dirty)
}

fn edited_tool(state: &State) -> Result<CamToolDto, String> {
    let draft = state.draft.as_ref().ok_or("Select a central tool")?;
    let snapshot = state
        .snapshot
        .as_ref()
        .ok_or("Load the central library first")?;
    let form = snapshot.form_document(state.project.units);
    if draft.creation.is_some() {
        let isolated = CamDocumentDto {
            units: form.units,
            next_tool_id: form.next_tool_id,
            ..Default::default()
        };
        let (next, Selection::Tool(id)) = creation::create(draft, &isolated)? else {
            return Err("Create a central tool".into());
        };
        return next
            .tool(id)
            .cloned()
            .ok_or("The new cutter is unavailable".into());
    }
    let next = draft.edited_without_validation(&form)?;
    let Selection::Tool(id) = draft.selection else {
        return Err("Select a central tool".into());
    };
    next.tool(id)
        .cloned()
        .ok_or("The central cutter is unavailable".into())
}

fn edit(state: &mut State, index: usize, input: &ControlInput) -> Result<(), String> {
    let units = state.project.units;
    let draft = state.draft.as_mut().ok_or("Select a central tool")?;
    let field = draft
        .fields
        .get_mut(index)
        .ok_or("The tool field was removed")?;
    if let Some(options) = field.options.as_ref() {
        field.text = super::choose(options, &field.text, input)?;
    } else {
        match input {
            ControlInput::SetValue(text) => field.text = text.clone(),
            ControlInput::Key(_) => return Ok(()),
            input if super::super::super::super::is_activation(input) => return Ok(()),
            _ => return Err("Edit the central tool field with text".into()),
        }
    }
    let path = field.path.clone();
    if path == "/native/ui/tool_section" {
        state.field_page = 0;
    }
    tool::changed(draft, &path);
    presets::changed_tool(draft, units, &path)?;
    Ok(())
}

pub(super) fn execute(
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
    if *command == Command::Open {
        if !super::super::super::super::is_activation(&action.control.input) {
            return Err("Open the tool library".into());
        }
        if super::editing_dirty(world) {
            return Err("Apply or reset the project tool first".into());
        }
        if worker::busy(world) {
            return Err("Wait for the current operation".into());
        }
        if super::awaiting(world) || files::awaiting(world) {
            return Err("Finish the library folder chooser first".into());
        }
        let project_tool = match super::selected(world) {
            Some(Selection::Tool(id)) => Some(id),
            _ => None,
        };
        let project = bridge.with_native_document_receipt(engine, &receipt.owner, |revision| {
            if revision != receipt.revision {
                return Err("The project changed before opening the library".into());
            }
            Ok(engine.cam_document_snapshot())
        })?;
        world.init_resource::<State>();
        let mut state = world.remove_resource::<State>().unwrap();
        state.serial = state.serial.wrapping_add(1);
        state.receipt = Some(receipt.clone());
        state.project = project;
        state.project_tool = project_tool;
        state.draft = None;
        state.snapshot = None;
        state.error.clear();
        state.status = "Loading central tools…".into();
        let serial = state.serial;
        world.insert_resource(state);
        return io::load(world, receipt, serial, None);
    }
    if worker::busy(world) || awaiting(world) {
        return Err("Wait for the current library operation".into());
    }
    let mut state = world
        .remove_resource::<State>()
        .ok_or("Open the central tool library")?;
    let result = (|| {
        if state.receipt.as_ref() != Some(&receipt) {
            return Err("The project changed; reopen the tool library".into());
        }
        if let Command::Edit(index) = *command {
            edit(&mut state, index, &action.control.input)?;
            state.error.clear();
            return Ok(json!({"changed":true}));
        }
        if let Command::Storage(storage::Command::EditPath) = *command {
            return storage::execute(
                world,
                &mut state,
                &action.control.input,
                storage::Command::EditPath,
            );
        }
        if !super::super::super::super::is_activation(&action.control.input) {
            return Err("Activate a library control".into());
        }
        if dirty(&state)
            && matches!(
                command,
                Command::Refresh
                    | Command::Select(_)
                    | Command::New
                    | Command::Duplicate
                    | Command::Delete
                    | Command::Import
                    | Command::Publish
                    | Command::Close
                    | Command::Storage(_)
            )
        {
            return Err("Apply or reset the current library edit first".into());
        }
        state.error.clear();
        match *command {
            Command::Close => close(&mut state),
            Command::Refresh => {
                state.status = "Refreshing central tools…".into();
                let selected = selected_id(&state);
                return io::load(world, receipt, state.serial, selected);
            }
            Command::Select(id) => select(&mut state, Some(id))?,
            Command::New => {
                let snapshot = state
                    .snapshot
                    .as_ref()
                    .ok_or("Load the central library first")?;
                let cam = snapshot.form_document(state.project.units);
                let context = creation::Context::shared(&Default::default(), &cam)?;
                state.draft = Some(creation::draft(Tab::Tools, &cam, context));
                state.creating = true;
                state.copied_from = None;
                state.field_page = 0;
            }
            Command::Duplicate => {
                let id = selected_id(&state).ok_or("Select a central tool")?;
                let snapshot = state.snapshot.as_ref().unwrap();
                let cam = snapshot.form_document(state.project.units);
                let source = cam.tool(id).ok_or("The central tool was removed")?;
                let mut draft = Draft::new(&cam, Selection::Tool(id))?;
                form::set(&mut draft, "/name", &format!("{} copy", source.name));
                let number = cam
                    .tools
                    .iter()
                    .filter_map(|tool| tool.number)
                    .max()
                    .unwrap_or(0)
                    .checked_add(1);
                form::set(
                    &mut draft,
                    "/number",
                    &number.map_or(String::new(), |value| value.to_string()),
                );
                state.draft = Some(draft);
                state.creating = true;
                state.copied_from = Some(id);
                state.field_page = 0;
            }
            Command::Delete => {
                let snapshot = state
                    .snapshot
                    .as_ref()
                    .ok_or("Load the central library first")?
                    .clone();
                let edit = snapshot.remove(selected_id(&state).ok_or("Select a central tool")?)?;
                state.status = "Deleting central tool…".into();
                return io::save(
                    world,
                    receipt,
                    state.serial,
                    snapshot,
                    edit,
                    "Deleted central tool",
                );
            }
            Command::Apply => {
                if !dirty(&state) {
                    return Ok(json!({"unchanged":true}));
                }
                let snapshot = state
                    .snapshot
                    .as_ref()
                    .ok_or("Load the central library first")?
                    .clone();
                let tool = edited_tool(&state)?;
                let edit = if state.creating {
                    snapshot.add(tool, state.copied_from)?
                } else {
                    snapshot.update(tool)?
                };
                state.status = "Saving central tool…".into();
                return io::save(
                    world,
                    receipt,
                    state.serial,
                    snapshot,
                    edit,
                    "Saved central tool",
                );
            }
            Command::Reset => {
                let previous = state.draft.take();
                let id = previous.as_ref().and_then(|draft| match draft.selection {
                    Selection::Tool(id) if id != 0 => Some(id),
                    _ => None,
                });
                select(&mut state, id)?;
                if let Some(next) = state.draft.as_mut() {
                    presets::retain(previous.as_ref(), next, state.project.units);
                }
            }
            Command::Preset(command) => {
                presets::edit_tool(
                    state.draft.as_mut().ok_or("Select a tool")?,
                    state.project.units,
                    command,
                )?;
                state.field_page = 0;
            }
            Command::Page(delta) => state.page = state.page.saturating_add_signed(delta as isize),
            Command::Fields(delta) => {
                state.field_page = state.field_page.saturating_add_signed(delta as isize)
            }
            Command::Import => {
                let id = selected_id(&state).ok_or("Select a central tool to import")?;
                let next = state
                    .snapshot
                    .as_ref()
                    .ok_or("Load the central library first")?
                    .import(&state.project, id)?;
                return io::import(world, receipt, next, id);
            }
            Command::Publish => {
                let id = state
                    .project_tool
                    .ok_or("Select a project tool before opening the library")?;
                let tool = state
                    .project
                    .tool(id)
                    .cloned()
                    .ok_or("The project tool was removed")?;
                let snapshot = state
                    .snapshot
                    .as_ref()
                    .ok_or("Load the central library first")?
                    .clone();
                let edit = snapshot.publish(tool)?;
                state.status = "Publishing project tool…".into();
                return io::save(
                    world,
                    receipt,
                    state.serial,
                    snapshot,
                    edit,
                    "Published the project snapshot to the central library",
                );
            }
            Command::Storage(command) => {
                return storage::execute(world, &mut state, &action.control.input, command)
            }
            Command::Open | Command::Edit(_) => unreachable!(),
        }
        Ok(json!({"changed":true}))
    })();
    if let Err(error) = &result {
        state.error = error.clone();
    }
    world.insert_resource(state);
    result
}

pub(super) fn synchronize(
    world: &mut World,
    camera: Entity,
    services: &NativeServices,
    owner: &DocumentContext,
    width: f32,
    height: f32,
    active: bool,
) -> Result<(), String> {
    let mut state = world.remove_resource::<State>().unwrap_or_default();
    state.widgets.begin();
    let result = (|| {
        if let Some(receipt) = state.receipt.as_ref() {
            let current = services
                .bridge
                .native_document_receipt(&services.engine, owner)?;
            if !active || receipt != &current {
                close(&mut state);
            }
        }
        if let Err(error) = storage::poll(world, &mut state) {
            state.error = error;
        }
        if state.receipt.is_some() {
            panel::paint(world, camera, &mut state, width, height)?;
        }
        Ok(())
    })();
    state.widgets.finish(world);
    world.insert_resource(state);
    result
}
