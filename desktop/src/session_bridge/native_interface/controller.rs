//! Main-window controller using the existing native document and MCP services.

use super::*;
use crate::native_viewport::{
    interface_shell::{self, spawn_button, InterfaceCamera, InterfaceFrame, InterfaceLayout},
    ui::{ViewportUiAssets, ViewportUiTheme},
    winit_host::NativeHostInput,
};
use crate::session_bridge::{
    apply_or_reject_one_inbox_op_with_editor_guards, control_for_window, now_ms,
};
use bevy::{
    ecs::{message::MessageCursor, system::SystemState},
    prelude::*,
    text::FontWeight,
    window::{PrimaryWindow, WindowEvent},
};
use limo_cad_interface::{Canvas, ControlRequest, DocumentContext, Rect as InterfaceRect, Surface};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

pub(crate) mod app_settings;
pub(crate) mod assembly;
pub(crate) mod body_appearance;
pub(crate) mod browser;
mod capture;
pub(crate) mod chrome;
pub(crate) mod files;
pub(crate) mod history;
pub(crate) mod named_views;
pub(crate) mod presentation;
pub(crate) mod print_intent;
mod retention;
pub(crate) mod section_review;
pub(crate) mod six_dof;
pub(crate) mod workbench;
pub(crate) mod worker;

#[derive(Resource, Clone)]
pub(crate) struct NativeServices {
    pub engine: Arc<AppState>,
    pub bridge: Arc<SessionBridgeState>,
}
impl Default for NativeServices {
    fn default() -> Self {
        Self {
            engine: Arc::new(AppState::new()),
            bridge: Arc::new(SessionBridgeState::default()),
        }
    }
}

struct PendingControl {
    response: Value,
    owner: DocumentContext,
    presentation_deadline: u64,
    inspect: bool,
}
struct PolledControl {
    owner: DocumentContext,
    session: String,
    id: String,
    interface_only: bool,
}
struct DeferredPointer {
    receipt: workspace::DocumentReceipt,
    camera: Option<(String, native_viewport::ViewportCamera)>,
    canvas: Option<InterfaceRect>,
    events: VecDeque<NativeHostInput>,
}

#[derive(Resource)]
struct Controller {
    workspace: Arc<Mutex<workspace::DocumentWorkspace>>,
    window_id: String,
    initial_model: Option<String>,
    initialized: bool,
    initial_revision: u64,
    initial_owner: Option<DocumentContext>,
    input: MessageCursor<NativeHostInput>,
    bodies: Vec<(u64, String)>,
    synchronized: Option<(DocumentContext, u64)>,
    controls: HashMap<String, Entity>,
    decoration: HashMap<String, Entity>,
    sidebar_scroll: f32,
    logical_size: Vec2,
    pending: Option<PendingControl>,
    status: String,
    sketch_input_error: Option<(DocumentContext, String, String)>,
    close_pending: bool,
    exit_after_receipt: bool,
    close_after_worker: bool,
    busy_controls: Vec<(Entity, bool)>,
    cached_session: Option<String>,
    polled_control: Option<PolledControl>,
    deferred_pointer: Option<DeferredPointer>,
    /// A Save key pressed during a read is replayed with its original owner.
    deferred_save: Option<NativeHostInput>,
    watch_session: Arc<Mutex<Option<String>>>,
    stop_watcher: Arc<AtomicBool>,
    retention_wake: retention::Wake,
}
impl Controller {
    fn new(
        window_id: String,
        initial_model: Option<String>,
        stop_watcher: Arc<AtomicBool>,
    ) -> Self {
        Self {
            workspace: Arc::new(Mutex::new(workspace::DocumentWorkspace::default())),
            window_id,
            initial_model,
            initialized: false,
            initial_revision: 0,
            initial_owner: None,
            input: MessageCursor::default(),
            bodies: vec![],
            synchronized: None,
            controls: HashMap::new(),
            decoration: HashMap::new(),
            sidebar_scroll: 0.,
            logical_size: Vec2::new(1360., 860.),
            pending: None,
            status: String::new(),
            sketch_input_error: None,
            close_pending: false,
            exit_after_receipt: false,
            close_after_worker: false,
            busy_controls: Vec::new(),
            cached_session: None,
            polled_control: None,
            deferred_pointer: None,
            deferred_save: None,
            watch_session: Arc::new(Mutex::new(None)),
            stop_watcher,
            retention_wake: retention::Wake::default(),
        }
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        self.stop_watcher.store(true, Ordering::Release);
    }
}

/// Install the native controller over the shared document engine and MCP bridge.
pub(crate) fn install(
    app: &mut App,
    handle: NativeInterfaceHandle,
    services: NativeServices,
    window_id: String,
    initial_model: Option<String>,
) {
    let stop = Arc::new(AtomicBool::new(false));
    let worker_error = worker::install(app.world_mut(), services.clone(), handle.clone()).err();
    let device_error = six_dof::install(app.world_mut(), &window_id, &handle).err();
    let controller = Controller::new(window_id.clone(), initial_model, stop.clone());
    let preferences_wake = app_settings::install(app.world_mut());
    start_watcher(
        &services,
        &window_id,
        handle.clone(),
        stop,
        controller.watch_session.clone(),
        preferences_wake,
        controller.retention_wake.clone(),
    );
    app.insert_resource(services).insert_resource(controller);
    if let Some(error) = worker_error.or(device_error) {
        app.world_mut().resource_mut::<Controller>().status = error;
    }
    interface_shell::install(app, handle, update);
    app.add_systems(PostUpdate, complete_control.after(InterfaceLayout));
}

/// `recipe` is an installed id, or the `limo-cad://recipe/...` URL delivered by a GetURL event.
pub(crate) fn open_startup_recipe(world: &mut World, recipe: &str) {
    let recipe = if recipe.starts_with("limo-cad:") || recipe.starts_with("nbcad:") {
        match limo_cad_mcp::recipe_id_from_uri(recipe) {
            Ok(id) => id,
            Err(error) => {
                world.resource_mut::<Controller>().status = error;
                return;
            }
        }
    } else {
        recipe
    };
    let workspace = world.resource::<Controller>().workspace.clone();
    files::initialize(world, workspace);
    if let Err(error) = files::queue_recipe(world, recipe) {
        world.resource_mut::<Controller>().status = error;
    }
}

#[derive(Resource)]
struct StartupProject(std::path::PathBuf);

pub(crate) fn open_startup_project(world: &mut World, path: std::path::PathBuf) {
    world.insert_resource(StartupProject(path));
}

#[cfg(test)]
pub(crate) fn insert_startup_controller(world: &mut World) {
    world.insert_resource(Controller::new(
        "main".into(),
        None,
        Arc::new(AtomicBool::new(false)),
    ));
}

#[cfg(test)]
pub(crate) fn queued_startup_recipe(world: &World) -> Option<String> {
    files::queued_recipe_id(world)
}

fn start_watcher(
    services: &NativeServices,
    window_id: &str,
    handle: NativeInterfaceHandle,
    stop: Arc<AtomicBool>,
    cached_session: Arc<Mutex<Option<String>>>,
    preferences_wake: app_settings::Wake,
    retention_wake: retention::Wake,
) {
    let bridge = Arc::downgrade(&services.bridge);
    let window = window_id.to_owned();
    let _ = std::thread::Builder::new()
        .name("cad-native-inbox".into())
        .spawn(move || {
            let mut keepalive = now_ms();
            let mut preferences = app_settings::watch();
            let mut last_preferences = None;
            let mut memory = retention::Watch::new(retention_wake);
            let heartbeat_running = Arc::new(AtomicBool::new(false));
            while !stop.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(25));
                if stop.load(Ordering::Acquire) {
                    break;
                }
                if let Some(value) = preferences
                    .as_mut()
                    .and_then(|p| p.poll(std::time::Instant::now(), false))
                {
                    if last_preferences.as_ref() != Some(&value) {
                        last_preferences = Some(value);
                        preferences_wake.changed();
                        handle.request_redraw();
                    }
                }
                if memory.poll(std::time::Instant::now()) {
                    handle.request_redraw();
                }
                let Some(bridge) = bridge.upgrade() else {
                    break;
                };
                if now_ms().saturating_sub(keepalive) >= 10_000 {
                    keepalive = now_ms();
                    if !heartbeat_running.swap(true, Ordering::AcqRel) {
                        let bridge = bridge.clone();
                        let window = window.clone();
                        let running = heartbeat_running.clone();
                        let fallback = running.clone();
                        if std::thread::Builder::new()
                            .name("cad-session-heartbeat".into())
                            .spawn(move || {
                                let _ = bridge.heartbeat_for_window(&window);
                                running.store(false, Ordering::Release);
                            })
                            .is_err()
                        {
                            fallback.store(false, Ordering::Release);
                        }
                    }
                }
                let Some(session) = cached_session
                    .lock()
                    .ok()
                    .and_then(|session| session.clone())
                else {
                    continue;
                };
                let root = crate::session_bridge::session_root().join(session);
                let controls = limo_cad_session_storage::read_dir(root.join("controls"))
                    .ok()
                    .is_some_and(|entries| {
                        entries.filter_map(Result::ok).any(|entry| {
                            entry
                                .file_name()
                                .to_string_lossy()
                                .ends_with(".request.json")
                        })
                    });
                let inbox = limo_cad_session_storage::read_dir(root.join("inbox"))
                    .ok()
                    .is_some_and(|entries| {
                        entries.filter_map(Result::ok).any(|entry| {
                            entry.file_type().is_ok_and(|kind| kind.is_file())
                                && entry.path().extension().is_some_and(|ext| ext == "json")
                        })
                    });
                if controls || inbox {
                    handle.request_redraw();
                }
            }
        });
}

fn update(world: &mut World, handle: &NativeInterfaceHandle) {
    let services = world.resource::<NativeServices>().clone();
    world.resource_scope(|world, mut state: Mut<Controller>| {
        let result = update_inner(world, handle, &services, &mut state);
        if let Err(error) = result {
            if world.contains_resource::<files::Files>() {
                files::dialog_error(world, &error);
            }
            state.status = error;
        }
    });
}

