//! Visible File/tab workflows over the existing, owner-checked workspace.
//! OS pickers choose paths only. The ordered kernel worker owns file/model work.
use super::super::workspace::{DocumentReceipt, DocumentWorkspace, TabSummary};
use super::*;
use bevy::window::RawHandleWrapper;
use limo_cad_interface::ControlInput;
use limo_cad_project_file::SaveMetadata;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::{path::PathBuf, sync::mpsc};

mod bambu;
mod drawing_output;
mod io;
mod lessons;
mod panel;
mod printing;
mod profile_output;
mod scripts;
pub(super) use panel::synchronize;
pub(super) use printing::message as print_message;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FileCommand {
    Menu,
    DismissMenu,
    New,
    Open,
    Save,
    SaveAs,
    Rename,
    Close,
    CloseTab(DocumentContext),
    Activate(DocumentContext),
    Exit,
    SaveAllAndExit,
    Name(u64),
    ApplyName(u64),
    Cancel(u64),
    ErrorDetails(u64),
    Discard(u64),
    SaveContinue(u64),
    /// Read the desktop stdio presence. Does not change the document or menus.
    ReportMcp,
    ShowScripts,
    ShowSettings,
    OpenScript,
    ScriptPath,
    ScriptSource(u64),
    ShowScriptSource,
    ValidateScript,
    SaveScriptAs,
    DiscardScriptEdits,
    BrowseExamples,
    ExamplePage(usize),
    OpenExample(String),
    CancelRecipe(u64),
    ScriptPreview(scripts::PreviewAction),
    ScriptLaunch(scripts::LaunchAction),
    ScriptChapter(scripts::ChapterAction),
    LoadScript,
    RunScript(u64),
    ImportStep,
    Export(io::Format, bool),
    ExportDrawing(drawing_output::Format),
    PrintDrawing,
    ExportProfile,
    ProfileSelect(u64),
    ApplyProfile(u64),
    ExportScope(u64, limo_cad_export::MeshExportScope),
    ExportView(u64),
    ExportPrinter(u64),
    ExportAllowIssues(u64),
    ApplyExport(u64),
    Bambu(u64, u64, bambu::Command),
}
#[derive(Clone, Debug)]
enum Intent {
    Close,
    Open(PathBuf),
    Exit,
}
#[derive(Clone, Debug)]
enum DialogKind {
    Rename(String),
    Confirm(Intent),
    Export(Arc<io::ExportIntent>),
    Profile(profile_output::Selection),
}
#[derive(Clone, Debug)]
struct Dialog {
    token: u64,
    receipt: DocumentReceipt,
    kind: DialogKind,
    error: Option<String>,
}
struct Picker {
    receipt: DocumentReceipt,
    kind: PickerKind,
    result: Mutex<mpsc::Receiver<Option<PathBuf>>>,
}
enum PickerKind {
    Project {
        save: bool,
        continuation: Option<Intent>,
    },
    ImportStep,
    Export(Arc<io::ExportIntent>),
    Drawing(drawing_output::ExportIntent),
    Script,
    ScriptSave(u64),
    Profile(profile_output::ExportIntent),
    BambuTemplate {
        token: u64,
        generation: u64,
    },
}
#[derive(Resource, Default)]
pub(super) struct Files {
    pub workspace: Arc<Mutex<DocumentWorkspace>>,
    menu: bool,
    scripts: bool,
    settings: bool,
    lesson: Option<lessons::Running>,
    script: scripts::State,
    next_token: u64,
    dialog: Option<Dialog>,
    picker: Option<Picker>,
    views: HashMap<String, (u64, native_viewport::ViewportCamera)>,
    verification: bambu::verification::Jobs,
}

fn remember_view(world: &mut World, owner: &DocumentContext) {
    let (document, camera) = native_viewport::interface_camera_snapshot(world);
    if document == owner.document_id {
        world
            .resource_mut::<Files>()
            .views
            .insert(document, (owner.epoch, camera));
    }
}

fn finish_document_transition(
    world: &mut World,
    services: &NativeServices,
    operation: &str,
    result: NativeMutationResult,
) -> Value {
    let owner = result.context.clone();
    let mut output = finish_mutation(&services.engine, &services.bridge, world, operation, result);
    workbench::observe_document(world, &owner);
    if output["render_error"].is_string() {
        return output;
    }
    let restored = (|| {
        let live = tabs(world, services, &owner)?;
        let mut files = world.resource_mut::<Files>();
        files.views.retain(|document, (epoch, _)| {
            live.iter()
                .any(|tab| tab.owner.document_id == *document && tab.owner.epoch == *epoch)
        });
        let remembered = files
            .views
            .get(&owner.document_id)
            .filter(|(epoch, _)| *epoch == owner.epoch)
            .map(|(_, camera)| *camera);

        let (_, _, presentation, size) = native_viewport::interface_view(world);
        let camera = if let Some(camera) = remembered {
            camera
        } else {
            view::fit_camera(
                world,
                native_viewport::interface_geometry(world),
                presentation,
                native_viewport::ViewportCamera::default(),
                size,
                Some(ViewDirection::Isometric),
            )?
        };
        services
            .bridge
            .with_native_document_owner(&services.engine, &owner, || {
                native_viewport::apply_interface_view(world, &owner.document_id, Some(camera), None)
            })
    })();
    if let Err(error) = restored {
        output["presentation_pending"] = json!(true);
        output["presentation_error"] = json!(error);
    }
    output
}

pub(super) fn initialize(world: &mut World, workspace: Arc<Mutex<DocumentWorkspace>>) {
    if !world.contains_resource::<Files>() {
        world.insert_resource(Files {
            workspace,
            ..default()
        });
    }
}
pub(super) fn awaiting(world: &World) -> bool {
    world
        .get_resource::<Files>()
        .is_some_and(|f| f.dialog.is_some() || f.picker.is_some())
        || workbench::cam::awaiting(world)
        || workbench::cam_view::nc_dialog::awaiting(world)
}
pub(crate) fn queue_recipe(world: &mut World, recipe: &str) -> Result<Value, String> {
    scripts::open_recipe(world, recipe)
}

#[cfg(test)]
pub(super) fn queued_recipe_id(world: &World) -> Option<String> {
    let files = world.get_resource::<Files>()?;
    Some(files.script.library.pending()?.example.id.clone())
}
pub(crate) fn guard_script_exit(world: &mut World) -> Result<(), String> {
    scripts::guard_exit(world)
}
pub(crate) fn script_preview_input(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    event: &NativeHostInput,
) -> Result<bool, String> {
    scripts::preview_input(world, handle, event)
}

