//! Authored JSONC drafts, inspected by the shared loader and saved explicitly.
use super::*;
use crate::native_viewport::interface_shell::fields;
use limo_cad_interface::Field;

pub(super) enum Change {
    Validated {
        generation: u64,
        loaded: Loaded,
    },
    Saved {
        generation: u64,
        path: PathBuf,
        source: String,
    },
}

pub(super) fn limit_error(world: &World) -> Option<String> {
    let state = &world.resource::<Files>().script;
    state
        .source_entity
        .and_then(|entity| fields::limits::error(world, entity))
        .map(str::to_owned)
        .or_else(|| state.source_error.clone())
}

pub(super) fn source_ready(world: &World) -> Result<(), String> {
    limit_error(world).map_or(Ok(()), Err)
}

pub(crate) fn retain_source_error(world: &mut World) {
    if let Some(error) = limit_error(world) {
        let buffer = world
            .resource::<Files>()
            .script
            .source_entity
            .and_then(|entity| world.get::<bevy::text::EditableText>(entity))
            .map(|editor| editor.value().to_string());
        let state = &mut world.resource_mut::<Files>().script;
        state.source_error = Some(error);
        if let Some(buffer) = buffer.filter(|buffer| *buffer != state.source) {
            if let Err(error) = state.advance() {
                state.status = Some(error);
            }
            state.source = buffer;
            state.validated = false;
        }
    }
}

fn clear_limit(world: &mut World) {
    if let Some(entity) = world.resource::<Files>().script.source_entity {
        fields::limits::clear(world, entity);
    }
    world.resource_mut::<Files>().script.source_error = None;
}

pub(super) fn replace_ready(world: &mut World) -> Result<(), String> {
    let error = if world.resource::<Files>().script.dirty() || limit_error(world).is_some() {
        Some("Unsaved script edits: Save script as or Discard edits before opening another script")
    } else {
        None
    };
    if let Some(error) = error {
        world.resource_mut::<Files>().script.status = Some(error.into());
        return Err(error.into());
    }
    Ok(())
}

pub(crate) fn show_source(world: &mut World) -> Result<Value, String> {
    available(world)?;
    retain_source_error(world);
    let state = &mut world.resource_mut::<Files>().script;
    if state.loaded.is_none() {
        return Err("Open a script before inspecting its source".into());
    }
    state.editor_open = !state.editor_open;
    state.library.open = false;
    Ok(json!({"source_editor":state.editor_open}))
}

pub(crate) fn edit_source(world: &mut World, input: &ControlInput) -> Result<Value, String> {
    available(world)?;
    let ControlInput::SetValue(source) = input else {
        return Err("Script source requires text".into());
    };
    let maximum = world
        .resource::<Files>()
        .script
        .loaded
        .as_ref()
        .ok_or("Open a script before editing its source")?
        .maximum;
    if source.len() > maximum {
        let error = format!("Script source exceeds {maximum} bytes; the edit was not inserted");
        if let Some(entity) = world.resource::<Files>().script.source_entity {
            if let Some(mut limit) = world.get_mut::<fields::limits::ByteLimit>(entity) {
                limit.rejected = Some(error.clone());
            }
        }
        world.resource_mut::<Files>().script.source_error = Some(error.clone());
        return Err(error);
    }
    {
        let state = &mut world.resource_mut::<Files>().script;
        if state.source != *source {
            state.advance()?;
            state.source = source.clone();
            state.validated = false;
            state.status = Some("Source changed; Validate before Run. Edits are not saved.".into());
        }
    }
    clear_limit(world);
    Ok(json!({"changed":true}))
}

pub(crate) fn discard(world: &mut World) -> Result<Value, String> {
    available(world)?;
    {
        let state = &mut world.resource_mut::<Files>().script;
        state.advance()?;
        state.editor_generation = state.generation;
        state.source = state.baseline.clone();
        state.validated = false;
        state.status =
            Some("Edits discarded; Validate to refresh included files before Run.".into());
    }
    clear_limit(world);
    Ok(json!({"discarded":true}))
}

fn inspect_source(path: Option<PathBuf>, source: &str) -> Result<Loaded, String> {
    let mut arguments = json!({"source":source});
    if let Some(path) = &path {
        let base = path
            .parent()
            .and_then(|path| path.to_str())
            .ok_or("Script include directory must be valid Unicode")?;
        arguments["include_base"] = json!(base);
    }
    let inspection = limo_cad_mcp::inspect_script(arguments)?;
    inspected(path, inspection)
}

fn start(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    status: &str,
    task: impl FnOnce() -> Result<Change, String> + Send + 'static,
) -> Result<Value, String> {
    let (send, receive) = mpsc::channel();
    let wake = handle.clone();
    std::thread::Builder::new()
        .name("cad-script-source".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task))
                .unwrap_or_else(|_| Err("Script file operation stopped unexpectedly".into()));
            let _ = send.send(result);
            wake.request_redraw();
        })
        .map_err(|error| format!("Cannot start script file operation: {error}"))?;
    let state = &mut world.resource_mut::<Files>().script;
    state.job = Some(Mutex::new(receive));
    state.status = Some(status.into());
    Ok(json!({"script_pending":true}))
}