fn update_inner(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    state: &mut Controller,
) -> Result<(), String> {
    let engine = &services.engine;
    let bridge = &services.bridge;
    files::initialize(world, state.workspace.clone());
    if crate::native_editor::mechanism::active(world) {
        let mut pending = state.input.clone();
        let events = pending
            .read(world.resource::<Messages<NativeHostInput>>())
            .cloned()
            .collect::<Vec<_>>();
        for event in events {
            crate::native_editor::mechanism::observe_busy(world, &event);
        }
    }
    if let Some(outcome) = worker::poll(world, services) {
        for (entity, disabled) in state.busy_controls.drain(..) {
            if let Some(mut control) = world.get_mut::<InterfaceControl>(entity) {
                control.disabled = disabled;
            }
        }
        if let Some(pending) = state
            .pending
            .as_mut()
            .filter(|pending| pending.response["value"]["mutation_id"].as_u64() == Some(outcome.id))
        {
            pending.owner = bridge.native_document_context(&state.window_id, engine)?;
            match &outcome.value {
                Ok(value) => {
                    pending.response["status"] = json!(if value["model_error"].is_string() {
                        "failed"
                    } else {
                        "applied"
                    });
                    if value["model_error"].is_string() {
                        pending.response["error"] = value["model_error"].clone();
                    }
                    pending.response["value"] = value.clone();
                }
                Err(error) => {
                    pending.response["status"] = json!("failed");
                    pending.response["error"] = json!(error);
                    pending.response["value"] = Value::Null;
                }
            }
            pending.presentation_deadline = now_ms().saturating_add(2_000);
        }
        if let Some(polled) = state.polled_control.take() {
            match outcome.value {
                Ok(value) => {
                    let request = &value["control_request"];
                    if request.get("id").is_some() {
                        start_control(world, handle, services, state, &polled.owner, request)?;
                    }
                }
                Err(error) => {
                    crate::session_bridge::reject_native_control(
                        &polled.session,
                        &polled.id,
                        "native_control_failed",
                        &error,
                    )?;
                    state.status = error;
                }
            }
        } else {
            match outcome.value {
                Ok(value) => {
                    apply_host_result(state, &value);
                    if value["request_exit"] == true {
                        request_close(world, state, bridge, engine)?;
                    }
                    if outcome.operation != "memory-retention" {
                        state.status = summary(&value);
                    }
                }
                Err(error) => {
                    files::dialog_error(world, &error);
                    eprintln!("Native operation {} failed: {error}", outcome.operation);
                    state.sketch_input_error =
                        crate::native_editor::creation_error_context(world, &error)
                            .map(|(owner, sketch)| (owner, sketch, error.clone()));
                    state.status = error;
                }
            }
        }
        if state.close_after_worker && !worker::busy(world) {
            state.close_after_worker = false;
            request_close(world, state, bridge, engine)?;
        }
    }
    if worker::busy(world) {
        return maintain_busy_window(world, handle, state);
    }
    app_settings::refresh(world, false);
    if !state.initialized {
        let owner = bridge.native_document_context(&state.window_id, engine)?;
        if let Some(model) = state.initial_model.take() {
            bridge.apply_native_mutation(
                engine,
                &owner,
                "cad_load_project_model",
                &json!({"model_json":model}),
                || Ok(()),
            )?;
        }
        let owner = bridge.native_document_context(&state.window_id, engine)?;
        bridge.with_native_document_owner(engine, &owner, || {
            refresh_native_model(engine, world, true)
        })?;
        bridge.publish_native_document(engine, &owner, "solid")?;
        state.initial_revision = bridge
            .engine_revision_for_window(&state.window_id)?
            .unwrap_or(1);
        state.initial_owner = Some(owner);
        state.initialized = true;
        state
            .workspace
            .lock()
            .map_err(|_| "Document workspace lock poisoned")?
            .observe(bridge, engine, &state.window_id)?;
    }

    if let Some(StartupProject(path)) = world.remove_resource::<StartupProject>() {
        let owner = bridge.native_document_context(&state.window_id, engine)?;
        files::open_path(world, services, &owner, path)?;
    }

    if !state.close_pending {
        if let Err(error) = files::poll(world, services) {
            files::dialog_error(world, &error);
            state.status = error;
        }
        if worker::busy(world) {
            return maintain_busy_window(world, handle, state);
        }
    }

    let mut events = state.deferred_save.take().into_iter().collect::<Vec<_>>();
    events.extend(take_deferred_pointer_input(world, handle, services, state)?);
    events.extend(
        state
            .input
            .read(world.resource::<Messages<NativeHostInput>>())
            .cloned(),
    );
    for mut event in events {
        if event.ui_scale != handle.presented_ui_scale()
            && matches!(
                event.event,
                WindowEvent::CursorMoved(_)
                    | WindowEvent::MouseButtonInput(_)
                    | WindowEvent::MouseWheel(_)
            )
        {
            cancel_deferred_pointer_input(world, handle, state);
            cancel_pointer_input(world);
            continue;
        }
        crate::native_editor::mechanism::observe_busy(world, &event);
        if state.exit_after_receipt {
            continue;
        }
        if worker::busy(world) {
            process_busy_input(world, handle, state, &event)?;
            continue;
        }
        if matches!(event.event, WindowEvent::WindowCloseRequested(_)) {
            workbench::cam::geometry_pick::cancel(world, handle);
            if let Err(error) =
                close_from_window_event(world, state, bridge, engine, event.context.as_ref())
            {
                state.status = error;
            }
            continue;
        }
        if !state.close_pending {
            match files::shortcut(world, handle, services, &event) {
                Ok(Some(value)) => {
                    state.status = summary(&value);
                    continue;
                }
                Err(error) => {
                    state.status = error;
                    continue;
                }
                Ok(None) => {}
            }
        }
        match files::script_preview_input(world, handle, &event) {
            Ok(true) => continue,
            Err(error) => {
                state.status = error;
                continue;
            }
            Ok(false) => {}
        }
        match workbench::cam::geometry_pick::input(world, handle, services, &event) {
            Ok(true) => continue,
            Err(error) => {
                state.status = error;
                continue;
            }
            Ok(false) => {}
        }
        match workbench::cam::reorder_drag::input(world, handle, services, &event) {
            Ok(true) => continue,
            Err(error) => {
                state.status = error;
                continue;
            }
            Ok(false) => {}
        }
        match history::pointer(world, handle, services, &event) {
            Ok(true) => continue,
            Err(error) => {
                state.status = error;
                continue;
            }
            Ok(false) => {}
        }
        match navigate_canvas_input(world, handle, &event) {
            Ok(true) => continue,
            Err(error) => {
                state.status = error;
                continue;
            }
            Ok(false) => {}
        }
        if let Err(error) =
            crate::native_viewport::winit_host::prepare_native_input(world, handle, &mut event)
        {
            state.status = error;
            continue;
        }
        let mut accepted = true;
        for action in std::mem::take(&mut event.actions) {
            if worker::busy(world) {
                state.status = "Modeling is in progress; later input was not applied".into();
                accepted = false;
                break;
            }
            if let Err(error) = apply_queued_control(world, handle, services, state, &action) {
                files::dialog_error(world, &error);
                state.status = error;
                accepted = false;
                break;
            }
        }
        if worker::busy(world) {
            continue;
        }
        process_modal_keys(world, handle, bridge, engine, state)?;
        if !accepted {
            continue;
        }
        if !state.close_pending {
            match history::shortcut_action(world, handle, &event) {
                Ok(Some(action)) => {
                    if let Err(error) =
                        apply_queued_control(world, handle, services, state, &action)
                    {
                        state.status = error;
                    }
                    continue;
                }
                Err(error) => {
                    state.status = error;
                    continue;
                }
                Ok(None) => {}
            }
        }
        match app_settings::shortcut(world, handle, services, &event) {
            Ok(Some(value)) => {
                state.status = summary(&value);
                continue;
            }
            Err(error) => {
                state.status = error;
                continue;
            }
            Ok(None) => {}
        }
        if app_settings::input(world, handle, &event) {
            continue;
        }
        match workbench::drawing_author_input(world, handle, services, &event) {
            Ok(true) => continue,
            Err(error) => {
                state.status = error;
                continue;
            }
            Ok(false) => {}
        }
        if let WindowEvent::MouseWheel(wheel) = &event.event {
            if let Some(cursor) = event.cursor {
                let factor = if matches!(wheel.unit, bevy::input::mouse::MouseScrollUnit::Line) {
                    36.
                } else {
                    world
                        .get::<Window>(wheel.window)
                        .map_or(1., |w| 1. / w.scale_factor())
                        / handle.presented_ui_scale()
                };
                if assembly::joint::scroll(world, cursor.to_array(), wheel.y * factor)
                    || feature::panel::scroll_panel(world, cursor.to_array(), wheel.y * factor)
                    || crate::native_editor::panel::scroll_panel(
                        world,
                        cursor.to_array(),
                        wheel.y * factor,
                    )
                {
                    continue;
                }
            }
            if !state.close_pending && files::modal(world).is_none() {
                if let (Some(cursor), Some(frame)) = (event.cursor, handle.frame()) {
                    if event.context.as_ref() != Some(&frame.context) {
                        continue;
                    }
                    if let Some(canvas) = frame
                        .canvases
                        .iter()
                        .find(|canvas| canvas.name == "viewport")
                    {
                        if cursor.x >= 0.
                            && f64::from(cursor.x) < canvas.bounds.x
                            && f64::from(cursor.y) >= canvas.bounds.y
                        {
                            let factor = if matches!(
                                wheel.unit,
                                bevy::input::mouse::MouseScrollUnit::Line
                            ) {
                                36.
                            } else {
                                world
                                    .get::<Window>(wheel.window)
                                    .map_or(1., |w| 1. / w.scale_factor())
                                    / handle.presented_ui_scale()
                            };
                            if !assembly::scroll(world, -wheel.y * factor) {
                                state.sidebar_scroll =
                                    (state.sidebar_scroll - wheel.y * factor).max(0.);
                            }
                        }
                    }
                }
            }
        }
        if !event.consumed {
            let creation = crate::native_editor::creation_context(world);
            match crate::native_editor::process_one(world, handle, services, &event) {
                Err(error) => {
                    state.sketch_input_error =
                        crate::native_editor::creation_error_context(world, &error)
                            .map(|(owner, sketch)| (owner, sketch, error.clone()));
                    state.status = error;
                }
                Ok(value) => {
                    let progress = value["preview"] == true
                        || value["cancelled"] == true
                        || value["picks"].is_u64()
                        || value["committed"] == true;
                    if let Some((owner, sketch, error)) = state.sketch_input_error.as_ref() {
                        let same_creation = creation
                            .as_ref()
                            .is_some_and(|(current, name)| current == owner && name == sketch)
                            && event.context.as_ref() == Some(owner);
                        let changed_creation = creation
                            .as_ref()
                            .is_some_and(|(current, name)| current != owner || name != sketch);
                        if state.status != *error || changed_creation {
                            state.sketch_input_error = None;
                        } else if same_creation && progress {
                            state.status = if value["presentation_pending"] == true {
                                value["presentation_error"]
                                    .as_str()
                                    .unwrap_or("Model changed; interface presentation needs retry")
                                    .to_owned()
                            } else {
                                summary(&value)
                            };
                            state.sketch_input_error = None;
                        }
                    }
                }
            }
        }
    }

    while !worker::busy(world) {
        let Some(action) = handle.take_next_action()? else {
            break;
        };
        if let Err(error) = apply_queued_control(world, handle, services, state, &action) {
            files::dialog_error(world, &error);
            state.status = error;
        }
    }
    if worker::busy(world) {
        return maintain_busy_window(world, handle, state);
    }
    process_modal_keys(world, handle, bridge, engine, state)?;
    if state.pending.is_none() {
        if let Some(session) = bridge.session_id_for_window(&state.window_id)? {
            let owner = bridge.native_document_context(&state.window_id, engine)?;
            let gate = presentation::gate(world, &owner);
            let playback_control_pending = crate::session_bridge::pending_control_requests(
                &crate::session_bridge::session_root()
                    .join(&session)
                    .join("controls"),
            )
            .iter()
            .any(|(_, request)| {
                request["ui"]["action"] == "presentation"
                    && request["ui"]
                        .get("command")
                        .is_some_and(|command| command != "status")
            });
            if !crate::session_bridge::pending_inbox_seqs(&session).is_empty()
                && gate != presentation::Gate::Waiting
                && !playback_control_pending
            {
                let reject = if gate == presentation::Gate::Stopped {
                    Some("Playback stopped")
                } else {
                    (state.close_pending || files::awaiting(world)).then_some(
                        crate::native_viewport::localization::translate(
                            world,
                            "file.dialogWaiting",
                        ),
                    )
                };
                let presentation_editor_active = named_views::presentation_locked(world);
                let replacement_reject_reason = print_intent::ensure_clean(world).err();
                worker::enqueue_inbox(
                    world,
                    move |services, guard| {
                        guard.validate()?;
                        let applied = apply_or_reject_one_inbox_op_with_editor_guards(
                            &services.bridge,
                            &owner.window_id,
                            &services.engine,
                            reject,
                            Some((&owner.document_id, &session)),
                            presentation_editor_active,
                            replacement_reject_reason.as_deref(),
                        )?;
                        if applied.is_null() {
                            return Err("The queued operation's document was replaced".into());
                        }
                        if applied["dead_lettered"] == true {
                            return Err(applied["error"]
                                .as_str()
                                .unwrap_or("The queued operation was rejected")
                                .into());
                        }
                        let current = services
                            .bridge
                            .native_document_context(&owner.window_id, &services.engine)?;
                        let receipt = services
                            .bridge
                            .native_document_receipt(&services.engine, &current)?;
                        Ok(NativeMutationResult {
                            context: receipt.owner,
                            engine_revision: receipt.revision,
                            value: applied,
                        })
                    },
                    |world, services, result| {
                        let result = result?;
                        if result.value["applied"] == true {
                            presentation::applied(world, &result.context, &result.value);
                            if result.value["model_changed"] == false {
                                return Ok(result.value);
                            }
                            Ok(finish_mutation(
                                &services.engine,
                                &services.bridge,
                                world,
                                "inbox",
                                result,
                            ))
                        } else {
                            Ok(result.value)
                        }
                    },
                )?;
                return maintain_busy_window(world, handle, state);
            }
        }
    }
    if state.pending.is_none() {
        let owner = bridge.native_document_context(&state.window_id, engine)?;
        if let Some(session) = bridge.session_id_for_window(&state.window_id)? {
            let dir = crate::session_bridge::session_root()
                .join(&session)
                .join("controls");
            if let Some((_, request)) = crate::session_bridge::pending_control_requests(&dir)
                .into_iter()
                .next()
            {
                let id = request["id"]
                    .as_str()
                    .expect("validated control id")
                    .to_owned();
                worker::enqueue_control_poll(world, owner.clone(), id.clone())?;
                state.polled_control = Some(PolledControl {
                    owner,
                    session,
                    id,
                    interface_only: control_poll_is_read_only(&request),
                });
                return maintain_busy_window(world, handle, state);
            }
        }
    }
    if worker::busy(world) {
        return maintain_busy_window(world, handle, state);
    }
    if state.close_pending || state.exit_after_receipt {
        view::cancel(
            world,
            "Camera transition interrupted by the window close request",
        );
    }
    six_dof::tick(
        world,
        handle,
        state.close_pending || state.exit_after_receipt,
    )?;
    if view::pending(world) {
        let owner = bridge.native_document_context(&state.window_id, engine)?;
        bridge.with_native_document_receipt(engine, &owner, |revision| {
            view::advance(
                world,
                &workspace::DocumentReceipt {
                    owner: owner.clone(),
                    revision,
                },
            );
            Ok(())
        })?;
        if view::pending(world) {
            handle.request_redraw();
        }
    }
    if crate::native_editor::mechanism::active(world) {
        let owner = bridge.native_document_context(&state.window_id, engine)?;
        if state.close_pending || files::awaiting(world) {
            crate::native_editor::mechanism::cancel(world);
        }
        crate::native_editor::mechanism::tick(world, handle, services, &owner)?;
        if worker::busy(world) {
            return maintain_busy_window(world, handle, state);
        }
    }
    if (assembly::motion::active(world) || assembly::studies::active(world))
        && state.pending.is_none()
        && !state.close_pending
        && !state.exit_after_receipt
        && !files::awaiting(world)
    {
        let owner = bridge.native_document_context(&state.window_id, engine)?;
        assembly::studies::tick(world, handle, services, &owner)?;
        if worker::busy(world) {
            return maintain_busy_window(world, handle, state);
        }
        assembly::motion::tick(world, handle, services, &owner)?;
        if worker::busy(world) {
            return maintain_busy_window(world, handle, state);
        }
    }
    history::tick(world, handle, services)?;
    workbench::cam::reorder_drag::tick(world, handle, services)?;
    workbench::cam::geometry_pick::tick(world, handle, services)?;
    if retention::tick(world, services, state)? {
        return maintain_busy_window(world, handle, state);
    }
    synchronize(world, handle, services, state)
}

