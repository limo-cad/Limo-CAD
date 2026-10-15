//! Owner-checked native actions over the existing desktop engine and renderer.
//! Control resolution never grants authority to mutate a later active tab.

use bevy::prelude::{Component, Entity, Resource, World};
use nbcad_interface::{ControlInput, DocumentContext};
use serde_json::{json, Value};
use tauri::{Emitter, Manager};

use super::{
    bump_engine_revision, dispatch_inbox_on_engine, dispatch_project_replacement,
    is_project_replacement, retire_project_publisher, ProjectPublisher, SessionBridgeState,
    WindowPublisher,
};
use crate::{
    native_viewport::{
        self,
        interface_shell::{InterfaceControl, NativeInterfaceAction, NativeInterfaceHandle},
        ViewportModel,
    },
    state::AppState,
};

#[cfg(feature = "dev-bevy-host")]
pub(crate) mod controller;
pub(crate) mod feature;
mod history;
mod prepared;
mod publication;
mod view;
pub(crate) use view::clear_selection;
pub(crate) mod workspace;
use prepared::{apply_prepared_scene, prepare_native_presentation, PreparedNativePresentation};
pub(crate) use view::ViewDirection;

#[derive(Debug)]
pub(crate) struct NativeMutationResult {
    pub context: DocumentContext,
    pub engine_revision: u64,
    pub value: Value,
}

#[derive(Resource, Clone)]
pub(crate) struct NativeRenderedDocument {
    pub owner: DocumentContext,
    pub revision: u64,
    pub bodies: Vec<(u64, String)>,
}

fn context(window: &str, document: &str, project: &ProjectPublisher) -> DocumentContext {
    DocumentContext {
        window_id: window.to_owned(),
        document_id: document.to_owned(),
        epoch: project.native_interface_epoch,
    }
}

fn check_owner(
    publisher: &WindowPublisher,
    engine: &AppState,
    expected: &DocumentContext,
) -> Result<(), String> {
    if publisher.active_project_session_id.as_deref() != Some(expected.document_id.as_str())
        || engine.active_project_session_id() != expected.document_id
    {
        return Err("The active design changed before the native action could run".into());
    }
    let project = publisher
        .by_project
        .get(&expected.document_id)
        .ok_or("The native action's design is no longer resident")?;
    if project.native_interface_epoch != expected.epoch {
        return Err("The design was replaced before the native action could run".into());
    }
    Ok(())
}

impl SessionBridgeState {
    /// The retained UI frame obtains its incarnation from the same publisher
    /// that owns MCP replacement and tab-transition fences. No independent UI
    /// counter may infer this from model revision, document name or file path.
    pub(crate) fn native_document_context(
        &self,
        window_label: &str,
        engine: &AppState,
    ) -> Result<DocumentContext, String> {
        if window_label.is_empty() {
            return Err("Native interface needs a window identity".into());
        }
        let mut publishers = self
            .publishers
            .lock()
            .map_err(|_| "Session publisher lock poisoned")?;
        let document = engine.active_project_session_id();
        let publisher = publishers
            .entry(window_label.to_owned())
            .or_insert_with(WindowPublisher::new);
        if publisher.active_project_session_id.is_none() {
            publisher.rebind_to(&document);
        }
        if publisher.active_project_session_id.as_deref() != Some(document.as_str()) {
            return Err("Native interface is waiting for the project transition".into());
        }
        Ok(context(window_label, &document, publisher.active_mut()))
    }

    /// View/selection effects use the same owner fence without advancing the
    /// model revision. The closure must apply synchronously and must not call
    /// another publisher-locking bridge method or enqueue an unowned effect.
    pub(crate) fn with_native_document_owner<T>(
        &self,
        engine: &AppState,
        expected: &DocumentContext,
        apply: impl FnOnce() -> Result<T, String>,
    ) -> Result<T, String> {
        let publishers = self
            .publishers
            .lock()
            .map_err(|_| "Session publisher lock poisoned")?;
        let publisher = publishers
            .get(&expected.window_id)
            .ok_or("Native interface window is no longer available")?;
        check_owner(publisher, engine, expected)?;
        apply()
    }

