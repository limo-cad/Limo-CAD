//! Viewport picking owns only a transient session and the existing form draft.
//! Shared edge references are resolved off the UI thread. Apply remains the
//! sole document mutation, using the unchanged CAM command/history path.
use super::*;
use crate::native_viewport::{winit_host::NativeHostInput, ViewportLineLayer, ViewportPreview};
use bevy::input::{keyboard::Key, ButtonState};
use bevy::math::DVec3;
use limo_cad_sketch::{ChainMode, ChainSource};
use operation_geometry::picking::{self, SelectionState};
use std::{collections::VecDeque, sync::Arc};
use workspace::DocumentReceipt;

mod hit;
mod holes;
mod point_target;
mod points;
#[cfg(test)]
mod tests;
mod worker;

struct Pending {
    key: String,
    click: bool,
}
struct Session {
    receipt: DocumentReceipt,
    selection: SelectionState,
    candidates: Vec<worker::Candidate>,
    loaded: bool,
    projection: Option<(native_viewport::ViewportCamera, InterfaceRect)>,
    projected: Vec<hit::Projected>,
    hover: Option<String>,
    hover_keys: Vec<String>,
    hover_resolved: bool,
    pending: Option<Pending>,
    clicks: VecDeque<(String, bool)>,
    captured: bool,
    direction: Option<native_viewport::ViewportArrow>,
    transforms: HashMap<u64, Transform>,
    cursor: Option<Vec2>,
    individual: bool,
}
#[derive(Resource, Default)]
struct State {
    session: Option<Session>,
    holes: Option<holes::Session>,
    points: Option<points::Session>,
    worker: Option<worker::Worker>,
}

pub(super) fn is_button(path: &str) -> bool {
    operation_geometry::picking::is_button(path)
        || path == setup::picking::BUTTON
        || operation_editor::linking_points::picking::is_button(path)
        || operation_editor::heights::picking::is_button(path)
}

