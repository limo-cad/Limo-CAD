//! Transient NC input on the existing native text adapter and shared simulator.
//! This form owns no CAM intent; the selected setup supplies tools and stock.
use super::*;
use limo_cad_cam::CamGcodeDialectDto;
use limo_cad_interface::{ChoiceOption, ControlInput, Field};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Open,
    Source,
    FileName,
    Dialect,
    Pick,
    Run,
    Close,
}

struct Picker {
    key: Key,
    serial: u64,
    receiver: Mutex<mpsc::Receiver<Result<Option<nc_input::Input>, String>>>,
}

#[derive(Resource, Default)]
struct Editor {
    key: Option<Key>,
    setup: Option<u64>,
    serial: u64,
    open: bool,
    input: nc_input::Input,
    error: String,
    source_error: Option<String>,
    limit_error: Option<String>,
    picker: Option<Picker>,
    dirty: bool,
    layout: Option<(u32, u32, bool, u64)>,
    widgets: Widgets,
}

fn native(serial: u64, command: Command) -> NativeCommand {
    NativeCommand::Workbench(super::super::Command::CamView(super::Command::Nc(
        serial, command,
    )))
}

pub(crate) fn modal(world: &World) -> bool {
    world.get_resource::<Editor>().is_some_and(|e| e.open)
}

pub(crate) fn awaiting(world: &World) -> bool {
    world
        .get_resource::<Editor>()
        .is_some_and(|e| e.picker.is_some())
}

pub(super) fn close(world: &mut World) {
    if let Some(mut editor) = world.get_resource_mut::<Editor>() {
        editor.open = false;
        editor.dirty = true;
    }
}

pub(crate) fn caption(world: &World) -> Option<String> {
    let editor = world.get_resource::<Editor>().filter(|e| e.open)?;
    Some(format!(
        "NC workpiece simulation: {}. {}",
        editor.input.file_name.as_deref().unwrap_or("Pasted NC"),
        source_limit_error(world, editor)
            .or(editor.source_error.as_deref())
            .unwrap_or(&editor.error)
    ))
}

fn source_limit_error<'a>(world: &'a World, editor: &Editor) -> Option<&'a str> {
    interface_shell::fields::limits::error(world, editor.widgets.entity("nc-source")?)
}

fn clear_source_limit(world: &mut World, editor: &mut Editor) {
    if let Some(entity) = editor.widgets.entity("nc-source") {
        interface_shell::fields::limits::clear(world, entity);
    }
    editor.limit_error = None;
}

fn mark_source_error(world: &mut World, editor: &mut Editor, error: String) {
    if let Some(entity) = editor.widgets.entity("nc-source") {
        if let Some(mut limit) = world.get_mut::<interface_shell::fields::limits::ByteLimit>(entity)
        {
            limit.rejected = Some(error.clone());
        }
    }
    editor.error = error.clone();
    editor.source_error = Some(error);
    editor.dirty = true;
}

fn dialects() -> Vec<ChoiceOption> {
    [
        ("auto", "Detect controller language"),
        ("siemens828d", "Siemens 828D"),
        ("iso", "ISO G-code"),
        ("fanuc", "Fanuc"),
        ("haas", "Haas"),
    ]
    .into_iter()
    .map(|(value, label)| ChoiceOption {
        value: value.into(),
        label: label.into(),
        disabled: false,
    })
    .collect()
}

fn dialect_key(value: CamGcodeDialectDto) -> Result<String, String> {
    serde_json::to_value(value)
        .map_err(|e| e.to_string())?
        .as_str()
        .map(str::to_owned)
        .ok_or("Invalid controller language".into())
}

