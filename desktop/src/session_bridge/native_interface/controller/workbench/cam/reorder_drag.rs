//! Pointer ordering is transient UI state. Only a completed, changed drop
//! submits the existing whole-document permutation to CAM validation/history.
use super::*;
use crate::native_viewport::winit_host::NativeHostInput;
use bevy::input::{keyboard::Key, ButtonState};
use limo_cad_interface::{ControlKey, Rect};
use std::time::Instant;
use workspace::DocumentReceipt;

const PAGE_INTERVAL: Duration = Duration::from_millis(120);
const THRESHOLD: f32 = 5.;

struct Drag {
    receipt: DocumentReceipt,
    action: NativeInterfaceAction,
    selection: Selection,
    tab: Tab,
    scope: Option<u64>,
    scope_start: usize,
    ids: Vec<u64>,
    label: String,
    start: Vec2,
    cursor: Option<Vec2>,
    moved: bool,
    target: Option<(usize, f32)>,
    last_page: Instant,
}
#[derive(Resource, Default)]
struct State {
    drag: Option<Drag>,
    cadence: Option<bevy::winit::UpdateMode>,
}
/// Pointer ownership survives releasing the ordinary button capture at the
/// drag threshold. Other navigation devices must also consult this fence.
pub(crate) fn active(world: &World) -> bool {
    world
        .get_resource::<State>()
        .is_some_and(|state| state.drag.is_some())
}

fn cadence(world: &mut World, active: bool) {
    world.init_resource::<State>();
    world.resource_scope(|world, mut state: Mut<State>| {
        let Some(mut settings) = world.get_resource_mut::<bevy::winit::WinitSettings>() else {
            return;
        };
        if active && state.cadence.is_none() {
            state.cadence = Some(settings.focused_mode);
            if let bevy::winit::UpdateMode::Reactive { wait, .. } = &mut settings.focused_mode {
                *wait = (*wait).min(PAGE_INTERVAL);
            }
        } else if !active {
            if let Some(previous) = state.cadence.take() {
                settings.focused_mode = previous;
            }
        }
    });
}
pub(crate) fn cancel(world: &mut World, handle: &NativeInterfaceHandle) {
    let active = active(world);
    if let Some(mut state) = world.get_resource_mut::<State>() {
        state.drag = None;
    }
    cadence(world, false);
    if active {
        crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
        handle.invalidate_presentation();
    }
}