pub(super) fn cancel_preview_pointer(world: &mut World) {
    scripts::cancel_preview_pointer(world);
}
pub(super) fn modal(world: &World) -> Option<&'static str> {
    let f = world.get_resource::<Files>()?;
    if f.picker.is_some() {
        Some("file-picker")
    } else if f.dialog.is_some() {
        Some("file-dialog")
    } else if f.menu {
        Some("file-menu")
    } else if f.settings {
        Some("app-settings")
    } else {
        None
    }
}
pub(super) fn settings_open(world: &World) -> bool {
    world
        .get_resource::<Files>()
        .is_some_and(|files| files.settings)
}
pub(super) fn close_settings(world: &mut World) {
    if let Some(mut files) = world.get_resource_mut::<Files>() {
        files.settings = false;
    }
}
pub(super) fn tabs(
    world: &World,
    services: &NativeServices,
    owner: &DocumentContext,
) -> Result<Vec<TabSummary>, String> {
    world
        .resource::<Files>()
        .workspace
        .lock()
        .map_err(|_| "Document workspace lock poisoned")?
        .summaries(&services.bridge, owner)
}
fn current(
    world: &World,
    services: &NativeServices,
    owner: &DocumentContext,
) -> Result<DocumentReceipt, String> {
    let receipt = services
        .bridge
        .native_document_receipt(&services.engine, owner)?;
    world
        .resource::<Files>()
        .workspace
        .lock()
        .map_err(|_| "Document workspace lock poisoned")?
        .observe(&services.bridge, &services.engine, &owner.window_id)?;
    Ok(receipt)
}
fn require_idle_model(world: &World) -> Result<(), String> {
    require_file_ready(world, false)
}
fn require_file_ready(world: &World, allow_active_sketch: bool) -> Result<(), String> {
    named_views::ensure_exportable(world)?;
    print_intent::ensure_clean(world)?;
    if workbench::cam_view::nc_dialog::awaiting(world) {
        return Err("Finish the NC file chooser first".into());
    }
    if workbench::cam::awaiting(world) {
        return Err("Finish the CAM library or post chooser first".into());
    }
    if worker::busy(world) {
        return Err("Wait for the current operation to finish".into());
    }
    if feature::panel(world).is_some() {
        return Err("Apply or cancel the feature before changing files".into());
    }
    let (_, _, view, _) = native_viewport::interface_view(world);
    if !allow_active_sketch && view.mode == native_viewport::ViewportMode::Sketch {
        return Err("Finish the active sketch before changing files".into());
    }
    Ok(())
}
fn show_dialog(
    world: &mut World,
    receipt: DocumentReceipt,
    kind: DialogKind,
) -> Result<Value, String> {
    let mut f = world.resource_mut::<Files>();
    f.next_token = f
        .next_token
        .checked_add(1)
        .ok_or("File dialog sequence exhausted")?;
    f.dialog = Some(Dialog {
        token: f.next_token,
        receipt,
        kind,
        error: None,
    });
    f.menu = false;
    Ok(json!({"awaiting_input":true}))
}
fn owned_dialog(
    world: &World,
    services: &NativeServices,
    owner: &DocumentContext,
    token: u64,
) -> Result<Dialog, String> {
    let dialog = world
        .resource::<Files>()
        .dialog
        .clone()
        .ok_or("File dialog was closed")?;
    if dialog.token != token
        || &dialog.receipt.owner != owner
        || services
            .bridge
            .native_document_receipt(&services.engine, owner)?
            != dialog.receipt
    {
        return Err("The document changed while the File dialog was open".into());
    }
    Ok(dialog)
}

pub(crate) fn reduce(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    engine: &AppState,
    bridge: &SessionBridgeState,
    action: &NativeInterfaceAction,
    command: &FileCommand,
) -> Result<Value, String> {
    bridge
        .with_native_document_owner(engine, &action.context, || handle.validate_action(action))?;
    if let FileCommand::ErrorDetails(token) = command {
        let services = world.resource::<NativeServices>().clone();
        let dialog = owned_dialog(world, &services, &action.context, *token)?;
        if matches!(action.control.input, ControlInput::SetValue(_)) {
            return Err("File error details are read-only".into());
        }
        return Ok(json!({"changed":false,"error":dialog.error}));
    }
    if let FileCommand::Bambu(token, generation, command) = command {
        let services = world.resource::<NativeServices>().clone();
        return bambu::reduce(
            world,
            handle,
            (&services, &action.context),
            *token,
            *generation,
            *command,
            &action.control.input,
        );
    }
    if matches!(command, FileCommand::ScriptPath) {
        return scripts::edit_path(world, &action.control.input);
    }
    if let FileCommand::ScriptSource(generation) = command {
        if *generation != world.resource::<Files>().script.editor_generation {
            return Err("Script source editor was replaced".into());
        }
        return scripts::edit_source(world, &action.control.input);
    }
    if let FileCommand::ProfileSelect(token) = command {
        let services = world.resource::<NativeServices>().clone();
        let dialog = owned_dialog(world, &services, &action.context, *token)?;
        let DialogKind::Profile(mut selection) = dialog.kind else {
            return Err("Not a profile export dialog".into());
        };
        selection.select(&action.control.input)?;
        world.resource_mut::<Files>().dialog.as_mut().unwrap().kind =
            DialogKind::Profile(selection);
        return Ok(json!({"changed":true}));
    }
    if matches!(
        command,
        FileCommand::ExportView(_)
            | FileCommand::ExportPrinter(_)
            | FileCommand::ExportAllowIssues(_)
    ) {
        let services = world.resource::<NativeServices>().clone();
        return edit_export_dialog(
            world,
            &services,
            &action.context,
            command,
            &action.control.input,
        );
    }
    if let FileCommand::ScriptPreview(command) = command {
        return scripts::preview_command(world, handle, *command, &action.control.input);
    }
    if matches!(command, FileCommand::Name(_)) {
        let FileCommand::Name(token) = command else {
            unreachable!()
        };
        let ControlInput::SetValue(value) = &action.control.input else {
            return Err("Document name requires text".into());
        };
        let name = value.clone();
        let f = world.resource::<Files>();
        let dialog = f.dialog.as_ref().ok_or("Rename dialog was closed")?;
        if dialog.token != *token || dialog.receipt.owner != action.context {
            return Err("Rename dialog was replaced".into());
        }
        world.resource_mut::<Files>().dialog.as_mut().unwrap().kind = DialogKind::Rename(name);
        return Ok(json!({"changed":true}));
    }
    if let FileCommand::Activate(target) = command {
        let services = world.resource::<NativeServices>().clone();
        if matches!(&action.control.input, ControlInput::DoubleClick) && target == &action.context {
            return execute(
                world,
                handle,
                &services,
                &action.context,
                FileCommand::Rename,
            );
        }
        if let ControlInput::Key(key) = &action.control.input {
            if matches!(key.key.as_str(), "ArrowLeft" | "ArrowRight")
                && !key.ctrl
                && !key.meta
                && !key.alt
                && !key.shift
            {
                let tabs = tabs(world, &services, &action.context)?;
                if let Some(index) = tabs.iter().position(|tab| &tab.owner == target) {
                    let next = if key.key == "ArrowLeft" {
                        index.saturating_sub(1)
                    } else {
                        (index + 1).min(tabs.len() - 1)
                    };
                    return execute(
                        world,
                        handle,
                        &services,
                        &action.context,
                        FileCommand::Activate(tabs[next].owner.clone()),
                    );
                }
            }
        }
    }
    if !super::super::is_activation(&action.control.input) {
        return Err("File command requires activation".into());
    }
    let services = world.resource::<NativeServices>().clone();
    execute(world, handle, &services, &action.context, command.clone())
}

