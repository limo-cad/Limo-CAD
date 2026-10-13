//! Existing library location flow: inspect first, then explicitly use or copy.
use super::*;
use crate::cam_library::{Location, LocationAction};
use std::{
    path::PathBuf,
    sync::{mpsc, Mutex},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Open,
    Back,
    Change,
    EditPath,
    Browse,
    Inspect,
    Default,
    Use,
    Copy,
}

struct Candidate {
    directory: Option<PathBuf>,
    location: Location,
}
pub(super) struct State {
    path: String,
    current: Option<Location>,
    candidate: Option<Candidate>,
    picker: Option<Mutex<mpsc::Receiver<Option<PathBuf>>>>,
}

fn location(value: &Value) -> Result<Location, String> {
    Ok(Location {
        directory: value["directory"]
            .as_str()
            .ok_or("Library directory missing")?
            .into(),
        path: value["path"].as_str().ok_or("Library path missing")?.into(),
        is_default: value["is_default"]
            .as_bool()
            .ok_or("Library location kind missing")?,
        exists: value["exists"]
            .as_bool()
            .ok_or("Library location existence missing")?,
        tool_count: value["tool_count"]
            .as_u64()
            .and_then(|count| usize::try_from(count).ok())
            .ok_or("Library tool count missing")?,
    })
}

fn inspect(
    world: &mut World,
    state: &super::State,
    directory: Option<PathBuf>,
) -> Result<Value, String> {
    let target = directory.clone();
    io::task(
        world,
        state.receipt.clone().ok_or("Open the library")?,
        state.serial,
        move || {
            serde_json::to_value(crate::cam_library::inspect_location(
                &crate::app_config::native_directory()?,
                target.as_deref(),
            )?)
            .map_err(|error| error.to_string())
        },
        move |state, value| {
            let candidate = location(&value)?;
            let storage = state.storage.as_mut().ok_or("Reopen library storage")?;
            storage.path = candidate.directory.clone();
            storage.candidate = Some(Candidate {
                directory,
                location: candidate,
            });
            state.error.clear();
            state.status =
                "Review the selected collection, then choose Use or Copy current here.".into();
            Ok(json!({"inspected":true}))
        },
    )
}