fn control_poll_is_read_only(request: &Value) -> bool {
    request.get("sketch_query").is_none()
        || request["sketch_query"]["method"]
            .as_str()
            .is_some_and(|method| {
                limo_cad_mcp_mutate::is_live_engine_query(method)
                    || (request.get("owner").is_some()
                        && limo_cad_mcp_mutate::is_routed_engine_query(method))
            })
}

fn start_control(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    state: &mut Controller,
    owner: &DocumentContext,
    request: &Value,
) -> Result<(), String> {
    let observation = matches!(
        request["ui"]["action"].as_str(),
        Some("inspect" | "capture")
    );
    let discard_deferred = !observation && state.deferred_pointer.take().is_some();
    let retire_picker = discard_deferred && workbench::cam::geometry_pick::active(world);
    if discard_deferred {
        history::cancel_drag(world);
        workbench::cam::reorder_drag::cancel(world, handle);
        workbench::cancel_drawing_author_input(world);
        crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
    }
    let mut response = json!({"request_id":request["id"],"session_id":request["session_id"]});
    let outcome = apply_control(world, handle, services, state, owner, request);
    if retire_picker {
        workbench::cam::geometry_pick::cancel(world, handle);
    }
    let current = if worker::busy(world) {
        owner.clone()
    } else {
        services
            .bridge
            .native_document_context(&state.window_id, &services.engine)?
    };
    match outcome {
        Ok(value) => {
            apply_host_result(state, &value);
            if !observation {
                state.status = summary(&value);
            }
            if value["request_exit"] == true {
                request_close(world, state, &services.bridge, &services.engine)?;
            }
            response["status"] = json!("applied");
            if let Some(presentation) = value.get("presentation") {
                response["presentation"] = presentation.clone();
            }
            if let Some(recipe) = value.get("recipe") {
                response["recipe"] = recipe.clone();
            }
            response["value"] = value;
        }
        Err(error) => {
            files::dialog_error(world, &error);
            state.status.clone_from(&error);
            response["status"] = json!("failed");
            response["error"] = json!(error);
        }
    }
    response["awaiting_input"] = json!(state.close_pending || files::awaiting(world));
    let now = now_ms();
    state.pending = Some(PendingControl {
        response,
        owner: current,
        inspect: request["ui"]["action"] == "inspect",
        presentation_deadline: now
            .saturating_add(2_000)
            .min(
                request["expires_ms"]
                    .as_u64()
                    .unwrap_or(now.saturating_add(2_000))
                    .saturating_sub(100),
            )
            .max(now.saturating_add(1)),
    });
    Ok(())
}

fn deferred_pointer_active(world: &World) -> bool {
    workbench::cam::geometry_pick::active(world)
        || workbench::cam::reorder_drag::active(world)
        || history::pointer_active(world)
        || workbench::drawing_author_pointer_active(world)
}

fn cancel_deferred_pointer_input(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    state: &mut Controller,
) {
    state.deferred_pointer = None;
    history::cancel_drag(world);
    workbench::cam::reorder_drag::cancel(world, handle);
    workbench::cam::geometry_pick::cancel(world, handle);
    workbench::cancel_drawing_author_input(world);
    crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
}

fn defer_pointer_input(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    state: &mut Controller,
    event: &NativeHostInput,
) {
    if !matches!(
        event.event,
        WindowEvent::CursorMoved(_) | WindowEvent::MouseButtonInput(_)
    ) || event.consumed
        || !event.actions.is_empty()
    {
        return;
    }
    let Some((owner, revision)) = state.synchronized.as_ref() else {
        cancel_deferred_pointer_input(world, handle, state);
        return;
    };
    if event.context.as_ref() != Some(owner)
        || handle
            .frame()
            .is_none_or(|frame| frame.context != *owner || !frame.modal_stack.is_empty())
    {
        cancel_deferred_pointer_input(world, handle, state);
        return;
    }
    let receipt = workspace::DocumentReceipt {
        owner: owner.clone(),
        revision: *revision,
    };
    if state
        .deferred_pointer
        .as_ref()
        .is_some_and(|deferred| deferred.receipt != receipt)
    {
        cancel_deferred_pointer_input(world, handle, state);
        return;
    }
    let deferred = state
        .deferred_pointer
        .get_or_insert_with(|| DeferredPointer {
            receipt,
            camera: workbench::cam::geometry_pick::active(world)
                .then(|| native_viewport::interface_camera_snapshot(world)),
            canvas: handle.frame().and_then(|frame| {
                frame
                    .canvases
                    .iter()
                    .find(|canvas| canvas.name == "viewport")
                    .map(|canvas| canvas.bounds)
            }),
            events: VecDeque::new(),
        });
    if matches!(event.event, WindowEvent::CursorMoved(_))
        && deferred.events.back().is_some_and(|last| {
            matches!(last.event, WindowEvent::CursorMoved(_))
                && last.context == event.context
                && last.modifiers == event.modifiers
        })
    {
        deferred.events.pop_back();
    }
    if deferred.events.len() == 64 {
        cancel_deferred_pointer_input(world, handle, state);
        return;
    }
    deferred.events.push_back(event.clone());
}

fn take_deferred_pointer_input(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    state: &mut Controller,
) -> Result<Vec<NativeHostInput>, String> {
    if worker::busy(world) || state.polled_control.is_some() {
        return Ok(Vec::new());
    }
    let Some(deferred) = state.deferred_pointer.take() else {
        return Ok(Vec::new());
    };
    let current = deferred_pointer_active(world)
        && !state.close_pending
        && !state.close_after_worker
        && !state.exit_after_receipt
        && handle.frame().is_some_and(|frame| {
            frame.context == deferred.receipt.owner
                && frame.modal_stack.is_empty()
                && frame
                    .canvases
                    .iter()
                    .find(|canvas| canvas.name == "viewport")
                    .map(|canvas| canvas.bounds)
                    == deferred.canvas
        })
        && deferred
            .camera
            .as_ref()
            .is_none_or(|camera| *camera == native_viewport::interface_camera_snapshot(world))
        && services
            .bridge
            .native_document_receipt(&services.engine, &deferred.receipt.owner)
            .is_ok_and(|receipt| receipt == deferred.receipt);
    if !current {
        cancel_deferred_pointer_input(world, handle, state);
        return Ok(Vec::new());
    }
    Ok(deferred.events.into_iter().collect())
}