fn edit_export_dialog(
    world: &mut World,
    services: &NativeServices,
    owner: &DocumentContext,
    command: &FileCommand,
    input: &ControlInput,
) -> Result<Value, String> {
    require_idle_model(world)?;
    let token = match command {
        FileCommand::ExportView(t)
        | FileCommand::ExportPrinter(t)
        | FileCommand::ExportAllowIssues(t) => *t,
        _ => unreachable!(),
    };
    owned_dialog(world, services, owner, token)?;
    let dialog = world
        .resource_mut::<Files>()
        .into_inner()
        .dialog
        .as_mut()
        .ok_or("Export dialog closed")?;
    let DialogKind::Export(intent) = &mut dialog.kind else {
        return Err("Not an export options dialog".into());
    };
    let intent = Arc::make_mut(intent);
    match command {
        FileCommand::ExportView(_) => {
            let options = io::view_choices(&services.engine)?;
            let selected = workbench::cam::choose(&options, &io::view_key(intent), input)?;
            intent.named_view = if selected == "current" {
                None
            } else if selected == "assembled" {
                Some(String::new())
            } else {
                Some(
                    selected
                        .strip_prefix("saved:")
                        .ok_or("Choose a saved named view")?
                        .into(),
                )
            };
            intent.layout_report = None;
            intent.allow_layout_issues = false;
        }
        FileCommand::ExportPrinter(_) => {
            let options = io::bed_choices();
            let selected = workbench::cam::choose(&options, &io::bed_key(intent), input)?;
            intent.print_bed = if selected == "layout" {
                None
            } else {
                Some(
                    named_views::printer_choices()
                        .into_iter()
                        .find(|(key, _, _)| key == &selected)
                        .ok_or("Choose an embedded printer bed")?
                        .2,
                )
            };
            intent.layout_report = None;
            intent.allow_layout_issues = false;
        }
        FileCommand::ExportAllowIssues(_) => {
            if !super::super::is_activation(input) {
                return Err("Activate deliberate export confirmation".into());
            }
            intent.allow_layout_issues = !intent.allow_layout_issues;
        }
        _ => unreachable!(),
    }
    if matches!(command, FileCommand::ExportAllowIssues(_)) {
        Ok(json!({"changed":true}))
    } else {
        queue_layout_check(world, services, owner, token)
    }
}

fn queue_layout_check(
    world: &mut World,
    services: &NativeServices,
    owner: &DocumentContext,
    token: u64,
) -> Result<Value, String> {
    let dialog = owned_dialog(world, services, owner, token)?;
    let DialogKind::Export(intent) = dialog.kind else {
        return Err("Not an export dialog".into());
    };
    if !io::needs_layout_check(&intent) {
        return Ok(json!({"changed":true}));
    }
    let arguments = io::layout_arguments(&intent);
    let expected = arguments.clone();
    let receipt = dialog.receipt;
    let query_owner = owner.clone();
    worker::enqueue_query(
        world,
        receipt.owner,
        receipt.revision,
        "print_layout_check".into(),
        arguments,
        move |world, services, result| {
            services.bridge.with_native_document_receipt(
                &services.engine,
                &query_owner,
                |revision| {
                    if revision != receipt.revision {
                        return Err("The model changed during the layout check".into());
                    }
                    let dialog = world
                        .resource_mut::<Files>()
                        .into_inner()
                        .dialog
                        .as_mut()
                        .ok_or("Export dialog closed")?;
                    if dialog.token != token || dialog.receipt.owner != query_owner {
                        return Err("Export dialog was replaced".into());
                    }
                    let DialogKind::Export(intent) = &mut dialog.kind else {
                        return Err("Export dialog changed".into());
                    };
                    let intent = Arc::make_mut(intent);
                    if !io::needs_layout_check(intent) || io::layout_arguments(intent) != expected {
                        return Err("Export options changed during the layout check".into());
                    }
                    match result {
                        Ok(result) => {
                            intent.layout_report = Some(result.value.clone());
                            Ok(json!({"checked":true,"report":result.value}))
                        }
                        Err(error) => {
                            dialog.error = Some(error.clone());
                            Err(error)
                        }
                    }
                },
            )
        },
    )
}