fn permitted(world: &World, handle: &NativeInterfaceHandle) -> bool {
    workspace(world) == Workspace::Cam
        && !worker::busy(world)
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
    receipt: &DocumentReceipt,
) -> Result<(), String> {
    if !permitted(world, handle) {
        return Err("CAM ordering was cancelled because the workspace changed".into());
    }
    let editor = world
        .get_resource::<Editor>()
        .ok_or("Open the CAM workspace")?;
    ensure_current(editor, &receipt.owner, receipt.revision)?;
    if editor.draft.as_ref().is_some_and(Draft::dirty) {
        return Err("Apply or reset the current CAM edit first".into());
    }
    if handle
        .frame()
        .is_none_or(|frame| frame.context != receipt.owner)
        || services
            .bridge
            .native_document_receipt(&services.engine, &receipt.owner)?
            != *receipt
    {
        return Err("CAM changed; start the drag again".into());
    }
    Ok(())
}
fn selected_key(world: &World, key: ControlKey) -> Option<Selection> {
    match world
        .get::<NativeCommandBinding>(Entity::from_bits(key.0))?
        .command
    {
        NativeCommand::Cam(Command::Select(
            selection @ (Selection::Setup(_) | Selection::Operation(_)),
        )) => Some(selection),
        _ => None,
    }
}
fn id(selection: Selection) -> u64 {
    match selection {
        Selection::Setup(id) | Selection::Operation(id) | Selection::Tool(id) => id,
    }
}
fn ids(cam: &CamDocumentDto, scope: Option<u64>) -> Vec<u64> {
    match scope {
        None => cam.setups.iter().map(|s| s.id).collect(),
        Some(scope) => cam
            .setup(scope)
            .map(|s| s.operations.iter().map(CamOperationDto::id).collect())
            .unwrap_or_default(),
    }
}
fn in_scope(selection: Selection, drag: &Drag) -> bool {
    matches!(
        (selection, drag.selection),
        (Selection::Setup(_), Selection::Setup(_))
            | (Selection::Operation(_), Selection::Operation(_))
    ) && drag.ids.contains(&id(selection))
}
fn slots(
    world: &World,
    handle: &NativeInterfaceHandle,
    drag: &Drag,
) -> Result<Vec<(usize, Rect)>, String> {
    handle.read_surface(|_, frame| {
        let mut rows = frame
            .controls
            .iter()
            .filter_map(|control| {
                let selection = selected_key(world, control.key)?;
                if !control.visible || control.disabled || !in_scope(selection, drag) {
                    return None;
                }
                Some((
                    drag.ids.iter().position(|id_| *id_ == id(selection))?,
                    control.bounds,
                ))
            })
            .collect::<Vec<_>>();
        rows.sort_by_key(|(index, _)| *index);
        rows
    })
}
fn target(world: &World, handle: &NativeInterfaceHandle, drag: &Drag) -> Option<(usize, f32)> {
    let cursor = drag.cursor.filter(|c| c.is_finite())?;
    let key = handle.hit_key(cursor.as_dvec2().to_array())?;
    let selection = selected_key(world, key).filter(|selection| in_scope(*selection, drag))?;
    let index = drag.ids.iter().position(|id_| *id_ == id(selection))?;
    let editor = world.get_resource::<Editor>()?;
    if editor.tab != drag.tab || (drag.scope_start + index) / 3 != editor.page {
        return None;
    }
    handle
        .read_surface(|_, frame| {
            let bounds = frame.controls.iter().find(|row| row.key == key)?.bounds;
            let after = f64::from(cursor.y) >= bounds.y + bounds.height * 0.5;
            Some((
                index + usize::from(after),
                (bounds.y + if after { bounds.height } else { 0. }) as f32,
            ))
        })
        .ok()
        .flatten()
}
pub(super) fn ordered(ids: &[u64], selected: u64, slot: usize) -> Option<Vec<u64>> {
    let source = ids.iter().position(|id| *id == selected)?;
    if slot > ids.len() || slot == source || slot == source + 1 {
        return None;
    }
    let mut result = ids.to_vec();
    result.remove(source);
    result.insert(slot - usize::from(slot > source), selected);
    Some(result)
}
fn commit(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    receipt: &DocumentReceipt,
    selection: Selection,
    scope: Option<u64>,
    ids: &[u64],
) -> Result<(), String> {
    current(world, handle, services, receipt)?;
    let next = reorder::apply_order(&world.resource::<Editor>().cam, scope, ids)?;
    world.resource_mut::<Editor>().pending_selection = Some(selection);
    submit(
        world,
        &services.engine,
        &services.bridge,
        receipt,
        "cam_set_document",
        serde_json::to_value(next).map_err(|error| error.to_string())?,
        || Ok(()),
    )?;
    handle.invalidate_presentation();
    Ok(())
}

