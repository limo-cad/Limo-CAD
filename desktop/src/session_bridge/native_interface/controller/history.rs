//! Parametric history controls use the same replay operations as MCP.
use super::chrome::{rect, Widgets};
use super::*;
use interface_shell::{fields, ribbon::Icon};
use limo_cad_core::{DocumentDto, Feature, FeatureKind};
use limo_cad_interface::{ControlInput, KeyChord};
use workspace::DocumentReceipt;
mod drag;
mod panel;
#[cfg(test)]
mod tests;
pub(super) use drag::{cancel_drag, pointer, tick};
pub(super) use panel::synchronize;

/// Route document shortcuts through the same visible, enabled Undo/Redo
/// controls as a click. Text editors and modal scopes keep their own keys.
pub(super) fn shortcut_action(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    event: &NativeHostInput,
) -> Result<Option<NativeInterfaceAction>, String> {
    use bevy::input::{keyboard::Key, ButtonState};
    let WindowEvent::KeyboardInput(key) = &event.event else {
        return Ok(None);
    };
    let command = if cfg!(target_os = "macos") {
        event.modifiers.meta
    } else {
        event.modifiers.ctrl
    };
    if event.consumed
        || !command
        || event.modifiers.alt
        || event.modifiers.alt_graph
        || key.state != ButtonState::Pressed
        || key.repeat
    {
        return Ok(None);
    }
    let Key::Character(character) = fields::shortcut_key(key) else {
        return Ok(None);
    };
    let redo = if character.eq_ignore_ascii_case("z") {
        event.modifiers.shift
    } else if !cfg!(target_os = "macos") && character.eq_ignore_ascii_case("y") {
        true
    } else {
        return Ok(None);
    };
    let Some(frame) = handle.frame().filter(|frame| {
        frame.modal_stack.is_empty() && event.context.as_ref() == Some(&frame.context)
    }) else {
        return Ok(None);
    };
    let mut controls = world.query::<(Entity, &NativeCommandBinding)>();
    Ok(controls.iter(world).find_map(|(entity, binding)| {
        let matching = if redo {
            matches!(binding.command, NativeCommand::Redo)
        } else {
            matches!(binding.command, NativeCommand::Undo)
        };
        matching
            .then(|| {
                handle
                    .resolve_input(
                        limo_cad_interface::ControlKey(entity.to_bits()),
                        ControlInput::Click,
                        &frame.context,
                    )
                    .ok()
            })
            .flatten()
    }))
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum HistoryCommand {
    Select(u64),
    Edit(u64),
    Rename(u64),
    RenameValue(u64),
    ConfirmRename(u64),
    Rollback(usize),
    RollbackMarker,
    Reorder {
        feature_id: u64,
        target_index: usize,
    },
    Delete(u64),
    ConfirmDelete(u64),
    Cancel,
    Scroll(i32),
}
#[derive(Clone)]
struct Target {
    receipt: DocumentReceipt,
    id: u64,
    anchor: [f32; 2],
}
struct Rename {
    target: Target,
    name: String,
    focus_requested: bool,
}
#[derive(Resource, Default)]
struct History {
    snapshot: Option<(DocumentReceipt, Arc<DocumentDto>)>,
    selected: Option<u64>,
    menu: Option<Target>,
    delete: Option<Target>,
    rename: Option<Rename>,
    error: Option<String>,
    scroll: usize,
    drag: Option<drag::Drag>,
    widgets: Widgets,
}
pub(super) fn modal(world: &World) -> Option<&'static str> {
    let state = world.get_resource::<History>()?;
    if state.rename.is_some() {
        Some("rename-feature")
    } else if state.delete.is_some() {
        Some("delete-feature")
    } else if state.menu.is_some() {
        Some("history-menu")
    } else {
        None
    }
}
pub(super) fn escape(world: &mut World) {
    cancel_drag(world);
    if let Some(mut state) = world.get_resource_mut::<History>() {
        state.menu = None;
        state.delete = None;
        state.rename = None;
        state.error = None;
        state.drag = None;
    }
}
pub(super) fn pointer_active(world: &World) -> bool {
    world
        .get_resource::<History>()
        .is_some_and(|state| state.drag.is_some())
}
fn feature(document: &DocumentDto, id: u64) -> Result<&Feature, String> {
    document
        .features
        .iter()
        .find(|feature| feature.id.0 == id)
        .ok_or("The history feature no longer exists".into())
}