fn execute(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    owner: &DocumentContext,
    command: FileCommand,
) -> Result<Value, String> {
    if matches!(command, FileCommand::ReportMcp) {
        let presence = match limo_cad_mcp::desktop_mcp_presence() {
            limo_cad_mcp::DesktopMcpPresence::Attached => "attached",
            limo_cad_mcp::DesktopMcpPresence::Waiting => "waiting",
            limo_cad_mcp::DesktopMcpPresence::Off => "off",
        };
        return Ok(json!({"mcp": presence}));
    }
    if matches!(
        command,
        FileCommand::ShowScripts | FileCommand::ShowSettings
    ) {
        scripts::retain_source_error(world);
        let mut files = world.resource_mut::<Files>();
        if files.dialog.is_some() || files.picker.is_some() {
            return Err("Finish the current File dialog first".into());
        }
        files.menu = false;
        match command {
            FileCommand::ShowScripts => {
                files.scripts = !files.scripts;
                files.settings = false;
            }
            _ => {
                files.settings = !files.settings;
                files.scripts = false;
            }
        }
        return Ok(json!({"scripts": files.scripts, "settings": files.settings}));
    }
    match &command {
        FileCommand::ScriptLaunch(action) => return scripts::launch_command(world, *action),
        FileCommand::ScriptChapter(action) => return scripts::chapter_command(world, *action),
        FileCommand::BrowseExamples => return scripts::browse(world),
        FileCommand::ExamplePage(page) => return scripts::page(world, *page),
        FileCommand::OpenExample(id) => return scripts::open_recipe(world, id),
        FileCommand::CancelRecipe(token) => return scripts::cancel_open(world, *token),
        _ => {}
    }
    if matches!(
        command,
        FileCommand::OpenScript
            | FileCommand::LoadScript
            | FileCommand::RunScript(_)
            | FileCommand::ShowScriptSource
            | FileCommand::ValidateScript
            | FileCommand::SaveScriptAs
            | FileCommand::DiscardScriptEdits
    ) {
        require_idle_model(world)?;
        let receipt = current(world, services, owner)?;
        return match command {
            FileCommand::OpenScript => scripts::choose(world, handle, receipt),
            FileCommand::LoadScript => {
                let path = PathBuf::from(world.resource::<Files>().script.path.trim());
                scripts::load(world, handle, services, receipt, path)
            }
            FileCommand::RunScript(generation) => scripts::run(world, handle, receipt, generation),
            FileCommand::ShowScriptSource => scripts::show_source(world),
            FileCommand::ValidateScript => scripts::validate(world, handle),
            FileCommand::SaveScriptAs => scripts::save_as(world, handle, receipt),
            FileCommand::DiscardScriptEdits => scripts::discard(world),
            _ => unreachable!(),
        };
    }
    if matches!(command, FileCommand::Menu | FileCommand::DismissMenu) {
        let mut f = world.resource_mut::<Files>();
        if f.dialog.is_some() || f.picker.is_some() {
            return Err("Finish the current File dialog first".into());
        }
        f.menu = matches!(command, FileCommand::Menu) && !f.menu;
        return Ok(json!({"menu_open":f.menu}));
    }
    if let FileCommand::Cancel(token) = command {
        if world
            .resource::<Files>()
            .dialog
            .as_ref()
            .is_some_and(|d| d.token == token && &d.receipt.owner == owner)
        {
            world.resource_mut::<Files>().dialog = None;
            return Ok(json!({"cancelled":true}));
        }
        return Err("File dialog was replaced".into());
    }
    if command == FileCommand::Exit {
        world.resource_mut::<Files>().menu = false;
        return Ok(json!({"request_exit":true}));
    }
    let closing = match &command {
        FileCommand::Close => true,
        FileCommand::CloseTab(target) => target == owner,
        FileCommand::Discard(_) | FileCommand::SaveContinue(_) => world
            .resource::<Files>()
            .dialog
            .as_ref()
            .is_some_and(|dialog| matches!(dialog.kind, DialogKind::Confirm(Intent::Close))),
        _ => false,
    };
    require_file_ready(world, closing)?;
    world.resource_mut::<Files>().menu = false;
    let receipt = current(world, services, owner)?;
    match command {
        FileCommand::PrintDrawing => printing::request(world, receipt),
        FileCommand::ImportStep => io::choose_import(world, handle, services, receipt),
        FileCommand::ExportDrawing(format) => {
            let intent = drawing_output::capture(services, &receipt, format)?;
            drawing_output::choose(world, handle, services, receipt, intent)
        }
        FileCommand::ExportProfile => {
            let selection = profile_output::capture(services, &receipt)?;
            show_dialog(world, receipt, DialogKind::Profile(selection))
        }
        FileCommand::ApplyProfile(token) => {
            let dialog = owned_dialog(world, services, owner, token)?;
            let DialogKind::Profile(selection) = dialog.kind else {
                return Err("Not a profile export dialog".into());
            };
            let intent = selection.choices[selection.selected].clone();
            profile_output::choose(world, handle, services, dialog.receipt, intent)
        }
        FileCommand::Export(format, selected) => {
            let intent = io::capture(world, services, &receipt, format, selected)?;
            if format == io::Format::Step {
                io::choose_export(world, handle, services, receipt, intent)
            } else {
                show_dialog(world, receipt, DialogKind::Export(intent))?;
                let token = world.resource::<Files>().dialog.as_ref().unwrap().token;
                queue_layout_check(world, services, owner, token)
            }
        }
        FileCommand::ExportScope(token, scope) => {
            owned_dialog(world, services, owner, token)?;
            let dialog = world
                .resource_mut::<Files>()
                .into_inner()
                .dialog
                .as_mut()
                .ok_or("Export dialog closed")?;
            let DialogKind::Export(intent) = &mut dialog.kind else {
                return Err("Not an export options dialog".into());
            };
            let intent = Arc::make_mut(intent);
            intent.scope = scope;
            intent.bambu.invalidate();
            intent.layout_report = None;
            intent.allow_layout_issues = false;
            queue_layout_check(world, services, owner, token)
        }
        FileCommand::ExportView(_)
        | FileCommand::ExportPrinter(_)
        | FileCommand::ExportAllowIssues(_) => {
            unreachable!("Export fields are reduced before activation")
        }
        FileCommand::ApplyExport(token) => {
            let dialog = owned_dialog(world, services, owner, token)?;
            let DialogKind::Export(intent) = dialog.kind else {
                return Err("Not an export options dialog".into());
            };
            io::check_layout_confirmation(&intent)?;
            bambu::check_review(&intent)?;
            io::choose_export(world, handle, services, dialog.receipt, intent)
        }
        FileCommand::SaveAllAndExit => {
            let mut value = save_all_and_exit(world, handle, services, &receipt)?;
            value["saving_before_exit"] = json!(true);
            Ok(value)
        }
        FileCommand::New => transition(world, receipt, None, None),
        FileCommand::Activate(target) if &target == owner => Ok(json!({"changed":false})),
        FileCommand::Activate(target) => transition(world, receipt, Some(target), None),
        FileCommand::Close => request_intent(world, services, receipt, Intent::Close),
        FileCommand::CloseTab(target) if &target == owner => {
            request_intent(world, services, receipt, Intent::Close)
        }
        FileCommand::CloseTab(target) => {
            workbench::drawing_editor::guard_document_switch(world, owner)?;
            remember_view(world, owner);
            let workspace = world.resource::<Files>().workspace.clone();
            worker::enqueue_transaction(
                world,
                "close_tab_activate".into(),
                move |services, guard| {
                    let result = workspace
                        .lock()
                        .map_err(|_| "Document workspace lock poisoned")?
                        .activate_guarded(
                            &services.bridge,
                            &services.engine,
                            &receipt,
                            &target,
                            || guard.validate(),
                        )?;
                    Ok(NativeMutationResult {
                        context: result.owner,
                        engine_revision: result.revision,
                        value: json!({"changed":true}),
                    })
                },
                |world, services, result| {
                    let result = result?;
                    let receipt = DocumentReceipt {
                        owner: result.context.clone(),
                        revision: result.engine_revision,
                    };
                    let presentation =
                        finish_document_transition(world, services, "close_tab_activate", result);
                    if presentation["render_error"].is_string() {
                        return Ok(presentation);
                    }
                    request_intent(world, services, receipt, Intent::Close)
                },
            )
        }
        FileCommand::Open => choose_path(world, handle, services, receipt, false, None),
        FileCommand::Save | FileCommand::SaveAs => {
            let path = if matches!(command, FileCommand::Save) {
                tabs(world, services, owner)?
                    .into_iter()
                    .find(|t| t.active)
                    .and_then(|t| t.path)
            } else {
                None
            };
            if let Some(path) = path {
                save(world, receipt, path, true, None)
            } else {
                choose_path(world, handle, services, receipt, true, None)
            }
        }
        FileCommand::Rename => show_dialog(
            world,
            receipt,
            DialogKind::Rename(services.engine.document_name()),
        ),
        FileCommand::Exit => Ok(json!({"request_exit":true})),
        FileCommand::ApplyName(token) => {
            let dialog = owned_dialog(world, services, owner, token)?;
            let DialogKind::Rename(name) = dialog.kind else {
                return Err("Not a Rename dialog".into());
            };
            let name = name.trim().to_owned();
            if name.is_empty() {
                return Err("Enter a document name".into());
            }
            let result = worker::enqueue_operation(
                world,
                receipt.owner,
                receipt.revision,
                "cad_set_document_name".into(),
                json!({"name":name}),
                move |world, services, result| {
                    let result = result?;
                    world.resource_mut::<Files>().dialog = None;
                    Ok(finish_mutation(
                        &services.engine,
                        &services.bridge,
                        world,
                        "cad_set_document_name",
                        result,
                    ))
                },
            );
            result
        }
        FileCommand::Discard(token) | FileCommand::SaveContinue(token) => {
            let dialog = owned_dialog(world, services, owner, token)?;
            let DialogKind::Confirm(intent) = dialog.kind else {
                return Err("Not a save confirmation".into());
            };
            if matches!(command, FileCommand::SaveContinue(_)) {
                let path = tabs(world, services, owner)?
                    .into_iter()
                    .find(|t| t.active)
                    .and_then(|t| t.path);
                if let Some(path) = path {
                    save(world, dialog.receipt, path, true, Some(intent))
                } else {
                    choose_path(world, handle, services, dialog.receipt, true, Some(intent))
                }
            } else {
                perform_intent(world, dialog.receipt, intent, true)
            }
        }
        _ => Err("Unsupported File action".into()),
    }
}