pub(crate) fn reduce(
    world: &mut World,
    owner: &DocumentContext,
    revision: u64,
    serial: u64,
    command: Command,
    input: &ControlInput,
) -> Result<Value, String> {
    if cam::editing_dirty(world) {
        return Err("Apply or cancel CAM edits before opening NC simulation".into());
    }
    let view = world
        .get_resource::<State>()
        .ok_or("Open the CAM workspace")?;
    let key = view.key.clone().ok_or("Choose a CAM setup")?;
    let setup = view.setup.ok_or("Choose a CAM setup")?;
    if &key.owner != owner || key.revision != revision {
        return Err("CAM changed before the NC action was applied".into());
    }
    let busy = view.pending.is_some();
    let mut editor = world.remove_resource::<Editor>().unwrap_or_default();
    let result = (|| {
        if command == Command::Open {
            if !super::super::super::super::is_activation(input) {
                return Err("Activate NC simulation".into());
            }
            if editor.picker.is_some() {
                return Err("Finish the current NC file chooser first".into());
            }
            editor.serial = editor
                .serial
                .checked_add(1)
                .ok_or("NC control generation exhausted")?;
            if editor
                .key
                .as_ref()
                .is_none_or(|prior| prior.owner != key.owner)
                || editor.setup != Some(setup)
            {
                editor.input = nc_input::Input::default();
            }
            editor.key = Some(key);
            editor.setup = Some(setup);
            editor.open = true;
            editor.error.clear();
            editor.source_error = None;
            clear_source_limit(world, &mut editor);
            editor.dirty = true;
            if let Some(mut view) = world.get_resource_mut::<State>() {
                if let Some(player) = &mut view.player {
                    player.playing = false;
                    player.ticket = player.ticket.wrapping_add(1);
                    player.requested = None;
                }
                view.report = false;
                view.settings_open = false;
            }
            return Ok(json!({"opened":true}));
        }
        if !editor.open
            || editor.serial != serial
            || editor.key.as_ref() != Some(&key)
            || editor.setup != Some(setup)
        {
            return Err("The NC form changed; use its refreshed controls".into());
        }
        if matches!(command, Command::Source | Command::FileName) {
            let ControlInput::SetValue(value) = input else {
                return Ok(json!({"focused":true}));
            };
            if command == Command::Source {
                if let Err(error) = editor.input.edit(value.clone()) {
                    mark_source_error(world, &mut editor, error.clone());
                    return Err(error);
                }
                editor.source_error = None;
                clear_source_limit(world, &mut editor);
            } else {
                editor.input.file_name = (!value.trim().is_empty()).then(|| value.clone());
            }
            if editor.source_error.is_none() {
                editor.error.clear();
            }
            editor.dirty = true;
            return Ok(json!({"handled":true}));
        }
        if command == Command::Dialect {
            let selected = cam::choose(&dialects(), &dialect_key(editor.input.dialect)?, input)?;
            editor.input.dialect =
                serde_json::from_value(json!(selected)).map_err(|e| e.to_string())?;
            editor.dirty = true;
            return Ok(json!({"handled":true}));
        }
        if !super::super::super::super::is_activation(input) {
            return Err("Activate an NC simulation control".into());
        }
        match command {
            Command::Close => editor.open = false,
            Command::Pick => {
                if files::awaiting(world) || editor.picker.is_some() {
                    return Err("Finish the current file chooser first".into());
                }
                let (dialog, parent) = files::parented_dialog(
                    world,
                    rfd::FileDialog::new()
                        .set_title("Open NC source")
                        .add_filter("NC source", &["mpf", "spf", "nc", "ngc", "tap", "txt"]),
                )?;
                let (send, receive) = mpsc::channel();
                let wake = world.get_resource::<NativeInterfaceHandle>().cloned();
                std::thread::Builder::new()
                    .name("cad-nc-source-file".into())
                    .spawn(move || {
                        let _parent = parent;
                        let result = dialog
                            .pick_file()
                            .map(|path| nc_input::read(&path))
                            .transpose();
                        let _ = send.send(result);
                        if let Some(wake) = wake {
                            wake.request_redraw();
                        }
                    })
                    .map_err(|e| format!("Could not open the NC chooser: {e}"))?;
                editor.picker = Some(Picker {
                    key,
                    serial,
                    receiver: Mutex::new(receive),
                });
            }
            Command::Run => {
                if busy || editor.picker.is_some() {
                    return Err("Wait for the current preview or file chooser".into());
                }
                if let Some(error) = &editor.source_error {
                    return Err(error.clone());
                }
                if let Some(error) = source_limit_error(world, &editor) {
                    return Err(error.to_owned());
                }
                if editor.input.source.trim().is_empty() {
                    return Err("Choose an NC file or paste controller code".into());
                }
                super::request_nc(world, editor.input.clone())?;
                editor.open = false;
            }
            _ => unreachable!(),
        }
        editor.dirty = true;
        Ok(json!({"handled":true}))
    })();
    if let Err(error) = &result {
        if source_limit_error(world, &editor) != Some(error.as_str()) {
            editor.error = error.clone();
        }
        editor.dirty = true;
    }
    world.insert_resource(editor);
    result
}

pub(super) fn synchronize(
    world: &mut World,
    camera: Entity,
    width: f32,
    height: f32,
) -> Result<(), String> {
    let mut editor = world.remove_resource::<Editor>().unwrap_or_default();
    let view = world.get_resource::<State>();
    let matches = view.is_some_and(|v| v.key == editor.key && v.setup == editor.setup);
    let busy = view.is_some_and(|v| v.pending.is_some());
    if !matches {
        editor.open = false;
    }
    let completed = editor.picker.as_ref().and_then(|picker| {
        match picker.receiver.lock().unwrap().try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("NC chooser stopped".into())),
            Err(mpsc::TryRecvError::Empty) => None,
        }
    });
    if let Some(result) = completed {
        let picker = editor.picker.take().unwrap();
        if editor.open
            && matches
            && picker.serial == editor.serial
            && Some(&picker.key) == editor.key.as_ref()
        {
            match result {
                Ok(Some(mut input)) => {
                    input.dialect = editor.input.dialect;
                    editor.input = input;
                    editor.error.clear();
                    editor.source_error = None;
                    clear_source_limit(world, &mut editor);
                }
                Ok(None) => (),
                Err(error) => {
                    mark_source_error(world, &mut editor, error);
                }
            }
            editor.dirty = true;
        }
    }
    let limit_error = source_limit_error(world, &editor).map(str::to_owned);
    if editor.limit_error != limit_error {
        editor.limit_error = limit_error;
        editor.dirty = true;
    }
    let layout = (
        width.to_bits(),
        height.to_bits(),
        busy || editor.picker.is_some(),
        crate::native_viewport::ui::appearance_revision(world),
    );
    let result = if !editor.open {
        editor.widgets.begin();
        editor.widgets.finish(world);
        editor.layout = None;
        Ok(())
    } else if editor.dirty || editor.layout != Some(layout) {
        editor.widgets.begin();
        let result = paint(world, camera, &mut editor, width, height, layout.2);
        editor.widgets.finish(world);
        editor.layout = Some(layout);
        editor.dirty = result.is_err();
        result
    } else {
        Ok(())
    };
    world.insert_resource(editor);
    result
}

#[path = "nc_dialog/panel.rs"]
mod panel;
use panel::paint;
