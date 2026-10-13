//! Private-post storage controls. Catalog work never edits the project or
//! chooses a machine: the existing setup picker remains an explicit copy.
use super::*;
use std::{
    path::PathBuf,
    sync::{mpsc, Mutex},
};

mod io;
mod panel;
#[cfg(test)]
mod tests;

pub(in super::super) use panel::paint;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Import,
    Refresh,
    OpenFolder,
    Page(i32),
}

enum Completion {
    Picked {
        config: PathBuf,
        source: Option<PathBuf>,
    },
    Catalog {
        catalog: crate::cam_posts::Catalog,
        notice: String,
    },
    Opened(PathBuf),
}

#[derive(Resource, Default)]
struct State {
    catalog: Option<Arc<crate::cam_posts::Catalog>>,
    receiver: Option<Mutex<mpsc::Receiver<Result<Completion, String>>>>,
    picking: bool,
    refresh_machine: bool,
    page: usize,
    notice: String,
    error: String,
}

impl State {
    fn accept(&mut self, result: Result<Completion, String>) -> Option<(PathBuf, PathBuf)> {
        self.receiver = None;
        self.picking = false;
        match result {
            Ok(Completion::Picked { config, source }) => {
                self.error.clear();
                if let Some(source) = source {
                    return Some((config, source));
                }
                self.notice = "Import cancelled".into();
            }
            Ok(Completion::Catalog { catalog, notice }) => {
                self.catalog = Some(Arc::new(catalog));
                self.page = 0;
                self.notice = notice;
                self.error.clear();
                self.refresh_machine = true;
            }
            Ok(Completion::Opened(path)) => {
                self.notice = format!("Opened {}", path.display());
                self.error.clear();
            }
            Err(error) => {
                self.error = error;
                self.notice.clear();
            }
        }
        None
    }

    fn text(&self) -> String {
        let mut text = String::new();
        if let Some(catalog) = &self.catalog {
            text.push_str(&format!(
                "{}\n{} private post file(s)\n\n",
                catalog.directory,
                catalog.entries.len()
            ));
            for entry in &catalog.entries {
                let kind = match entry.kind.as_str() {
                    "native_profile" => "Machine profile",
                    "reference_only" => "Reference only",
                    _ => "Needs attention",
                };
                text.push_str(&format!(
                    "{} · {kind}\n{}\n\n",
                    entry.file_name, entry.message
                ));
            }
            if catalog.entries.is_empty() {
                text.push_str("No private posts saved.\n\n");
            }
        } else {
            text.push_str("Refresh to read the private post folder.\n\n");
        }
        text.push_str("Select a profile explicitly in Machine and post to copy it into a setup.");
        if !self.notice.is_empty() {
            text.push_str(&format!("\n\n{}", self.notice));
        }
        if !self.error.is_empty() {
            text.push_str(&format!("\n\n{}", self.error));
        }
        text
    }
}

fn start(
    world: &mut World,
    picking: bool,
    notice: &str,
    operation: impl FnOnce() -> Result<Completion, String> + Send + 'static,
) -> Result<Value, String> {
    if busy(world) {
        return Err("Wait for the private post storage action".into());
    }
    let (send, receive) = mpsc::sync_channel(1);
    let wake = world.get_resource::<NativeInterfaceHandle>().cloned();
    std::thread::Builder::new()
        .name("native-private-posts".into())
        .spawn(move || {
            let _ = send.send(operation());
            if let Some(handle) = wake {
                handle.request_redraw();
            }
        })
        .map_err(|error| format!("Could not start private post storage: {error}"))?;
    world.init_resource::<State>();
    let mut state = world.resource_mut::<State>();
    state.receiver = Some(Mutex::new(receive));
    state.picking = picking;
    state.notice = notice.into();
    state.error.clear();
    Ok(json!({"pending":true,"awaiting_input":picking}))
}

pub(in super::super) fn execute(world: &mut World, command: Command) -> Result<Value, String> {
    let result = execute_inner(world, command);
    if let Err(error) = &result {
        world.init_resource::<State>();
        world.resource_mut::<State>().error = error.clone();
    }
    result
}

fn execute_inner(world: &mut World, command: Command) -> Result<Value, String> {
    if let Command::Page(delta) = command {
        world.init_resource::<State>();
        let mut state = world.resource_mut::<State>();
        state.page = state.page.saturating_add_signed(delta as isize);
        return Ok(json!({"page":state.page}));
    }
    let config = crate::app_config::native_directory()?;
    match command {
        Command::Import => {
            if files::awaiting(world) {
                return Err("Finish the File dialog first".into());
            }
            let (dialog, parent) = files::parented_dialog(
                world,
                rfd::FileDialog::new()
                    .set_title("Import private post")
                    .add_filter("Private post", &["nbpost", "cps"]),
            )?;
            start(world, true, "Choose a private post file…", move || {
                let _parent = parent;
                let source = dialog.pick_file();
                Ok(Completion::Picked { config, source })
            })
        }
        Command::Refresh => start(world, false, "Reading private posts…", move || {
            io::load(&config).map(|catalog| Completion::Catalog {
                catalog,
                notice: "Private posts refreshed".into(),
            })
        }),
        Command::OpenFolder => start(world, false, "Opening private post folder…", move || {
            io::open_folder(&config).map(Completion::Opened)
        }),
        Command::Page(_) => unreachable!(),
    }
}

/// Call while synchronizing even when the storage section is hidden. Confirmed
/// per-user storage work may finish after navigating back to the setup fields.
pub(in super::super) fn poll(world: &mut World) -> bool {
    let result = world
        .get_resource::<State>()
        .and_then(|state| state.receiver.as_ref())
        .and_then(|receiver| match receiver.lock() {
            Ok(receiver) => match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Private post worker stopped".into()))
                }
            },
            Err(_) => Some(Err("Private post worker failed".into())),
        });
    let mut changed = false;
    if let Some(result) = result {
        changed = true;
        let import = world.resource_mut::<State>().accept(result);
        if let Some((config, source)) = import {
            let name = source
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            if let Err(error) = start(world, false, "Importing private post…", move || {
                io::import(&config, &source).map(|catalog| Completion::Catalog {
                    catalog,
                    notice: format!("Imported {name}"),
                })
            }) {
                let _ = world.resource_mut::<State>().accept(Err(error));
            }
        }
    }
    let refresh = world
        .get_resource::<State>()
        .is_some_and(|state| state.refresh_machine);
    if refresh && !super::busy(world) {
        world.resource_mut::<State>().refresh_machine = false;
        if let Err(error) = super::reload(world) {
            world.resource_mut::<State>().error = error;
        }
        changed = true;
    }
    changed
}

pub(in super::super) fn ensure_loaded(world: &mut World) {
    if !world.contains_resource::<State>() {
        if let Err(error) = execute(world, Command::Refresh) {
            world.init_resource::<State>();
            let _ = world.resource_mut::<State>().accept(Err(error));
        }
    }
}
pub(in super::super) fn busy(world: &World) -> bool {
    world
        .get_resource::<State>()
        .is_some_and(|state| state.receiver.is_some())
}
pub(in super::super) fn awaiting(world: &World) -> bool {
    world
        .get_resource::<State>()
        .is_some_and(|state| state.picking)
}
pub(in super::super) fn caption(world: &World) -> String {
    world
        .get_resource::<State>()
        .map_or_else(String::new, State::text)
}