fn request_intent(
    world: &mut World,
    services: &NativeServices,
    receipt: DocumentReceipt,
    intent: Intent,
) -> Result<Value, String> {
    // Unapplied drawing fields are not part of the model's dirty state. Check
    // before deciding a clean tab can close without a confirmation dialog.
    workbench::drawing_editor::guard_document_switch(world, &receipt.owner)?;
    if services
        .bridge
        .native_document_receipt(&services.engine, &receipt.owner)?
        != receipt
    {
        return Err("The document changed while the file chooser was open".into());
    }
    let dirty = tabs(world, services, &receipt.owner)?
        .iter()
        .any(|t| t.active && t.dirty);
    if dirty {
        show_dialog(world, receipt, DialogKind::Confirm(intent))
    } else {
        perform_intent(world, receipt, intent, false)
    }
}
fn perform_intent(
    world: &mut World,
    receipt: DocumentReceipt,
    intent: Intent,
    discard: bool,
) -> Result<Value, String> {
    match intent {
        Intent::Exit => {
            let services = world.resource::<NativeServices>().clone();
            let handle = world.resource::<NativeInterfaceHandle>().clone();
            save_all_and_exit(world, &handle, &services, &receipt)
        }
        Intent::Close => transition(world, receipt, None, Some(discard)),
        Intent::Open(path) => {
            remember_view(world, &receipt.owner);
            let workspace = world.resource::<Files>().workspace.clone();
            worker::enqueue_transaction(
                world,
                "open_project".into(),
                move |services, guard| {
                    workspace
                        .lock()
                        .map_err(|_| "Document workspace lock poisoned")?
                        .open_guarded(
                            &services.bridge,
                            &services.engine,
                            &receipt,
                            path,
                            discard,
                            || guard.validate(),
                        )
                },
                |world, services, result| {
                    let result = result?;
                    world.resource_mut::<Files>().dialog = None;
                    Ok(finish_document_transition(
                        world,
                        services,
                        "open_project",
                        result,
                    ))
                },
            )
        }
    }
}