    pub(crate) fn with_native_document_receipt<T>(
        &self,
        engine: &AppState,
        expected: &DocumentContext,
        apply: impl FnOnce(u64) -> Result<T, String>,
    ) -> Result<T, String> {
        let publishers = self
            .publishers
            .lock()
            .map_err(|_| "Session publisher lock poisoned")?;
        let publisher = publishers
            .get(&expected.window_id)
            .ok_or("Native interface window is no longer available")?;
        check_owner(publisher, engine, expected)?;
        apply(publisher.by_project[&expected.document_id].engine_revision)
    }

    /// Revalidation, live mutation and revision publication share the existing
    /// publisher -> engine lock order. Human and MCP controls call this once;
    /// the modeling operation dispatch remains nbcad_mcp_mutate's shared map.
    pub(crate) fn apply_native_mutation(
        &self,
        engine: &AppState,
        expected: &DocumentContext,
        operation: &str,
        arguments: &Value,
        validate_control: impl FnOnce() -> Result<(), String>,
    ) -> Result<NativeMutationResult, String> {
        self.apply_native_mutation_with(
            engine,
            expected,
            operation,
            || Ok(arguments.clone()),
            validate_control,
        )
    }

    /// State-relative commands derive their arguments while holding the same
    /// owner fence as the write; read/modify/write cannot lose another edit.
    pub(crate) fn apply_native_mutation_with(
        &self,
        engine: &AppState,
        expected: &DocumentContext,
        operation: &str,
        arguments: impl FnOnce() -> Result<Value, String>,
        validate_control: impl FnOnce() -> Result<(), String>,
    ) -> Result<NativeMutationResult, String> {
        self.apply_native_mutation_guarded(
            engine,
            expected,
            None,
            operation,
            arguments,
            validate_control,
        )
    }

    /// Forms and asynchronous File intents are authored against one exact
    /// engine revision, not merely the document incarnation.
    pub(crate) fn apply_native_mutation_at(
        &self,
        engine: &AppState,
        expected: &DocumentContext,
        expected_revision: u64,
        operation: &str,
        arguments: &Value,
        validate_control: impl FnOnce() -> Result<(), String>,
    ) -> Result<NativeMutationResult, String> {
        self.apply_native_mutation_guarded(
            engine,
            expected,
            Some(expected_revision),
            operation,
            || Ok(arguments.clone()),
            validate_control,
        )
    }

    fn apply_native_mutation_guarded(
        &self,
        engine: &AppState,
        expected: &DocumentContext,
        expected_revision: Option<u64>,
        operation: &str,
        arguments: impl FnOnce() -> Result<Value, String>,
        validate_control: impl FnOnce() -> Result<(), String>,
    ) -> Result<NativeMutationResult, String> {
        let mut publishers = self
            .publishers
            .lock()
            .map_err(|_| "Session publisher lock poisoned")?;
        let publisher = publishers
            .get_mut(&expected.window_id)
            .ok_or("Native interface window is no longer available")?;
        check_owner(publisher, engine, expected)?;
        if expected_revision
            .is_some_and(|revision| revision != publisher.active_mut().engine_revision)
        {
            return Err(
                "The design changed since this operation was prepared; review it again".into(),
            );
        }
        validate_control()?;
        let arguments = arguments()?;

        let result = if is_project_replacement(operation) {
            let (outcome, changed) = dispatch_project_replacement(engine, operation, &arguments);
            if changed {
                // Retirement is required even after a partially applied load
                // fails. A verified unchanged rejection retains its identity.
                retire_project_publisher(
                    publisher,
                    &expected.window_id,
                    &expected.document_id,
                    &self.process_instance_id,
                    ProjectPublisher::new(),
                );
            }
            outcome?
        } else {
            let next_revision = publisher
                .active_mut()
                .engine_revision
                .checked_add(1)
                .ok_or("Session engine revision exhausted")?;
            let edit_history = if operation == "solid_delete_feature"
                || operation == "solid_reorder_feature"
                || operation.starts_with("solid_edit_")
                || matches!(operation, "assembly_set_occurrence_pose" | "assembly_duplicate_occurrence" | "assembly_create_component" | "assembly_create_occurrence" | "assembly_update_component" | "assembly_update_occurrence" | "assembly_set_occurrence_grounded" | "assembly_create_joint" | "assembly_update_joint" | "assembly_delete_joint" | "assembly_set_joint_enabled" | "assembly_set_joint_coordinates" | "assembly_apply_joint_motions" | "assembly_create_position" | "assembly_update_position" | "assembly_delete_position" | "assembly_apply_position" | "assembly_create_motion_study" | "assembly_update_motion_study" | "assembly_delete_motion_study" | "assembly_create_contact_set" | "assembly_update_contact_set" | "assembly_delete_contact_set")
            {
                let model =
                    super::parse_engine_envelope(engine.engine_call("project_export_model", ""))?;
                let model = model
                    .as_str()
                    .ok_or("Engine did not return a complete edit snapshot")?;
                let before = super::native_history::HistoryState {
                    context: expected.clone(),
                    engine_revision: publisher.active_mut().engine_revision,
                };
                let mut history = publisher.active_mut().native_history.clone();
                history.record_edit(
                    &before,
                    model.into(),
                    super::native_history::HistoryState {
                        context: expected.clone(),
                        engine_revision: next_revision,
                    },
                )?;
                Some(history)
            } else {
                None
            };
            let value = dispatch_inbox_on_engine(engine, operation, &arguments)?;
            // Match the established UI mutation contract: a publication I/O
            // failure must not relabel an already committed operation failed
            // and invite unsafe blind replay. The advanced in-memory revision
            // remains authoritative; subsequent publication can retry.
            if let Err(error) = bump_engine_revision(
                publisher.active_mut(),
                &expected.window_id,
                Some(&expected.document_id),
                &self.process_instance_id,
            ) {
                eprintln!("Native interface could not publish engine revision: {error}");
            }
            if let Some(history) = edit_history {
                publisher.active_mut().native_history = history;
            }
            value
        };
        let project = publisher.active_mut();
        Ok(NativeMutationResult {
            context: context(&expected.window_id, &expected.document_id, project),
            engine_revision: project.engine_revision,
            value: result,
        })
    }
}