fn require_feature(engine: &AppState, id: u64) -> Result<(), String> {
    engine.with_document(|document| {
        document
            .features()
            .features
            .iter()
            .any(|feature| feature.id.0 == id)
            .then_some(())
            .ok_or_else(|| "The history feature no longer exists".into())
    })
}
fn idle(world: &World) -> Result<(), String> {
    if native_viewport::interface_view(world).2.mode == native_viewport::ViewportMode::Sketch
        || feature::panel(world).is_some()
    {
        return Err("Finish or cancel the current edit before changing feature history".into());
    }
    Ok(())
}
fn mutation(
    world: &mut World,
    receipt: DocumentReceipt,
    operation: &str,
    args: Value,
) -> Result<Value, String> {
    let operation = operation.to_owned();
    let completion = operation.clone();
    worker::enqueue_operation(
        world,
        receipt.owner,
        receipt.revision,
        operation,
        args,
        move |world, services, result| match result {
            Ok(result) => {
                escape(world);
                Ok(finish_mutation(
                    &services.engine,
                    &services.bridge,
                    world,
                    &completion,
                    result,
                ))
            }
            Err(error) => {
                if let Some(mut state) = world.get_resource_mut::<History>() {
                    state.error = Some(error.clone());
                }
                Err(error)
            }
        },
    )
}
pub(crate) fn reduce(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    engine: &AppState,
    bridge: &SessionBridgeState,
    action: &NativeInterfaceAction,
    command: &HistoryCommand,
) -> Result<Value, String> {
    bridge
        .with_native_document_owner(engine, &action.context, || handle.validate_action(action))?;
    world.init_resource::<History>();
    let input = &action.control.input;
    if let HistoryCommand::RenameValue(id) = *command {
        let receipt = bridge.native_document_receipt(engine, &action.context)?;
        {
            let mut state = world.resource_mut::<History>();
            let rename = state
                .rename
                .as_mut()
                .filter(|rename| rename.target.id == id && rename.target.receipt == receipt)
                .ok_or("The design changed; start Rename again")?;
            if let ControlInput::SetValue(value) = input {
                rename.name.clone_from(value);
                state.error = None;
                return Ok(json!({"edited":true}));
            }
        }
        if matches!(input, ControlInput::Key(key) if key == &KeyChord::plain("Enter")) {
            return reduce(
                world,
                handle,
                engine,
                bridge,
                action,
                &HistoryCommand::ConfirmRename(id),
            );
        }
        if matches!(input, ControlInput::Key(key) if key == &KeyChord::plain("Escape")) {
            escape(world);
            return Ok(json!({"cancelled":true}));
        }
        return Ok(json!({"handled":true}));
    }
    if *command == HistoryCommand::RollbackMarker {
        let (rollback, count) = engine.document_history_position();
        let index = match input {
            ControlInput::Key(key) if !key.ctrl && !key.meta && !key.alt && !key.shift => {
                match key.key.as_str() {
                    "ArrowLeft" | "ArrowDown" => Some(rollback.saturating_sub(1)),
                    "ArrowRight" | "ArrowUp" => Some((rollback + 1).min(count)),
                    "Home" => Some(0),
                    "End" => Some(count),
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(index) = index.filter(|index| *index != rollback) {
            idle(world)?;
            let receipt = bridge.native_document_receipt(engine, &action.context)?;
            return mutation(
                world,
                receipt,
                "solid_set_rollback",
                json!({"rollback_index": index}),
            );
        }
        return Ok(json!({"handled": true}));
    }
    if !matches!(command, HistoryCommand::Cancel | HistoryCommand::Select(_))
        && world
            .get::<InterfaceControl>(Entity::from_bits(action.control.key.0))
            .is_some_and(|control| control.modal_scope.as_deref() == Some("history-menu"))
    {
        let current = bridge.native_document_receipt(engine, &action.context)?;
        if world
            .resource::<History>()
            .menu
            .as_ref()
            .is_none_or(|target| target.receipt != current)
        {
            return Err("The design changed; open this history menu again".into());
        }
    }
    let context_menu = matches!(input, ControlInput::ContextMenu)
        || matches!(input,ControlInput::Key(key) if key.key=="ContextMenu" || (key.key=="F10"&&key.shift));
    if let HistoryCommand::Select(id) = *command {
        let receipt = bridge.native_document_receipt(engine, &action.context)?;
        require_feature(engine, id)?;
        if context_menu {
            let anchor = handle
                .read_surface(|_, frame| {
                    frame
                        .controls
                        .iter()
                        .find(|control| control.key == action.control.key)
                        .map(|c| [c.bounds.x as f32, c.bounds.y as f32])
                })?
                .ok_or("History control has no current layout")?;
            let mut state = world.resource_mut::<History>();
            state.menu = Some(Target {
                receipt,
                id,
                anchor,
            });
            state.selected = Some(id);
            return Ok(json!({"menu_open":true}));
        }
        if matches!(input, ControlInput::DoubleClick) {
            return edit(world, handle, engine, bridge, action, id);
        }
    }
    if !super::super::is_activation(input) {
        return Err("History control requires activation".into());
    }
    match *command {
        HistoryCommand::Select(id) => {
            world.resource_mut::<History>().selected = Some(id);
            Ok(json!({"selected_feature_id":id}))
        }
        HistoryCommand::Edit(id) => edit(world, handle, engine, bridge, action, id),
        HistoryCommand::Rename(id) => {
            idle(world)?;
            let receipt = bridge.native_document_receipt(engine, &action.context)?;
            let name = engine.with_document(|document| {
                let feature = document
                    .features()
                    .features
                    .iter()
                    .find(|feature| feature.id.0 == id)
                    .ok_or("The history feature no longer exists")?;
                if feature.kind == FeatureKind::ConstructionPlane {
                    return Err("Name datum planes when creating them");
                }
                Ok(feature.name.clone())
            })?;
            let mut state = world.resource_mut::<History>();
            state.menu = None;
            state.rename = Some(Rename {
                target: Target {
                    receipt,
                    id,
                    anchor: [0., 0.],
                },
                name,
                focus_requested: false,
            });
            state.error = None;
            Ok(json!({"awaiting_input":true}))
        }
        HistoryCommand::ConfirmRename(id) => {
            idle(world)?;
            let receipt = bridge.native_document_receipt(engine, &action.context)?;
            let name = world
                .resource::<History>()
                .rename
                .as_ref()
                .filter(|rename| rename.target.id == id && rename.target.receipt == receipt)
                .ok_or("The design changed; start Rename again")?
                .name
                .clone();
            mutation(
                world,
                receipt,
                "solid_rename_feature",
                json!({"feature_id":id,"name":name}),
            )
        }
        HistoryCommand::RenameValue(_) => unreachable!(),
        HistoryCommand::Cancel => {
            escape(world);
            Ok(json!({"cancelled":true}))
        }
        HistoryCommand::Scroll(delta) => {
            let mut state = world.resource_mut::<History>();
            state.scroll = state.scroll.saturating_add_signed(delta as isize);
            Ok(json!({"scrolled":true}))
        }
        HistoryCommand::Rollback(index) => {
            idle(world)?;
            let receipt = bridge.native_document_receipt(engine, &action.context)?;
            mutation(
                world,
                receipt,
                "solid_set_rollback",
                json!({"rollback_index":index}),
            )
        }
        HistoryCommand::RollbackMarker => unreachable!(),
        HistoryCommand::Reorder {
            feature_id,
            target_index,
        } => {
            idle(world)?;
            let receipt = bridge.native_document_receipt(engine, &action.context)?;
            mutation(
                world,
                receipt,
                "solid_reorder_feature",
                json!({"feature_id": feature_id, "target_index": target_index}),
            )
        }
        HistoryCommand::Delete(id) => {
            idle(world)?;
            let receipt = bridge.native_document_receipt(engine, &action.context)?;
            require_feature(engine, id)?;
            let mut state = world.resource_mut::<History>();
            state.menu = None;
            state.delete = Some(Target {
                receipt,
                id,
                anchor: [0., 0.],
            });
            state.error = None;
            Ok(json!({"awaiting_input":true}))
        }
        HistoryCommand::ConfirmDelete(id) => {
            idle(world)?;
            let target = world
                .resource::<History>()
                .delete
                .clone()
                .ok_or("Delete confirmation was closed")?;
            if target.id != id
                || target.receipt != bridge.native_document_receipt(engine, &action.context)?
            {
                return Err("The design changed; review the deletion again".into());
            }
            mutation(
                world,
                target.receipt,
                "solid_delete_feature",
                json!({"feature_id":id}),
            )
        }
    }
}
fn edit(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    engine: &AppState,
    bridge: &SessionBridgeState,
    action: &NativeInterfaceAction,
    id: u64,
) -> Result<Value, String> {
    idle(world)?;
    let (kind, name) = engine.with_document(|document| {
        document
            .features()
            .features
            .iter()
            .find(|feature| feature.id.0 == id)
            .map(|feature| (feature.kind, feature.name.clone()))
            .ok_or("The history feature no longer exists")
    })?;
    let result = match kind {
        FeatureKind::Sketch => crate::native_editor::execute(
            world,
            engine,
            bridge,
            &action.context,
            crate::native_editor::EditorCommand::Edit(name),
            || handle.validate_action(action),
        ),
        kind if feature::SolidFormKind::from_feature_kind(kind).is_some() => feature::reduce(
            engine,
            bridge,
            world,
            &action.context,
            &feature::FeatureCommand::Open {
                kind: feature::SolidFormKind::from_feature_kind(kind).unwrap(),
                feature_id: Some(id),
            },
            &ControlInput::Click,
            || handle.validate_action(action),
        ),
        _ => Err("This feature has no editable parameters".into()),
    };
    if result.is_ok() {
        escape(world);
    }
    result
}
fn control(label: &str, scope: Option<&str>, disabled: bool) -> InterfaceControl {
    let mut c = InterfaceControl::button(scope.unwrap_or("document/history"), label);
    c.modal_scope = scope.map(str::to_owned);
    c.disabled = disabled;
    if scope == Some("history-menu") {
        c.role = "menuitem".into();
        c.owned_keys = ["ArrowUp", "ArrowDown", "Home", "End"]
            .into_iter()
            .map(KeyChord::plain)
            .collect();
    }
    c
}
fn icon(feature: &Feature) -> Icon {
    match feature.kind {
        FeatureKind::Sketch => Icon::PenLine,
        FeatureKind::ConstructionPlane => Icon::Layers,
        _ => Icon::Box,
    }
}
