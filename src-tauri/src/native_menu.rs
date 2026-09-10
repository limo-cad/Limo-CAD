//! Native application-menu integration.
//!
//! Tauri's default macOS Undo/Redo entries are predefined AppKit responder
//! actions. They work for native text controls, but a CAD operation stack in
//! the webview/Rust engine is not part of that responder chain. Keep the
//! standard menu and replace only those two entries with application commands
//! that the frontend routes through the same history controller as shortcuts.

#[cfg(target_os = "macos")]
use std::io;
use std::sync::Mutex;

#[cfg(target_os = "macos")]
use tauri::menu::{
    IsMenuItem, Menu, MenuEvent, MenuItemKind, PredefinedMenuItem, Submenu,
};
use tauri::menu::MenuItem;
#[cfg(target_os = "macos")]
use tauri::{AppHandle, Emitter, Manager};
use tauri::Wry;

#[cfg(target_os = "macos")]
pub const EDIT_COMMAND_EVENT: &str = "native-edit-command";
#[cfg(target_os = "macos")]
pub const QUIT_COMMAND_EVENT: &str = "native-quit-request";
#[cfg(target_os = "macos")]
pub const FILE_COMMAND_EVENT: &str = "native-file-command";
#[cfg(target_os = "macos")]
const UNDO_ID: &str = "nbcad-edit-undo";
#[cfg(target_os = "macos")]
const REDO_ID: &str = "nbcad-edit-redo";
#[cfg(target_os = "macos")]
const QUIT_ID: &str = "nbcad-app-quit";

#[cfg(target_os = "macos")]
const FILE_NEW_ID: &str = "nbcad-file-new";
#[cfg(target_os = "macos")]
const FILE_OPEN_ID: &str = "nbcad-file-open";
#[cfg(target_os = "macos")]
const FILE_OPEN_SCRIPT_ID: &str = "nbcad-file-open-script";
#[cfg(target_os = "macos")]
const FILE_SAVE_ID: &str = "nbcad-file-save";
#[cfg(target_os = "macos")]
const FILE_SAVE_AS_ID: &str = "nbcad-file-save-as";
#[cfg(target_os = "macos")]
const FILE_RENAME_ID: &str = "nbcad-file-rename";
#[cfg(target_os = "macos")]
const FILE_IMPORT_STEP_ID: &str = "nbcad-file-import-step";
#[cfg(target_os = "macos")]
const FILE_EXPORT_STEP_ALL_ID: &str = "nbcad-file-export-step-all";
#[cfg(target_os = "macos")]
const FILE_EXPORT_STEP_SELECTED_ID: &str = "nbcad-file-export-step-selected";
#[cfg(target_os = "macos")]
const FILE_EXPORT_3MF_ALL_ID: &str = "nbcad-file-export-3mf-all";
#[cfg(target_os = "macos")]
const FILE_EXPORT_3MF_SELECTED_ID: &str = "nbcad-file-export-3mf-selected";
#[cfg(target_os = "macos")]
const FILE_EXPORT_STL_ALL_ID: &str = "nbcad-file-export-stl-all";
#[cfg(target_os = "macos")]
const FILE_EXPORT_STL_SELECTED_ID: &str = "nbcad-file-export-stl-selected";
#[cfg(target_os = "macos")]
const FILE_EXPORT_DRAWING_DXF_ID: &str = "nbcad-file-export-drawing-dxf";
#[cfg(target_os = "macos")]
const FILE_EXPORT_PROFILE_DXF_ID: &str = "nbcad-file-export-profile-dxf";
#[cfg(target_os = "macos")]
const APP_SETTINGS_ID: &str = "nbcad-app-settings";