fn process_busy_input(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    state: &mut Controller,
    event: &NativeHostInput,
) -> Result<(), String> {
    handle.record_file_shortcut(
        event,
        if state
            .polled_control
            .as_ref()
            .is_some_and(|poll| poll.interface_only)
        {
            "busy_read"
        } else {
            "busy_mutation"
        },
    );
    crate::native_editor::mechanism::observe_busy(world, event);
    if files::script_preview_input(world, handle, event)? {
        return Ok(());
    }
    if state
        .polled_control
        .as_ref()
        .is_some_and(|poll| poll.interface_only)
    {
        if !event.consumed
            && files::is_save_shortcut(event)
            && handle.frame().is_some_and(|frame| {
                event.context.as_ref() == Some(&frame.context) && frame.modal_stack.is_empty()
            })
        {
            handle.record_file_shortcut(event, "deferred_save");
            state.deferred_save = Some(event.clone());
            return Ok(());
        }
        let lifecycle = matches!(&event.event, WindowEvent::WindowFocused(focus) if !focus.focused)
            || matches!(
                event.event,
                WindowEvent::KeyboardFocusLost(_)
                    | WindowEvent::WindowResized(_)
                    | WindowEvent::WindowScaleFactorChanged(_)
                    | WindowEvent::WindowBackendScaleFactorChanged(_)
                    | WindowEvent::WindowCloseRequested(_)
                    | WindowEvent::WindowDestroyed(_)
            );
        let escape = !event.consumed
            && handle
                .frame()
                .is_some_and(|frame| event.context.as_ref() == Some(&frame.context))
            && matches!(&event.event, WindowEvent::KeyboardInput(key)
                if key.state == bevy::input::ButtonState::Pressed
                    && key.logical_key == bevy::input::keyboard::Key::Escape);
        if lifecycle || escape {
            cancel_deferred_pointer_input(world, handle, state);
        }
        if matches!(event.event, WindowEvent::WindowCloseRequested(_)) {
            state.close_after_worker = true;
        }
        if lifecycle || escape {
            workbench::drawing_navigate(world, handle, event)?;
            view::navigate(world, handle, event)?;
        } else if deferred_pointer_active(world) {
            defer_pointer_input(world, handle, state, event);
        } else {
            if six_dof::busy_input(world, handle, event)? {
                return Ok(());
            }
            if presentation::busy_input(world, handle, event)? {
                return Ok(());
            }
            if workbench::drawing_navigate(world, handle, event)? {
                return Ok(());
            }
            view::navigate(world, handle, event)?;
        }
        return Ok(());
    }
    state.deferred_pointer = None;
    history::cancel_drag(world);
    workbench::cam::reorder_drag::cancel(world, handle);
    workbench::cam::geometry_pick::cancel(world, handle);
    workbench::cancel_drawing_author_input(world);
    if matches!(event.event, WindowEvent::WindowCloseRequested(_)) {
        state.close_after_worker = true;
    }
    if six_dof::busy_input(world, handle, event)? {
        return Ok(());
    }
    if presentation::busy_input(world, handle, event)? {
        return Ok(());
    }
    if workbench::drawing_navigate(world, handle, event)? {
        return Ok(());
    }
    view::navigate(world, handle, event)?;
    Ok(())
}

fn maintain_busy_window(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    state: &mut Controller,
) -> Result<(), String> {
    let interface_only = state
        .polled_control
        .as_ref()
        .is_some_and(|poll| poll.interface_only);
    if !interface_only {
        cancel_deferred_pointer_input(world, handle, state);
    }
    if !interface_only && worker::started(world) && state.busy_controls.is_empty() {
        crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
    }
    if let Some(session) = state.cached_session.as_deref().filter(|_| !interface_only) {
        let except = state
            .pending
            .as_ref()
            .and_then(|pending| pending.response["request_id"].as_str())
            .or_else(|| {
                state
                    .polled_control
                    .as_ref()
                    .map(|control| control.id.as_str())
            });
        crate::session_bridge::reject_busy_controls(session, except)?;
    }
    let events = state
        .input
        .read(world.resource::<Messages<NativeHostInput>>())
        .cloned()
        .collect::<Vec<_>>();
    for event in events {
        process_busy_input(world, handle, state, &event)?;
    }
    six_dof::tick(
        world,
        handle,
        state.close_after_worker || state.close_pending || state.exit_after_receipt,
    )?;
    if !interface_only {
        let _ = handle.take_actions()?;
        let _ = handle.take_modal_keys()?;
    }
    let mut status_changed = false;
    // Retention leaves the active model unchanged and preserves status on success.
    // Do not replace that status with a modeling caption it will never retire.
    if !interface_only
        && (state.close_after_worker
            || worker::pending_operation(world) != Some("memory-retention"))
    {
        let message = if state.close_after_worker {
            "Finishing the current modeling operation before closing…"
        } else {
            presentation::busy_status(world)
                .unwrap_or("Building the model… You can still pan, orbit and zoom.")
        };
        status_changed = state.status != message;
        if status_changed {
            state.status.clear();
            state.status.push_str(message);
        }
        if let Some(entity) = state.decoration.get("status-clip") {
            let background = BackgroundColor(crate::native_viewport::ui::theme(world).panel);
            if world.get::<BackgroundColor>(*entity) != Some(&background) {
                world.entity_mut(*entity).insert(background);
            }
        }
        if let Some(entity) = state.decoration.get("status") {
            if let Some(mut text) = world.get_mut::<Text>(*entity) {
                if text.0 != message {
                    text.0 = message.into();
                    status_changed = true;
                    handle.invalidate_presentation();
                }
            }
        }
    }
    if !interface_only && worker::started(world) && state.busy_controls.is_empty() {
        let mut query = world.query::<(Entity, &mut InterfaceControl)>();
        let playback_controls = world
            .query::<(Entity, &NativeCommandBinding)>()
            .iter(world)
            .filter_map(|(entity, binding)| {
                matches!(
                    binding.command,
                    NativeCommand::Presentation(
                        presentation::Command::Pause | presentation::Command::Stop
                    ) | NativeCommand::SixDof(_)
                )
                .then_some(entity)
            })
            .collect::<Vec<_>>();
        for (entity, mut control) in query.iter_mut(world) {
            if playback_controls.contains(&entity) {
                continue;
            }
            state.busy_controls.push((entity, control.disabled));
            control.disabled = true;
        }
        if let Some(mut frame) = handle.frame() {
            if let Some(surface) = frame
                .surfaces
                .iter_mut()
                .find(|surface| surface.name == "document/session")
            {
                surface.text = Some(state.status.clone());
            }
            handle.present(frame)?;
        }
    }
    if status_changed {
        if let Some(mut frame) = handle.frame() {
            if let Some(surface) = frame
                .surfaces
                .iter_mut()
                .find(|surface| surface.name == "document/status")
            {
                if surface.text.as_deref() != Some(state.status.as_str()) {
                    surface.text = Some(state.status.clone());
                    handle.present(frame)?;
                }
            }
        }
    }
    Ok(())
}

fn process_modal_keys(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    bridge: &SessionBridgeState,
    engine: &AppState,
    state: &mut Controller,
) -> Result<(), String> {
    for request in handle.take_modal_keys()? {
        bridge.with_native_document_owner(engine, &request.context, || {
            handle.validate_modal_key(&request)
        })?;
        if request.key.key == "Escape"
            && !request.key.ctrl
            && !request.key.meta
            && !request.key.alt
            && !request.key.shift
        {
            match request.modal_scope.as_str() {
                "close-document" => state.close_pending = false,
                "file-menu" | "file-dialog" | "app-settings" => files::escape(world),
                "history-menu" | "delete-feature" => history::escape(world),
                "rename-feature" => {
                    if let Some(key) = handle.focused_key() {
                        if world
                            .get::<NativeCommandBinding>(Entity::from_bits(key.0))
                            .is_some_and(|binding| {
                                matches!(
                                    binding.command,
                                    NativeCommand::History(history::HistoryCommand::RenameValue(_))
                                )
                            })
                        {
                            let action = handle.resolve_input(
                                key,
                                ControlInput::Key(request.key.clone()),
                                &request.context,
                            )?;
                            reduce_control_input(engine, bridge, world, handle, &action)?;
                        }
                    }
                }
                "sketch-menu" => crate::native_editor::panel::escape(world),
                "sketch-origin" => {
                    if crate::native_editor::support::modal(world) == Some("sketch-origin") {
                        crate::native_editor::execute(
                            world,
                            engine,
                            bridge,
                            &request.context,
                            crate::native_editor::EditorCommand::Cancel,
                            || handle.validate_modal_key(&request),
                        )?;
                    }
                }
                "section-review" => {
                    section_review::escape(world);
                }
                "workbench-menu"
                | "cam-export"
                | "cam-report"
                | "cam-simulation-settings"
                | "cam-nc-source"
                | "cam-library" => workbench::escape(world),
                _ => {}
            }
        }
    }
    Ok(())
}

