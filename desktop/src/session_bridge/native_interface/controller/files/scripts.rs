//! File loading inspects source only. An explicit run creates a retained blank
//! tab and hands frozen, expanded source to the existing live script runner.
use super::*;
mod catalog;
mod chapters;
mod dialog;
mod editor;
mod exit;
mod launch;
mod preview;
pub(super) use catalog::{browse, cancel_open, open_recipe, page, paint_library};
pub(super) use chapters::{
    command as chapter_command, paint as paint_chapters, Action as ChapterAction,
};
pub(super) use editor::{
    discard, edit_source, paint_source, retain_source_error, save, save_as, show_source, validate,
};
pub(super) use exit::guard_exit;
pub(super) use launch::{command as launch_command, Action as LaunchAction};
pub(super) use preview::{
    cancel_pointer as cancel_preview_pointer, command as preview_command, input as preview_input,
    paint as paint_preview, Action as PreviewAction,
};

pub(super) struct Loaded {
    pub path: Option<PathBuf>,
    example: Option<&'static catalog::Example>,
    pub name: String,
    pub steps: u64,
    pub checks: u64,
    pub maximum: usize,
    inspection: Value,
}
impl Loaded {
    fn source(&self) -> &str {
        self.inspection["source"].as_str().unwrap()
    }
    fn authored(&self) -> &str {
        self.inspection["authored_source"].as_str().unwrap()
    }
}

#[derive(Default)]
pub(super) struct State {
    pub path: String,
    pub loaded: Option<Arc<Loaded>>,
    pub generation: u64,
    pub status: Option<String>,
    pub source: String,
    pub source_path: Option<PathBuf>,
    pub editor_open: bool,
    pub editor_generation: u64,
    pub library: catalog::Library,
    pub example: Option<&'static catalog::Example>,
    pub preview: preview::State,
    launch: launch::Options,
    pub chapters: chapters::State,
    baseline: String,
    validated: bool,
    source_entity: Option<Entity>,
    source_error: Option<String>,
    job: Option<Mutex<mpsc::Receiver<Result<editor::Change, String>>>>,
    loading: Option<Mutex<mpsc::Receiver<Result<Loaded, String>>>>,
}
impl State {
    pub fn loading(&self) -> bool {
        self.loading.is_some() || self.job.is_some()
    }
    pub fn dirty(&self) -> bool {
        self.source != self.baseline
    }
    pub fn source_label(&self) -> String {
        self.source_path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| {
                self.example
                    .map(|example| {
                        format!(
                            "{}: {} ({})",
                            if self.source == example.source {
                                "Bundled source"
                            } else {
                                "Edited from bundled source"
                            },
                            example.name,
                            example.id
                        )
                    })
                    .unwrap_or_else(|| {
                        "Unsaved source; Save As to resolve relative includes".into()
                    })
            })
    }
    fn advance(&mut self) -> Result<(), String> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or("Script generation exhausted")?;
        self.chapters = chapters::State::default();
        Ok(())
    }
    fn accept(&mut self, loaded: Loaded) -> Result<(), String> {
        self.advance()?;
        self.editor_generation = self.generation;
        self.path = loaded
            .path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.source_path = loaded.path.clone();
        self.example = loaded.example;
        self.editor_open = true;
        self.library.open = false;
        self.source = loaded.authored().to_owned();
        self.baseline = self.source.clone();
        self.loaded = Some(Arc::new(loaded));
        self.validated = true;
        self.source_error = None;
        self.status = None;
        Ok(())
    }
    pub(super) fn selected(&self, generation: u64) -> Result<Arc<Loaded>, String> {
        if self.loading() || self.generation != generation {
            return Err("The loaded script changed; choose Run again".into());
        }
        if !self.validated || self.source_error.is_some() {
            return Err("Validate the current script source before running it".into());
        }
        let loaded = self
            .loaded
            .clone()
            .ok_or("Open a script before running it")?;
        if loaded.authored() != self.source || self.source_path != loaded.path {
            return Err("Script source or include directory changed; Validate again".into());
        }
        Ok(loaded)
    }
}

fn inspect(path: PathBuf) -> Result<Loaded, String> {
    let text_path = path.to_str().ok_or("Script path must be valid Unicode")?;
    let inspection = limo_cad_mcp::inspect_script(json!({"path":text_path}))?;
    inspected(Some(path), inspection)
}

fn inspected(path: Option<PathBuf>, inspection: Value) -> Result<Loaded, String> {
    let name = inspection["name"]
        .as_str()
        .ok_or("Script name is missing")?
        .to_owned();
    let steps = inspection["step_count"]
        .as_u64()
        .ok_or("Script step count is missing")?;
    let checks = inspection["check_count"]
        .as_u64()
        .ok_or("Script check count is missing")?;
    inspection["source"]
        .as_str()
        .ok_or("Expanded script source is missing")?;
    inspection["authored_source"]
        .as_str()
        .ok_or("Authored script source is missing")?;
    let maximum = inspection["max_source_bytes"]
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .ok_or("Script size limit is missing")?;
    Ok(Loaded {
        path,
        example: None,
        name,
        steps,
        checks,
        maximum,
        inspection,
    })
}

fn available(world: &World) -> Result<(), String> {
    let files = world.resource::<Files>();
    if files.lesson.is_some() {
        return Err("Stop the running script or wait for it to finish".into());
    }
    if files.script.loading() {
        return Err("Wait for the script file operation to finish".into());
    }
    if files.script.preview.building() {
        return Err("Wait for the isolated lesson preview to finish preparing".into());
    }
    if awaiting(world) {
        return Err("Finish the current File dialog first".into());
    }
    Ok(())
}