fn save_all_and_exit(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    receipt: &DocumentReceipt,
) -> Result<Value, String> {
    workbench::drawing_editor::guard_document_switch(world, &receipt.owner)?;
    let all_tabs = tabs(world, services, &receipt.owner)?;
    let next = all_tabs
        .iter()
        .find(|tab| tab.active && tab.dirty)
        .or_else(|| all_tabs.iter().find(|tab| tab.dirty));
    let Some(next) = next else {
        return Ok(json!({"request_exit":true}));
    };
    if next.active {
        if let Some(path) = &next.path {
            return save(
                world,
                receipt.clone(),
                path.clone(),
                true,
                Some(Intent::Exit),
            );
        }
        return choose_path(
            world,
            handle,
            services,
            receipt.clone(),
            true,
            Some(Intent::Exit),
        );
    }
    let target = next.owner.clone();
    let receipt = receipt.clone();
    remember_view(world, &receipt.owner);
    let workspace = world.resource::<Files>().workspace.clone();
    worker::enqueue_transaction(
        world,
        "save_all_activate".into(),
        move |services, guard| {
            let result = workspace
                .lock()
                .map_err(|_| "Document workspace lock poisoned")?
                .activate_guarded(
                    &services.bridge,
                    &services.engine,
                    &receipt,
                    &target,
                    || guard.validate(),
                )?;
            Ok(NativeMutationResult {
                context: result.owner,
                engine_revision: result.revision,
                value: json!({"changed":true}),
            })
        },
        |world, services, result| {
            let result = result?;
            let receipt = DocumentReceipt {
                owner: result.context.clone(),
                revision: result.engine_revision,
            };
            let presentation =
                finish_document_transition(world, services, "save_all_activate", result);
            if presentation["render_error"].is_string() {
                return Ok(presentation);
            }
            let handle = world.resource::<NativeInterfaceHandle>().clone();
            save_all_and_exit(world, &handle, services, &receipt)
        },
    )
}
fn transition(
    world: &mut World,
    receipt: DocumentReceipt,
    target: Option<DocumentContext>,
    close: Option<bool>,
) -> Result<Value, String> {
    if close.is_none() {
        workbench::drawing_editor::guard_document_switch(world, &receipt.owner)?;
    }
    remember_view(world, &receipt.owner);
    let closed_document = close.map(|_| receipt.owner.clone());
    let workspace = world.resource::<Files>().workspace.clone();
    let switched_to = target.clone();
    worker::enqueue_transaction(
        world,
        "document_tab".into(),
        move |services, guard| {
            let mut workspace = workspace
                .lock()
                .map_err(|_| "Document workspace lock poisoned")?;
            let result = if let Some(discard) = close {
                workspace.close_active_guarded(
                    &services.bridge,
                    &services.engine,
                    &receipt,
                    discard,
                    || guard.validate(),
                )?
            } else if let Some(target) = target {
                workspace.activate_guarded(
                    &services.bridge,
                    &services.engine,
                    &receipt,
                    &target,
                    || guard.validate(),
                )?
            } else {
                workspace.new_tab_guarded(&services.bridge, &services.engine, &receipt, || {
                    guard.validate()
                })?
            };
            Ok(NativeMutationResult {
                context: result.owner,
                engine_revision: result.revision,
                value: json!({"changed":true}),
            })
        },
        move |world, services, result| {
            let result = result?;
            world.resource_mut::<Files>().dialog = None;
            let window_id = result.context.window_id.clone();
            let document_id = result.context.document_id.clone();
            let presentation = finish_document_transition(world, services, "document_tab", result);
            if switched_to.is_some() {
                let workspace = match workbench::workspace(world) {
                    workbench::Workspace::Drawing => "drawing",
                    workbench::Workspace::Solid => "part",
                    workbench::Workspace::Cam => "cam",
                };
                super::super::switch_timing::annotate(&window_id, &document_id, workspace);
            }
            if let Some(closed) = closed_document {
                native_viewport::retire_interface_model_session(world, &closed.document_id);
                workbench::retire_document(world, &closed);
            }
            Ok(presentation)
        },
    )
}
fn save(
    world: &mut World,
    receipt: DocumentReceipt,
    path: PathBuf,
    overwrite: bool,
    continuation: Option<Intent>,
) -> Result<Value, String> {
    if native_viewport::interface_view_snapshot(world).2.mode
        == native_viewport::ViewportMode::Sketch
    {
        // A close confirmation must leave the sketch intact until the user
        // chooses Save and accepts a destination. Project archives require a
        // finished sketch, so finish it on the ordered worker before saving.
        return worker::enqueue_operation(
            world,
            receipt.owner.clone(),
            receipt.revision,
            "sketch_finish".into(),
            json!({}),
            move |world, services, result| {
                let result = result?;
                let finished = DocumentReceipt {
                    owner: result.context.clone(),
                    revision: result.engine_revision,
                };
                if let Some(dialog) = world.resource_mut::<Files>().dialog.as_mut() {
                    if dialog.receipt == receipt {
                        dialog.receipt = finished.clone();
                    }
                }
                let presented = finish_mutation(
                    &services.engine,
                    &services.bridge,
                    world,
                    "sketch_finish",
                    result,
                );
                if let Some(error) = presented["render_error"].as_str() {
                    return Err(error.into());
                }
                save(world, finished, path, overwrite, continuation)
            },
        );
    }
    let workspace = world.resource::<Files>().workspace.clone();
    worker::enqueue_document_io(
        world,
        "save_project".into(),
        move |services, guard| {
            let saved_at = time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|e| e.to_string())?;
            let prepared = workspace
                .lock()
                .map_err(|_| "Document workspace lock poisoned")?
                .prepare_save_guarded(
                    &services.bridge,
                    &services.engine,
                    &receipt,
                    (path, overwrite),
                    SaveMetadata {
                        application_version: env!("CARGO_PKG_VERSION"),
                        saved_at: &saved_at,
                    },
                    || guard.validate(),
                )?;
            let completed = prepared.write();
            let receipt = workspace
                .lock()
                .map_err(|_| "Document workspace lock poisoned")?
                .complete_save(&services.bridge, completed)?;
            Ok(NativeMutationResult {
                context: receipt.owner,
                engine_revision: receipt.revision,
                value: json!({"saved":true}),
            })
        },
        move |world, _services, result| {
            let result = result?;
            world.resource_mut::<Files>().dialog = None;
            if let Some(intent) = continuation {
                perform_intent(
                    world,
                    DocumentReceipt {
                        owner: result.context,
                        revision: result.engine_revision,
                    },
                    intent,
                    false,
                )
            } else {
                Ok(result.value)
            }
        },
    )
}

/// Qualify the owning window before moving a picker to its worker thread.
/// Keep the returned handle alive until the dialog has closed.
pub(super) fn parented_dialog(
    world: &World,
    dialog: rfd::FileDialog,
) -> Result<(rfd::FileDialog, RawHandleWrapper), String> {
    let mut windows = world
        .try_query_filtered::<(Entity, &RawHandleWrapper), With<PrimaryWindow>>()
        .ok_or("File selection requires the active desktop window")?;
    let (window, parent) = windows
        .single(world)
        .map(|(entity, handle)| (entity, handle.clone()))
        .map_err(|_| "File selection requires the active desktop window")?;
    let dialog = bevy::winit::WINIT_WINDOWS.with_borrow(|windows| {
        let window = windows
            .get_window(window)
            .ok_or("The active desktop window is unavailable")?;
        window
            .window_handle()
            .map_err(|error| format!("File selection window handle: {error}"))?;
        window
            .display_handle()
            .map_err(|error| format!("File selection display handle: {error}"))?;
        Ok::<_, String>(dialog.set_parent(&**window))
    })?;
    Ok((dialog, parent))
}