/// Native File-menu item id → frontend command payload. Mirrors the in-app
/// File menu one-to-one so both entry points run the same project actions.
#[cfg(target_os = "macos")]
const FILE_COMMANDS: [(&str, &str); 16] = [
    (FILE_NEW_ID, "new"),
    (FILE_OPEN_ID, "open"),
    (FILE_OPEN_SCRIPT_ID, "open-script"),
    (FILE_SAVE_ID, "save"),
    (FILE_SAVE_AS_ID, "save-as"),
    (FILE_RENAME_ID, "rename"),
    (FILE_IMPORT_STEP_ID, "import-step"),
    (FILE_EXPORT_STEP_ALL_ID, "export-step-all"),
    (FILE_EXPORT_STEP_SELECTED_ID, "export-step-selected"),
    (FILE_EXPORT_3MF_ALL_ID, "export-3mf-all"),
    (FILE_EXPORT_3MF_SELECTED_ID, "export-3mf-selected"),
    (FILE_EXPORT_STL_ALL_ID, "export-stl-all"),
    (FILE_EXPORT_STL_SELECTED_ID, "export-stl-selected"),
    (FILE_EXPORT_DRAWING_DXF_ID, "export-drawing-dxf"),
    (FILE_EXPORT_PROFILE_DXF_ID, "export-profile-dxf"),
    (APP_SETTINGS_ID, "settings"),
];

#[derive(Default)]
pub struct NativeEditMenuState {
    items: Mutex<Option<(MenuItem<Wry>, MenuItem<Wry>)>>,
}

/// File-menu handles grouped by the store condition that enables them.
/// Settings stays enabled while project/model mutations are busy; every
/// project action, including New and Open, is disabled until the mutation
/// finishes.
pub struct NativeFileItems {
    idle_items: Vec<MenuItem<Wry>>,
    document_items: Vec<MenuItem<Wry>>,
    all_body_items: Vec<MenuItem<Wry>>,
    selected_body_items: Vec<MenuItem<Wry>>,
    drawing_dxf: MenuItem<Wry>,
    profile_dxf: MenuItem<Wry>,
}

#[derive(Default)]
pub struct NativeFileMenuState {
    items: Mutex<Option<NativeFileItems>>,
}

impl NativeFileMenuState {
    #[cfg(target_os = "macos")]
    fn install(&self, items: NativeFileItems) {
        if let Ok(mut slot) = self.items.lock() {
            *slot = Some(items);
        }
    }

