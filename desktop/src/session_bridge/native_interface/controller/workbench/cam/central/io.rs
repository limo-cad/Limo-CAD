//! Disk work uses the existing ordered native worker. Collection writes carry
//! the exact opened path/revision; project writes keep their original receipt.
use super::*;

pub(super) fn decode(value: &Value) -> Result<model::Snapshot, String> {
    model::Snapshot::new(crate::cam_library::Snapshot {
        json: match value.get("json") {
            Some(Value::Null) => None,
            Some(Value::String(text)) => Some(text.clone()),
            _ => return Err("The library worker returned an invalid snapshot".into()),
        },
        path: value["path"]
            .as_str()
            .ok_or("The library worker omitted its path")?
            .into(),
        revision: value["revision"]
            .as_str()
            .ok_or("The library worker omitted its revision")?
            .into(),
    })
}

fn enqueue(
    world: &mut World,
    receipt: DocumentReceipt,
    serial: u64,
    selected: Option<u64>,
    message: &'static str,
    operation: impl FnOnce() -> Result<crate::cam_library::Snapshot, String> + Send + 'static,
) -> Result<Value, String> {
    task(
        world,
        receipt,
        serial,
        move || serde_json::to_value(operation()?).map_err(|error| error.to_string()),
        move |state, value| {
            install(state, &value, selected, message)?;
            Ok(json!({"completed":true}))
        },
    )
}

pub(super) fn install(
    state: &mut State,
    value: &Value,
    selected: Option<u64>,
    message: &str,
) -> Result<(), String> {
    let previous = state.draft.take();
    state.snapshot = Some(Arc::new(decode(value)?));
    select(state, selected)?;
    if let Some(next) = state.draft.as_mut() {
        presets::retain(previous.as_ref(), next, state.project.units);
    }
    state.status = message.into();
    state.error.clear();
    Ok(())
}

pub(super) fn task(
    world: &mut World,
    receipt: DocumentReceipt,
    serial: u64,
    operation: impl FnOnce() -> Result<Value, String> + Send + 'static,
    complete: impl FnOnce(&mut State, Value) -> Result<Value, String> + Send + 'static,
) -> Result<Value, String> {
    worker::enqueue_document_io(
        world,
        "cam_tool_library".into(),
        move |services, guard| {
            services.bridge.with_native_document_receipt(
                &services.engine,
                &receipt.owner,
                |revision| {
                    if revision != receipt.revision {
                        return Err(
                            "The project changed before the library action could start".into()
                        );
                    }
                    guard.validate()
                },
            )?;
            let value = operation()?;
            Ok(NativeMutationResult {
                context: receipt.owner,
                engine_revision: receipt.revision,
                value,
            })
        },
        move |world, _, result| {
            let mut state = world.remove_resource::<State>().unwrap_or_default();
            let result = (|| {
                let result = result?;
                if state.serial != serial
                    || state.receipt.as_ref().is_none_or(|receipt| {
                        receipt.owner != result.context
                            || receipt.revision != result.engine_revision
                    })
                {
                    return Ok(json!({"completed":true,"dialog_changed":true}));
                }
                complete(&mut state, result.value)
            })();
            if let Err(error) = &result {
                if state.serial == serial {
                    state.error = error.clone();
                    state.status.clear();
                }
            }
            world.insert_resource(state);
            result
        },
    )
}

pub(super) fn load(
    world: &mut World,
    receipt: DocumentReceipt,
    serial: u64,
    selected: Option<u64>,
) -> Result<Value, String> {
    enqueue(
        world,
        receipt,
        serial,
        selected,
        "Central tools loaded. Imports and publishes are explicit snapshots.",
        || crate::cam_library::load(&crate::app_config::native_directory()?),
    )
}

pub(super) fn save(
    world: &mut World,
    receipt: DocumentReceipt,
    serial: u64,
    snapshot: Arc<model::Snapshot>,
    edit: model::Edit,
    message: &'static str,
) -> Result<Value, String> {
    let selected = edit.tool.as_ref().map(|tool| tool.id);
    enqueue(world, receipt, serial, selected, message, move || {
        crate::cam_library::save(
            &crate::app_config::native_directory()?,
            &edit.json,
            &snapshot.path,
            &snapshot.revision,
        )
    })
}

