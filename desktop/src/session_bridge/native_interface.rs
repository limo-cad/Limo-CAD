//! Owner-checked native actions over the existing desktop engine and renderer.
//! Control resolution never grants authority to mutate a later active tab.

use bevy::prelude::{Component, Entity, Resource, World};
use limo_cad_interface::{ControlInput, DocumentContext};
use serde_json::{json, Value};

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

pub(crate) mod controller;
pub(crate) mod feature;
mod history;
mod prepared;
mod publication;
mod view;
pub(crate) use view::{clear_selection, fit_sketch_begin_camera};
pub(crate) mod switch_timing;
pub(crate) mod workspace;
use prepared::{
    apply_prepared_scene, prepare_native_presentation, PreparedNativePresentation,
    PreparedNativeScene,
};
pub(crate) use view::ViewDirection;

pub(crate) fn restore_model_view(
    world: &mut World,
    owner: &DocumentContext,
    revision: u64,
) -> Result<Value, String> {
    view::request(
        world,
        owner,
        revision,
        &json!({"view":"isometric","fit":true,"duration_ms":300,"expires_ms":crate::session_bridge::now_ms()+5000}),
    )
}

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

/// Prepare a copy of the shared edit history while the publisher owner fence
/// is held. The caller commits it only after the engine mutation succeeds.
/// Presentation changes retain existing snapshots without exporting geometry.
pub(super) fn prepare_edit_history(
    engine: &AppState,
    project: &ProjectPublisher,
    owner: &DocumentContext,
    next_revision: u64,
    operation: &str,
) -> Result<Option<super::native_history::SolidHistory>, String> {
    let snapshot_edit = operation == "solid_delete_feature"
        || operation == "solid_rename_feature"
        || operation == "solid_import_step"
        || operation == "solid_reorder_feature"
        || operation.starts_with("solid_edit_")
        || operation.starts_with("drawing_")
        || operation == "set_body_appearance"
        || is_print_intent_edit(operation)
        || matches!(
            operation,
            "upsert_named_view" | "set_named_views" | "rename_named_view" | "delete_named_view"
        )
        || matches!(
            operation,
            "cam_set_document" | "cam_regenerate_operation" | "cam_regenerate_setup"
        )
        || matches!(
            operation,
            "assembly_set_occurrence_pose"
                | "assembly_duplicate_occurrence"
                | "assembly_remove_occurrence"
                | "assembly_create_component"
                | "assembly_create_occurrence"
                | "assembly_update_component"
                | "assembly_update_occurrence"
                | "assembly_set_occurrence_grounded"
                | "assembly_create_joint"
                | "assembly_update_joint"
                | "assembly_delete_joint"
                | "assembly_set_joint_enabled"
                | "assembly_set_joint_coordinates"
                | "assembly_apply_joint_motions"
                | "assembly_create_position"
                | "assembly_update_position"
                | "assembly_delete_position"
                | "assembly_apply_position"
                | "assembly_create_motion_study"
                | "assembly_update_motion_study"
                | "assembly_delete_motion_study"
                | "assembly_create_contact_set"
                | "assembly_update_contact_set"
                | "assembly_delete_contact_set"
        );
    let presentation = matches!(
        operation,
        "project_set_visibility"
            | "construction_set_visibility"
            | "recall_named_view"
            | "clear_named_view"
    );
    if !snapshot_edit && !presentation {
        return Ok(None);
    }
    let before = super::native_history::HistoryState {
        context: owner.clone(),
        engine_revision: project.engine_revision,
    };
    let mut history = project.native_history.clone();
    let after = super::native_history::HistoryState {
        context: owner.clone(),
        engine_revision: next_revision,
    };
    if presentation {
        history.advance_visibility(&before, after)?;
    } else {
        let model = super::parse_engine_envelope(engine.engine_call("project_export_model", ""))?;
        let model = model
            .as_str()
            .ok_or("Engine did not return a complete edit snapshot")?;
        history.record_edit(&before, model.into(), after)?;
    }
    Ok(Some(history))
}