pub(super) fn execute(
    world: &mut World,
    state: &mut super::State,
    input: &ControlInput,
    command: Command,
) -> Result<Value, String> {
    if command == Command::EditPath {
        let storage = state.storage.as_mut().ok_or("Open library storage")?;
        match input {
            ControlInput::SetValue(value) => {
                storage.path = value.clone();
                storage.candidate = None;
            }
            ControlInput::Key(_) => {}
            input if super::super::super::super::super::is_activation(input) => {}
            _ => return Err("Enter a library folder path".into()),
        }
        return Ok(json!({"changed":true}));
    }
    match command {
        Command::Open => {
            state.storage = Some(State {
                path: String::new(),
                current: None,
                candidate: None,
                picker: None,
            });
            io::task(
                world,
                state.receipt.clone().ok_or("Open the library")?,
                state.serial,
                || {
                    serde_json::to_value(crate::cam_library::get_location(
                        &crate::app_config::native_directory()?,
                    )?)
                    .map_err(|error| error.to_string())
                },
                |state, value| {
                    let current = location(&value)?;
                    if let Some(storage) = state.storage.as_mut() {
                        storage.path = current.directory.clone();
                        storage.current = Some(current);
                    }
                    state.status =
                        "Changing the collection leaves every project tool unchanged.".into();
                    Ok(json!({"storage":true}))
                },
            )
        }
        Command::Back => {
            state.storage = None;
            Ok(json!({"storage":false}))
        }
        Command::Change => {
            state
                .storage
                .as_mut()
                .ok_or("Open library storage")?
                .candidate = None;
            Ok(json!({"changed":true}))
        }
        Command::Inspect => {
            let directory = PathBuf::from(
                state
                    .storage
                    .as_ref()
                    .ok_or("Open library storage")?
                    .path
                    .trim(),
            );
            inspect(world, state, Some(directory))
        }
        Command::Default => inspect(world, state, None),
        Command::Browse => {
            if files::awaiting(world) {
                return Err("Finish the current file chooser first".into());
            }
            let storage = state.storage.as_mut().ok_or("Open library storage")?;
            if storage.picker.is_some() {
                return Err("A folder chooser is already open".into());
            }
            let (chooser, parent) = files::parented_dialog(
                world,
                rfd::FileDialog::new().set_title("Choose the CAM tool library folder"),
            )?;
            let (send, receive) = mpsc::channel();
            let wake = world.get_resource::<NativeInterfaceHandle>().cloned();
            let path = storage.path.clone();
            std::thread::Builder::new()
                .name("cad-tool-library-folder".into())
                .spawn(move || {
                    let _parent = parent;
                    let chooser = if PathBuf::from(&path).is_dir() {
                        chooser.set_directory(path)
                    } else {
                        chooser
                    };
                    let _ = send.send(chooser.pick_folder());
                    if let Some(handle) = wake {
                        handle.request_redraw();
                    }
                })
                .map_err(|error| format!("Could not open the folder chooser: {error}"))?;
            storage.picker = Some(Mutex::new(receive));
            Ok(json!({"awaiting_input":true}))
        }
        Command::Use | Command::Copy => {
            let candidate = state
                .storage
                .as_ref()
                .and_then(|storage| storage.candidate.as_ref())
                .ok_or("Inspect a folder before changing the library")?;
            if command == Command::Copy && candidate.location.exists {
                return Err(
                    "The chosen folder already contains a library; nothing will be overwritten"
                        .into(),
                );
            }
            let target = candidate.directory.clone();
            let expected = if command == Command::Copy {
                let snapshot = state
                    .snapshot
                    .as_ref()
                    .ok_or("Load the current library before copying it")?;
                Some((snapshot.path.clone(), snapshot.revision.clone()))
            } else {
                None
            };
            io::task(
                world,
                state.receipt.clone().ok_or("Open the library")?,
                state.serial,
                move || {
                    let config = crate::app_config::native_directory()?;
                    crate::cam_library::set_location_at(
                        &config,
                        target.as_deref(),
                        if command == Command::Copy {
                            LocationAction::CopyCurrent
                        } else {
                            LocationAction::UseExisting
                        },
                        expected
                            .as_ref()
                            .map(|(path, revision)| (path.as_str(), revision.as_str())),
                    )?;
                    serde_json::to_value(crate::cam_library::load(&config)?)
                        .map_err(|error| error.to_string())
                },
                |state, value| {
                    state.storage = None;
                    io::install(
                        state,
                        &value,
                        None,
                        "Library location changed. Project snapshots are unchanged.",
                    )?;
                    Ok(json!({"location_changed":true}))
                },
            )
        }
        Command::EditPath => unreachable!(),
    }
}

pub(super) fn awaiting(state: &super::State) -> bool {
    state
        .storage
        .as_ref()
        .is_some_and(|storage| storage.picker.is_some())
}

pub(super) fn caption(state: &super::State) -> String {
    let Some(storage) = state.storage.as_ref() else {
        return String::new();
    };
    let mut text = format!("Library folder: {}", storage.path);
    if let Some(current) = &storage.current {
        text.push_str(&format!("\nCurrent library: {}", current.path));
    }
    if let Some(candidate) = &storage.candidate {
        text.push_str(&format!(
            "\nSelected library: {}\n{} tools; {}",
            candidate.location.path,
            candidate.location.tool_count,
            if candidate.location.exists {
                "existing collection"
            } else {
                "empty folder"
            }
        ));
    }
    text
}

pub(super) fn poll(world: &mut World, state: &mut super::State) -> Result<(), String> {
    let Some(storage) = state.storage.as_mut() else {
        return Ok(());
    };
    let result = storage
        .picker
        .as_ref()
        .and_then(|receive| match receive.lock() {
            Ok(receive) => match receive.try_recv() {
                Ok(path) => Some(Ok(path)),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("The folder chooser stopped".to_owned()))
                }
            },
            Err(_) => Some(Err("The folder chooser failed".to_owned())),
        });
    if let Some(result) = result {
        storage.picker = None;
        if state.receipt.is_none() {
            state.storage = None;
            return Ok(());
        }
        if let Some(path) = result? {
            inspect(world, state, Some(path))?;
        }
    }
    Ok(())
}