/// A real widget's native command. Forms construct the same public operation
/// arguments used by MCP; there is no second switch over modeling tools here.
/// Camera and selection presentation are the existing renderer DTOs, not a
/// competing camera/selection implementation.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NativeCommand {
    #[cfg(feature = "dev-bevy-host")]
    Sketch(crate::native_editor::EditorCommand),
    #[cfg(feature = "dev-bevy-host")]
    File(controller::files::FileCommand),
    #[cfg(feature = "dev-bevy-host")]
    Browser(controller::browser::BrowserCommand),
    #[cfg(feature = "dev-bevy-host")]
    Assembly(controller::assembly::Command),
    #[cfg(feature = "dev-bevy-host")]
    History(controller::history::HistoryCommand),
    Feature(feature::FeatureCommand),
    Mutation {
        operation: String,
        arguments: Value,
    },
    ClearSelection,
    SelectBody {
        body_id: u64,
        occurrence_id: Option<u64>,
    },
    Fit,
    Orient(ViewDirection),
    Undo,
    Redo,
    CancelClose,
    DiscardAndClose,
}

#[derive(Component, Clone, Debug)]
pub(crate) struct NativeCommandBinding {
    generation: u64,
    command: NativeCommand,
}

/// Rebinding a retained widget always advances its semantic stamp, even if
/// its label and Entity are unchanged. Root attaches commands through this
/// function instead of updating a command component behind the registry.
pub(crate) fn bind_command(
    world: &mut World,
    entity: Entity,
    command: NativeCommand,
) -> Result<(), String> {
    if let NativeCommand::Mutation {
        operation,
        arguments,
    } = &command
    {
        let spec = nbcad_mcp_mutate::lookup_mutate(operation)
            .ok_or_else(|| format!("Unknown native operation '{operation}'"))?;
        nbcad_mcp_mutate::encode_payload(spec.payload, arguments)?;
    }
    let mut control = world
        .get_mut::<InterfaceControl>(entity)
        .ok_or("Native command needs a retained control")?;
    let generation = control
        .binding
        .checked_add(1)
        .ok_or("Native command bindings exhausted")?;
    control.binding = generation;
    drop(control);
    world.entity_mut(entity).insert(NativeCommandBinding {
        generation,
        command,
    });
    Ok(())
}