pub(crate) fn validate(world: &mut World, handle: &NativeInterfaceHandle) -> Result<Value, String> {
    available(world)?;
    source_ready(world)?;
    let (path, source, generation) = {
        let state = &mut world.resource_mut::<Files>().script;
        state
            .loaded
            .as_ref()
            .ok_or("Open a script before validating it")?;
        let path = state.source_path.clone();
        let source = state.source.clone();
        state.advance()?;
        state.validated = false;
        (path, source, state.generation)
    };
    start(
        world,
        handle,
        "Validating source and includes; no commands have run",
        move || {
            inspect_source(path, &source).map(|loaded| Change::Validated { generation, loaded })
        },
    )
}

pub(crate) fn save_as(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    receipt: DocumentReceipt,
) -> Result<Value, String> {
    available(world)?;
    source_ready(world)?;
    let state = &world.resource::<Files>().script;
    let loaded = state
        .loaded
        .as_ref()
        .ok_or("Open a script before saving source")?;
    let path = state.source_path.clone();
    let suggested = path
        .as_ref()
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| {
            format!(
                "{}.limo.jsonc",
                loaded
                    .name
                    .chars()
                    .map(|ch| if "<>:\"/\\|?*".contains(ch) || ch.is_control() {
                        '_'
                    } else {
                        ch
                    })
                    .collect::<String>()
            )
        });
    let generation = state.generation;
    let (dialog, parent) = super::dialog::parented(
        world,
        rfd::FileDialog::new()
            .set_title("Save Limo CAD script")
            .add_filter("Limo CAD command script (.limo.jsonc)", &["jsonc"]),
    )?;
    let (send, receive) = mpsc::channel();
    let wake = handle.clone();
    std::thread::Builder::new()
        .name("cad-script-save-picker".into())
        .spawn(move || {
            let _parent = parent;
            let mut dialog = dialog;
            if let Some(parent) = path.as_ref().and_then(|path| path.parent()) {
                dialog = dialog.set_directory(parent);
            }
            dialog = dialog.set_file_name(suggested);
            let _ = send.send(super::dialog::pick(dialog, true));
            wake.request_redraw();
        })
        .map_err(|error| format!("Cannot open script save chooser: {error}"))?;
    world.resource_mut::<Files>().picker = Some(Picker {
        receipt,
        kind: PickerKind::ScriptSave(generation),
        result: Mutex::new(receive),
    });
    Ok(json!({"awaiting_input":true}))
}

fn write_source(path: &std::path::Path, source: &str, maximum: usize) -> Result<(), String> {
    if !path.is_absolute()
        || !path
            .to_str()
            .is_some_and(|path| path.ends_with(".limo.jsonc") || path.ends_with(".nbcad.jsonc"))
    {
        return Err("Save script source to an absolute .limo.jsonc path".into());
    }
    if source.len() > maximum {
        return Err(format!("Script source exceeds {maximum} bytes"));
    }
    limo_cad_project_file::write_binary_file_atomic(path, source.as_bytes())
        .map_err(|error| error.to_string())
}

pub(crate) fn save(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    generation: u64,
    path: PathBuf,
) -> Result<Value, String> {
    available(world)?;
    source_ready(world)?;
    let state = &world.resource::<Files>().script;
    if state.generation != generation {
        return Err("Script changed while the save chooser was open".into());
    }
    let source = state.source.clone();
    let maximum = state
        .loaded
        .as_ref()
        .ok_or("Open a script before saving source")?
        .maximum;
    start(world, handle, "Saving authored script source", move || {
        write_source(&path, &source, maximum)?;
        Ok(Change::Saved {
            generation,
            path,
            source,
        })
    })
}

fn apply(state: &mut State, change: Change) -> Result<(), String> {
    match change {
        Change::Validated { generation, loaded } => {
            if generation != state.generation
                || loaded.authored() != state.source
                || state.source_path != loaded.path
            {
                return Err("Script changed before validation completed; Validate again".into());
            }
            state.loaded = Some(Arc::new(loaded));
            state.validated = true;
            state.status = Some(if state.dirty() {
                "Valid script; edits are unsaved. Run uses this validated source and include snapshot."
            } else { "Valid script; Run uses this validated source and include snapshot." }.into());
        }
        Change::Saved {
            generation,
            path,
            source,
        } => {
            if generation != state.generation || source != state.source {
                return Err(
                    "An earlier script snapshot was saved; current edits remain unsaved".into(),
                );
            }
            state.advance()?;
            state.path = path.to_string_lossy().into_owned();
            state.source_path = Some(path);
            state.baseline = source;
            state.validated = false;
            state.status = Some(
                "Authored source saved. Validate again: includes resolve beside the saved file."
                    .into(),
            );
        }
    }
    Ok(())
}

pub(super) fn poll(world: &mut World) {
    if let Some(error) = limit_error(world) {
        world.resource_mut::<Files>().script.source_error = Some(error);
    }
    let result = world
        .resource::<Files>()
        .script
        .job
        .as_ref()
        .and_then(|job| {
            Some(match job.lock() {
                Ok(channel) => match channel.try_recv() {
                    Ok(result) => result,
                    Err(mpsc::TryRecvError::Empty) => return None,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Err("Script file worker disconnected".into())
                    }
                },
                Err(_) => Err("Script file result could not be read".into()),
            })
        });
    let Some(result) = result else { return };
    let state = &mut world.resource_mut::<Files>().script;
    state.job = None;
    if let Err(error) = result.and_then(|change| apply(state, change)) {
        state.status = Some(format!("Script: {error}"));
    }
}

#[path = "editor/panel.rs"]
mod panel;
pub(crate) use panel::paint_source;

#[cfg(test)]
#[path = "editor/tests.rs"]
mod tests;