pub(crate) fn input(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    event: &NativeHostInput,
) -> Result<bool, String> {
    let result = input_inner(world, handle, services, event);
    if let Err(error) = &result {
        cancel(world, handle);
        crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
        if let Some(mut editor) = world.get_resource_mut::<Editor>() {
            editor.message = error.clone();
        }
    }
    result
}
fn input_inner(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    event: &NativeHostInput,
) -> Result<bool, String> {
    let active = world
        .get_resource_mut::<State>()
        .and_then(|mut state| state.drag.take());
    if let Some(mut drag) = active {
        let escape = matches!(&event.event, WindowEvent::KeyboardInput(k) if k.state == ButtonState::Pressed && k.logical_key == Key::Escape);
        let cancelled = escape
            || matches!(
                &event.event,
                WindowEvent::WindowFocused(bevy::window::WindowFocused { focused: false, .. })
                    | WindowEvent::KeyboardFocusLost(_)
                    | WindowEvent::WindowCloseRequested(_)
                    | WindowEvent::WindowDestroyed(_)
                    | WindowEvent::WindowResized(_)
                    | WindowEvent::WindowScaleFactorChanged(_)
                    | WindowEvent::WindowBackendScaleFactorChanged(_)
            );
        if cancelled {
            cadence(world, false);
            crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
            handle.invalidate_presentation();
            return Ok(escape);
        }
        if event.context.as_ref() != Some(&drag.receipt.owner) {
            return Err("CAM changed; start the drag again".into());
        }
        current(world, handle, services, &drag.receipt)?;
        if world.resource::<Editor>().tab != drag.tab {
            return Err("CAM list changed; start the drag again".into());
        }
        let release = matches!(&event.event, WindowEvent::MouseButtonInput(k) if k.button == MouseButton::Left && k.state == ButtonState::Released);
        let motion = matches!(
            event.event,
            WindowEvent::CursorMoved(_) | WindowEvent::CursorLeft(_)
        );
        if !release && !motion {
            world.resource_mut::<State>().drag = Some(drag);
            return Ok(false);
        }
        drag.cursor = if matches!(event.event, WindowEvent::CursorLeft(_)) {
            None
        } else {
            event.cursor
        };
        if !drag.moved
            && drag
                .cursor
                .is_some_and(|c| c.distance(drag.start) >= THRESHOLD)
        {
            handle.validate_action(&drag.action)?;
            drag.moved = true;
            crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
        }
        if drag.moved {
            drag.target = target(world, handle, &drag);
        }
        if release {
            cadence(world, false);
            if !drag.moved {
                return Ok(false);
            }
            crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
            handle.invalidate_presentation();
            if let Some(next) = drag
                .target
                .and_then(|(slot, _)| ordered(&drag.ids, id(drag.selection), slot))
            {
                commit(
                    world,
                    handle,
                    services,
                    &drag.receipt,
                    drag.selection,
                    drag.scope,
                    &next,
                )?;
            }
            return Ok(true);
        }
        let moved = drag.moved;
        world.resource_mut::<State>().drag = Some(drag);
        if moved {
            handle.invalidate_presentation();
        }
        return Ok(moved);
    }
    if !permitted(world, handle) {
        return Ok(false);
    }
    if let WindowEvent::KeyboardInput(key) = &event.event {
        if key.state == ButtonState::Pressed
            && event.modifiers.alt
            && !event.modifiers.alt_graph
            && !event.modifiers.ctrl
            && !event.modifiers.meta
            && !event.modifiers.shift
        {
            let delta = match key.logical_key {
                Key::ArrowUp => -1,
                Key::ArrowDown => 1,
                _ => return Ok(false),
            };
            let Some(focused) = handle.focused_key() else {
                return Ok(false);
            };
            let Some(selection) = selected_key(world, focused) else {
                return Ok(false);
            };
            let action = handle.resolve_retained(focused)?;
            if event.context.as_ref() != Some(&action.context) {
                return Ok(false);
            }
            handle.validate_action(&action)?;
            let receipt = services
                .bridge
                .native_document_receipt(&services.engine, &action.context)?;
            current(world, handle, services, &receipt)?;
            let cam = &world.resource::<Editor>().cam;
            if !reorder::can_step(cam, selection, delta) {
                return Ok(true);
            }
            let (scope, _, _) = reorder::position(cam, selection)?;
            let next = reorder::step(cam, selection, delta)?;
            commit(
                world,
                handle,
                services,
                &receipt,
                selection,
                scope,
                &ids(&next, scope),
            )?;
            return Ok(true);
        }
    }
    if !matches!(&event.event, WindowEvent::MouseButtonInput(k) if k.button == MouseButton::Left && k.state == ButtonState::Pressed)
    {
        return Ok(false);
    }
    let Some(cursor) = event.cursor else {
        return Ok(false);
    };
    let Some(key) = handle.hit_key(cursor.as_dvec2().to_array()) else {
        return Ok(false);
    };
    let Some(selection) = selected_key(world, key) else {
        return Ok(false);
    };
    let action = handle.resolve_retained(key)?;
    if event.context.as_ref() != Some(&action.context) {
        return Ok(false);
    }
    let receipt = services
        .bridge
        .native_document_receipt(&services.engine, &action.context)?;
    current(world, handle, services, &receipt)?;
    let cam = &world.resource::<Editor>().cam;
    let (scope, _, count) = reorder::position(cam, selection)?;
    if count < 2 {
        return Ok(false);
    }
    let drag = Drag {
        receipt,
        action,
        selection,
        tab: world.resource::<Editor>().tab,
        scope,
        scope_start: scope.map_or(0, |id| {
            cam.setups
                .iter()
                .take_while(|s| s.id != id)
                .map(|s| s.operations.len())
                .sum()
        }),
        ids: ids(cam, scope),
        label: rows(cam, world.resource::<Editor>().tab)
            .into_iter()
            .find(|(s, _)| *s == selection)
            .map(|(_, label)| label)
            .unwrap_or_default(),
        start: cursor,
        cursor: Some(cursor),
        moved: false,
        target: None,
        last_page: Instant::now(),
    };
    world.init_resource::<State>();
    world.resource_mut::<State>().drag = Some(drag);
    Ok(false)
}