fn choose_path(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    receipt: DocumentReceipt,
    save: bool,
    continuation: Option<Intent>,
) -> Result<Value, String> {
    if world.resource::<Files>().picker.is_some() {
        return Err("A file chooser is already open".into());
    }
    let active = tabs(world, services, &receipt.owner)?
        .into_iter()
        .find(|tab| tab.active)
        .ok_or("Active tab disappeared")?;
    let (dialog, parent) = parented_dialog(
        world,
        rfd::FileDialog::new().add_filter("Limo CAD project", &["limo", "nbcad"]),
    )?;
    let (send, receive) = mpsc::channel();
    let handle = handle.clone();
    std::thread::Builder::new()
        .name("cad-file-picker".into())
        .spawn(move || {
            let _parent = parent;
            let mut dialog = dialog;
            if let Some(path) = &active.path {
                if let Some(parent) = path.parent() {
                    dialog = dialog.set_directory(parent);
                }
            }
            let name = active
                .path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| {
                    format!(
                        "{}.limo",
                        active
                            .name
                            .replace(['<', '>', ':', '\"', '/', '\\', '|', '?', '*'], "_")
                    )
                });
            let chosen = if save {
                dialog.set_file_name(name).save_file()
            } else {
                dialog.pick_file()
            };
            let _ = send.send(chosen);
            handle.request_redraw();
        })
        .map_err(|e| format!("Cannot open file chooser: {e}"))?;
    world.resource_mut::<Files>().picker = Some(Picker {
        receipt,
        kind: PickerKind::Project { save, continuation },
        result: Mutex::new(receive),
    });
    Ok(json!({"awaiting_input":true}))
}
pub(super) fn poll(world: &mut World, services: &NativeServices) -> Result<(), String> {
    printing::poll(world);
    lessons::poll(world, services);
    scripts::poll(world);
    let result = world.resource::<Files>().picker.as_ref().map(|p| {
        p.result
            .lock()
            .map_err(|_| "File chooser channel poisoned".to_owned())
            .and_then(|channel| match channel.try_recv() {
                Ok(path) => Ok(Some(path)),
                Err(mpsc::TryRecvError::Empty) => Ok(None),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Err("File chooser closed without returning a result".into())
                }
            })
    });
    let path = match result {
        None | Some(Ok(None)) => return Ok(()),
        Some(Ok(Some(path))) => path,
        Some(Err(error)) => {
            world.resource_mut::<Files>().picker = None;
            return Err(error);
        }
    };
    let picker = world.resource_mut::<Files>().picker.take().unwrap();
    let Some(path) = path else {
        return Ok(());
    };
    match picker.kind {
        PickerKind::Project {
            save: true,
            continuation,
        } => {
            save(world, picker.receipt, path, true, continuation)?;
        }
        PickerKind::Project { save: false, .. } => {
            request_intent(world, services, picker.receipt, Intent::Open(path))?;
        }
        PickerKind::ImportStep => {
            io::import(world, picker.receipt, path)?;
        }
        PickerKind::Export(intent) => {
            io::export(world, picker.receipt, intent, path, true)?;
        }
        PickerKind::Drawing(intent) => {
            drawing_output::export(world, picker.receipt, intent, path, true)?;
        }
        PickerKind::Script => {
            let handle = world.resource::<NativeInterfaceHandle>().clone();
            scripts::load(world, &handle, services, picker.receipt, path)?;
        }
        PickerKind::ScriptSave(generation) => {
            let handle = world.resource::<NativeInterfaceHandle>().clone();
            scripts::save(world, &handle, generation, path)?;
        }
        PickerKind::Profile(intent) => {
            profile_output::export(world, picker.receipt, intent, path, true)?;
        }
        PickerKind::BambuTemplate { token, generation } => {
            bambu::selected_path(world, services, picker.receipt, token, generation, path)?;
        }
    }
    Ok(())
}

pub(super) fn request(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    owner: &DocumentContext,
    ui: &Value,
) -> Result<Value, String> {
    require_file_ready(world, ui["command"] == "close")?;
    if awaiting(world) {
        return Err("Finish the current File dialog first".into());
    }
    let receipt = current(world, services, owner)?;
    match ui["command"].as_str().unwrap_or("") {
        "print_drawing" => printing::request(world, receipt),
        "print_status" => Ok(json!({"printing":printing::status(world, owner)})),
        "export_profile_dxf" => {
            let feature_id = ui["feature_id"]
                .as_u64()
                .ok_or("Choose a sketch feature ID")?;
            let profile_index = ui["profile_index"]
                .as_u64()
                .and_then(|i| u32::try_from(i).ok())
                .ok_or("Choose a zero-based profile index")?;
            let selection = profile_output::capture(services, &receipt)?;
            let intent = selection
                .choices
                .into_iter()
                .find(|i| i.feature_id == feature_id && i.profile_index == profile_index)
                .ok_or("Choose an available material profile")?;
            let path = PathBuf::from(
                ui["path"]
                    .as_str()
                    .ok_or("Export requires an absolute path")?,
            );
            profile_output::export(world, receipt, intent, path, ui["overwrite"] == true)
        }
        "import_step" => io::import(
            world,
            receipt,
            PathBuf::from(
                ui["path"]
                    .as_str()
                    .ok_or("Import requires an absolute STEP/STP path")?,
            ),
        ),
        "export_drawing_svg" | "export_drawing_dxf" => {
            let format = if ui["command"] == "export_drawing_svg" {
                drawing_output::Format::Svg
            } else {
                drawing_output::Format::Dxf
            };
            let intent = drawing_output::capture(services, &receipt, format)?;
            let path = PathBuf::from(
                ui["path"]
                    .as_str()
                    .ok_or("Export requires an absolute path")?,
            );
            drawing_output::export(world, receipt, intent, path, ui["overwrite"] == true)
        }
        "export_step" | "export_3mf" | "export_stl" => {
            let format = match ui["command"].as_str().unwrap() {
                "export_step" => io::Format::Step,
                "export_3mf" => io::Format::ThreeMf,
                _ => io::Format::Stl,
            };
            let mut intent = io::capture(
                world,
                services,
                &receipt,
                format,
                ui["selected_only"] == true,
            )?;
            let draft = Arc::make_mut(&mut intent);
            if format != io::Format::Step {
                draft.scope = serde_json::from_value(ui["scope"].clone())
                    .map_err(|_| "Mesh export requires scope assembly or definition")?;
            }
            if format == io::Format::ThreeMf && !ui["slicer_target"].is_null() {
                draft.slicer_target = serde_json::from_value(ui["slicer_target"].clone())
                    .map_err(|_| "Choose an existing shared 3MF slicer target")?;
            }
            if format != io::Format::Step {
                if let Some(name) = ui["named_view"].as_str() {
                    draft.named_view = Some(name.into());
                }
                if !ui["print_bed"].is_null() {
                    draft.print_bed = Some(
                        serde_json::from_value(ui["print_bed"].clone())
                            .map_err(|e| format!("Invalid print bed: {e}"))?,
                    );
                }
                draft.allow_layout_issues = ui["allow_layout_issues"] == true;
            }
            let path = PathBuf::from(
                ui["path"]
                    .as_str()
                    .ok_or("Export requires an absolute path")?,
            );
            io::export(world, receipt, intent, path, ui["overwrite"] == true)
        }
        "new" => execute(world, handle, services, owner, FileCommand::New),
        "close" => execute(world, handle, services, owner, FileCommand::Close),
        "rename" => {
            let name = ui["name"].as_str().unwrap_or("").trim();
            if name.is_empty() {
                return Err("Enter a document name".into());
            }
            worker::enqueue_operation(
                world,
                owner.clone(),
                receipt.revision,
                "cad_set_document_name".into(),
                json!({"name":name}),
                |world, services, result| {
                    Ok(finish_mutation(
                        &services.engine,
                        &services.bridge,
                        world,
                        "cad_set_document_name",
                        result?,
                    ))
                },
            )
        }
        "save" => {
            let path = PathBuf::from(
                ui["path"]
                    .as_str()
                    .ok_or("Save requires an absolute .limo path")?,
            );
            let overwrite = ui["overwrite"] == true;
            if path.exists() && !overwrite {
                return Err("The destination exists; overwrite must be explicit".into());
            }
            save(world, receipt, path, overwrite, None)
        }
        "open" => {
            let path = PathBuf::from(
                ui["path"]
                    .as_str()
                    .ok_or("Open requires an absolute .limo path")?,
            );
            if ui["discard_changes"] == true {
                perform_intent(world, receipt, Intent::Open(path), true)
            } else {
                request_intent(world, services, receipt, Intent::Open(path))
            }
        }
        _ => Err("Unknown File command".into()),
    }
}