pub(crate) fn active(world: &World) -> bool {
    world.get_resource::<State>().is_some_and(|state| {
        state.session.is_some() || state.holes.is_some() || state.points.is_some()
    })
}
pub(super) fn loading(world: &World) -> bool {
    world.get_resource::<State>().is_some_and(|state| {
        state
            .session
            .as_ref()
            .is_some_and(|session| !session.loaded)
            || state.holes.as_ref().is_some_and(|session| !session.loaded)
            || state.points.as_ref().is_some_and(|session| !session.loaded)
    })
}
pub(super) fn label(world: &World, kind: &str, path: &str) -> &'static str {
    if active(world) && target_matches(world, path) {
        "Done picking"
    } else if operation_editor::heights::picking::is_button(path) {
        "Pick height geometry"
    } else if kind == "linking" {
        "Pick position"
    } else if kind == "wcs" {
        "Pick origin"
    } else if matches!(kind, "drill" | "thread") {
        "Pick holes"
    } else if kind == "pocket2d" {
        "Pick boundary"
    } else {
        "Pick edges"
    }
}
fn changed(world: &mut World, handle: &NativeInterfaceHandle) {
    super::super::cam_view::geometry_changed(world);
    handle.invalidate_presentation();
}
fn stop(state: &mut State) {
    state.session = None;
    state.holes = None;
    state.points = None;
    if let Some(worker) = &mut state.worker {
        worker.cancel();
    }
}
pub(crate) fn cancel(world: &mut World, handle: &NativeInterfaceHandle) {
    if let Some(mut state) = world.get_resource_mut::<State>() {
        let was_active = state.session.is_some() || state.holes.is_some() || state.points.is_some();
        stop(&mut state);
        if was_active {
            crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
            changed(world, handle);
        }
    }
}
fn permitted(world: &World, handle: &NativeInterfaceHandle) -> bool {
    workspace(world) == Workspace::Cam
        && !super::super::super::worker::busy(world)
        && !files::awaiting(world)
        && files::modal(world).is_none()
        && super::super::modal(world).is_none()
        && history::modal(world).is_none()
        && crate::native_editor::panel::modal(world).is_none()
        && crate::native_editor::support::modal(world).is_none()
        && feature::panel(world).is_none()
        && handle
            .frame()
            .is_some_and(|frame| frame.modal_stack.is_empty())
}
fn current(
    world: &World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    session: &Session,
) -> Result<(), String> {
    if !permitted(world, handle) {
        return Err("CAM picking ended because the workspace changed".into());
    }
    let editor = world
        .get_resource::<Editor>()
        .ok_or("Open the CAM editor")?;
    ensure_current(editor, &session.receipt.owner, session.receipt.revision)?;
    let draft = editor.draft.as_ref().ok_or("Open the geometry editor")?;
    if picking::snapshot(draft)? != session.selection
        || session.transforms.iter().any(|(body, transform)| {
            native_viewport::interface_body_transform(world, *body, None) != *transform
        })
        || handle
            .frame()
            .is_none_or(|frame| frame.context != session.receipt.owner)
        || services
            .bridge
            .native_document_receipt(&services.engine, &session.receipt.owner)?
            != session.receipt
    {
        return Err("CAM geometry changed; start picking again".into());
    }
    Ok(())
}
pub(super) fn toggle(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    receipt: &DocumentReceipt,
    editor: &Editor,
) -> Result<Value, String> {
    toggle_target(world, handle, receipt, editor, setup::picking::BUTTON)
}
pub(super) fn toggle_target(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    receipt: &DocumentReceipt,
    editor: &Editor,
    path: &str,
) -> Result<Value, String> {
    if active(world) {
        settled(world)?;
        cancel(world, handle);
        return Ok(json!({"picking":false,"committed":false}));
    }
    if !permitted(world, handle) {
        return Err("Close the current editor or dialog before picking".into());
    }
    world.init_resource::<State>();
    if let Some(worker) = world.resource::<State>().worker.as_ref() {
        if !matches!(
            worker.receive(),
            Err(std::sync::mpsc::TryRecvError::Disconnected)
        ) {
            return Err("The previous geometry picker is finishing; try again".into());
        }
    }
    world.resource_mut::<State>().worker = None;
    let draft = editor.draft.as_ref().ok_or("Open the geometry editor")?;
    if matches!(draft.selection, Selection::Setup(_))
        || operation_editor::linking_points::picking::is_button(path)
        || operation_editor::heights::picking::is_button(path)
    {
        return points::start(world, handle, receipt, draft, &editor.cam, path);
    }
    if matches!(draft.record["kind"].as_str(), Some("drill" | "thread")) {
        return holes::start(world, handle, receipt, draft);
    }
    let selection = picking::snapshot(draft)?;
    let context = operation_editor::geometry(draft)
        .ok_or("Reopen the geometry editor")?
        .clone();
    if !context.scene.errors.is_empty() {
        return Err("Resolve model errors before picking geometry".into());
    }
    let transforms: HashMap<_, _> = context
        .scene
        .bodies
        .iter()
        .filter(|body| {
            context.setup.body_ids.is_empty() || context.setup.body_ids.contains(&body.id)
        })
        .map(|body| {
            (
                body.id.0,
                native_viewport::interface_body_transform(world, body.id.0, None),
            )
        })
        .collect();
    let worker = worker::start(
        context,
        selection.clone(),
        transforms.clone(),
        handle.clone(),
    )?;
    world.resource_mut::<State>().worker = Some(worker);
    world.resource_mut::<State>().session = Some(Session {
        receipt: receipt.clone(),
        selection,
        candidates: Vec::new(),
        loaded: false,
        projection: None,
        projected: Vec::new(),
        hover: None,
        hover_keys: Vec::new(),
        hover_resolved: false,
        pending: None,
        clicks: VecDeque::new(),
        captured: false,
        direction: None,
        transforms,
        cursor: None,
        individual: false,
    });
    crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
    handle.blur();
    changed(world, handle);
    Ok(json!({"picking":true,"committed":false}))
}
pub(super) fn settled(world: &World) -> Result<(), String> {
    if world
        .get_resource::<State>()
        .and_then(|state| state.points.as_ref())
        .is_some_and(|session| session.captured)
    {
        return Err("Release the point handle before finishing picking".into());
    }
    if world
        .get_resource::<State>()
        .and_then(|state| state.holes.as_ref())
        .is_some_and(|session| {
            session
                .pending
                .as_ref()
                .is_some_and(|request| request.click)
                || !session.clicks.is_empty()
        })
    {
        return Err("Wait for the selected face before finishing picking".into());
    }
    if world
        .get_resource::<State>()
        .and_then(|state| state.session.as_ref())
        .is_some_and(|session| {
            session
                .pending
                .as_ref()
                .is_some_and(|pending| pending.click)
                || !session.clicks.is_empty()
        })
    {
        Err("Wait for the selected loop before finishing picking".into())
    } else {
        Ok(())
    }
}
fn viewport(handle: &NativeInterfaceHandle) -> Option<InterfaceRect> {
    handle
        .read_surface(|_, frame| {
            frame
                .canvases
                .iter()
                .find(|canvas| canvas.name == "viewport")
                .map(|canvas| canvas.bounds)
        })
        .ok()
        .flatten()
}
fn project(
    world: &World,
    handle: &NativeInterfaceHandle,
    session: &mut Session,
) -> Result<(), String> {
    let bounds = viewport(handle).ok_or("The CAD viewport is unavailable")?;
    let (owner, camera) = native_viewport::interface_camera_snapshot(world);
    if owner != session.receipt.owner.document_id {
        return Err("The CAD viewport changed document".into());
    }
    if session.projection != Some((camera, bounds)) {
        session.projected =
            hit::project(world, &session.receipt.owner, bounds, &session.candidates)?;
        session.projection = Some((camera, bounds));
    }
    Ok(())
}
fn hovered(
    world: &World,
    handle: &NativeInterfaceHandle,
    session: &Session,
    cursor: Vec2,
    individual: bool,
) -> Option<String> {
    if !canvas_pointer(handle, cursor) {
        return None;
    }
    if handle
        .focused_key()
        .and_then(|key| world.get::<InterfaceControl>(Entity::from_bits(key.0)))
        .is_some_and(|control| matches!(control.field, Field::Text { .. }))
    {
        return None;
    }
    hit::closest(
        &session.candidates,
        &session.projected,
        cursor,
        session.selection.pocket || session.selection.mode == ChainMode::Closed,
        individual,
    )
    .map(|index| session.candidates[index].key.clone())
}
fn canvas_pointer(handle: &NativeInterfaceHandle, cursor: Vec2) -> bool {
    cursor.is_finite()
        && viewport(handle).is_some_and(|bounds| {
            let [x, y] = cursor.as_dvec2().to_array();
            x >= bounds.x
                && y >= bounds.y
                && x < bounds.x + bounds.width
                && y < bounds.y + bounds.height
        })
        && !handle.owns_pointer(cursor.as_dvec2().to_array())
        && !handle.has_capture()
}
fn stage(
    world: &mut World,
    session: &mut Session,
    keys: Vec<String>,
    individual: bool,
) -> Result<(), String> {
    let mut editor = world
        .remove_resource::<Editor>()
        .ok_or("Open the CAM editor")?;
    let result = (|| {
        let draft = editor.draft.as_mut().ok_or("Open the geometry editor")?;
        session.selection =
            picking::stage(draft, &editor.cam, &session.selection, keys, individual)?;
        editor.message = "Geometry is staged. Done picking keeps the draft; Apply saves it.".into();
        Ok(())
    })();
    world.insert_resource(editor);
    result
}
fn pump(world: &mut World, session: &mut Session, worker: &worker::Worker) -> Result<(), String> {
    if !session.loaded || session.pending.is_some() {
        return Ok(());
    }
    while let Some((key, individual)) = session.clicks.pop_front() {
        if individual || (!session.selection.pocket && session.selection.mode == ChainMode::Manual)
        {
            let mut keys = session.selection.keys.clone();
            if let Some(index) = keys.iter().position(|saved| *saved == key) {
                keys.remove(index);
            } else {
                keys.push(key);
            }
            if let Err(error) = stage(world, session, keys, true) {
                world.resource_mut::<Editor>().message = error;
            } else {
                session.direction = None;
            }
        } else {
            worker.resolve(key.clone())?;
            session.pending = Some(Pending { key, click: true });
            return Ok(());
        }
    }
    if !session.individual
        && (session.selection.pocket || session.selection.mode == ChainMode::Closed)
    {
        if let Some(key) = session.hover.clone().filter(|_| !session.hover_resolved) {
            worker.resolve(key.clone())?;
            session.pending = Some(Pending { key, click: false });
        }
    }
    Ok(())
}
fn same_loop_keys(left: &[String], right: &[String]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let right = right.iter().collect::<std::collections::HashSet<_>>();
    right.len() == left.len() && left.iter().all(|key| right.contains(key))
}
pub(crate) fn input(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    event: &NativeHostInput,
) -> Result<bool, String> {
    if !active(world) {
        return Ok(false);
    }
    if world.resource::<State>().points.is_some() {
        return points::input(world, handle, services, event);
    }
    if world.resource::<State>().holes.is_some() {
        return holes::input(world, handle, services, event);
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
    if !lifecycle
        && world
            .resource::<State>()
            .session
            .as_ref()
            .is_some_and(|session| event.context.as_ref() != Some(&session.receipt.owner))
    {
        return Ok(false);
    }
    let escape = matches!(&event.event, WindowEvent::KeyboardInput(key) if !event.consumed && key.state == ButtonState::Pressed && key.logical_key == Key::Escape);
    if escape || lifecycle {
        crate::session_bridge::native_interface::view::navigate(world, handle, event)?;
        cancel(world, handle);
        return Ok(escape);
    }
    let mut state = world.remove_resource::<State>().unwrap();
    let mut updated = false;
    let result: Result<bool, String> = (|| {
        let session = state.session.as_mut().unwrap();
        if event.context.as_ref() != Some(&session.receipt.owner) {
            return Err("CAM document changed; start picking again".into());
        }
        current(world, handle, services, session)?;
        if event.consumed {
            return Ok(false);
        }
        let release = matches!(&event.event, WindowEvent::MouseButtonInput(button) if button.button == MouseButton::Left && button.state == ButtonState::Released);
        if release && session.captured {
            session.captured = false;
            return Ok(true);
        }
        let press = matches!(&event.event, WindowEvent::MouseButtonInput(button) if button.button == MouseButton::Left && button.state == ButtonState::Pressed);
        if !press
            && !matches!(
                event.event,
                WindowEvent::CursorMoved(_) | WindowEvent::CursorLeft(_)
            )
        {
            return Ok(false);
        }
        let navigation = if matches!(
            event.event,
            WindowEvent::CursorMoved(_) | WindowEvent::CursorLeft(_)
        ) {
            crate::session_bridge::native_interface::view::navigate(world, handle, event)?
        } else {
            false
        };
        project(world, handle, session)?;
        session.cursor = event
            .cursor
            .filter(|_| !matches!(event.event, WindowEvent::CursorLeft(_)));
        let individual_changed = session.individual != event.modifiers.alt;
        session.individual = event.modifiers.alt;
        let key = session
            .cursor
            .and_then(|cursor| hovered(world, handle, session, cursor, session.individual));
        if session.hover != key || individual_changed {
            session.hover = key.clone();
            session.hover_keys = key.clone().into_iter().collect();
            session.hover_resolved = false;
            updated = true;
        }
        if press {
            if !event
                .cursor
                .is_some_and(|cursor| canvas_pointer(handle, cursor))
            {
                return Ok(false);
            }
            if handle
                .focused_key()
                .and_then(|key| world.get::<InterfaceControl>(Entity::from_bits(key.0)))
                .is_some_and(|control| matches!(control.field, Field::Text { .. }))
            {
                crate::native_viewport::interface_shell::fields::before_window_input(
                    world,
                    handle,
                    &event.event,
                    event.cursor,
                    event.modifiers,
                )?;
                handle.blur();
                session.captured = true;
                return Ok(true);
            }
            if session.clicks.len() >= 16 {
                return Err("Finish the queued geometry picks before selecting more".into());
            }
            if let Some(key) = key {
                session.clicks.push_back((key, event.modifiers.alt));
            }
            session.captured = true;
            updated = true;
            crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
        }
        pump(
            world,
            session,
            state.worker.as_ref().ok_or("Geometry resolver stopped")?,
        )?;
        Ok(press
            || navigation
            || matches!(event.event, WindowEvent::CursorLeft(_))
            || event
                .cursor
                .is_some_and(|cursor| canvas_pointer(handle, cursor)))
    })();
    if let Err(error) = &result {
        stop(&mut state);
        if let Some(mut editor) = world.get_resource_mut::<Editor>() {
            editor.message = error.clone();
        }
    }
    world.insert_resource(state);
    if updated || result.is_err() {
        changed(world, handle);
    }
    result
}
pub(crate) fn tick(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
) -> Result<(), String> {
    if world
        .get_resource::<State>()
        .is_some_and(|state| state.points.is_some())
    {
        return points::tick(world, handle, services);
    }
    if world
        .get_resource::<State>()
        .is_some_and(|state| state.holes.is_some())
    {
        return holes::tick(world, handle, services);
    }
    let Some(mut state) = world.remove_resource::<State>() else {
        return Ok(());
    };
    let result: Result<bool, String> = (|| {
        if let Some(session) = &state.session {
            current(world, handle, services, session)?;
        }
        let mut updated = false;
        loop {
            let message = state.worker.as_ref().map(|worker| worker.receive());
            match message {
                Some(Ok(message)) => {
                    let Some(session) = state.session.as_mut() else {
                        continue;
                    };
                    updated = true;
                    match message {
                        worker::ResultMessage::Candidates(candidates) => {
                            session.candidates = candidates?;
                            session.loaded = true;
                            session.projection = None;
                        }
                        worker::ResultMessage::Chain(result) => {
                            let Some(pending) = session.pending.take() else {
                                continue;
                            };
                            if pending.click {
                                match result {
                                    Ok(chain) => {
                                        let same =
                                            same_loop_keys(&chain.keys, &session.selection.keys);
                                        if let Err(error) = stage(
                                            world,
                                            session,
                                            if same { Vec::new() } else { chain.keys.clone() },
                                            false,
                                        ) {
                                            world.resource_mut::<Editor>().message = error;
                                        } else {
                                            session.direction =
                                                (!same).then(|| direction(world, &chain)).flatten();
                                        }
                                        if session.hover.as_ref() == Some(&pending.key) {
                                            session.hover_keys = chain.keys;
                                            session.hover_resolved = true;
                                        }
                                    }
                                    Err(error) => world.resource_mut::<Editor>().message = error,
                                }
                            } else if !session.individual
                                && session.hover.as_ref() == Some(&pending.key)
                            {
                                session.hover_keys =
                                    result.map(|chain| chain.keys).unwrap_or_default();
                                session.hover_resolved = true;
                            }
                        }
                        worker::ResultMessage::Points(_)
                        | worker::ResultMessage::Holes(_)
                        | worker::ResultMessage::Face(..) => {
                            return Err("The geometry picker target changed".into());
                        }
                    }
                }
                Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                    state.worker = None;
                    if state.session.is_some() {
                        return Err("Geometry preview worker stopped".into());
                    }
                    break;
                }
                _ => break,
            }
        }
        if let (Some(session), Some(worker)) = (state.session.as_mut(), state.worker.as_ref()) {
            project(world, handle, session)?;
            let hover = session
                .cursor
                .and_then(|cursor| hovered(world, handle, session, cursor, session.individual));
            if session.hover != hover {
                session.hover = hover.clone();
                session.hover_keys = hover.into_iter().collect();
                session.hover_resolved = false;
                updated = true;
            }
            pump(world, session, worker)?;
        }
        Ok(updated)
    })();
    if let Err(error) = &result {
        stop(&mut state);
        if let Some(mut editor) = world.get_resource_mut::<Editor>() {
            editor.message = error.clone();
        }
    }
    world.insert_resource(state);
    if result.as_ref().is_ok_and(|updated| *updated) || result.is_err() {
        changed(world, handle);
    }
    result.map(|_| ())
}
fn direction(
    world: &World,
    chain: &limo_cad_core::edge_chain::Chain,
) -> Option<native_viewport::ViewportArrow> {
    let n = chain.points.len();
    if n < 2 {
        return None;
    }
    let (a, b) = (0..n - 1 + usize::from(chain.closed))
        .map(|index| {
            (
                DVec3::from_array(chain.points[index]),
                DVec3::from_array(chain.points[(index + 1) % n]),
            )
        })
        .max_by(|(a, b), (c, d)| a.distance_squared(*b).total_cmp(&c.distance_squared(*d)))?;
    let transform = chain
        .keys
        .first()
        .and_then(|key| key.strip_prefix("edge:"))
        .and_then(|key| key.split_once(':'))
        .and_then(|(body, _)| body.parse::<u64>().ok())
        .map(|body| native_viewport::interface_body_transform(world, body, None));
    let point = |ratio| {
        let point = a.lerp(b, ratio).as_vec3();
        transform
            .map_or(point, |transform| transform.transform_point(point))
            .to_array()
    };
    Some(native_viewport::ViewportArrow {
        start: point(0.35),
        end: point(0.65),
        color: [0.2, 0.85, 0.45, 1.],
        width: 2.,
        xray: true,
    })
}
pub(in super::super) fn overlay(world: &World) -> Option<ViewportPreview> {
    if let Some(session) = world.get_resource::<State>()?.points.as_ref() {
        return Some(points::overlay(session));
    }
    if let Some(session) = world.get_resource::<State>()?.holes.as_ref() {
        return Some(holes::overlay(session));
    }
    let session = world.get_resource::<State>()?.session.as_ref()?;
    let mut preview = ViewportPreview::default();
    if let Some(arrow) = session.direction {
        preview.arrows.push(arrow);
    }
    let selected_keys = session
        .selection
        .keys
        .iter()
        .collect::<std::collections::HashSet<_>>();
    let hover_keys = session
        .hover_keys
        .iter()
        .collect::<std::collections::HashSet<_>>();
    for (color, width, which) in [
        ([0.47, 0.56, 0.65, 0.6], 1.5, 0),
        ([0.2, 0.85, 0.45, 1.], 3., 1),
        ([1., 0.67, 0.17, 1.], 3., 2),
    ] {
        let mut segments = Vec::new();
        for candidate in &session.candidates {
            let selected = selected_keys.contains(&candidate.key);
            let hover = hover_keys.contains(&candidate.key);
            if match which {
                0 => selected || hover,
                1 => !selected || hover,
                _ => !hover,
            } {
                continue;
            }
            let n = candidate.points.len();
            for index in 0..n.saturating_sub(1) + usize::from(candidate.closed) {
                segments.extend(candidate.points[index].map(|v| v as f32));
                segments.extend(candidate.points[(index + 1) % n].map(|v| v as f32));
            }
        }
        if !segments.is_empty() {
            preview.lines.push(ViewportLineLayer {
                color,
                width,
                segments: segments.into(),
                ..default()
            });
        }
    }
    Some(preview)
}

pub(super) fn target_matches(world: &World, path: &str) -> bool {
    world
        .get_resource::<State>()
        .and_then(|state| state.points.as_ref())
        .is_none_or(|session| points::target_matches(session, path))
}