pub(crate) fn tick(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
) -> Result<(), String> {
    tick_at(world, handle, services, Instant::now())
}
pub(super) fn tick_at(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    now: Instant,
) -> Result<(), String> {
    let Some(mut drag) = world
        .get_resource_mut::<State>()
        .and_then(|mut state| state.drag.take())
    else {
        cadence(world, false);
        return Ok(());
    };
    if current(world, handle, services, &drag.receipt).is_err()
        || world.resource::<Editor>().tab != drag.tab
    {
        cadence(world, false);
        crate::native_viewport::winit_host::cancel_native_pointer(world, handle);
        handle.invalidate_presentation();
        return Ok(());
    }
    let mut direction = 0isize;
    if drag.moved {
        drag.target = target(world, handle, &drag);
        if let Some(cursor) = drag.cursor.filter(|_| drag.target.is_some()) {
            let visible = slots(world, handle, &drag)?;
            let editor = world.resource::<Editor>();
            let first = drag.scope_start / 3;
            let last = (drag.scope_start + drag.ids.len() - 1) / 3;
            if editor.page > first
                && visible
                    .first()
                    .is_some_and(|(_, r)| f64::from(cursor.y) < r.y + 8.)
            {
                direction = -1;
            } else if editor.page < last
                && visible
                    .last()
                    .is_some_and(|(_, r)| f64::from(cursor.y) > r.y + r.height - 8.)
            {
                direction = 1;
            }
        }
    }
    if direction != 0 && now.saturating_duration_since(drag.last_page) >= PAGE_INTERVAL {
        let page = world
            .resource::<Editor>()
            .page
            .saturating_add_signed(direction);
        world.resource_mut::<Editor>().page = page;
        drag.last_page = now;
        drag.target = None;
        handle.invalidate_presentation();
    }
    world.resource_mut::<State>().drag = Some(drag);
    cadence(world, direction != 0);
    Ok(())
}

pub(super) fn paint(world: &mut World, camera: Entity, widgets: &mut Widgets, width: f32) {
    let display = world
        .get_resource::<State>()
        .and_then(|state| state.drag.as_ref())
        .filter(|drag| drag.moved)
        .map(|drag| (drag.target, drag.cursor, drag.label.clone()));
    let Some((target, cursor, label)) = display else {
        return;
    };
    let theme = crate::native_viewport::ui::theme(world);
    if let Some((_, y)) = target {
        widgets.panel(
            world,
            camera,
            "cam-drop-slot",
            rect(10., y - 1., width - 20., 2.),
            theme.accent,
            48,
        );
        world
            .entity_mut(widgets.entity("cam-drop-slot").unwrap())
            .remove::<interface_shell::InterfaceOccluder>();
    }
    if let Some(cursor) = cursor {
        widgets.panel(
            world,
            camera,
            "cam-drag-ghost",
            rect(14., cursor.y + 12., width - 28., 26.),
            theme.panel,
            49,
        );
        world
            .entity_mut(widgets.entity("cam-drag-ghost").unwrap())
            .remove::<interface_shell::InterfaceOccluder>();
        widgets.text(
            world,
            camera,
            "cam-drag-label",
            rect(20., cursor.y + 16., width - 40., 18.),
            &label,
            11.,
            50,
        );
        world
            .get_mut::<Node>(widgets.entity("cam-drag-label").unwrap())
            .unwrap()
            .overflow = Overflow::clip();
    }
}
