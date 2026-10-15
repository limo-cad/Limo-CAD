//! Main-window controller using the existing native document and MCP services.
//! The development host exposes only migrated, functional controls; it does
//! not publish synthetic counterparts for the remaining web-shell controls.

use super::*;
use crate::native_viewport::{
    interface_shell::{self, spawn_button, InterfaceCamera, InterfaceFrame, InterfaceLayout},
    ui::{ViewportUiAssets, ViewportUiTheme},
    winit_host::NativeHostInput,
    ViewportPalette,
};
use crate::session_bridge::{apply_or_reject_one_inbox_op, control_for_window, now_ms};
use bevy::{
    ecs::{message::MessageCursor, system::SystemState},
    prelude::*,
    text::FontWeight,
    window::{PrimaryWindow, WindowEvent},
};
use nbcad_interface::{Canvas, ControlRequest, DocumentContext, Rect as InterfaceRect, Surface};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

pub(crate) mod browser;
pub(crate) mod assembly;
mod capture;
pub(crate) mod chrome;
pub(crate) mod files;
pub(crate) mod history;
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
}
struct PolledControl {
    owner: DocumentContext,
    session: String,
    id: String,
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
    close_pending: bool,
    exit_after_receipt: bool,
    close_after_worker: bool,
    busy_controls: Vec<(Entity, bool)>,
    cached_session: Option<String>,
    polled_control: Option<PolledControl>,
    watch_session: Arc<Mutex<Option<String>>>,
    stop_watcher: Arc<AtomicBool>,
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
            close_pending: false,
            exit_after_receipt: false,
            close_after_worker: false,
            busy_controls: Vec::new(),
            cached_session: None,
            polled_control: None,
            watch_session: Arc::new(Mutex::new(None)),
            stop_watcher,
        }
    }
}
impl Drop for Controller {
    fn drop(&mut self) {
        self.stop_watcher.store(true, Ordering::Release);
    }
}

/// Called only by the explicitly built native host. No new MCP endpoint or
/// independent engine is introduced when running the existing Tauri host.
pub(crate) fn install(
    app: &mut App,
    handle: NativeInterfaceHandle,
    services: NativeServices,
    window_id: String,
    initial_model: Option<String>,
) {
    let stop = Arc::new(AtomicBool::new(false));
    let worker_error = worker::install(app.world_mut(), services.clone(), handle.clone()).err();
    let controller = Controller::new(window_id.clone(), initial_model, stop.clone());
    start_watcher(
        &services,
        &window_id,
        handle.clone(),
        stop,
        controller.watch_session.clone(),
    );
    app.insert_resource(services).insert_resource(controller);
    if let Some(error) = worker_error {
        app.world_mut().resource_mut::<Controller>().status = error;
    }
    interface_shell::install(app, handle, update);
    app.add_systems(PostUpdate, complete_control.after(InterfaceLayout));
}