pub(super) fn edit_path(world: &mut World, input: &ControlInput) -> Result<Value, String> {
    available(world)?;
    let ControlInput::SetValue(path) = input else {
        return Err("Script path requires text".into());
    };
    world.resource_mut::<Files>().script.path = path.clone();
    Ok(json!({"changed":true}))
}

pub(super) fn choose(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    receipt: DocumentReceipt,
) -> Result<Value, String> {
    available(world)?;
    editor::replace_ready(world)?;
    let directory = world
        .resource::<Files>()
        .script
        .loaded
        .as_ref()
        .and_then(|loaded| loaded.path.as_ref())
        .and_then(|path| path.parent())
        .map(std::path::Path::to_path_buf);
    let (dialog, parent) = dialog::parented(
        world,
        rfd::FileDialog::new()
            .set_title("Open Limo CAD script")
            .add_filter("Limo CAD command script (.limo.jsonc)", &["jsonc"]),
    )?;
    let (send, receive) = mpsc::channel();
    let wake = handle.clone();
    std::thread::Builder::new()
        .name("cad-script-picker".into())
        .spawn(move || {
            let _parent = parent;
            let mut dialog = dialog;
            if let Some(directory) = directory {
                dialog = dialog.set_directory(directory);
            }
            let _ = send.send(dialog::pick(dialog, false));
            wake.request_redraw();
        })
        .map_err(|error| format!("Cannot open script chooser: {error}"))?;
    world.resource_mut::<Files>().picker = Some(Picker {
        receipt,
        kind: PickerKind::Script,
        result: Mutex::new(receive),
    });
    Ok(json!({"awaiting_input":true}))
}

pub(super) fn load(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    receipt: DocumentReceipt,
    path: PathBuf,
) -> Result<Value, String> {
    available(world)?;
    editor::replace_ready(world)?;
    if services
        .bridge
        .native_document_receipt(&services.engine, &receipt.owner)?
        != receipt
    {
        return Err("The document changed while the script chooser was open".into());
    }
    begin_load(
        world,
        handle,
        move || inspect(path),
        "Loading script; no commands have run",
    )
}

fn begin_load(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    task: impl FnOnce() -> Result<Loaded, String> + Send + 'static,
    status: &str,
) -> Result<Value, String> {
    let (send, receive) = mpsc::channel();
    let wake = handle.clone();
    std::thread::Builder::new()
        .name("cad-script-inspect".into())
        .spawn(move || {
            let inspected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task))
                .unwrap_or_else(|_| Err("Script inspection stopped unexpectedly".into()));
            let _ = send.send(inspected);
            wake.request_redraw();
        })
        .map_err(|error| format!("Cannot inspect script: {error}"))?;
    let mut files = world.resource_mut::<Files>();
    files.script.loading = Some(Mutex::new(receive));
    files.script.status = Some(status.into());
    Ok(json!({"script_loading":true}))
}

pub(super) fn poll(world: &mut World) {
    editor::poll(world);
    let result = world
        .resource::<Files>()
        .script
        .loading
        .as_ref()
        .and_then(|loading| {
            Some(match loading.lock() {
                Ok(channel) => match channel.try_recv() {
                    Ok(result) => result,
                    Err(mpsc::TryRecvError::Empty) => return None,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        Err("Script inspection worker disconnected".into())
                    }
                },
                Err(_) => Err("Script inspection result could not be read".into()),
            })
        });
    if let Some(result) = result {
        let mut files = world.resource_mut::<Files>();
        files.script.loading = None;
        if let Err(error) = result.and_then(|loaded| files.script.accept(loaded)) {
            files.script.status = Some(format!("Script not loaded: {error}"));
        }
    }
    preview::poll(world);
    catalog::poll(world);
    chapters::poll(world);
}

fn new_document(
    services: &NativeServices,
    workspace: &Mutex<DocumentWorkspace>,
    receipt: &DocumentReceipt,
    validate: impl FnOnce() -> Result<(), String>,
) -> Result<NativeMutationResult, String> {
    let created = workspace
        .lock()
        .map_err(|_| "Document workspace lock poisoned")?
        .new_tab_guarded(&services.bridge, &services.engine, receipt, validate)?;
    Ok(NativeMutationResult {
        context: created.owner,
        engine_revision: created.revision,
        value: json!({"changed":true}),
    })
}

pub(super) fn run(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    receipt: DocumentReceipt,
    generation: u64,
) -> Result<Value, String> {
    available(world)?;
    editor::source_ready(world)?;
    if world.resource::<Files>().script.library.pending().is_some() {
        return Err("Finish opening or cancel the queued example before running a script".into());
    }
    let loaded = world.resource::<Files>().script.selected(generation)?;
    let options = world.resource::<Files>().script.launch;
    remember_view(world, &receipt.owner);
    let workspace = world.resource::<Files>().workspace.clone();
    let wake = handle.clone();
    worker::enqueue_transaction(
        world,
        "script_new_document".into(),
        move |services, guard| new_document(services, &workspace, &receipt, || guard.validate()),
        move |world, services, result| {
            let result = result?;
            let owner = result.context.clone();
            let mut output =
                finish_document_transition(world, services, "script_new_document", result);
            if output["render_error"].is_string() {
                return Ok(output);
            }
            match lessons::start_source_with_options(
                world,
                &wake,
                (services, &owner),
                &loaded.name,
                loaded.source().to_owned(),
                (options.mode(), options.speed()),
            ) {
                Ok(()) => {
                    output["script_started"] = json!({"name":loaded.name,"path":loaded.path});
                }
                Err(error) => {
                    world.resource_mut::<Files>().script.status =
                        Some(format!("Script not started: {error}"));
                    output["script_error"] = json!(error);
                }
            }
            Ok(output)
        },
    )
}

#[cfg(test)]
mod tests;