pub(super) fn is_print_intent_edit(operation: &str) -> bool {
    matches!(
        operation,
        "print_intent_set_part"
            | "print_intent_reset_part"
            | "print_intent_copy_part"
            | "print_intent_set_document"
            | "print_intent_upsert_preset"
            | "print_intent_remove_preset"
            | "print_intent_upsert_handoff"
            | "print_intent_remove_handoff"
            | "print_modifier_create"
            | "print_modifier_update"
            | "print_modifier_remove"
            | "print_modifier_copy"
            | "print_modifier_reset"
            | "print_intent_upsert_height_range"
            | "print_intent_upsert_layer_profile"
            | "print_intent_remove_height"
            | "print_intent_rebind_height"
    )
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
    /// modeling operations use the shared MCP map, with the existing full
    /// drawing-document setter reserved for owner-checked native forms.
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
                retire_project_publisher(
                    publisher,
                    &expected.window_id,
                    &expected.document_id,
                    &self.process_instance_id,
                    ProjectPublisher::new(),
                );
            }
            outcome?
        } else if limo_cad_mcp_mutate::lookup_mutate(operation)
            .is_some_and(limo_cad_mcp_mutate::MutateSpec::is_read_only)
        {
            dispatch_inbox_on_engine(engine, operation, &arguments)?
        } else {
            let next_revision = publisher
                .active_mut()
                .engine_revision
                .checked_add(1)
                .ok_or("Session engine revision exhausted")?;
            let edit_history = prepare_edit_history(
                engine,
                publisher.active_mut(),
                expected,
                next_revision,
                operation,
            )?;
            let value = if operation == "drawing_set_document" {
                super::parse_engine_envelope(engine.engine_call(
                    "drawing_set_document",
                    &serde_json::to_string(&arguments).map_err(|error| error.to_string())?,
                ))?
            } else {
                dispatch_inbox_on_engine(engine, operation, &arguments)?
            };
            if bump_engine_revision(
                publisher.active_mut(),
                &expected.window_id,
                Some(&expected.document_id),
                &self.process_instance_id,
            )
            .is_err()
            {
                eprintln!(
                    "Native interface could not publish engine revision; publication will retry"
                );
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
    Sketch(crate::native_editor::EditorCommand),
    File(controller::files::FileCommand),
    Browser(controller::browser::BrowserCommand),
    Assembly(controller::assembly::Command),
    History(controller::history::HistoryCommand),
    Presentation(controller::presentation::Command),
    Workbench(controller::workbench::Command),
    Cam(controller::workbench::cam::Command),
    Drawing(controller::workbench::drawing_editor::Command),
    BodyAppearance(u64, controller::body_appearance::Command),
    NamedView(u64, controller::named_views::Command),
    PrintIntent(u64, controller::print_intent::Command),
    SectionReview(u64, controller::section_review::Command),
    AppSettings(controller::app_settings::Command),
    SixDof(controller::six_dof::Command),
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
        let spec = limo_cad_mcp_mutate::lookup_mutate(operation)
            .ok_or_else(|| format!("Unknown native operation '{operation}'"))?;
        limo_cad_mcp_mutate::encode_payload(spec.payload, arguments)?;
    }
    let mut control = world
        .get_mut::<InterfaceControl>(entity)
        .ok_or("Native command needs a retained control")?;
    let generation = control
        .binding
        .checked_add(1)
        .ok_or("Native command bindings exhausted")?;
    control.binding = generation;

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
    if controller::assembly::joint::active(world)
        && matches!(
            &binding.command,
            NativeCommand::Feature(_) | NativeCommand::Sketch(_)
        )
    {
        return Err(
            "Finish or cancel the joint editor before starting another modeling command".into(),
        );
    }
    if matches!(
        &binding.command,
        NativeCommand::Feature(_) | NativeCommand::Sketch(_)
    ) && (controller::assembly::motion::active(world)
        || controller::assembly::studies::active(world))
    {
        bridge.with_native_document_receipt(engine, &action.context, |revision| {
            handle.validate_action(action)?;
            controller::assembly::motion::cancel(world, &action.context, revision)?;
            controller::assembly::studies::cancel(world, &action.context, revision)
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
    if let NativeCommand::Sketch(crate::native_editor::EditorCommand::Size {
        generation,
        field,
        ..
    }) = &binding.command
    {
        use crate::native_editor::EditorCommand;
        let text = match &action.control.input {
            ControlInput::SetValue(value) => value.clone(),
            ControlInput::DoubleClick => String::new(),
            ControlInput::Click => {
                return bridge.with_native_document_owner(engine, &action.context, || {
                    handle.validate_action(action)?;
                    Ok(json!({"focused":true}))
                });
            }
            ControlInput::Key(key) if key.key == "Escape" => {
                return crate::native_editor::execute(
                    world,
                    engine,
                    bridge,
                    &action.context,
                    EditorCommand::Cancel,
                    || handle.validate_action(action),
                );
            }
            ControlInput::Key(key) if key == &limo_cad_interface::KeyChord::plain("Enter") => {
                return crate::native_editor::confirm_size(
                    world,
                    engine,
                    bridge,
                    &action.context,
                    *generation,
                    || handle.validate_action(action),
                );
            }
            _ => return Err("Use the drawing size field to enter a value".into()),
        };
        return crate::native_editor::execute(
            world,
            engine,
            bridge,
            &action.context,
            EditorCommand::Size {
                generation: *generation,
                field: *field,
                text,
            },
            || handle.validate_action(action),
        );
    }
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
                    });
                }
                ControlInput::Key(key) if key == &limo_cad_interface::KeyChord::plain("Enter") => {
                    let command = match command {
                        InteractionCommand::FormValue { id, .. } => {
                            InteractionCommand::ApplyForm { id: *id }
                        }
                        _ => InteractionCommand::ApplyDimension,
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
    if let NativeCommand::File(command) = &binding.command {
        return controller::files::reduce(world, handle, engine, bridge, action, command);
    }
    if let NativeCommand::AppSettings(command) = &binding.command {
        return controller::app_settings::reduce(world, handle, engine, bridge, action, *command);
    }
    if let NativeCommand::SixDof(command) = &binding.command {
        return controller::six_dof::reduce(world, handle, action, *command);
    }
    if let NativeCommand::Browser(command) = &binding.command {
        return controller::browser::reduce(world, handle, engine, bridge, action, command);
    }
    if let NativeCommand::Assembly(command) = &binding.command {
        return controller::assembly::reduce(world, handle, engine, bridge, action, command);
    }
    if let NativeCommand::History(command) = &binding.command {
        return controller::history::reduce(world, handle, engine, bridge, action, command);
    }
    if let NativeCommand::Presentation(command) = &binding.command {
        return controller::presentation::reduce(world, handle, engine, bridge, action, command);
    }
    if let NativeCommand::Cam(command) = &binding.command {
        return controller::workbench::cam::reduce(world, handle, engine, bridge, action, command);
    }
    if let NativeCommand::Drawing(command) = &binding.command {
        return controller::workbench::drawing_editor::reduce(
            world, handle, engine, bridge, action, command,
        );
    }
    if let NativeCommand::BodyAppearance(generation, command) = &binding.command {
        return controller::body_appearance::reduce(
            world,
            handle,
            engine,
            bridge,
            action,
            *generation,
            command,
        );
    }
    if let NativeCommand::SectionReview(generation, command) = &binding.command {
        return controller::section_review::reduce(
            world,
            handle,
            engine,
            bridge,
            action,
            *generation,
            command,
        );
    }
    if let NativeCommand::NamedView(generation, command) = &binding.command {
        return controller::named_views::reduce(
            world,
            handle,
            engine,
            bridge,
            action,
            *generation,
            command,
        );
    }
    if let NativeCommand::PrintIntent(generation, command) = &binding.command {
        return controller::print_intent::reduce(
            world,
            handle,
            engine,
            bridge,
            action,
            *generation,
            command,
        );
    }
    if let NativeCommand::Workbench(controller::workbench::Command::CamExport(command)) =
        &binding.command
    {
        return controller::workbench::cam_export::reduce(
            world, handle, engine, bridge, action, command,
        );
    }
    if let NativeCommand::Workbench(controller::workbench::Command::CamView(
        controller::workbench::cam_view::Command::Seek,
    )) = &binding.command
    {
        return bridge.with_native_document_receipt(engine, &action.context, |revision| {
            handle.validate_action(action)?;
            controller::workbench::cam_view::seek(
                world,
                &action.context,
                revision,
                &action.control.input,
            )
        });
    }
    if let NativeCommand::Workbench(controller::workbench::Command::CamView(
        command @ (controller::workbench::cam_view::Command::Detail
        | controller::workbench::cam_view::Command::Tolerance),
    )) = &binding.command
    {
        return bridge.with_native_document_receipt(engine, &action.context, |revision| {
            handle.validate_action(action)?;
            controller::workbench::cam_view::settings::reduce(
                world,
                &action.context,
                revision,
                command,
                &action.control.input,
            )
        });
    }
    if let NativeCommand::Workbench(controller::workbench::Command::CamView(
        controller::workbench::cam_view::Command::Nc(serial, command),
    )) = &binding.command
    {
        return bridge.with_native_document_receipt(engine, &action.context, |revision| {
            handle.validate_action(action)?;
            controller::workbench::cam_view::nc_dialog::reduce(
                world,
                &action.context,
                revision,
                *serial,
                *command,
                &action.control.input,
            )
        });
    }
    if !is_activation(&action.control.input) {
        return Err("This native button does not handle the requested input".into());
    }
    if let NativeCommand::Workbench(command) = &binding.command {
        bridge.with_native_document_owner(engine, &action.context, || {
            handle.validate_action(action)
        })?;
        return controller::workbench::execute(world, command);
    }
    match binding.command {
        NativeCommand::ClearSelection if crate::native_editor::active(engine)?.is_some()=>crate::native_editor::execute(world,engine,bridge,&action.context,crate::native_editor::EditorCommand::Interaction(crate::native_editor::InteractionCommand::Select),||handle.validate_action(action)),
        NativeCommand::Sketch(command)=>crate::native_editor::execute(world,engine,bridge,&action.context,command,||handle.validate_action(action)),
        NativeCommand::File(_)=>unreachable!("File fields are reduced before button activation"),
        NativeCommand::AppSettings(_)=>unreachable!("Settings fields are reduced before button activation"),
        NativeCommand::SixDof(_)=>unreachable!("3D mouse input is reduced before button activation"),
        NativeCommand::Assembly(_)=>unreachable!("Assembly input is reduced before button activation"),
        NativeCommand::Browser(_)=>unreachable!("Browser input is reduced before button activation"),
        NativeCommand::History(_)=>unreachable!("History input is reduced before button activation"),
        NativeCommand::Presentation(_)=>unreachable!("Presentation input is reduced before button activation"),
        NativeCommand::Cam(_)=>unreachable!("CAM fields are reduced before button activation"),
        NativeCommand::Drawing(_) | NativeCommand::BodyAppearance(_, _) | NativeCommand::NamedView(_, _) | NativeCommand::PrintIntent(_, _) | NativeCommand::SectionReview(_, _)=>unreachable!("Document fields are reduced before button activation"),
        NativeCommand::Feature(_)=>unreachable!("Extrude fields are reduced before button activation"),
        NativeCommand::CancelClose | NativeCommand::DiscardAndClose => {
            bridge.with_native_document_owner(engine, &action.context, || {
                handle.validate_action(action)?;
                Ok(json!({"close_decision":if matches!(binding.command,NativeCommand::CancelClose) {"cancel"} else {"discard"}}))
            })
        }
        NativeCommand::Undo | NativeCommand::Redo => {
            controller::print_intent::ensure_clean(world)?;
            controller::named_views::ensure_exportable(world)?;
            let redo = matches!(binding.command, NativeCommand::Redo);
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
            controller::workbench::drawing_editor::guard_ribbon_edit(world, &operation)?;
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
    engine.viewport_frame()
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
    let refresh = bridge.with_native_document_receipt(engine, &result.context, |revision| {
        if revision != result.engine_revision {
            return Err("A newer model revision superseded this scene".into());
        }
        let sheet_selection_from = world
            .get_resource::<NativeRenderedDocument>()
            .filter(|rendered| {
                (operation == "drawing_select_sheet" || is_print_intent_edit(operation))
                    && rendered.owner == result.context
                    && rendered.revision.checked_add(1) == Some(result.engine_revision)
            })
            .map(|rendered| rendered.revision);
        let reset = is_project_replacement(operation)
            || operation == "redo"
            || world
                .get_resource::<NativeRenderedDocument>()
                .is_none_or(|rendered| rendered.owner != result.context);
        let bodies = match prepared_scene.transpose()? {
            Some(PreparedNativeScene::Model(model, visibility)) => {
                apply_prepared_scene(world, model, visibility, reset)?
            }
            Some(PreparedNativeScene::Unchanged { from_revision })
                if sheet_selection_from == Some(from_revision)
                    && prepared::can_retain_scene(world) =>
            {
                world.resource::<NativeRenderedDocument>().bodies.clone()
            }
            None if sheet_selection_from.is_some() && prepared::can_retain_scene(world) => {
                world.resource::<NativeRenderedDocument>().bodies.clone()
            }
            _ => refresh_native_model(engine, world, reset)?,
        };
        if let Some(from) = sheet_selection_from.filter(|_| operation == "drawing_select_sheet") {
            controller::workbench::advance_sheet_selection(
                world,
                &result.context,
                from,
                result.engine_revision,
            );
        }
        if operation == "assembly_remove_occurrence" {
            let removed = result.value["id"]
                .as_u64()
                .ok_or("The removal receipt did not identify its component instance")?;
            let (_, _, mut presentation, _) = native_viewport::interface_view_snapshot(world);
            let selected = presentation.selected_occurrence_id == Some(removed);
            let hovered = presentation.hovered_occurrence_id == Some(removed);
            if selected {
                presentation.selected_occurrence_id = None;
                presentation.selected_body_ids.clear();
                presentation.selected_face_ids.clear();
                presentation.selected_edge_ids.clear();
                presentation.selected_surface_point = None;
            }
            if hovered {
                presentation.hovered_occurrence_id = None;
                presentation.hovered_body_id = None;
                presentation.hovered_face_id = None;
                presentation.hovered_edge_id = None;
                presentation.hovered_surface_point = None;
            }
            if selected || hovered {
                native_viewport::apply_interface_view(
                    world,
                    &result.context.document_id,
                    None,
                    Some(presentation),
                )?;
            }
        }
        world.insert_resource(NativeRenderedDocument {
            owner: result.context.clone(),
            revision: result.engine_revision,
            bodies,
        });
        if matches!(operation, "undo" | "redo") && result.value["print_metadata_history"] == true {
            controller::print_intent::after_history(world, &result.context);
            controller::named_views::after_metadata_history(
                world,
                &result.context,
                result.engine_revision,
            );
        }
        if !is_print_intent_edit(operation) {
            controller::named_views::after_mutation(world, operation, &result.value)?;
        } else {
            controller::named_views::advance_metadata(
                world,
                &result.context,
                result.engine_revision,
            );
        }
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

#[cfg(test)]
mod tests;