fn start_watcher(
    services: &NativeServices,
    window_id: &str,
    handle: NativeInterfaceHandle,
    stop: Arc<AtomicBool>,
    cached_session: Arc<Mutex<Option<String>>>,
) {
    let bridge = Arc::downgrade(&services.bridge);
    let window = window_id.to_owned();
    let _ = std::thread::Builder::new()
        .name("cad-native-inbox".into())
        .spawn(move || {
            let mut keepalive = now_ms();
            let heartbeat_running = Arc::new(AtomicBool::new(false));
            while !stop.load(Ordering::Acquire) {
                std::thread::sleep(Duration::from_millis(25));
                if stop.load(Ordering::Acquire) {
                    break;
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
                // Never wait for the publisher on the file-notification
                // thread: a long kernel transaction must still wake native
                // busy replies for new MCP requests.
                let Some(session) = cached_session
                    .lock()
                    .ok()
                    .and_then(|session| session.clone())
                else {
                    continue;
                };
                let root = crate::session_bridge::session_root().join(session);
                let controls =
                    std::fs::read_dir(root.join("controls"))
                        .ok()
                        .is_some_and(|entries| {
                            entries.filter_map(Result::ok).any(|entry| {
                                entry
                                    .file_name()
                                    .to_string_lossy()
                                    .ends_with(".request.json")
                            })
                        });
                let inbox = std::fs::read_dir(root.join("inbox"))
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
                    state.status.clear();
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
                        request_close(state, bridge, engine)?;
                    }
                    state.status = summary(&value);
                }
                Err(error) => {
                    files::dialog_error(world, &error);
                    state.status = format!("{}: {error}", outcome.operation);
                }
            }
        }
        if state.close_after_worker && !worker::busy(world) {
            state.close_after_worker = false;
            request_close(state, bridge, engine)?;
        }
    }
    if worker::busy(world) {
        return maintain_busy_window(world, handle, state);
    }
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

    if !state.close_pending {
        if let Err(error) = files::poll(world, services) {
            files::dialog_error(world, &error);
            state.status = error;
        }
        if worker::busy(world) {
            return maintain_busy_window(world, handle, state);
        }
    }

    // Raw OS events are already ordered and stamped by the Winit adapter.
    // Reading them here prevents a close delivered with A from closing B.
    let events = state
        .input
        .read(world.resource::<Messages<NativeHostInput>>())
        .cloned()
        .collect::<Vec<_>>();
    for mut event in events {
        if state.exit_after_receipt {
            continue;
        }
        if worker::busy(world) {
            retain_busy_intent(state, &event);
            crate::native_editor::mechanism::observe_busy(world,&event);
            continue;
        }
        // Lifecycle input can arrive before the first semantic frame. Handle
        // it before the ordinary widget ownership check rejects an unstamped
        // event after initialization has published that first frame.
        if matches!(event.event, WindowEvent::WindowCloseRequested(_)) {
            if let Err(error) =
                close_from_window_event(state, bridge, engine, event.context.as_ref())
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
        if let WindowEvent::MouseWheel(wheel) = &event.event {
            if let Some(cursor) = event.cursor {
                let factor = if matches!(wheel.unit, bevy::input::mouse::MouseScrollUnit::Line) {
                    36.
                } else {
                    1.
                };
                if assembly::joint::scroll(world,cursor.to_array(),wheel.y*factor) || feature::panel::scroll_panel(world, cursor.to_array(), wheel.y * factor)
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
                                1.
                            };
                            if !assembly::scroll(world,-wheel.y*factor) {
                                state.sidebar_scroll=(state.sidebar_scroll-wheel.y*factor).max(0.);
                            }
                        }
                    }
                }
            }
        }
        if !event.consumed {
            if let Err(error) = crate::native_editor::process_one(world, handle, services, &event) {
                state.status = error;
            }
        }
    }

    while !worker::busy(world) {
        let Some(action)=handle.take_next_action()? else {break;};
        if let Err(error) = apply_queued_control(world, handle, services, state, &action) {
            files::dialog_error(world, &error);
            state.status = error;
        }
    }
    if worker::busy(world) {
        return maintain_busy_window(world, handle, state);
    }
    process_modal_keys(world, handle, bridge, engine, state)?;
    // Serialize controls through their completed semantic frame. The native
    // apply function remains the sole inbox dispatcher and OCC gate.
    if state.pending.is_none() {
        if let Some(session) = bridge.session_id_for_window(&state.window_id)? {
            if !crate::session_bridge::pending_inbox_seqs(&session).is_empty() {
                let owner = bridge.native_document_context(&state.window_id, engine)?;
                let reject = (state.close_pending || files::awaiting(world))
                    .then_some("A document dialog is waiting for input");
                worker::enqueue_transaction(
                    world,
                    "inbox".into(),
                    move |services, guard| {
                        guard.validate()?;
                        let applied = apply_or_reject_one_inbox_op(
                            &services.bridge,
                            &owner.window_id,
                            &services.engine,
                            reject,
                            Some((&owner.document_id, &session)),
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
                state.polled_control = Some(PolledControl { owner, session, id });
                return maintain_busy_window(world, handle, state);
            }
        }
    }
    if worker::busy(world) {
        return maintain_busy_window(world, handle, state);
    }
    if state.close_pending || state.exit_after_receipt {
        view::cancel(world, "Camera transition interrupted by the window close request");
    }
    if view::pending(world) {
        let owner = bridge.native_document_context(&state.window_id, engine)?;
        bridge.with_native_document_receipt(engine, &owner, |revision| {
            view::advance(world, &workspace::DocumentReceipt { owner: owner.clone(), revision });
            Ok(())
        })?;
        if view::pending(world) { handle.request_redraw(); }
    }
    if crate::native_editor::mechanism::active(world) {
        let owner=bridge.native_document_context(&state.window_id,engine)?;
        if state.close_pending || files::awaiting(world) {crate::native_editor::mechanism::cancel(world);}
        crate::native_editor::mechanism::tick(world,handle,services,&owner)?;
        if worker::busy(world) {return maintain_busy_window(world,handle,state);}
    }
    if (assembly::motion::active(world) || assembly::studies::active(world)) && state.pending.is_none() && !state.close_pending && !state.exit_after_receipt && !files::awaiting(world) {
        let owner=bridge.native_document_context(&state.window_id,engine)?;
        assembly::studies::tick(world,handle,services,&owner)?;
        if worker::busy(world) {return maintain_busy_window(world,handle,state);}
        assembly::motion::tick(world,handle,services,&owner)?;
        if worker::busy(world) {return maintain_busy_window(world,handle,state);}
    }
    synchronize(world, handle, services, state)
}

fn start_control(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    state: &mut Controller,
    owner: &DocumentContext,
    request: &Value,
) -> Result<(), String> {
    let mut response = json!({"request_id":request["id"],"session_id":request["session_id"]});
    let outcome = apply_control(world, handle, services, state, owner, request);
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
            if value["request_exit"] == true {
                request_close(state, &services.bridge, &services.engine)?;
            }
            response["status"] = json!("applied");
            response["value"] = value;
        }
        Err(error) => {
            files::dialog_error(world, &error);
            response["status"] = json!("failed");
            response["error"] = json!(error);
        }
    }
    response["awaiting_input"] = json!(state.close_pending || files::awaiting(world));
    let now = now_ms();
    state.pending = Some(PendingControl {
        response,
        owner: current,
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

fn retain_busy_intent(state: &mut Controller, event: &NativeHostInput) {
    if matches!(event.event, WindowEvent::WindowCloseRequested(_)) {
        state.close_after_worker = true;
    }
    // Pointer, tool and text events refer to the cached pre-mutation scene.
    // Replaying them against newly built geometry could pick a different face.
}

fn maintain_busy_window(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    state: &mut Controller,
) -> Result<(), String> {
    if let Some(session) = state.cached_session.as_deref() {
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
        retain_busy_intent(state, &event);
            crate::native_editor::mechanism::observe_busy(world,&event);
    }
    let _ = handle.take_actions()?;
    let _ = handle.take_modal_keys()?;
    let message = if state.close_after_worker {
        "Finishing the current modeling operation before closing…"
    } else {
        "Building the model… Controls resume when this operation finishes."
    };
    state.status = message.into();
    if let Some(entity) = state.decoration.get("status") {
        if let Some(mut text) = world.get_mut::<Text>(*entity) {
            if text.0 != message {
                text.0 = message.into();
                handle.invalidate_presentation();
            }
        }
    }
    if worker::started(world) && state.busy_controls.is_empty() {
        let mut query = world.query::<(Entity, &mut InterfaceControl)>();
        for (entity, mut control) in query.iter_mut(world) {
            state.busy_controls.push((entity, control.disabled));
            control.disabled = true;
        }
        if let Some(mut frame) = handle.frame() {
            if let Some(surface) = frame
                .surfaces
                .iter_mut()
                .find(|surface| surface.name == "document/session")
            {
                surface.text = Some(message.into());
            }
            handle.present(frame)?;
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
                "file-menu" | "file-dialog" => files::escape(world),
                "history-menu" | "delete-feature" => history::escape(world),
                "sketch-menu" => crate::native_editor::panel::escape(world),
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
    for preceding in fields::prepare_control_input(world, handle, action)? {
        let result = reduce_action(engine, bridge, world, handle, &preceding);
        fields::acknowledge_control_input(world, &preceding, result.is_ok());
        result?;
    }
    handle.prepare_activation(action)?;
    if let ControlInput::Key(key) = &action.control.input {
        bridge.with_native_document_owner(engine, &action.context, || {
            handle.validate_action(action)
        })?;
        if handle.navigate_menu(key)? {
            return Ok(json!({"handled":true,"menu_navigation":true}));
        }
        if key == &nbcad_interface::KeyChord::plain("Escape") {
            if let Some(scope) = handle
                .frame()
                .and_then(|frame| frame.modal_stack.last().cloned())
            {
                match scope.as_str() {
                    "file-menu" | "file-dialog" => files::escape(world),
                    "history-menu" | "delete-feature" => history::escape(world),
                    "sketch-menu" => crate::native_editor::panel::escape(world),
                    "sketch-origin" => return crate::native_editor::execute(world,engine,bridge,&action.context,crate::native_editor::EditorCommand::Cancel,||handle.validate_action(action)),
                    "close-document" => return Ok(json!({"close_decision":"cancel"})),
                    _ => return Err("This dialog does not handle Escape".into()),
                }
                return Ok(json!({"cancelled":true}));
            }
            if matches!(
                world
                    .get::<NativeCommandBinding>(Entity::from_bits(action.control.key.0))
                    .map(|b| &b.command),
                Some(NativeCommand::Sketch(_))
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
    if matches!(&action.control.input, ControlInput::Key(k) if k.key=="Escape"&&!k.ctrl&&!k.meta&&!k.alt&&!k.shift) && assembly::joint::active(world) {
        bridge.with_native_document_owner(engine,&action.context,||handle.validate_action(action))?;
        return assembly::joint::cancel(world,engine,bridge,&action.context);
    }
    fields::after_window_input(world, handle)?;
    let Some(adapted) = fields::adapt_control_input(world, handle, action)? else {
        return Ok(json!({"handled":true,"field_navigation":true}));
    };
    world.insert_resource(worker::ActiveControl(adapted.clone()));
    let result = reduce_action(engine, bridge, world, handle, &adapted);
    world.remove_resource::<worker::ActiveControl>();
    fields::acknowledge_control_input(world, &adapted, result.is_ok());
    // A committed operation must not be reported as failed if subsequent
    // editor focus synchronization fails: retrying it could duplicate a part.
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
        request_close(state, &services.bridge, &services.engine)?;
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
    state: &mut Controller,
    bridge: &SessionBridgeState,
    engine: &AppState,
) -> Result<(), String> {
    let owner = bridge.native_document_context(&state.window_id, engine)?;
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
    state: &mut Controller,
    bridge: &SessionBridgeState,
    engine: &AppState,
    stamped_owner: Option<&DocumentContext>,
) -> Result<(), String> {
    if let Some(owner) = stamped_owner {
        bridge.with_native_document_owner(engine, owner, || Ok(()))?;
    }
    request_close(state, bridge, engine)
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
        return services.bridge.with_native_document_receipt(&services.engine, owner, |revision| {
            view::request(world, owner, revision, request)
        });
    };
    match ui["action"].as_str().unwrap_or("") {
        "inspect" => Ok(Value::Null),
        "capture" => capture::begin(world, handle, services, owner, ui),
        "viewport" => crate::native_editor::mcp::drive(world, handle, services, owner, ui),
        "file" if ui["command"] == "exit" => {
            request_close(state, &services.bridge, &services.engine)?;
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
                request_close(state, &services.bridge, &services.engine)?;
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
    let mut windows = world.query_filtered::<&Window, With<PrimaryWindow>>();
    let window = windows
        .single(world)
        .map_err(|_| "Native window is unavailable")?;
    // Windows reports a zero-size client on minimization. Keep the last real
    // layout so background MCP control remains useful without claiming a render.
    if window.width() > 0. && window.height() > 0. {
        state.logical_size = Vec2::new(window.width(), window.height());
    }
    let width = state.logical_size.x;
    let height = state.logical_size.y;
    let scale = window.resolution.scale_factor();
    let visible = window.visible;
    let side = (if assembly::active(world) {286_f32} else {240_f32}).min(width * 0.45);
    let top = 120_f32.min(height * 0.3);
    let bottom = 74_f32.min(height * 0.1);
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
    assembly::joint::synchronize(world,services,&owner,revision,InterfaceRect{x:(width-400.).max(side)as f64,y:(top+12.)as f64,width:380_f32.min(width-side).max(1.)as f64,height:(height-top-bottom-24.).max(1.)as f64})?;
    feature::synchronize(&services.engine, &services.bridge, world, &owner)?;
    feature::panel::synchronize_panel(
        world,
        handle,
        &owner,
        InterfaceRect {
            x: (width - 336.).max(side) as f64,
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
            x: if width <= 1400. { 60. } else { 112. },
            y: 34.,
            width: (width - if width <= 1400. { 60. } else { 112. }).max(1.) as f64,
            height: 72.,
        },
        InterfaceRect {
            x: canvas.min.x as f64,
            y: canvas.min.y as f64,
            width: canvas.width() as f64,
            height: canvas.height() as f64,
        },
    )?;
    let (_, _, presentation, _) = native_viewport::interface_view_snapshot(world);
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
            NativeCommand::Feature(feature::FeatureCommand::Open { kind:feature::SolidFormKind::Extrude, feature_id: None }),
            presentation.mode == native_viewport::ViewportMode::Sketch
                || feature::panel(world).is_some() || assembly::joint::active(world),
            270.,
            34.,
            48.,
        ),
        (
            "revolve".to_owned(),"Revolve".to_owned(),
            NativeCommand::Feature(feature::FeatureCommand::Open {kind:feature::SolidFormKind::Revolve,feature_id:None}),
            presentation.mode==native_viewport::ViewportMode::Sketch || feature::panel(world).is_some() || assembly::joint::active(world),
            320.,34.,48.,
        ),
    ];
    for (key,kind,x) in [("sweep",feature::SolidFormKind::Sweep,370.),("loft",feature::SolidFormKind::Loft,420.),("rib",feature::SolidFormKind::Rib,470.),("solid-fillet",feature::SolidFormKind::Fillet,530.),("solid-chamfer",feature::SolidFormKind::Chamfer,580.),("solid-shell",feature::SolidFormKind::Shell,630.),("combine",feature::SolidFormKind::Combine,690.),("offset-plane",feature::SolidFormKind::OffsetPlane,750.),("midplane",feature::SolidFormKind::Midplane,800.),("angle-plane",feature::SolidFormKind::AnglePlane,850.),("solid-mirror",feature::SolidFormKind::Mirror,910.),("split-body",feature::SolidFormKind::SplitBody,960.),("solid-rectangular-pattern",feature::SolidFormKind::RectangularPattern,1010.),("solid-circular-pattern",feature::SolidFormKind::CircularPattern,1060.),("external-thread",feature::SolidFormKind::ExternalThread,1110.),("hole",feature::SolidFormKind::Hole,1160.),("move-copy",feature::SolidFormKind::MoveCopy,1210.)] {
        rows.push((key.into(),kind.label().into(),NativeCommand::Feature(feature::FeatureCommand::Open {kind,feature_id:None}),
            presentation.mode==native_viewport::ViewportMode::Sketch||feature::panel(world).is_some() || assembly::joint::active(world),x,34.,48.));
    }
    rows.push(("assembly".into(),"Assembly".into(),NativeCommand::Assembly(assembly::Command::Show(!assembly::active(world))),presentation.mode==native_viewport::ViewportMode::Sketch,1270.,34.,48.));
    rows.push(("joint".into(),"Joint".into(),NativeCommand::Assembly(assembly::Command::Joint(assembly::joint::Command::Open(None))),presentation.mode==native_viewport::ViewportMode::Sketch || feature::panel(world).is_some() || assembly::joint::active(world),1320.,34.,40.));
    if state.close_pending {
        rows.push((
            "cancel-close".into(),
            "Keep working".into(),
            NativeCommand::CancelClose,
            false,
            (width / 2. - 190.).max(0.),
            height / 2.,
            180.,
        ));
        rows.push((
            "discard-close".into(),
            "Discard changes and close".into(),
            NativeCommand::DiscardAndClose,
            false,
            width / 2.,
            height / 2.,
            220.,
        ));
        rows.push((
            "save-close".into(),
            "Save all and close".into(),
            NativeCommand::File(files::FileCommand::SaveAllAndExit),
            false,
            width / 2. - 100.,
            height / 2. + 42.,
            200.,
        ));
    }
    let assets = world.resource::<ViewportUiAssets>().clone();
    let theme = ViewportUiTheme::from_palette(&ViewportPalette::default());
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
    let status = if state.status.is_empty() {
        crate::native_editor::status(world).unwrap_or_default()
    } else {
        state.status.clone()
    };
    decorate(
        world,
        state,
        camera,
        &assets,
        theme,
        "status",
        4.,
        height - bottom,
        width - 8.,
        bottom,
        Some(&status),
        Some(theme.header),
        20,
    );
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
            Some("Unsaved changes in this window\nSave all documents, keep working, or discard all changes."),
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
        let is_extrude = matches!(key.as_str(), "extrude" | "revolve" | "sweep" | "loft" | "rib" | "solid-fillet" | "solid-chamfer" | "solid-shell" | "combine" | "offset-plane" | "midplane" | "angle-plane" | "solid-mirror" | "split-body" | "solid-rectangular-pattern" | "solid-circular-pattern" | "external-thread" | "hole" | "move-copy" | "assembly" | "joint");
        let build_icon = match key.as_str() {"joint"=>interface_shell::ribbon::Icon::Joint,"assembly"=>interface_shell::ribbon::Icon::Boxes,"revolve"=>interface_shell::ribbon::Icon::Revolve,"sweep"=>interface_shell::ribbon::Icon::Sweep,"loft"=>interface_shell::ribbon::Icon::Loft,"rib"=>interface_shell::ribbon::Icon::Rib,"solid-fillet"=>interface_shell::ribbon::Icon::Fillet,"solid-chamfer"=>interface_shell::ribbon::Icon::Chamfer,"solid-shell"=>interface_shell::ribbon::Icon::Shell,"external-thread"=>interface_shell::ribbon::Icon::ExternalThread,"hole"=>interface_shell::ribbon::Icon::Hole,"move-copy"=>interface_shell::ribbon::Icon::MoveCopy,"combine"=>interface_shell::ribbon::Icon::Combine,"offset-plane"=>interface_shell::ribbon::Icon::OffsetPlane,"midplane"=>interface_shell::ribbon::Icon::Midplane,"angle-plane"=>interface_shell::ribbon::Icon::AnglePlane,"solid-mirror"=>interface_shell::ribbon::Icon::Mirror,"split-body"=>interface_shell::ribbon::Icon::SplitBody,"solid-rectangular-pattern"=>interface_shell::ribbon::Icon::RectangularPattern,"solid-circular-pattern"=>interface_shell::ribbon::Icon::CircularPattern,_=>interface_shell::ribbon::Icon::Extrude};
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
                interface_shell::ribbon::decorate(
                    world,
                    entity,
                    build_icon,
                );
            }
            bind_command(world, entity, command.clone())?;
            state.controls.insert(key, entity);
            entity
        };
        let desired = if is_extrude {
            interface_shell::ribbon::node(x, y, width)
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
            presentation.mode != native_viewport::ViewportMode::Sketch
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
        let selected = match command {
            NativeCommand::SelectBody { body_id, .. } => {
                Some(presentation.selected_body_ids.contains(&body_id))
            }
            _ => None,
        };
        if control.selected != selected {
            control.selected = selected;
        }
        drop(control);
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
    files::synchronize(world, services, &owner, width, height)?;
    if assembly::active(world) { browser::hide(world); } else {
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
    assembly::synchronize(world,services,&owner,revision,InterfaceRect{x:0.,y:top as f64,width:side as f64,height:(height-top-bottom) as f64})?;
    let client = InterfaceRect {
        // History is a retained footer outside the model canvas.
        x: 0.,
        y: 0.,
        width: width as f64,
        height: height as f64,
    };
    history::synchronize(world, services, &owner, revision, width, height)?;
    let document = services.engine.document_snapshot();
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
        }],
        surfaces: vec![
            Surface {
                name: "document/session".into(),
                text: Some(if state.close_pending {
                    "This document has unsaved changes. Keep working or discard changes and close."
                        .into()
                } else {
                    document.name
                }),
            },
            Surface {
                name: "document/history".into(),
                text: None,
            },
            Surface {
                name: "solid/selection".into(),
                text: None,
            },
            Surface {
                name: "document/appearance".into(),
                text: None,
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
        .chain(history::modal(world).map(|name| Surface {
            name: name.into(),
            text: None,
        }))
        .chain(
            crate::native_editor::panel::modal(world).or_else(|| crate::native_editor::support::modal(world)).map(|name| Surface {
                name: name.into(),
                text: None,
            }),
        )
        .collect(),
        modal_stack: if state.close_pending {
            vec!["close-document".into()]
        } else {
            files::modal(world)
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
            nbcad_interface::catalog::group_for(operation).unwrap_or("document/session")
        }
        NativeCommand::Undo | NativeCommand::Redo => "document/history",
        NativeCommand::ClearSelection | NativeCommand::SelectBody { .. } => "solid/selection",
        _ => nbcad_interface::catalog::group_for("cad_interface")
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
                    None => { state.pending = Some(pending); handle.request_redraw(); return; }
                    Some(Ok(value)) => { pending.response["value"] = value; pending.presentation_deadline = now_ms().saturating_add(2_000); }
                    Some(Err(error)) => { pending.response["status"] = json!("failed"); pending.response["error"] = json!(error); pending.response["value"] = Value::Null; }
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
                    }
                    Err(error) => {
                        if !deadline_elapsed {
                            state.pending = Some(pending);
                            handle.request_redraw();
                            return;
                        }
                        // The operation already ran. Preserve that outcome
                        // and make missing publication explicit, without
                        // suggesting that retrying a mutation is safe.
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
            // This certifies renderer submission of the requested native
            // frame; it never claims physical display or GPU completion.
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