pub(super) fn import(
    world: &mut World,
    receipt: DocumentReceipt,
    next: CamDocumentDto,
    id: u64,
) -> Result<Value, String> {
    worker::enqueue_operation(
        world,
        receipt.owner,
        receipt.revision,
        "cam_set_document".into(),
        serde_json::to_value(next).map_err(|error| error.to_string())?,
        move |world, services, result| {
            let result = result?;
            let owner = result.context.clone();
            let value = finish_mutation(
                &services.engine,
                &services.bridge,
                world,
                "cam_set_document",
                result,
            );
            if let Some(mut state) = world.get_resource_mut::<State>() {
                close(&mut state);
            }
            if let Some(mut editor) = world.get_resource_mut::<super::super::Editor>() {
                if super::super::super::same_document(editor.owner.as_ref(), &owner) {
                    editor.tab = Tab::Tools;
                    editor.pending_selection = Some(Selection::Tool(id));
                }
            }
            Ok(value)
        },
    )
}

/// Desktop project creation also defines a central cutter. The project copy
/// may need a different free ID; that never renumbers the central collection.
pub(in super::super) fn create_project_tool(
    world: &mut World,
    receipt: DocumentReceipt,
    project: CamDocumentDto,
    tool: CamToolDto,
) -> Result<Value, String> {
    worker::enqueue_transaction(
        world,
        "cam_set_document".into(),
        move |services, guard| {
            services.bridge.with_native_document_receipt(
                &services.engine,
                &receipt.owner,
                |revision| {
                    if revision != receipt.revision {
                        return Err("The project changed before creating its tool".into());
                    }
                    guard.validate()
                },
            )?;
            let config = crate::app_config::native_directory()?;
            let (tool, note) = match crate::cam_library::load(&config) {
                Ok(source) => {
                    let snapshot = model::Snapshot::new(source)?;
                    let edit = snapshot.add(tool, None)?;
                    let tool = edit.tool.clone().ok_or("New central tool missing")?;
                    crate::cam_library::save(
                        &config,
                        &edit.json,
                        &snapshot.path,
                        &snapshot.revision,
                    )?;
                    (tool, None)
                }
                Err(error) => (
                    tool,
                    Some(format!(
                        "Saved in the project only; central library unavailable: {error}"
                    )),
                ),
            };
            let next = model::import_created(&project, tool.clone())?;
            let added = next
                .tools
                .iter()
                .find(|entry| !project.tools.iter().any(|prior| prior.id == entry.id))
                .ok_or("New project tool missing")?
                .id;
            let centrally_registered = note.is_none();
            let mut result = services.bridge.apply_native_mutation_at(
                &services.engine,
                &receipt.owner,
                receipt.revision,
                "cam_set_document",
                &serde_json::to_value(next).map_err(|error|error.to_string())?,
                || Ok(()),
            ).map_err(|error| if centrally_registered {
                format!("The central tool was created, but the project could not accept it: {error}. Import the saved central tool explicitly.")
            } else { error })?;
            result.value = json!({"project_tool_id":added,"notice":note});
            Ok(result)
        },
        |world, services, result| {
            let result = result?;
            let id = result.value["project_tool_id"]
                .as_u64()
                .ok_or("New project tool identity missing")?;
            let notice = result.value["notice"].as_str().unwrap_or("").to_owned();
            let owner = result.context.clone();
            let value = finish_mutation(
                &services.engine,
                &services.bridge,
                world,
                "cam_set_document",
                result,
            );
            if let Some(mut editor) = world.get_resource_mut::<super::super::Editor>() {
                if super::super::super::same_document(editor.owner.as_ref(), &owner) {
                    editor.pending_selection = Some(Selection::Tool(id));
                    editor.pending_notice = Some(notice);
                }
            }
            Ok(value)
        },
    )
}