/// Human, accessibility and MCP actions all flush the same editor transaction
/// before activation. Only an accepted SetValue becomes the field baseline.
pub(crate) fn reduce_control_input(
    engine: &AppState,
    bridge: &SessionBridgeState,
    world: &mut World,
    handle: &NativeInterfaceHandle,
    action: &NativeInterfaceAction,
) -> Result<Value, String> {
    use crate::native_viewport::interface_shell::fields;
    if let ControlInput::Key(key) = &action.control.input {
        bridge.with_native_document_owner(engine, &action.context, || {
            handle.validate_action(action)
        })?;
        if handle.navigate_menu(key)? {
            return Ok(json!({"handled":true,"menu_navigation":true}));
        }
        if key == &limo_cad_interface::KeyChord::plain("Escape") {
            if let Some(scope) = handle
                .frame()
                .and_then(|frame| frame.modal_stack.last().cloned())
            {
                match scope.as_str() {
                    "file-menu" | "file-dialog" | "app-settings" => files::escape(world),
                    "history-menu" | "delete-feature" => history::escape(world),
                    "rename-feature" => {
                        return reduce_action(engine, bridge, world, handle, action);
                    }
                    "sketch-menu" => crate::native_editor::panel::escape(world),
                    "section-review" => {
                        section_review::escape(world);
                    }
                    "workbench-menu"
                    | "cam-export"
                    | "cam-report"
                    | "cam-simulation-settings"
                    | "cam-nc-source"
                    | "cam-library" => workbench::escape(world),
                    "sketch-origin" => {
                        return crate::native_editor::execute(
                            world,
                            engine,
                            bridge,
                            &action.context,
                            crate::native_editor::EditorCommand::Cancel,
                            || handle.validate_action(action),
                        );
                    }
                    "close-document" => return Ok(json!({"close_decision":"cancel"})),
                    _ => return Err("This dialog does not handle Escape".into()),
                }
                return Ok(json!({"cancelled":true}));
            }
            if section_review::in_3d(world) {
                section_review::escape(world);
                return Ok(json!({"cancelled":true}));
            }
            if let Some(panel) = feature::panel(world) {
                return feature::reduce(
                    engine,
                    bridge,
                    world,
                    &action.context,
                    &feature::FeatureCommand::Control {
                        form_id: panel.form_id,
                        action: panel.choice_field.map_or(
                            feature::FeatureControl::Cancel,
                            feature::FeatureControl::Field,
                        ),
                    },
                    &ControlInput::Click,
                    || handle.validate_action(action),
                );
            }
            if assembly::joint::active(world) {
                handle.validate_action(action)?;
                return assembly::joint::cancel(world, engine, bridge, &action.context);
            }
            if print_intent::active(world) {
                handle.validate_action(action)?;
                return print_intent::cancel(world, engine, bridge, &action.context);
            }
            if named_views::active(world) {
                handle.validate_action(action)?;
                return named_views::cancel(world, engine, bridge, &action.context);
            }
            if matches!(
                world
                    .get::<NativeCommandBinding>(Entity::from_bits(action.control.key.0))
                    .map(|b| &b.command),
                Some(NativeCommand::Sketch(_) | NativeCommand::BodyAppearance(..))
            ) {
                return crate::native_editor::execute(
                    world,
                    engine,
                    bridge,
                    &action.context,
                    crate::native_editor::EditorCommand::Cancel,
                    || handle.validate_action(action),
                );
            }
        }
    }
    for preceding in fields::prepare_control_input(world, handle, action)? {
        let result = reduce_action(engine, bridge, world, handle, &preceding);
        fields::acknowledge_control_input(world, &preceding, result.is_ok());
        result?;
    }
    fields::prepare_activation(world, handle, action)?;
    fields::after_window_input(world, handle)?;
    let Some(adapted) = fields::adapt_control_input(world, handle, action)? else {
        return Ok(json!({"handled":true,"field_navigation":true}));
    };
    world.insert_resource(worker::ActiveControl(adapted.clone()));
    let dismiss_menu = world
        .get::<InterfaceControl>(Entity::from_bits(adapted.control.key.0))
        .is_some_and(|c| {
            c.modal_scope.as_deref() == Some("workbench-menu") && c.role == "menuitem"
        });
    let result = reduce_action(engine, bridge, world, handle, &adapted);
    if result.is_ok() && dismiss_menu {
        workbench::escape(world);
    }
    world.remove_resource::<worker::ActiveControl>();
    fields::acknowledge_control_input(world, &adapted, result.is_ok());
    let focus = fields::after_window_input(world, handle);
    let mut value = result?;
    if let Err(error) = focus {
        value["focus_error"] = json!(error);
    }
    Ok(value)
}

fn apply_queued_control(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    state: &mut Controller,
    action: &NativeInterfaceAction,
) -> Result<(), String> {
    if state.exit_after_receipt {
        return Err("The window is closing".into());
    }
    let value = reduce_control_input(&services.engine, &services.bridge, world, handle, action)?;
    apply_host_result(state, &value);
    if value["request_exit"] == true {
        request_close(world, state, &services.bridge, &services.engine)?;
    }
    state.status = summary(&value);
    Ok(())
}

fn summary(value: &Value) -> String {
    if let Some(error) = value["model_error"].as_str() {
        error.into()
    } else if value["publication_pending"] == true {
        "Model changed; snapshot publication needs retry".into()
    } else if let Some(error) = value["render_error"].as_str() {
        error.into()
    } else if let Some(message) = value["status_message"].as_str() {
        message.into()
    } else {
        String::new()
    }
}

fn apply_host_result(state: &mut Controller, value: &Value) {
    if value["saving_before_exit"] == true {
        state.close_pending = false;
    }
    match value["close_decision"].as_str() {
        Some("cancel") => state.close_pending = false,
        Some("discard") if state.close_pending => {
            state.close_pending = false;
            state.exit_after_receipt = true;
        }
        _ => {}
    }
}

fn request_close(
    world: &mut World,
    state: &mut Controller,
    bridge: &SessionBridgeState,
    engine: &AppState,
) -> Result<(), String> {
    files::guard_script_exit(world)?;
    named_views::ensure_exportable(world)?;
    print_intent::ensure_clean(world)?;
    let owner = bridge.native_document_context(&state.window_id, engine)?;
    workbench::drawing_editor::guard_document_switch(world, &owner)?;
    let tabs = state
        .workspace
        .lock()
        .map_err(|_| "Document workspace lock poisoned")?
        .summaries(bridge, &owner)?;
    let dirty = if tabs.is_empty() {
        state.initial_owner.as_ref() != Some(&owner)
            || bridge
                .engine_revision_for_window(&state.window_id)?
                .is_some_and(|revision| revision != state.initial_revision)
    } else {
        tabs.iter().any(|tab| tab.dirty)
    };
    if dirty {
        state.close_pending = true;
    } else {
        state.exit_after_receipt = true;
    }
    Ok(())
}

fn close_from_window_event(
    world: &mut World,
    state: &mut Controller,
    bridge: &SessionBridgeState,
    engine: &AppState,
    stamped_owner: Option<&DocumentContext>,
) -> Result<(), String> {
    if let Some(owner) = stamped_owner {
        bridge.with_native_document_owner(engine, owner, || Ok(()))?;
    }
    request_close(world, state, bridge, engine)
}

#[cfg(all(windows, feature = "native-computer-control"))]
fn inspect_native_window(world: &World, window_id: &str) -> Result<Value, String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let mut windows = world
        .try_query_filtered::<Entity, With<PrimaryWindow>>()
        .ok_or("The CAD primary window is unavailable")?;
    let entity = windows
        .single(world)
        .map_err(|_| "The CAD primary window is not uniquely available")?;
    bevy::winit::WINIT_WINDOWS.with_borrow(|windows| {
        let window = windows
            .get_window(entity)
            .ok_or("The CAD native window is unavailable")?;
        let handle = window
            .window_handle()
            .map_err(|error| format!("Cannot inspect the CAD window handle: {error}"))?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return Err("The CAD native window does not have a Win32 handle".into());
        };
        Ok(json!({"hwnd":handle.hwnd.get() as usize,
            "pid":std::process::id(),"window_id":window_id}))
    })
}

fn inspect_document(
    world: &World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    state: &Controller,
    owner: &DocumentContext,
) -> Result<(Value, Value), String> {
    services
        .bridge
        .with_native_document_owner(&services.engine, owner, || {
            let available = |command: &str| {
                state
                    .controls
                    .get(command)
                    .and_then(|entity| {
                        handle
                            .resolve_retained(limo_cad_interface::ControlKey(entity.to_bits()))
                            .ok()
                    })
                    .is_some_and(|action| &action.context == owner)
            };
            Ok((
                named_views::inspect(
                    world,
                    &services.engine,
                    state.bodies.iter().map(|(id, _)| *id),
                )?,
                json!({"can_undo":available("undo"),"can_redo":available("redo")}),
            ))
        })
}

fn apply_control(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    state: &mut Controller,
    owner: &DocumentContext,
    request: &Value,
) -> Result<Value, String> {
    if state.exit_after_receipt {
        return Err("The window is closing".into());
    }
    if request["expires_ms"].as_u64().unwrap_or(0) < now_ms() {
        return Err("Native control request expired".into());
    }
    let Some(ui) = request.get("ui") else {
        let mut request = request.clone();
        if let Some(duration) = request["duration_ms"].as_u64() {
            request["duration_ms"] = json!(presentation::motion_duration(world, owner, duration));
        }
        return services
            .bridge
            .with_native_document_receipt(&services.engine, owner, |revision| {
                view::request(world, owner, revision, &request)
            });
    };
    if let Some(pace) = ui.get("pace_ms") {
        presentation::pace(world, services, owner, pace)?;
    }
    match ui["action"].as_str().unwrap_or("") {
        "inspect" => Ok(Value::Null),
        "capture" => capture::begin(world, handle, services, owner, ui),
        "presentation" => presentation::request(world, handle, services, owner, ui),
        "open_recipe" => {
            if state.close_pending || state.close_after_worker {
                return Err("Finish the window close request before opening a recipe".into());
            }
            files::initialize(world, state.workspace.clone());
            files::queue_recipe(
                world,
                ui["recipe"]
                    .as_str()
                    .ok_or("Choose an installed recipe ID")?,
            )
        }
        "viewport" => crate::native_editor::mcp::drive(world, handle, services, owner, ui),
        "history" => {
            if state.close_pending {
                return Err("Finish the close confirmation first".into());
            }
            let command = ui["command"].as_str().ok_or("Choose undo or redo")?;
            if !matches!(command, "undo" | "redo") {
                return Err("history requires command undo or redo".into());
            }
            let entity = state
                .controls
                .get(command)
                .ok_or("History control is unavailable")?;
            let action =
                handle.resolve_retained(limo_cad_interface::ControlKey(entity.to_bits()))?;
            if &action.context != owner {
                return Err("The history control belongs to another document".into());
            }
            reduce_control_input(&services.engine, &services.bridge, world, handle, &action)
        }
        "file" if ui["command"] == "exit" => {
            request_close(world, state, &services.bridge, &services.engine)?;
            Ok(json!({"awaiting_input":state.close_pending}))
        }
        "file" => {
            if state.close_pending {
                return Err("Finish the close confirmation first".into());
            }
            files::initialize(world, state.workspace.clone());
            files::request(world, handle, services, owner, ui)
        }
        "window" => {
            if ui["mode"] == "close" {
                request_close(world, state, &services.bridge, &services.engine)?;
                return Ok(json!({"awaiting_input":state.close_pending}));
            }
            let mut query = world.query_filtered::<&mut Window, With<PrimaryWindow>>();
            let mut window = query
                .single_mut(world)
                .map_err(|_| "Native window is unavailable")?;
            match ui["mode"].as_str().unwrap_or("inspect") {
                "inspect" => {}
                "foreground" => {
                    window.visible = true;
                    window.set_minimized(false);
                    window.focused = true;
                }
                "background" => {
                    window.set_minimized(true);
                }
                "hide" => window.visible = false,
                _ => return Err("Unknown native window action".into()),
            }
            let visible = window.visible;
            let focused = world
                .get_resource::<crate::native_viewport::winit_host::NativeRenderAvailability>()
                .is_some_and(|state| state.focused);
            Ok(json!({"visible":visible,"focused":focused,"window_transition":ui["mode"]}))
        }
        _ => {
            let request: ControlRequest = serde_json::from_value(ui.clone())
                .map_err(|error| format!("Native interface action: {error}"))?;
            let action = handle.resolve(&request, owner)?;
            reduce_control_input(&services.engine, &services.bridge, world, handle, &action)
        }
    }
}