pub(super) fn paint(
    world: &mut World,
    camera: Entity,
    state: &mut super::State,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> Result<(), String> {
    let busy = worker::busy(world) || awaiting(state);
    let storage = state.storage.as_ref().ok_or("Open library storage")?;
    if let Some(candidate) = storage.candidate.as_ref() {
        let description = format!(
            "Selected: {}\n{}",
            candidate.location.directory,
            if candidate.location.exists {
                format!("Existing library · {} tools", candidate.location.tool_count)
            } else {
                "Empty folder · using it starts a separate library".into()
            }
        );
        state.widgets.text(
            world,
            camera,
            "library-storage-candidate",
            rect(x + 16., y + 80., w - 32., 88.),
            &panel::wrapped(&description, ((w - 32.) / 7.) as usize, 5),
            12.,
            84,
        );
        let message = if state.error.is_empty() {
            "Use selects this separate collection. Copy writes the current collection into an empty folder. Neither action merges or changes project tools."
        } else {
            &state.error
        };
        state.widgets.text(
            world,
            camera,
            "library-storage-review",
            rect(x + 16., y + 174., w - 32., 44.),
            &panel::wrapped(message, ((w - 32.) / 6.) as usize, 3),
            11.,
            84,
        );
        for (index, (label, command, disabled)) in [
            (
                if candidate.location.exists {
                    "Use this library"
                } else {
                    "Use empty folder"
                },
                Command::Use,
                false,
            ),
            (
                "Copy current here",
                Command::Copy,
                candidate.location.exists || state.snapshot.is_none(),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let bw = (w - 36.) / 2.;
            panel::button(
                world,
                camera,
                &mut state.widgets,
                &format!("library-storage-apply-{index}"),
                label,
                None,
                super::Command::Storage(command),
                rect(x + 16. + index as f32 * (bw + 4.), y + 226., bw, 30.),
                busy || disabled,
                None,
            )?;
        }
        panel::button(
            world,
            camera,
            &mut state.widgets,
            "library-storage-another",
            "Choose another folder",
            None,
            super::Command::Storage(Command::Change),
            rect(x + 16., y + 264., w - 32., 28.),
            busy,
            None,
        )?;
    } else {
        let current = storage.current.as_ref().map_or(
            "The current collection is unavailable. Choose a valid folder to reconnect.".into(),
            |location| {
                format!(
                    "Current: {}\n{} tools · {}",
                    location.directory,
                    location.tool_count,
                    if location.is_default {
                        "Default location"
                    } else {
                        "Selected location"
                    }
                )
            },
        );
        state.widgets.text(
            world,
            camera,
            "library-storage-current",
            rect(x + 16., y + 80., w - 32., 48.),
            &panel::wrapped(&current, ((w - 32.) / 7.) as usize, 3),
            12.,
            84,
        );
        let mut control = InterfaceControl::button("cam-library", "Library folder");
        control.modal_scope = Some("cam-library".into());
        control.disabled = busy;
        control.field = Field::Text {
            value: storage.path.clone(),
            read_only: false,
            selection: None,
        };
        state.widgets.button(
            world,
            camera,
            "library-storage-path",
            control,
            None,
            NativeCommand::Cam(super::super::Command::Central(super::Command::Storage(
                Command::EditPath,
            ))),
            rect(x + 16., y + 140., w - 32., 29.),
            None,
            84,
        )?;
        for (index, (label, command)) in [
            ("Browse library folder…", Command::Browse),
            ("Inspect library folder", Command::Inspect),
            ("Default library folder", Command::Default),
        ]
        .into_iter()
        .enumerate()
        {
            let bw = (w - 40.) / 3.;
            panel::button(
                world,
                camera,
                &mut state.widgets,
                &format!("library-storage-pick-{index}"),
                label,
                Some(match command {
                    Command::Browse => "Browse…",
                    Command::Inspect => "Inspect folder",
                    _ => "Default folder",
                }),
                super::Command::Storage(command),
                rect(x + 16. + index as f32 * (bw + 4.), y + 180., bw, 29.),
                busy,
                None,
            )?;
        }
        let message = if state.error.is_empty() {
            &state.status
        } else {
            &state.error
        };
        state.widgets.text(
            world,
            camera,
            "library-storage-status",
            rect(x + 16., y + 228., w - 32., (h - 284.).min(80.)),
            &panel::wrapped(message, ((w - 32.) / 6.) as usize, 3),
            11.,
            84,
        );
    }
    panel::button(
        world,
        camera,
        &mut state.widgets,
        "library-storage-back",
        "Back to central tools",
        None,
        super::Command::Storage(Command::Back),
        rect(x + 16., y + h - 42., w - 32., 28.),
        busy,
        None,
    )?;
    Ok(())
}