fn is_activation(input: &ControlInput) -> bool {
    matches!(input, ControlInput::Click | ControlInput::DoubleClick)
        || matches!(input, ControlInput::Key(chord)
            if !chord.ctrl && !chord.meta && !chord.alt && !chord.shift
                && matches!(chord.key.as_str(), "Enter" | " " | "Space"))
}

pub(crate) fn execute_action(
    app: &tauri::AppHandle,
    world: &mut World,
    handle: &NativeInterfaceHandle,
    action: &NativeInterfaceAction,
) -> Result<Value, String> {
    reduce_action(
        &app.state::<AppState>(),
        &app.state::<SessionBridgeState>(),
        world,
        handle,
        action,
    )
}

/// One reducer for both hosts and both human/MCP control paths.
pub(crate) fn reduce_action(
    engine: &AppState,
    bridge: &SessionBridgeState,
    world: &mut World,
    handle: &NativeInterfaceHandle,
    action: &NativeInterfaceAction,
) -> Result<Value, String> {
    let entity = Entity::from_bits(action.control.key.0);
    let binding = world
        .get::<NativeCommandBinding>(entity)
        .cloned()
        .ok_or("Native command was removed")?;
    let control = world
        .get::<InterfaceControl>(entity)
        .ok_or("Native control was removed")?;
    if binding.generation != control.binding || action.control.binding() != control.binding {
        return Err("Native command changed; inspect the interface again".into());
    }
    if !control.visible || control.disabled {
        return Err("Native control is no longer available".into());
    }
    #[cfg(feature = "dev-bevy-host")]
    if controller::assembly::joint::active(world) && matches!(&binding.command,NativeCommand::Feature(_) | NativeCommand::Sketch(_)) {
        return Err("Finish or cancel the joint editor before starting another modeling command".into());
    }
    #[cfg(feature = "dev-bevy-host")]
    if matches!(&binding.command, NativeCommand::Feature(_) | NativeCommand::Sketch(_)) && (controller::assembly::motion::active(world) || controller::assembly::studies::active(world)) {
        bridge.with_native_document_receipt(engine,&action.context,|revision|{
            handle.validate_action(action)?;
            controller::assembly::motion::cancel(world,&action.context,revision)?;
            controller::assembly::studies::cancel(world,&action.context,revision)
        })?;
    }
    if let NativeCommand::Feature(command) = &binding.command {
        return feature::reduce(
            engine,
            bridge,
            world,
            &action.context,
            command,
            &action.control.input,
            || handle.validate_action(action),
        );
    }
    #[cfg(feature = "dev-bevy-host")]
    if let NativeCommand::Sketch(crate::native_editor::EditorCommand::Size {generation,field,..}) = &binding.command {
        use crate::native_editor::EditorCommand;
        let text=match &action.control.input {
            ControlInput::SetValue(value)=>value.clone(),
            ControlInput::DoubleClick=>String::new(),
            ControlInput::Click=>return bridge.with_native_document_owner(engine,&action.context,|| {
                handle.validate_action(action)?;Ok(json!({"focused":true}))
            }),
            ControlInput::Key(key) if key.key=="Escape"=>return crate::native_editor::execute(
                world,engine,bridge,&action.context,EditorCommand::Cancel,||handle.validate_action(action)),
            _=>return Err("Use the drawing size field to enter a value".into()),
        };
        return crate::native_editor::execute(world,engine,bridge,&action.context,
            EditorCommand::Size {generation:*generation,field:*field,text},||handle.validate_action(action));
    }
    #[cfg(feature = "dev-bevy-host")]
    if let NativeCommand::Sketch(crate::native_editor::EditorCommand::Interaction(command)) =
        &binding.command
    {
        use crate::native_editor::{EditorCommand, InteractionCommand};
        if matches!(
            command,
            InteractionCommand::DimensionText(_) | InteractionCommand::FormValue { .. }
        ) {
            let text = match &action.control.input {
                ControlInput::SetValue(value) => value.clone(),
                ControlInput::Click => {
                    return bridge.with_native_document_owner(engine, &action.context, || {
                        handle.validate_action(action)?;
                        Ok(json!({"focused":true}))
                    })
                }
                _ => return Err("Use the expression field to enter a value".into()),
            };
            let command = match command {
                InteractionCommand::FormValue { id, index, .. } => InteractionCommand::FormValue {
                    id: *id,
                    index: *index,
                    text,
                },
                _ => InteractionCommand::DimensionText(text),
            };
            return crate::native_editor::execute(
                world,
                engine,
                bridge,
                &action.context,
                EditorCommand::Interaction(command),
                || handle.validate_action(action),
            );
        }
    }
    #[cfg(feature = "dev-bevy-host")]
    if let NativeCommand::File(command) = &binding.command {
        return controller::files::reduce(world, handle, engine, bridge, action, command);
    }
    #[cfg(feature = "dev-bevy-host")]
    if let NativeCommand::Browser(command) = &binding.command {
        return controller::browser::reduce(world, handle, engine, bridge, action, command);
    }
    #[cfg(feature = "dev-bevy-host")]
    if let NativeCommand::Assembly(command) = &binding.command {
        return controller::assembly::reduce(world, handle, engine, bridge, action, command);
    }
    #[cfg(feature = "dev-bevy-host")]
    if let NativeCommand::History(command) = &binding.command {
        return controller::history::reduce(world, handle, engine, bridge, action, command);
    }
    if !is_activation(&action.control.input) {
        return Err("This native button does not handle the requested input".into());
    }
    match binding.command {
        #[cfg(feature="dev-bevy-host")]
        NativeCommand::ClearSelection if crate::native_editor::active(engine)?.is_some()=>crate::native_editor::execute(world,engine,bridge,&action.context,crate::native_editor::EditorCommand::Interaction(crate::native_editor::InteractionCommand::Select),||handle.validate_action(action)),
        #[cfg(feature="dev-bevy-host")]
        NativeCommand::Sketch(command)=>crate::native_editor::execute(world,engine,bridge,&action.context,command,||handle.validate_action(action)),
        #[cfg(feature="dev-bevy-host")]
        NativeCommand::File(_)=>unreachable!("File fields are reduced before button activation"),
        #[cfg(feature="dev-bevy-host")]
        NativeCommand::Assembly(_)=>unreachable!("Assembly input is reduced before button activation"),
        #[cfg(feature="dev-bevy-host")]
        NativeCommand::Browser(_)=>unreachable!("Browser input is reduced before button activation"),
        #[cfg(feature="dev-bevy-host")]
        NativeCommand::History(_)=>unreachable!("History input is reduced before button activation"),
        NativeCommand::Feature(_)=>unreachable!("Extrude fields are reduced before button activation"),
        NativeCommand::CancelClose | NativeCommand::DiscardAndClose => {
            bridge.with_native_document_owner(engine, &action.context, || {
                handle.validate_action(action)?;
                Ok(json!({"close_decision":if matches!(binding.command,NativeCommand::CancelClose) {"cancel"} else {"discard"}}))
            })
        }
        NativeCommand::Undo | NativeCommand::Redo => {
            let redo = matches!(binding.command, NativeCommand::Redo);
            #[cfg(feature="dev-bevy-host")]
            if controller::worker::available(world) {
                let receipt = bridge.native_document_receipt(engine, &action.context)?;
                let operation = if redo { "redo" } else { "undo" };
                return controller::worker::enqueue_transaction(world, operation.into(), move |services, guard| {
                    services.bridge.apply_native_history_at(&services.engine, &receipt.owner, receipt.revision, redo, || guard.validate())
                }, move |world, services, result| Ok(finish_mutation(&services.engine, &services.bridge, world, operation, result?)));
            }
            let result = bridge.apply_native_history(engine, &action.context, redo, || handle.validate_action(action))?;
            Ok(finish_mutation(engine, bridge, world, if redo {"redo"} else {"undo"}, result))
        }
        NativeCommand::Mutation {
            operation,
            arguments,
        } => {
            #[cfg(feature="dev-bevy-host")]
            if controller::worker::available(world) {
                let receipt = bridge.native_document_receipt(engine, &action.context)?;
                let completion_operation = operation.clone();
                return controller::worker::enqueue_operation(world, receipt.owner, receipt.revision, operation, arguments, move |world, services, result| {
                    Ok(finish_mutation(&services.engine, &services.bridge, world, &completion_operation, result?))
                });
            }
            let result = bridge.apply_native_mutation(
                engine,
                &action.context,
                &operation,
                &arguments,
                || handle.validate_action(action),
            )?;
            Ok(finish_mutation(engine, bridge, world, &operation, result))
        }
        command => {
            bridge.with_native_document_receipt(engine, &action.context, |revision| {
                handle.validate_action(action)?;
                view::apply(engine, world, &action.context, revision, command)
            })
        }
    }
}