/// Give camera navigation the same priority for host and MCP canvas input.
pub(crate) fn navigate_canvas_input(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    event: &NativeHostInput,
) -> Result<bool, String> {
    if workbench::drawing_navigate(world, handle, event)? {
        return Ok(true);
    }
    view::navigate(world, handle, event)
}

pub(crate) fn cancel_canvas_navigation(world: &mut World) {
    view::cancel_pointer(world);
    workbench::cancel_navigation(world);
}

/// Retire every pointer owner together when input coordinates are replaced.
/// Drafts, focused text buffers, and IME composition remain owned separately.
fn cancel_pointer_input(world: &mut World) {
    cancel_canvas_navigation(world);
    crate::native_editor::cancel_pointer(world);
    files::cancel_preview_pointer(world);
    history::cancel_drag(world);
    workbench::cancel_drawing_author_input(world);
    if let Some(handle) = world.get_resource::<NativeInterfaceHandle>().cloned() {
        workbench::cam::geometry_pick::cancel(world, &handle);
        workbench::cam::reorder_drag::cancel(world, &handle);
        crate::native_viewport::winit_host::cancel_native_pointer(world, &handle);
        handle.invalidate_presentation();
    }
}

fn synchronize(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    state: &mut Controller,
) -> Result<(), String> {
    files::initialize(world, state.workspace.clone());
    state
        .workspace
        .lock()
        .map_err(|_| "Document workspace lock poisoned")?
        .observe(&services.bridge, &services.engine, &state.window_id)?;
    let owner = services
        .bridge
        .native_document_context(&state.window_id, &services.engine)?;
    presentation::observe(world, &owner);
    let revision = services
        .bridge
        .engine_revision_for_window(&state.window_id)?
        .unwrap_or(1);
    state.cached_session = services.bridge.session_id_for_window(&state.window_id)?;
    if let Ok(mut cached) = state.watch_session.lock() {
        cached.clone_from(&state.cached_session);
    }
    if state.synchronized.as_ref() != Some(&(owner.clone(), revision)) {
        if let Some(cached) = world
            .get_resource::<NativeRenderedDocument>()
            .filter(|cached| cached.owner == owner && cached.revision == revision)
        {
            state.bodies = cached.bodies.clone();
        } else {
            let reset = state
                .synchronized
                .as_ref()
                .is_none_or(|(prior, _)| prior != &owner);
            state.bodies =
                services
                    .bridge
                    .with_native_document_owner(&services.engine, &owner, || {
                        refresh_native_model(&services.engine, world, reset)
                    })?;
            world.insert_resource(NativeRenderedDocument {
                owner: owner.clone(),
                revision,
                bodies: state.bodies.clone(),
            });
        }
        state.synchronized = Some((owner.clone(), revision));
    }
    app_settings::apply_scale(world);
    let mut windows = world.query_filtered::<&Window, With<PrimaryWindow>>();
    let window = windows
        .single(world)
        .map_err(|_| "Native window is unavailable")?;
    if window.width() > 0. && window.height() > 0. {
        state.logical_size = Vec2::new(window.width(), window.height());
    }
    let ui_scale = world
        .get_resource::<bevy::ui::UiScale>()
        .map_or(1., |scale| scale.0);
    let width = state.logical_size.x / ui_scale;
    let height = state.logical_size.y / ui_scale;
    let scale = window.resolution.scale_factor() * ui_scale;
    let visible = window.visible;
    let side = (if assembly::active(world) {
        286_f32
    } else {
        240_f32
    })
    .min(width * 0.45);
    let top = 120_f32.min(height * 0.3);
    let bottom = 48_f32.min(height * 0.1);
    let canvas = Rect::from_corners(Vec2::new(side, top), Vec2::new(width, height - bottom));
    native_viewport::apply_interface_viewport(
        world,
        InterfaceRect {
            x: canvas.min.x as f64,
            y: canvas.min.y as f64,
            width: canvas.width() as f64,
            height: canvas.height() as f64,
        },
        scale,
    )?;
    let mut cameras = world.query_filtered::<Entity, With<InterfaceCamera>>();
    let Ok(camera) = cameras.single(world) else {
        return Ok(());
    };
    let history = services
        .bridge
        .native_history_available(&services.engine, &owner)?;
    assembly::joint::synchronize(
        world,
        services,
        &owner,
        revision,
        InterfaceRect {
            x: (width - 400.).max(side) as f64,
            y: (top + 12.) as f64,
            width: 380_f32.min(width - side).max(1.) as f64,
            height: (height - top - bottom - 24.).max(1.) as f64,
        },
    )?;
    feature::synchronize(&services.engine, &services.bridge, world, &owner)?;
    feature::panel::synchronize_panel(
        world,
        handle,
        &owner,
        InterfaceRect {
            x: (width - 340.).max(side) as f64,
            y: (top + 12.) as f64,
            width: 320_f32.min(width - side).max(1.) as f64,
            height: (height - top - bottom - 24.).max(1.) as f64,
        },
    )?;
    crate::native_editor::synchronize_controls(
        world,
        handle,
        services,
        &owner,
        InterfaceRect {
            x: 116.,
            y: 34.,
            width: (width - 128.).max(1.) as f64,
            height: 72.,
        },
        InterfaceRect {
            x: canvas.min.x as f64,
            y: canvas.min.y as f64,
            width: canvas.width() as f64,
            height: canvas.height() as f64,
        },
    )?;
    let sketch_mode =
        native_viewport::interface_view(world).2.mode == native_viewport::ViewportMode::Sketch;
    let mut rows = vec![
        (
            "undo".to_owned(),
            "Undo".to_owned(),
            NativeCommand::Undo,
            !history.0,
            0.,
            0.,
            78.,
        ),
        (
            "redo".to_owned(),
            "Redo".to_owned(),
            NativeCommand::Redo,
            !history.1,
            80.,
            0.,
            78.,
        ),
        (
            "fit".to_owned(),
            "Fit".to_owned(),
            NativeCommand::Fit,
            false,
            160.,
            0.,
            78.,
        ),
        (
            "isometric".to_owned(),
            "Isometric".to_owned(),
            NativeCommand::Orient(ViewDirection::Isometric),
            false,
            240.,
            0.,
            100.,
        ),
        (
            "front".to_owned(),
            "Front".to_owned(),
            NativeCommand::Orient(ViewDirection::Front),
            false,
            342.,
            0.,
            78.,
        ),
        (
            "top".to_owned(),
            "Top".to_owned(),
            NativeCommand::Orient(ViewDirection::Top),
            false,
            422.,
            0.,
            78.,
        ),
        (
            "clear".to_owned(),
            "Clear selection".to_owned(),
            NativeCommand::ClearSelection,
            false,
            502.,
            0.,
            108.,
        ),
        (
            "extrude".to_owned(),
            "Extrude".to_owned(),
            NativeCommand::Feature(feature::FeatureCommand::Open {
                kind: feature::SolidFormKind::Extrude,
                feature_id: None,
            }),
            sketch_mode || feature::panel(world).is_some() || assembly::joint::active(world),
            270.,
            34.,
            48.,
        ),
        (
            "revolve".to_owned(),
            "Revolve".to_owned(),
            NativeCommand::Feature(feature::FeatureCommand::Open {
                kind: feature::SolidFormKind::Revolve,
                feature_id: None,
            }),
            sketch_mode || feature::panel(world).is_some() || assembly::joint::active(world),
            320.,
            34.,
            48.,
        ),
    ];
    for (key, kind, x) in [
        ("sweep", feature::SolidFormKind::Sweep, 370.),
        ("loft", feature::SolidFormKind::Loft, 420.),
        ("rib", feature::SolidFormKind::Rib, 470.),
        ("solid-fillet", feature::SolidFormKind::Fillet, 530.),
        ("solid-chamfer", feature::SolidFormKind::Chamfer, 580.),
        ("solid-shell", feature::SolidFormKind::Shell, 630.),
        ("combine", feature::SolidFormKind::Combine, 690.),
        ("offset-plane", feature::SolidFormKind::OffsetPlane, 750.),
        ("midplane", feature::SolidFormKind::Midplane, 800.),
        ("angle-plane", feature::SolidFormKind::AnglePlane, 850.),
        ("solid-mirror", feature::SolidFormKind::Mirror, 910.),
        ("split-body", feature::SolidFormKind::SplitBody, 960.),
        (
            "solid-rectangular-pattern",
            feature::SolidFormKind::RectangularPattern,
            1010.,
        ),
        (
            "solid-circular-pattern",
            feature::SolidFormKind::CircularPattern,
            1060.,
        ),
        (
            "external-thread",
            feature::SolidFormKind::ExternalThread,
            1110.,
        ),
        ("hole", feature::SolidFormKind::Hole, 1160.),
        ("move-copy", feature::SolidFormKind::MoveCopy, 1210.),
    ] {
        rows.push((
            key.into(),
            kind.label().into(),
            NativeCommand::Feature(feature::FeatureCommand::Open {
                kind,
                feature_id: None,
            }),
            sketch_mode || feature::panel(world).is_some() || assembly::joint::active(world),
            x,
            34.,
            48.,
        ));
    }
    rows.push((
        "assembly".into(),
        "Assembly".into(),
        NativeCommand::Assembly(assembly::Command::Show(!assembly::active(world))),
        sketch_mode,
        1270.,
        34.,
        48.,
    ));
    rows.push((
        "joint".into(),
        "Joint".into(),
        NativeCommand::Assembly(assembly::Command::Joint(assembly::joint::Command::Open(
            None,
        ))),
        sketch_mode || feature::panel(world).is_some() || assembly::joint::active(world),
        1320.,
        34.,
        40.,
    ));
    if state.close_pending {
        rows.push((
            "cancel-close".into(),
            crate::native_viewport::localization::translate(world, "file.keepWorking").into(),
            NativeCommand::CancelClose,
            false,
            (width / 2. - 190.).max(0.),
            height / 2.,
            180.,
        ));
        rows.push((
            "discard-close".into(),
            crate::native_viewport::localization::translate(world, "file.discardAndClose").into(),
            NativeCommand::DiscardAndClose,
            false,
            width / 2.,
            height / 2.,
            220.,
        ));
        rows.push((
            "save-close".into(),
            crate::native_viewport::localization::translate(world, "file.saveAllAndClose").into(),
            NativeCommand::File(files::FileCommand::SaveAllAndExit),
            false,
            width / 2. - 100.,
            height / 2. + 42.,
            200.,
        ));
    }
    let assets = world.resource::<ViewportUiAssets>().clone();
    let theme = crate::native_viewport::ui::theme(world);
    decorate(
        world,
        state,
        camera,
        &assets,
        theme,
        "toolbar",
        0.,
        0.,
        width,
        top,
        None,
        Some(theme.header.with_alpha(1.)),
        10,
    );
    decorate(
        world,
        state,
        camera,
        &assets,
        theme,
        "browser",
        0.,
        top,
        side,
        height - top - bottom,
        None,
        Some(theme.panel),
        10,
    );
    let playback_caption = presentation::caption(world);
    let display_warning_count =
        services
            .bridge
            .with_native_document_owner(&services.engine, &owner, || {
                Ok(services
                    .engine
                    .solid_scene_snapshot()
                    .bodies
                    .iter()
                    .map(|body| body.display_warnings.len())
                    .sum::<usize>())
            })?;
    let display_warning = if display_warning_count == 0 {
        String::new()
    } else {
        format!("{display_warning_count} imported STEP faces could not be displayed. Exact geometry is retained.")
    };
    decorate(
        world,
        state,
        camera,
        &assets,
        theme,
        "import-display-warning",
        side + 16.,
        top + 8.,
        (width - side - 32.).max(0.),
        if display_warning.is_empty() { 0. } else { 42. },
        Some(&display_warning),
        Some(Color::srgb(0.95, 0.72, 0.30)),
        25,
    );
    let warning_entity = state.decoration["import-display-warning"];
    world.entity_mut(warning_entity).insert((
        TextColor(Color::srgb(0.12, 0.09, 0.03)),
        TextLayout::new(Justify::Left, bevy::text::LineBreak::WordOrCharacter),
    ));
    let status = if let Some(caption) = playback_caption {
        caption
    } else if state.status.is_empty() && files::print_message(world, &owner).is_some() {
        files::print_message(world, &owner).unwrap_or_default()
    } else if state.status.is_empty() {
        crate::native_editor::status(world).unwrap_or_default()
    } else {
        state.status.clone()
    };
    let status_height = 58_f32.min((workbench::navigation_top(height) - top - 8.).max(0.));
    let status_width = (width - side - 360.).max(1.);
    decorate(
        world,
        state,
        camera,
        &assets,
        theme,
        "status-clip",
        side + 12.,
        (workbench::navigation_top(height) - status_height - 8.).max(top),
        status_width,
        status_height,
        None,
        Some(if status.is_empty() {
            Color::NONE
        } else {
            theme.panel
        }),
        20,
    );
    decorate(
        world,
        state,
        camera,
        &assets,
        theme,
        "status",
        8.,
        4.,
        (status_width - 16.).max(0.),
        (status_height - 8.).max(0.),
        Some(&status),
        None,
        20,
    );
    let status_entity = state.decoration["status"];
    let clip = state.decoration["status-clip"];
    if world
        .get::<ChildOf>(status_entity)
        .is_none_or(|parent| parent.parent() != clip)
    {
        world.entity_mut(status_entity).insert((
            ChildOf(clip),
            TextLayout::new(Justify::Left, bevy::text::LineBreak::WordOrCharacter),
        ));
    }
    if state.close_pending {
        decorate(
            world,
            state,
            camera,
            &assets,
            theme,
            "close-shade",
            0.,
            0.,
            width,
            height,
            None,
            Some(Color::srgba(0., 0., 0., 0.45)),
            80,
        );
        decorate(
            world,
            state,
            camera,
            &assets,
            theme,
            "close-panel",
            (width / 2. - 240.).max(0.),
            height / 2. - 80.,
            480_f32.min(width),
            185.,
            None,
            Some(theme.panel),
            81,
        );
        decorate(
            world,
            state,
            camera,
            &assets,
            theme,
            "close-message",
            (width / 2. - 220.).max(0.),
            height / 2. - 60.,
            440_f32.min(width),
            48.,
            Some(crate::native_viewport::localization::translate(
                world,
                "file.unsavedWindow",
            )),
            None,
            82,
        );
    } else {
        for key in ["close-shade", "close-panel", "close-message"] {
            if let Some(entity) = state.decoration.remove(key) {
                world.despawn(entity);
            }
        }
    }
    let nav_width = ((width - side - 16.) / 7.).clamp(1., 108.);
    for (index, row) in rows.iter_mut().take(7).enumerate() {
        row.4 = side + 8. + index as f32 * nav_width;
        row.5 = (height - bottom - 38.).max(top);
        row.6 = (nav_width - 2.).max(1.);
    }
    let live = rows
        .iter()
        .map(|row| row.0.clone())
        .collect::<std::collections::HashSet<_>>();
    state.controls.retain(|key, entity| {
        if live.contains(key) {
            true
        } else {
            world.despawn(*entity);
            false
        }
    });
    for (key, label, command, disabled, x, y, width) in rows {
        let is_extrude = matches!(
            key.as_str(),
            "extrude"
                | "revolve"
                | "sweep"
                | "loft"
                | "rib"
                | "solid-fillet"
                | "solid-chamfer"
                | "solid-shell"
                | "combine"
                | "offset-plane"
                | "midplane"
                | "angle-plane"
                | "solid-mirror"
                | "split-body"
                | "solid-rectangular-pattern"
                | "solid-circular-pattern"
                | "external-thread"
                | "hole"
                | "move-copy"
                | "assembly"
                | "joint"
        );
        let build_icon = match key.as_str() {
            "joint" => interface_shell::ribbon::Icon::Joint,
            "assembly" => interface_shell::ribbon::Icon::Boxes,
            "revolve" => interface_shell::ribbon::Icon::Revolve,
            "sweep" => interface_shell::ribbon::Icon::Sweep,
            "loft" => interface_shell::ribbon::Icon::Loft,
            "rib" => interface_shell::ribbon::Icon::Rib,
            "solid-fillet" => interface_shell::ribbon::Icon::Fillet,
            "solid-chamfer" => interface_shell::ribbon::Icon::Chamfer,
            "solid-shell" => interface_shell::ribbon::Icon::Shell,
            "external-thread" => interface_shell::ribbon::Icon::ExternalThread,
            "hole" => interface_shell::ribbon::Icon::Hole,
            "move-copy" => interface_shell::ribbon::Icon::MoveCopy,
            "combine" => interface_shell::ribbon::Icon::Combine,
            "offset-plane" => interface_shell::ribbon::Icon::OffsetPlane,
            "midplane" => interface_shell::ribbon::Icon::Midplane,
            "angle-plane" => interface_shell::ribbon::Icon::AnglePlane,
            "solid-mirror" => interface_shell::ribbon::Icon::Mirror,
            "split-body" => interface_shell::ribbon::Icon::SplitBody,
            "solid-rectangular-pattern" => interface_shell::ribbon::Icon::RectangularPattern,
            "solid-circular-pattern" => interface_shell::ribbon::Icon::CircularPattern,
            _ => interface_shell::ribbon::Icon::Extrude,
        };
        let is_body = key.starts_with("body-") || key.starts_with("visibility-");
        let surface = command_group(&command);
        let entity = if let Some(entity) = state.controls.get(&key) {
            *entity
        } else {
            let mut system = SystemState::<Commands>::new(world);
            let entity = {
                let mut commands = system.get_mut(world).map_err(|error| error.to_string())?;
                spawn_button(
                    &mut commands,
                    camera,
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(x),
                        top: px(y),
                        width: px(width),
                        height: px(32.),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border: UiRect::all(px(1.)),
                        ..default()
                    },
                    InterfaceControl::button(surface, &label),
                    theme,
                    &assets,
                )
            };
            system.apply(world);
            if is_extrude {
                interface_shell::ribbon::decorate(world, entity, build_icon);
            }
            bind_command(world, entity, command.clone())?;
            state.controls.insert(key, entity);
            entity
        };
        let desired = if is_extrude {
            workbench::tool_node()
        } else {
            Node {
                position_type: PositionType::Absolute,
                left: px(x),
                top: px(y),
                width: px(width),
                height: px(32.),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(1.)),
                ..default()
            }
        };
        if world.get::<Node>(entity) != Some(&desired) {
            world.entity_mut(entity).insert(desired);
        }
        let drawing = workbench::workspace(world) != workbench::Workspace::Solid;
        let selected = match command {
            NativeCommand::SelectBody { body_id, .. } => Some(
                native_viewport::interface_view(world)
                    .2
                    .selected_body_ids
                    .contains(&body_id),
            ),
            _ => None,
        };
        let mut control = world
            .get_mut::<InterfaceControl>(entity)
            .ok_or("Native control was removed")?;
        if control.label != label {
            control.label = label;
        }
        if control.disabled != disabled {
            control.disabled = disabled;
        }
        let visible = if is_extrude {
            !sketch_mode && !drawing
        } else {
            !is_body || (y >= top && y + 32. <= height - bottom)
        };
        if control.visible != visible {
            control.visible = visible;
        }
        let scope = matches!(
            command,
            NativeCommand::CancelClose
                | NativeCommand::DiscardAndClose
                | NativeCommand::File(files::FileCommand::SaveAllAndExit)
        )
        .then(|| "close-document".to_owned());
        if control.modal_scope != scope {
            control.modal_scope = scope;
        }

        if control.selected != selected {
            control.selected = selected;
        }

        let z = if matches!(
            command,
            NativeCommand::CancelClose
                | NativeCommand::DiscardAndClose
                | NativeCommand::File(files::FileCommand::SaveAllAndExit)
        ) {
            90
        } else {
            30
        };
        if world.get::<ZIndex>(entity) != Some(&ZIndex(z)) {
            world.entity_mut(entity).insert(ZIndex(z));
        }
    }
    workbench::synchronize(
        world,
        camera,
        &state.controls,
        (width, height, side),
        sketch_mode,
        &owner,
        services,
    )?;
    files::synchronize(world, services, &owner, width, height)?;
    app_settings::synchronize(world, camera, services, width, height)?;
    let body_appearance_visible = workbench::workspace(world) == workbench::Workspace::Solid
        && !sketch_mode
        && feature::panel(world).is_none()
        && !assembly::joint::active(world)
        && !named_views::active(world)
        && !print_intent::active(world);
    body_appearance::synchronize(
        world,
        camera,
        services,
        &owner,
        width,
        height,
        body_appearance_visible,
    )?;
    crate::native_editor::synchronize_selection_readout(world, body_appearance_visible);
    named_views::synchronize(world, camera, services, &owner, width, height)?;
    print_intent::synchronize(world, camera, services, &owner, width, height)?;
    section_review::synchronize(world, camera, services, &owner, width, height)?;
    if assembly::active(world) || workbench::workspace(world) != workbench::Workspace::Solid {
        browser::hide(world);
    } else {
        browser::synchronize(
            world,
            services,
            &owner,
            revision,
            InterfaceRect {
                x: 0.,
                y: top as f64,
                width: side as f64,
                height: (height - top - bottom) as f64,
            },
            &mut state.sidebar_scroll,
        )?;
    }
    assembly::synchronize(
        world,
        services,
        &owner,
        revision,
        InterfaceRect {
            x: 0.,
            y: top as f64,
            width: side as f64,
            height: (height - top - bottom) as f64,
        },
    )?;
    let client = InterfaceRect {
        x: 0.,
        y: 0.,
        width: width as f64,
        height: height as f64,
    };
    history::synchronize(world, services, &owner, revision, width, height)?;
    presentation::synchronize(world, &owner, camera, width, height)?;
    handle.present(InterfaceFrame {
        context: owner,
        client,
        surface: client,
        canvases: vec![Canvas {
            name: "viewport".into(),
            bounds: InterfaceRect {
                x: canvas.min.x as f64,
                y: canvas.min.y as f64,
                width: canvas.width() as f64,
                height: canvas.height() as f64,
            },
        }]
        .into_iter()
        .chain(workbench::drawing_canvas(world))
        .collect(),
        surfaces: vec![
            Surface {
                name: "document/session".into(),
                text: Some(if state.close_pending {
                    crate::native_viewport::localization::translate(world, "file.unsavedDocument")
                        .into()
                } else {
                    services.engine.document_name()
                }),
            },
            Surface {
                name: "document/history".into(),
                text: None,
            },
            Surface {
                name: "document/status".into(),
                text: Some(status),
            },
            Surface {
                name: "document/import-display-warning".into(),
                text: (!display_warning.is_empty()).then_some(display_warning),
            },
            Surface {
                name: "document/presentation".into(),
                text: presentation::caption(world),
            },
            Surface {
                name: "solid/selection".into(),
                text: crate::native_editor::selection_caption(world),
            },
            Surface {
                name: "document/appearance".into(),
                text: app_settings::caption(world),
            },
            Surface {
                name: "body/appearance".into(),
                text: body_appearance::caption(world),
            },
            Surface {
                name: "body/print-intent".into(),
                text: print_intent::caption(world),
            },
            Surface {
                name: "sketch/draw".into(),
                text: None,
            },
        ]
        .into_iter()
        .chain(files::modal(world).map(|name| Surface {
            name: name.into(),
            text: None,
        }))
        .chain(state.close_pending.then(|| Surface {
            name: "close-document".into(),
            text: Some("Unsaved changes".into()),
        }))
        .chain(workbench::modal(world).map(|name| Surface {
            name: name.into(),
            text: match name {
                "cam-export" => workbench::cam_export::caption(world),
                "cam-report" => workbench::cam_view::report_caption(world),
                "cam-nc-source" => workbench::cam_view::nc_dialog::caption(world),
                "cam-library" => workbench::cam::caption(world),
                _ => None,
            },
        }))
        .chain(workbench::cam::caption(world).map(|text| Surface {
            name: "cam/tools".into(),
            text: Some(text),
        }))
        .chain(workbench::cam_view::caption(world).map(|text| Surface {
            name: "cam/view".into(),
            text: Some(text),
        }))
        .chain(history::modal(world).map(|name| Surface {
            name: name.into(),
            text: None,
        }))
        .chain(
            crate::native_editor::panel::modal(world)
                .or_else(|| crate::native_editor::support::modal(world))
                .map(|name| Surface {
                    name: name.into(),
                    text: None,
                }),
        )
        .collect(),
        modal_stack: if state.close_pending {
            vec!["close-document".into()]
        } else {
            files::modal(world)
                .or_else(|| workbench::modal(world))
                .or_else(|| history::modal(world))
                .or_else(|| crate::native_editor::panel::modal(world))
                .or_else(|| crate::native_editor::support::modal(world))
                .into_iter()
                .map(str::to_owned)
                .collect()
        },
        document_visible: visible,
    })?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn decorate(
    world: &mut World,
    state: &mut Controller,
    camera: Entity,
    assets: &ViewportUiAssets,
    theme: ViewportUiTheme,
    key: &str,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    text: Option<&str>,
    background: Option<Color>,
    z: i32,
) {
    let entity = *state.decoration.entry(key.into()).or_insert_with(|| {
        world
            .spawn((
                Name::new(format!("Application {key}")),
                UiTargetCamera(camera),
            ))
            .id()
    });
    let node = Node {
        position_type: PositionType::Absolute,
        left: px(x),
        top: px(y),
        width: px(width),
        height: px(height),
        overflow: Overflow::clip(),
        ..default()
    };
    if world.get::<Node>(entity) != Some(&node) {
        world.entity_mut(entity).insert(node);
    }
    if world.get::<ZIndex>(entity) != Some(&ZIndex(z)) {
        world.entity_mut(entity).insert(ZIndex(z));
    }
    if let Some(background) = background {
        if world.get::<BackgroundColor>(entity) != Some(&BackgroundColor(background)) {
            world.entity_mut(entity).insert(BackgroundColor(background));
        }
    }
    if let Some(text) = text {
        if world.get::<TextColor>(entity) != Some(&TextColor(theme.ink)) {
            world.entity_mut(entity).insert(TextColor(theme.ink));
        }
        if world
            .get::<Text>(entity)
            .is_none_or(|value| value.0 != text)
        {
            world.entity_mut(entity).insert((
                Text::new(text),
                theme.text(assets, 13., FontWeight::NORMAL),
                TextColor(theme.ink),
            ));
        }
    }
}