    fn set_state(
        &self,
        busy: bool,
        document_open: bool,
        has_bodies: bool,
        has_selected_body: bool,
        drawing_workspace: bool,
        drawing_sheet_ready: bool,
    ) -> Result<(), String> {
        let items = self
            .items
            .lock()
            .map_err(|_| "native File menu state is unavailable".to_string())?;
        let Some(items) = items.as_ref() else {
            // Non-macOS builds do not install an application menu.
            return Ok(());
        };
        for item in &items.idle_items {
            item.set_enabled(!busy)
                .map_err(|error| error.to_string())?;
        }
        for item in &items.document_items {
            item.set_enabled(!busy && document_open)
                .map_err(|error| error.to_string())?;
        }
        for item in &items.all_body_items {
            item.set_enabled(!busy && document_open && has_bodies)
                .map_err(|error| error.to_string())?;
        }
        for item in &items.selected_body_items {
            item.set_enabled(!busy && document_open && has_selected_body)
                .map_err(|error| error.to_string())?;
        }
        items
            .drawing_dxf
            .set_enabled(!busy && drawing_workspace && drawing_sheet_ready)
            .map_err(|error| error.to_string())?;
        items
            .profile_dxf
            .set_enabled(!busy && drawing_workspace)
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

impl NativeEditMenuState {
    #[cfg(target_os = "macos")]
    fn install(&self, undo: MenuItem<Wry>, redo: MenuItem<Wry>) {
        if let Ok(mut items) = self.items.lock() {
            *items = Some((undo, redo));
        }
    }

    fn set_enabled(&self, can_undo: bool, can_redo: bool) -> Result<(), String> {
        let items = self
            .items
            .lock()
            .map_err(|_| "native Edit menu state is unavailable".to_string())?;
        let Some((undo, redo)) = items.as_ref() else {
            // Non-macOS builds do not install an application menu.
            return Ok(());
        };
        undo.set_enabled(can_undo)
            .map_err(|error| error.to_string())?;
        redo.set_enabled(can_redo)
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

/// Build the normal Tauri/macOS menu, replacing only responder-chain
/// Undo/Redo with application-owned items and standard accelerators.
#[cfg(target_os = "macos")]
pub fn build(app: &AppHandle<Wry>) -> tauri::Result<Menu<Wry>> {
    let menu = Menu::default(app)?;
    let application = menu
        .items()?
        .into_iter()
        .find_map(|item| match item {
            MenuItemKind::Submenu(submenu) => Some(submenu),
            _ => None,
        })
        .ok_or_else(|| io::Error::other("default macOS menu has no application submenu"))?;
    let quit_index = application
        .items()?
        .into_iter()
        .enumerate()
        .find_map(|(index, item)| match item {
            MenuItemKind::Predefined(item) if item.text().ok().is_some_and(|text| {
                text.replace('&', "").trim_start().starts_with("Quit")
            }) => Some(index),
            _ => None,
        })
        .ok_or_else(|| io::Error::other("default macOS application menu has no Quit item"))?;
    application.remove_at(quit_index)?;
    let quit = MenuItem::with_id(
        app,
        QUIT_ID,
        format!("Quit {}", app.package_info().name),
        true,
        Some("CmdOrCtrl+Q"),
    )?;
    application.insert(&quit, quit_index)?;

    // Mirror the in-app File menu so the macOS menu bar and the window's
    // File dropdown run the same commands. Fall back to creating the submenu
    // if a future Tauri default menu ever drops it; Close Window stays last.
    let file = match menu
        .items()?
        .into_iter()
        .find_map(|item| match item {
            MenuItemKind::Submenu(submenu) if submenu.text().ok().as_deref() == Some("File") => {
                Some(submenu)
            }
            _ => None,
        }) {
        Some(submenu) => submenu,
        None => {
            let submenu = Submenu::with_id(app, "nbcad-file-menu", "File", true)?;
            menu.insert(&submenu, 1)?;
            submenu
        }
    };
    let new_project =
        MenuItem::with_id(app, FILE_NEW_ID, "New Project", true, Some("CmdOrCtrl+N"))?;
    let open =
        MenuItem::with_id(app, FILE_OPEN_ID, "Open Project…", true, Some("CmdOrCtrl+O"))?;
    let open_script =
        MenuItem::with_id(app, FILE_OPEN_SCRIPT_ID, "Open Script…", true, None::<&str>)?;
    let save = MenuItem::with_id(app, FILE_SAVE_ID, "Save", false, Some("CmdOrCtrl+S"))?;
    let save_as =
        MenuItem::with_id(app, FILE_SAVE_AS_ID, "Save As…", false, Some("CmdOrCtrl+Shift+S"))?;
    let rename = MenuItem::with_id(app, FILE_RENAME_ID, "Rename Project…", false, None::<&str>)?;
    let import_step =
        MenuItem::with_id(app, FILE_IMPORT_STEP_ID, "Import STEP/STP…", false, None::<&str>)?;
    let export_step_all = MenuItem::with_id(
        app,
        FILE_EXPORT_STEP_ALL_ID,
        "Export All Bodies as STEP…",
        false,
        None::<&str>,
    )?;
    let export_step_selected = MenuItem::with_id(
        app,
        FILE_EXPORT_STEP_SELECTED_ID,
        "Export Selected Body as STEP…",
        false,
        None::<&str>,
    )?;
    let export_3mf_all = MenuItem::with_id(
        app,
        FILE_EXPORT_3MF_ALL_ID,
        "Export All Bodies as 3MF…",
        false,
        None::<&str>,
    )?;
    let export_3mf_selected = MenuItem::with_id(
        app,
        FILE_EXPORT_3MF_SELECTED_ID,
        "Export Selected Body as 3MF…",
        false,
        None::<&str>,
    )?;
    let export_stl_all = MenuItem::with_id(
        app,
        FILE_EXPORT_STL_ALL_ID,
        "Export All Bodies as STL…",
        false,
        None::<&str>,
    )?;
    let export_stl_selected = MenuItem::with_id(
        app,
        FILE_EXPORT_STL_SELECTED_ID,
        "Export Selected Body as STL…",
        false,
        None::<&str>,
    )?;
    let drawing_dxf = MenuItem::with_id(
        app,
        FILE_EXPORT_DRAWING_DXF_ID,
        "Export Active Drawing as DXF…",
        false,
        None::<&str>,
    )?;
    let profile_dxf = MenuItem::with_id(
        app,
        FILE_EXPORT_PROFILE_DXF_ID,
        "Export 1:1 Manufacturing Profile DXF…",
        false,
        None::<&str>,
    )?;
    let settings = MenuItem::with_id(app, APP_SETTINGS_ID, "Settings", true, None::<&str>)?;
    let separators: Vec<_> = (0..5)
        .map(|_| PredefinedMenuItem::separator(app))
        .collect::<tauri::Result<_>>()?;
    let file_items: Vec<&dyn IsMenuItem<Wry>> = vec![
        &new_project,
        &open,
        &open_script,
        &save,
        &save_as,
        &rename,
        &separators[0],
        &import_step,
        &separators[1],
        &export_step_all,
        &export_step_selected,
        &export_3mf_all,
        &export_3mf_selected,
        &export_stl_all,
        &export_stl_selected,
        &separators[2],
        &drawing_dxf,
        &profile_dxf,
        &separators[3],
        &settings,
        &separators[4],
    ];
    file.insert_items(&file_items, 0)?;
    app.state::<NativeFileMenuState>().install(NativeFileItems {
        idle_items: vec![new_project, open, open_script],
        document_items: vec![save, save_as, rename, import_step],
        all_body_items: vec![export_step_all, export_3mf_all, export_stl_all],
        selected_body_items: vec![export_step_selected, export_3mf_selected, export_stl_selected],
        drawing_dxf,
        profile_dxf,
    });

    let edit = menu
        .items()?
        .into_iter()
        .find_map(|item| match item {
            MenuItemKind::Submenu(submenu) if submenu.text().ok().as_deref() == Some("Edit") => {
                Some(submenu)
            }
            _ => None,
        })
        .ok_or_else(|| io::Error::other("default macOS menu has no Edit submenu"))?;

    // Default order is Undo, Redo, separator. Removing index zero twice keeps
    // the separator and every standard Cut/Copy/Paste item untouched.
    edit.remove_at(0)?;
    edit.remove_at(0)?;
    let undo = MenuItem::with_id(app, UNDO_ID, "Undo", false, Some("CmdOrCtrl+Z"))?;
    let redo = MenuItem::with_id(app, REDO_ID, "Redo", false, Some("CmdOrCtrl+Shift+Z"))?;
    edit.insert_items(&[&undo, &redo], 0)?;
    app.state::<NativeEditMenuState>().install(undo, redo);
    Ok(menu)
}

#[cfg(target_os = "macos")]
pub fn handle_event(app: &AppHandle<Wry>, event: MenuEvent) {
    if event.id() == QUIT_ID {
        let _ = app.emit(QUIT_COMMAND_EVENT, ());
        return;
    }
    if event.id() == UNDO_ID || event.id() == REDO_ID {
        let command = if event.id() == UNDO_ID { "undo" } else { "redo" };
        let _ = app.emit(EDIT_COMMAND_EVENT, command);
        return;
    }
    if let Some((_, command)) = FILE_COMMANDS.iter().find(|(id, _)| event.id() == *id) {
        let _ = app.emit(FILE_COMMAND_EVENT, *command);
    }
}

#[tauri::command]
pub fn native_edit_menu_set_state(
    state: tauri::State<'_, NativeEditMenuState>,
    can_undo: bool,
    can_redo: bool,
) -> Result<(), String> {
    state.set_enabled(can_undo, can_redo)
}

#[tauri::command]
pub fn native_file_menu_set_state(
    state: tauri::State<'_, NativeFileMenuState>,
    busy: bool,
    document_open: bool,
    has_bodies: bool,
    has_selected_body: bool,
    drawing_workspace: bool,
    drawing_sheet_ready: bool,
) -> Result<(), String> {
    state.set_state(
        busy,
        document_open,
        has_bodies,
        has_selected_body,
        drawing_workspace,
        drawing_sheet_ready,
    )
}
