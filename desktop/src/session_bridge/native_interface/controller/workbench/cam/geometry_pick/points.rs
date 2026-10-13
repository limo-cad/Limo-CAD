//! WCS and linking handles share one projected-point owner and worker slot.
use super::point_target as adapter;
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Mutex,
};

pub(super) struct Session {
    receipt: DocumentReceipt,
    selection: adapter::SelectionState,
    source_stamp: u32,
    model_revision: u64,
    candidates: Vec<adapter::Candidate>,
    pub loaded: bool,
    projection: Option<(native_viewport::ViewportCamera, InterfaceRect)>,
    projected: Vec<Option<Vec2>>,
    hover: Option<usize>,
    pressed: Option<usize>,
    pub captured: bool,
    cursor: Option<Vec2>,
}
pub(super) fn start(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    receipt: &DocumentReceipt,
    draft: &Draft,
    cam: &CamDocumentDto,
    path: &str,
) -> Result<Value, String> {
    let selection = adapter::snapshot(draft, path)?;
    let source = adapter::source(draft, cam, &selection)?;
    let (sender, requests) = mpsc::sync_channel::<worker::Request>(1);
    let (send, receiver) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancel = cancelled.clone();
    let wake = handle.clone();
    std::thread::Builder::new()
        .name("cad-native-point-pick".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                adapter::candidates(source)
            }))
            .unwrap_or_else(|_| Err("Point preview worker stopped unexpectedly".into()));
            if cancel.load(Ordering::Acquire) {
                return;
            }
            if send.send(worker::ResultMessage::Points(result)).is_ok() {
                wake.request_redraw();
                let _ = requests.recv();
            }
        })
        .map_err(|error| error.to_string())?;
    let source_stamp = native_viewport::interface_navigation_source(world).0[0];
    let model_revision = native_viewport::interface_model_revision(world);
    let mut state = world.resource_mut::<State>();
    state.worker = Some(worker::Worker {
        sender: Some(sender),
        receiver: Mutex::new(receiver),
        cancelled,
    });
    state.points = Some(Session {
        receipt: receipt.clone(),
        selection,
        source_stamp,
        model_revision,
        candidates: vec![],
        loaded: false,
        projection: None,
        projected: vec![],
        hover: None,
        pressed: None,
        captured: false,
        cursor: None,
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
    session: &Session,
) -> Result<(), String> {
    if !permitted(world, handle) {
        return Err("Point picking ended because the workspace changed".into());
    }
    let editor = world
        .get_resource::<Editor>()
        .ok_or("Open the CAM editor")?;
    ensure_current(editor, &session.receipt.owner, session.receipt.revision)?;
    let draft = editor.draft.as_ref().ok_or("Open the point editor")?;
    if adapter::current(draft, &session.selection)? != session.selection
        || native_viewport::interface_navigation_source(world).0[0] != session.source_stamp
        || native_viewport::interface_model_revision(world) != session.model_revision
        || handle
            .frame()
            .is_none_or(|frame| frame.context != session.receipt.owner)
        || services
            .bridge
            .native_document_receipt(&services.engine, &session.receipt.owner)?
            != session.receipt
    {
        return Err("Point source or draft changed; start picking again".into());
    }
    Ok(())
}
fn project(
    world: &World,
    handle: &NativeInterfaceHandle,
    session: &mut Session,
) -> Result<bool, String> {
    let bounds = viewport(handle).ok_or("The CAD viewport is unavailable")?;
    let (owner, camera) = native_viewport::interface_camera_snapshot(world);
    if owner != session.receipt.owner.document_id {
        return Err("The CAD viewport changed document".into());
    }
    if session.projection == Some((camera, bounds)) {
        return Ok(false);
    }
    if session.captured {
        return Err("The view changed during the pick; start picking again".into());
    }
    session.projected = session
        .candidates
        .iter()
        .map(|candidate| {
            native_viewport::interface_world_point(world, &owner, candidate.point).map(|point| {
                point.map(|p| Vec2::new(p[0] + bounds.x as f32, p[1] + bounds.y as f32))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    session.projection = Some((camera, bounds));
    Ok(true)
}
fn closest(projected: &[Option<Vec2>], cursor: Vec2) -> Option<usize> {
    let mut best = 16.0f32 * 16.;
    let mut hit = None;
    for (index, point) in projected.iter().enumerate() {
        if let Some(point) = point {
            let distance = point.distance_squared(cursor);
            if distance <= best {
                best = distance;
                hit = Some(index);
            }
        }
    }
    hit
}
fn hover(handle: &NativeInterfaceHandle, session: &Session) -> Option<usize> {
    if matches!(session.selection, adapter::SelectionState::Height(_)) {
        let cursor = session
            .cursor
            .filter(|cursor| canvas_pointer(handle, *cursor))?;
        let (camera, _) = session.projection?;
        let origin = Vec3::from_array(camera.position).as_dvec3();
        let forward = (Vec3::from_array(camera.target).as_dvec3() - origin).normalize_or_zero();
        let mut best = 16. * 16.;
        let mut depth = f64::INFINITY;
        let mut hit = None;
        for (index, (candidate, point)) in session
            .candidates
            .iter()
            .zip(&session.projected)
            .enumerate()
        {
            let Some(point) = point else {
                continue;
            };
            let distance = point.distance_squared(cursor);
            let next_depth = (DVec3::from_array(candidate.point) - origin).dot(forward);
            if next_depth > 0.
                && distance <= 16. * 16.
                && (distance < best - 0.01
                    || ((distance - best).abs() <= 0.01 && next_depth < depth))
            {
                best = distance;
                depth = next_depth;
                hit = Some(index);
            }
        }
        return hit;
    }
    session
        .cursor
        .filter(|cursor| canvas_pointer(handle, *cursor))
        .and_then(|cursor| closest(&session.projected, cursor))
}
fn finish(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    mut state: State,
    result: &Result<bool, String>,
) {
    if let Err(error) = result {
        stop(&mut state);
        crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
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
            .points
            .as_ref()
            .is_some_and(|session| event.context.as_ref() != Some(&session.receipt.owner))
    {
        return Ok(false);
    }
    let escape = matches!(&event.event, WindowEvent::KeyboardInput(key) if !event.consumed && key.state == ButtonState::Pressed && key.logical_key == Key::Escape);
    if lifecycle || escape {
        crate::session_bridge::native_interface::view::navigate(world, handle, event)?;
        cancel(world, handle);
        return Ok(escape);
    }
    let mut state = world.remove_resource::<State>().unwrap();
    let mut consumed = false;
    let result = (|| {
        let session = state.points.as_mut().unwrap();
        current(world, handle, services, session)?;
        if event.consumed {
            return Ok(false);
        }
        let press = matches!(&event.event, WindowEvent::MouseButtonInput(button) if button.button == MouseButton::Left && button.state == ButtonState::Pressed);
        let release = matches!(&event.event, WindowEvent::MouseButtonInput(button) if button.button == MouseButton::Left && button.state == ButtonState::Released);
        if !press
            && !release
            && !matches!(
                event.event,
                WindowEvent::CursorMoved(_) | WindowEvent::CursorLeft(_)
            )
        {
            return Ok(false);
        }
        if !press && !release {
            consumed =
                crate::session_bridge::native_interface::view::navigate(world, handle, event)?;
        }
        let mut updated = project(world, handle, session)?;
        let released_outside = release && session.captured && session.cursor.is_none();
        session.cursor = event
            .cursor
            .filter(|_| !released_outside && !matches!(event.event, WindowEvent::CursorLeft(_)));
        let next_hover = hover(handle, session);
        let hover_changed = session.hover != next_hover;
        updated |= hover_changed;
        session.hover = next_hover;
        if hover_changed {
            show_hover(world, session);
        }
        if release && session.captured {
            consumed = true;
            session.captured = false;
            if let Some(index) = session
                .pressed
                .take()
                .filter(|index| session.hover == Some(*index))
            {
                let key = &session.candidates[index].key;
                let mut editor = world.resource_mut::<Editor>();
                let Editor { draft, cam, .. } = &mut *editor;
                let draft = draft.as_mut().ok_or("Open the point editor")?;
                adapter::stage(draft, cam, &session.selection, key)?;
                editor.message = adapter::staged_message(&session.selection).into();
                stop(&mut state);
                crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
            }
            return Ok(true);
        }
        if press
            && event
                .cursor
                .is_some_and(|cursor| canvas_pointer(handle, cursor))
        {
            consumed = true;
            let text_focused = handle
                .focused_key()
                .and_then(|key| world.get::<InterfaceControl>(Entity::from_bits(key.0)))
                .is_some_and(|control| matches!(control.field, Field::Text { .. }));
            if text_focused {
                crate::native_viewport::interface_shell::fields::before_window_input(
                    world,
                    handle,
                    &event.event,
                    event.cursor,
                    event.modifiers,
                )?;
                handle.blur();
                session.pressed = None;
            } else {
                session.pressed = session.hover;
            }
            session.captured = true;
            updated = true;
            crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
        } else if session
            .cursor
            .is_some_and(|cursor| canvas_pointer(handle, cursor))
            || matches!(event.event, WindowEvent::CursorLeft(_))
        {
            consumed = true;
        }
        Ok(updated)
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
        let session = state.points.as_mut().unwrap();
        current(world, handle, services, session)?;
        let mut updated = false;
        loop {
            match state
                .worker
                .as_ref()
                .ok_or("Point preview worker stopped")?
                .receive()
            {
                Ok(worker::ResultMessage::Points(result)) => {
                    session.candidates = result?;
                    session.loaded = true;
                    session.projection = None;
                    updated = true;
                }
                Ok(_) => return Err("The geometry picker target changed".into()),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("Point preview worker stopped".into())
                }
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
        updated |= project(world, handle, session)?;
        let next = hover(handle, session);
        let hover_changed = next != session.hover;
        updated |= hover_changed;
        session.hover = next;
        if hover_changed {
            show_hover(world, session);
        }
        Ok(updated)
    })();
    finish(world, handle, state, &result);
    result.map(|_| ())
}
pub(super) fn overlay(session: &Session) -> ViewportPreview {
    let mut preview = ViewportPreview::default();
    let radius = session
        .projection
        .map(|(camera, bounds)| {
            let distance =
                Vec3::from_array(camera.position).distance(Vec3::from_array(camera.target));
            2. * distance * (camera.vertical_fov_degrees.to_radians() * 0.5).tan() * 5.
                / bounds.height.max(1.) as f32
        })
        .unwrap_or(0.3);
    for selected in [false, true] {
        let positions: Vec<f32> = session
            .candidates
            .iter()
            .enumerate()
            .filter(|(index, _)| (Some(*index) == session.hover) == selected)
            .flat_map(|(_, point)| point.point.map(|v| v as f32))
            .collect();
        if !positions.is_empty() {
            preview.points.push(native_viewport::ViewportPointLayer {
                positions: positions.into(),
                radius: radius * if selected { 1.35 } else { 1. },
                hollow: !selected,
                color: if selected {
                    [1., 0.67, 0.17, 1.]
                } else {
                    [0.2, 0.85, 0.45, 0.95]
                },
                ..default()
            });
        }
    }
    preview
}

fn show_hover(world: &mut World, session: &Session) {
    if let Some(label) = session
        .hover
        .and_then(|index| adapter::label(&session.candidates[index].key))
    {
        if let Some(mut editor) = world.get_resource_mut::<Editor>() {
            editor.message = format!("Height geometry: {label}. Click to stage; Apply saves.");
        }
    }
}

pub(super) fn target_matches(session: &Session, path: &str) -> bool {
    adapter::target_matches(&session.selection, path)
}

#[cfg(test)]
mod hit_tests {
    use super::*;
    #[test]
    fn logical_point_hit_has_stable_ties_and_a_finite_sixteen_pixel_boundary() {
        let points = [
            Some(Vec2::ZERO),
            None,
            Some(Vec2::ZERO),
            Some(Vec2::new(40., 0.)),
        ];
        assert_eq!(closest(&points, Vec2::ZERO), Some(2));
        assert_eq!(closest(&points, Vec2::new(16., 0.)), Some(2));
        assert_eq!(closest(&points, Vec2::new(20., 0.)), None);
        assert_eq!(closest(&points, Vec2::splat(f32::NAN)), None);
    }
}
