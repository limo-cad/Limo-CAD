//! Drill/thread use the same transient picker owner and worker slot as chains.
//! Pointer input queues only a camera ray; physical mesh work stays off-thread.
use super::*;
use crate::native_viewport::{physical_pick, ViewportTriangleLayer};
pub(super) use hole_picking::FaceKey;
use operation_geometry::hole_picking;

#[path = "holes_worker.rs"]
mod resolver;

pub(super) struct Candidate {
    pub key: FaceKey,
    pub triangles: Vec<f32>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Request {
    pub id: u64,
    pub ray: physical_pick::Ray,
    pub click: bool,
}
pub(super) struct Session {
    receipt: DocumentReceipt,
    selection: hole_picking::SelectionState,
    geometry: physical_pick::Snapshot,
    source_stamp: [u32; 2],
    candidates: Vec<Candidate>,
    pub loaded: bool,
    projection: Option<(native_viewport::ViewportCamera, InterfaceRect)>,
    hover: Option<FaceKey>,
    next_hover: Option<Request>,
    last_hover: u64,
    serial: u64,
    pub pending: Option<Request>,
    pub clicks: VecDeque<Request>,
    captured: bool,
}
pub(super) fn start(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    receipt: &DocumentReceipt,
    draft: &Draft,
) -> Result<Value, String> {
    let selection = hole_picking::snapshot(draft)?;
    let context = operation_editor::geometry(draft).ok_or("Reopen the geometry editor")?;
    if !context.scene.errors.is_empty() {
        return Err("Resolve model errors before picking geometry".into());
    }
    let geometry = geometry_snapshot(world)?;
    if geometry.owner != receipt.owner.document_id {
        return Err("The CAD viewport changed document".into());
    }
    let worker = resolver::start(context, geometry.clone(), handle.clone())?;
    let source_stamp = native_viewport::interface_navigation_source(world).0;
    let mut state = world.resource_mut::<State>();
    debug_assert!(state.session.is_none() && state.holes.is_none() && state.worker.is_none());
    state.worker = Some(worker);
    state.holes = Some(Session {
        receipt: receipt.clone(),
        selection,
        geometry,
        source_stamp,
        candidates: Vec::new(),
        loaded: false,
        projection: None,
        hover: None,
        next_hover: None,
        last_hover: 0,
        serial: 0,
        pending: None,
        clicks: VecDeque::new(),
        captured: false,
    });
    crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
    handle.blur();
    changed(world, handle);
    Ok(json!({"picking":true,"committed":false}))
}
fn current(
    world: &World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    session: &mut Session,
) -> Result<(), String> {
    if !permitted(world, handle) {
        return Err("CAM picking ended because the workspace changed".into());
    }
    let editor = world
        .get_resource::<Editor>()
        .ok_or("Open the CAM editor")?;
    ensure_current(editor, &session.receipt.owner, session.receipt.revision)?;
    let draft = editor.draft.as_ref().ok_or("Open the geometry editor")?;
    let source_stamp = native_viewport::interface_navigation_source(world).0;
    if source_stamp != session.source_stamp {
        if geometry_snapshot(world)? != session.geometry {
            return Err("Visible geometry changed; start picking again".into());
        }
        session.source_stamp = source_stamp;
    }
    if !hole_picking::unchanged(draft, &session.selection)?
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
fn geometry_snapshot(world: &World) -> Result<physical_pick::Snapshot, String> {
    let hidden = super::super::super::cam_view::geometry_hidden_bodies(world)?;
    physical_pick::snapshot(world, &hidden)
}
fn projection(
    world: &World,
    handle: &NativeInterfaceHandle,
    session: &mut Session,
) -> Result<(native_viewport::ViewportCamera, InterfaceRect), String> {
    let bounds = viewport(handle).ok_or("The CAD viewport is unavailable")?;
    let (owner, camera) = native_viewport::interface_camera_snapshot(world);
    if owner != session.receipt.owner.document_id {
        return Err("The CAD viewport changed document".into());
    }
    if session
        .projection
        .is_some_and(|old| old != (camera, bounds))
    {
        if session
            .pending
            .as_ref()
            .is_some_and(|request| request.click)
            || !session.clicks.is_empty()
        {
            return Err("The view changed during the pick; start picking again".into());
        }
        session.hover = None;
        session.next_hover = None;
        session.last_hover = 0;
    }
    session.projection = Some((camera, bounds));
    Ok((camera, bounds))
}
fn pump(session: &mut Session, worker: &worker::Worker) -> Result<(), String> {
    if !session.loaded || session.pending.is_some() {
        return Ok(());
    }
    if let Some(request) = session
        .clicks
        .pop_front()
        .or_else(|| session.next_hover.take())
    {
        worker.face(request)?;
        session.pending = Some(request);
    }
    Ok(())
}
fn stage(world: &mut World, session: &mut Session, key: FaceKey) -> Result<(), String> {
    let mut editor = world
        .remove_resource::<Editor>()
        .ok_or("Open the CAM editor")?;
    let result = (|| {
        let draft = editor.draft.as_mut().ok_or("Open the geometry editor")?;
        session.selection = hole_picking::stage(draft, &editor.cam, &session.selection, key)?;
        editor.message =
            "Hole selection is staged. Done picking keeps the draft; Apply saves it.".into();
        Ok(())
    })();
    world.insert_resource(editor);
    result
}
fn finish(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    mut state: State,
    result: &Result<bool, String>,
) {
    if let Err(error) = result {
        stop(&mut state);
        if let Some(mut editor) = world.get_resource_mut::<Editor>() {
            editor.message = error.clone();
        }
    }
    world.insert_resource(state);
    if result.as_ref().is_ok_and(|changed| *changed) || result.is_err() {
        changed(world, handle);
    }
}
pub(super) fn input(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    event: &NativeHostInput,
) -> Result<bool, String> {
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
            .holes
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
    let mut consumed = false;
    let result = (|| {
        let session = state.holes.as_mut().unwrap();
        current(world, handle, services, session)?;
        if event.consumed {
            return Ok(false);
        }
        let release = matches!(&event.event, WindowEvent::MouseButtonInput(button) if button.button == MouseButton::Left && button.state == ButtonState::Released);
        if release && session.captured {
            session.captured = false;
            consumed = true;
            return Ok(false);
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
        if !press {
            consumed =
                crate::session_bridge::native_interface::view::navigate(world, handle, event)?;
        }
        let (camera, bounds) = projection(world, handle, session)?;
        if matches!(event.event, WindowEvent::CursorLeft(_))
            || !event
                .cursor
                .is_some_and(|cursor| canvas_pointer(handle, cursor))
        {
            session.hover = None;
            session.next_hover = None;
            session.last_hover = 0;
            return Ok(true);
        }
        consumed = true;
        let text_focused = handle
            .focused_key()
            .and_then(|key| world.get::<InterfaceControl>(Entity::from_bits(key.0)))
            .is_some_and(|control| matches!(control.field, Field::Text { .. }));
        if text_focused {
            if press {
                crate::native_viewport::interface_shell::fields::before_window_input(
                    world,
                    handle,
                    &event.event,
                    event.cursor,
                    event.modifiers,
                )?;
                handle.blur();
                session.captured = true;
            }
            return Ok(false);
        }
        let cursor = event.cursor.unwrap();
        session.serial = session.serial.wrapping_add(1).max(1);
        let request = Request {
            id: session.serial,
            click: press,
            ray: physical_pick::Ray {
                camera,
                viewport: [bounds.width as f32, bounds.height as f32],
                point: [cursor.x - bounds.x as f32, cursor.y - bounds.y as f32],
            },
        };
        if press {
            if session.clicks.len() >= 16 {
                return Err("Finish the queued hole picks before selecting more".into());
            }
            session.clicks.push_back(request);
            session.captured = true;
            crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
        } else {
            session.last_hover = request.id;
            session.next_hover = Some(request);
        }
        pump(
            session,
            state.worker.as_ref().ok_or("Geometry resolver stopped")?,
        )?;
        Ok(false)
    })();
    finish(world, handle, state, &result);
    result.map(|_| consumed)
}
pub(super) fn tick(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
) -> Result<(), String> {
    let mut state = world.remove_resource::<State>().unwrap();
    let result = (|| {
        let session = state.holes.as_mut().unwrap();
        current(world, handle, services, session)?;
        projection(world, handle, session)?;
        let mut updated = false;
        loop {
            match state
                .worker
                .as_ref()
                .ok_or("Geometry resolver stopped")?
                .receive()
            {
                Ok(worker::ResultMessage::Holes(candidates)) => {
                    session.candidates = candidates?;
                    session.loaded = true;
                    updated = true;
                }
                Ok(worker::ResultMessage::Face(request, result)) => {
                    if session.pending != Some(request) {
                        return Err("The pending hole pick changed".into());
                    }
                    session.pending = None;
                    let key = result?;
                    if request.click {
                        if let Some(key) = key {
                            stage(world, session, key)?;
                        }
                        updated = true;
                    } else if request.id == session.last_hover {
                        session.hover = key;
                        updated = true;
                    }
                }
                Ok(_) => return Err("The geometry picker target changed".into()),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err("Geometry preview worker stopped".into())
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
            }
        }
        pump(session, state.worker.as_ref().unwrap())?;
        Ok(updated)
    })();
    finish(world, handle, state, &result);
    result.map(|_| ())
}
pub(super) fn overlay(session: &Session) -> ViewportPreview {
    let mut preview = ViewportPreview::default();
    let selected: std::collections::HashSet<_> = session.selection.keys.iter().copied().collect();
    for hover in [false, true] {
        let mut positions = Vec::new();
        for candidate in &session.candidates {
            if if hover {
                session.hover == Some(candidate.key)
            } else {
                selected.contains(&candidate.key) && session.hover != Some(candidate.key)
            } {
                positions.extend_from_slice(&candidate.triangles);
            }
        }
        if !positions.is_empty() {
            preview.triangles.push(ViewportTriangleLayer {
                positions: positions.into(),
                color: if hover {
                    [1., 0.67, 0.17, 0.5]
                } else {
                    [0.2, 0.85, 0.45, 0.45]
                },
                ..default()
            });
        }
    }
    preview
}