pub(crate) fn model_snapshot(engine: &AppState) -> ViewportModel {
    let snapshot = engine.viewport_snapshot();
    ViewportModel {
        session_id: snapshot.0,
        geometry_revision: snapshot.1,
        scene: snapshot.2,
        active_sketch: snapshot.3,
        finished_sketches: snapshot.4,
        datum_planes: snapshot.5,
        profile_catalog: snapshot.6,
        body_appearances: snapshot.7,
        body_poses: snapshot.8,
        instance_body_poses: snapshot.9,
    }
}

/// Refresh the renderer from one owned native model. Presentation updates
/// must carry the new rigid poses: replaying an old visibility-only DTO after
/// model refresh would otherwise replace assembly placement with stale poses.
pub(crate) fn refresh_native_model(
    engine: &AppState,
    world: &mut World,
    reset_selection: bool,
) -> Result<Vec<(u64, String)>, String> {
    apply_prepared_scene(
        world,
        model_snapshot(engine),
        prepared::read_visibility(engine)?,
        reset_selection,
    )
}

pub(crate) fn finish_mutation(
    engine: &AppState,
    bridge: &SessionBridgeState,
    world: &mut World,
    operation: &str,
    result: NativeMutationResult,
) -> Value {
    let prepared = world
        .remove_resource::<PreparedNativePresentation>()
        .filter(|prepared| {
            prepared.owner == result.context && prepared.revision == result.engine_revision
        });
    let (prepared_scene, prepared_publication) = match prepared {
        Some(prepared) => (Some(prepared.scene), Some(prepared.publication)),
        None => (None, None),
    };
    let refresh = bridge.with_native_document_owner(engine, &result.context, || {
        let reset = is_project_replacement(operation)
            || operation == "redo"
            || world
                .get_resource::<NativeRenderedDocument>()
                .is_none_or(|rendered| rendered.owner != result.context);
        let bodies = if let Some(scene) = prepared_scene {
            let (model, visibility) = scene?;
            apply_prepared_scene(world, model, visibility, reset)?
        } else {
            refresh_native_model(engine, world, reset)?
        };
        world.insert_resource(NativeRenderedDocument {
            owner: result.context.clone(),
            revision: result.engine_revision,
            bodies,
        });
        Ok(())
    });
    let publication = prepared_publication.unwrap_or_else(|| {
        let focus = if super::parse_engine_envelope(engine.engine_call("active_sketch", ""))
            .is_ok_and(|value| !value.is_null())
        {
            "sketch"
        } else {
            "solid"
        };
        bridge.publish_native_document(engine, &result.context, focus)
    });
    let (publication, publication_error) = match publication {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error)),
    };
    json!({"operation":operation,"result":result.value,"document_id":result.context.document_id,
        "document_epoch":result.context.epoch,"engine_revision":result.engine_revision,
        "publication_pending":publication_error.is_some(),"publication":publication,
        "publication_error":publication_error,"render_error":refresh.err()})
}

/// Called on the existing native UI thread before layout/render publication.
/// The typed command is obtained from the same retained widget MCP resolves.
pub(crate) fn drain_actions(
    app: &tauri::AppHandle,
    world: &mut World,
    handle: &NativeInterfaceHandle,
) {
    let actions = match handle.take_actions() {
        Ok(actions) => actions,
        Err(error) => {
            eprintln!("Native interface queue failed: {error}");
            return;
        }
    };
    for action in actions {
        let result = execute_action(app, world, handle, &action);
        let response = match result {
            Ok(value) => json!({"status":"applied","value":value}),
            Err(error) => json!({"status":"failed","error":error}),
        };
        if let Err(error) = app.emit_to(
            &action.context.window_id,
            "native-interface-action",
            response,
        ) {
            eprintln!("Native interface result delivery failed: {error}");
        }
    }
}

#[cfg(test)]
mod tests;