pub(super) fn open_path(
    world: &mut World,
    services: &NativeServices,
    owner: &DocumentContext,
    path: PathBuf,
) -> Result<Value, String> {
    require_idle_model(world)?;
    if awaiting(world) {
        return Err("Finish the current File dialog first".into());
    }
    let receipt = current(world, services, owner)?;
    request_intent(world, services, receipt, Intent::Open(path))
}

pub(super) fn escape(world: &mut World) {
    let mut f = world.resource_mut::<Files>();
    f.menu = false;
    f.scripts = false;
    f.settings = false;
    f.dialog = None;
}
pub(super) fn dialog_error(world: &mut World, error: &str) {
    if let Some(dialog) = world.resource_mut::<Files>().dialog.as_mut() {
        dialog.error = Some(error.to_owned());
    }
}

/// File shortcuts use the same commands as the visible menu. The caller checks
/// document ownership before offering the event; a modal always owns its keys.
pub(super) fn is_save_shortcut(event: &NativeHostInput) -> bool {
    use bevy::input::{
        keyboard::{Key, KeyCode},
        ButtonState,
    };
    let WindowEvent::KeyboardInput(key) = &event.event else {
        return false;
    };
    key.state == ButtonState::Pressed
        && !key.repeat
        && !event.modifiers.alt
        && !event.modifiers.alt_graph
        && (event.modifiers.ctrl || event.modifiers.meta)
        && matches!(&key.logical_key, Key::Character(character)
            if character.eq_ignore_ascii_case("s")
                || (character.chars().all(char::is_control) && key.key_code == KeyCode::KeyS))
}

/// File shortcuts retain the original document owner when a read defers input.
pub(super) fn shortcut(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    event: &NativeHostInput,
) -> Result<Option<Value>, String> {
    use bevy::input::{
        keyboard::{Key, KeyCode},
        ButtonState,
    };
    if modal(world).is_some()
        || awaiting(world)
        || handle
            .frame()
            .is_some_and(|frame| !frame.modal_stack.is_empty())
    {
        handle.record_file_shortcut(event, "blocked_modal_or_awaiting");
        return Ok(None);
    }
    let WindowEvent::KeyboardInput(key) = &event.event else {
        return Ok(None);
    };
    if key.state != ButtonState::Pressed
        || key.repeat
        || event.modifiers.alt
        || event.modifiers.alt_graph
        || !(event.modifiers.ctrl || event.modifiers.meta)
    {
        handle.record_file_shortcut(event, "not_command_chord");
        return Ok(None);
    }
    let Key::Character(character) = &key.logical_key else {
        handle.record_file_shortcut(event, "non_character_key");
        return Ok(None);
    };
    let character = if character.chars().all(char::is_control) {
        match key.key_code {
            KeyCode::KeyN => "n",
            KeyCode::KeyO => "o",
            KeyCode::KeyS => "s",
            KeyCode::KeyW => "w",
            KeyCode::KeyP => "p",
            _ => "",
        }
        .into()
    } else {
        character.to_lowercase()
    };
    let command = match (character.as_str(), event.modifiers.shift) {
        ("n", false) => FileCommand::New,
        ("o", false) => FileCommand::Open,
        ("s", false) => FileCommand::Save,
        ("s", true) => FileCommand::SaveAs,
        ("w", false) => FileCommand::Close,
        ("p", false) => FileCommand::PrintDrawing,
        _ => {
            handle.record_file_shortcut(event, "unmapped_chord");
            return Ok(None);
        }
    };
    handle.record_file_shortcut(event, "recognized_owner_check");
    let owner = event
        .context
        .as_ref()
        .ok_or("File shortcut has no document context")?;
    if let Err(error) = services
        .bridge
        .with_native_document_owner(&services.engine, owner, || Ok(()))
    {
        handle.record_file_shortcut(event, "rejected_owner");
        return Err(error);
    }
    let result = execute(world, handle, services, owner, command);
    handle.record_file_shortcut(
        event,
        if result.is_ok() {
            "dispatched"
        } else {
            "command_error"
        },
    );
    result.map(Some)
}

#[cfg(test)]
mod tests;