fn command_group(command: &NativeCommand) -> &'static str {
    match command {
        NativeCommand::Sketch(_) => "sketch/draw",
        NativeCommand::Assembly(assembly::Command::Study(_)) => "assembly/motion",
        NativeCommand::Assembly(assembly::Command::Inspect(_)) => "assembly/inspect",
        NativeCommand::Assembly(_) => "assembly/joints",
        NativeCommand::Feature(feature::FeatureCommand::Open { kind, .. }) => kind.group(),
        NativeCommand::Feature(_) => "document/history",
        NativeCommand::Mutation { operation, .. } => {
            limo_cad_interface::catalog::group_for(operation).unwrap_or("document/session")
        }
        NativeCommand::Undo | NativeCommand::Redo => "document/history",
        NativeCommand::ClearSelection | NativeCommand::SelectBody { .. } => "solid/selection",
        _ => limo_cad_interface::catalog::group_for("cad_interface")
            .expect("Interface is in the product catalog"),
    }
}

/// This runs after actual native layout. Merely queuing an action or updating
/// a component cannot produce a completed inspection receipt.
fn complete_control(world: &mut World) {
    if worker::busy(world) {
        return;
    }
    let services = world.resource::<NativeServices>().clone();
    let handle = world.resource::<NativeInterfaceHandle>().clone();
    world.resource_scope(|world, mut state: Mut<Controller>| {
        if let Some(mut pending) = state.pending.take() {
            if let Some(id) = pending.response["value"]["camera_pending"].as_u64() {
                match view::poll(world, id) {
                    None => {
                        state.pending = Some(pending);
                        handle.request_redraw();
                        return;
                    }
                    Some(Ok(value)) => {
                        pending.response["value"] = value;
                        pending.presentation_deadline = now_ms().saturating_add(2_000);
                    }
                    Some(Err(error)) => {
                        pending.response["status"] = json!("failed");
                        pending.response["error"] = json!(error);
                        pending.response["value"] = Value::Null;
                    }
                }
            }
            if pending.response["value"]["capture_pending"] == true {
                match capture::poll(world) {
                    None => {
                        state.pending = Some(pending);
                        handle.request_redraw();
                        return;
                    }
                    Some(Ok(value)) => pending.response["value"] = value,
                    Some(Err(error)) => {
                        pending.response["status"] = json!("failed");
                        pending.response["error"] = json!(error);
                        pending.response["value"] = Value::Null;
                    }
                }
            }
            pending.response["awaiting_input"] =
                json!(state.close_pending || files::awaiting(world));
            let drawable = world
                .get_resource::<crate::native_viewport::winit_host::NativeRenderAvailability>()
                .is_some_and(|availability| availability.drawable);
            let deadline_elapsed = now_ms() >= pending.presentation_deadline;
            let target_drawable = match pending.response["value"]["window_transition"].as_str() {
                Some("foreground") => Some(true),
                Some("background") => Some(false),
                _ => None,
            };
            if target_drawable.is_some_and(|target| target != drawable) && !deadline_elapsed {
                state.pending = Some(pending);
                handle.request_redraw();
                return;
            }
            let mut presented = false;
            let mut render_status = "unavailable";
            let current = services
                .bridge
                .native_document_context(&state.window_id, &services.engine);
            if current.as_ref() != Ok(&pending.owner) {
                pending.response["status"] = json!("failed");
                pending.response["error"] =
                    json!("Document changed before native presentation completed");
            } else {
                let receipt = handle.render_receipt().unwrap_or_default();
                let frame_matches = handle
                    .frame()
                    .is_some_and(|frame| frame.context == pending.owner);
                if drawable && frame_matches && receipt.laid_out_revision > 0 {
                    presented = handle.wait_for_submission(receipt.laid_out_revision);
                    if !presented && !deadline_elapsed {
                        state.pending = Some(pending);
                        return;
                    }
                    render_status = if presented {
                        "submitted"
                    } else {
                        "submission_timeout"
                    };
                }
                match if frame_matches {
                    handle.inspect()
                } else {
                    Err("The current document's interface has not been laid out".into())
                } {
                    Ok(snapshot) => {
                        pending.response["ui"] = snapshot;
                        if pending.inspect {
                            pending.response["paper_navigation"] =
                                workbench::inspect_paper_navigation(world).unwrap_or(Value::Null);
                            pending.response["desktop_build"] =
                                json!(limo_cad_build_info::build_info());
                            #[cfg(all(windows, feature = "native-computer-control"))]
                            {
                                pending.response["native_window"] =
                                    inspect_native_window(world, &state.window_id).unwrap_or_else(
                                        |message| {
                                            json!({"status":"unavailable",
                                                "code":"native_window_unavailable",
                                                "message":message,"pid":std::process::id(),
                                                "window_id":state.window_id})
                                        },
                                    );
                            }
                            match inspect_document(
                                world,
                                &handle,
                                &services,
                                &state,
                                &pending.owner,
                            ) {
                                Ok((view, history)) => {
                                    pending.response["view_state"] = view;
                                    pending.response["state"] = json!({"history":history});
                                }
                                Err(error) => {
                                    pending.response["status"] = json!("failed");
                                    pending.response["error"] = json!(error);
                                }
                            }
                        }
                    }
                    Err(error) => {
                        if !deadline_elapsed {
                            state.pending = Some(pending);
                            handle.request_redraw();
                            return;
                        }
                        pending.response["presentation_pending"] = json!(true);
                        pending.response["presentation_error"] = json!(error);
                        presented = false;
                        render_status = "layout_timeout";
                    }
                }
            }
            handle.cancel_submission_wait();
            let receipt = handle.render_receipt().unwrap_or_default();
            pending.response["native_layout_revision"] = json!(receipt.laid_out_revision);
            pending.response["native_submitted_revision"] = json!(receipt.submitted_revision);
            pending.response["presented"] = json!(presented);
            pending.response["render_status"] = json!(render_status);
            if pending.response["value"].get("focused").is_some()
                && pending.response["value"].get("visible").is_some()
            {
                pending.response["value"]["focused"] = json!(world
                    .get_resource::<crate::native_viewport::winit_host::NativeRenderAvailability>()
                    .is_some_and(|availability| availability.focused));
            }
            if let Err(error) = control_for_window(
                &services.bridge,
                &state.window_id,
                &services.engine,
                Some(pending.response),
            ) {
                state.status = error;
            }
        }
        if state.exit_after_receipt && state.pending.is_none() {
            state.stop_watcher.store(true, Ordering::Release);
            services.bridge.drop_window(&state.window_id);
            world.write_message(AppExit::Success);
        }
    });
}

#[cfg(test)]
mod tests;
