//! The same immutable Apply ticket supports the native worker and the
//! immediate embedded host. An accepted enqueue is never retried inline.

use super::*;
use crate::native_forms::ApplyTicket;
use crate::session_bridge::native_interface::NativeMutationResult;

fn prepare(
    engine: &AppState,
    bridge: &SessionBridgeState,
    owner: &DocumentContext,
    editor: &mut Editor,
    validate: impl FnOnce() -> Result<(), String>,
) -> Result<ApplyTicket, String> {
    with_receipt(bridge, engine, owner, |receipt| {
        check_revision(editor, &receipt)?;
        validate()?;
        editor
            .form
            .prepare_apply(&editor.snapshot.model(editor.form.parameter_sketch()))
    })
}

pub(super) fn begin(
    engine: &AppState,
    bridge: &SessionBridgeState,
    world: &mut World,
    owner: &DocumentContext,
    form_id: u64,
    validate_control: impl FnOnce() -> Result<(), String>,
    state: &mut NativeFeature,
) -> Result<Value, String> {
    let editor = state
        .editor
        .as_mut()
        .filter(|editor| editor.id == form_id)
        .ok_or("The feature form changed")?;
    if editor.form.is_busy() {
        return Err("The feature is still applying".into());
    }
    {
        use crate::session_bridge::native_interface::controller::worker;
        if worker::available(world) {
            let ticket = prepare(engine, bridge, owner, editor, validate_control)?;
            let callback_ticket = ticket.clone();
            let stage = editor.stage.clone();
            let dispatch_ticket = ticket.clone();
            let queued = worker::enqueue_transaction(
                world,
                ticket.operation().to_owned(),
                move |services, guard| {
                    if let Some(stage) = stage {
                        services.bridge.apply_native_prepared_edit_at(
                            &services.engine,
                            dispatch_ticket.owner(),
                            dispatch_ticket.model_revision(),
                            dispatch_ticket.operation(),
                            dispatch_ticket.arguments(),
                            &stage,
                            || guard.validate(),
                        )
                    } else {
                        services.bridge.apply_native_mutation_at(
                            &services.engine,
                            dispatch_ticket.owner(),
                            dispatch_ticket.model_revision(),
                            dispatch_ticket.operation(),
                            dispatch_ticket.arguments(),
                            || guard.validate(),
                        )
                    }
                },
                move |world, services, outcome| {
                    let mut state = world.remove_resource::<NativeFeature>().unwrap_or_default();
                    let result = complete(
                        &services.engine,
                        &services.bridge,
                        world,
                        &mut state,
                        form_id,
                        &callback_ticket,
                        outcome,
                    );
                    world.insert_resource(state);
                    result
                },
            );
            return match queued {
                Ok(value) => Ok(value),
                // This error means no transaction was queued. Restore the
                // unchanged form, but never fall back to another dispatch.
                Err(error) => complete(engine, bridge, world, state, form_id, &ticket, Err(error)),
            };
        }
    }
    // Embedded hosts without a native worker still use the exact shared
    // mutation dispatcher and validate the original control at the write.
    let ticket = prepare(engine, bridge, owner, editor, || Ok(()))?;
    let outcome = if let Some(stage) = &editor.stage {
        bridge.apply_native_prepared_edit_at(
            engine,
            ticket.owner(),
            ticket.model_revision(),
            ticket.operation(),
            ticket.arguments(),
            stage,
            validate_control,
        )
    } else {
        bridge.apply_native_mutation_at(
            engine,
            ticket.owner(),
            ticket.model_revision(),
            ticket.operation(),
            ticket.arguments(),
            validate_control,
        )
    };
    complete(engine, bridge, world, state, form_id, &ticket, outcome)
}

fn complete(
    engine: &AppState,
    bridge: &SessionBridgeState,
    world: &mut World,
    state: &mut NativeFeature,
    form_id: u64,
    ticket: &ApplyTicket,
    outcome: Result<NativeMutationResult, String>,
) -> Result<Value, String> {
    let matches = state.editor.as_ref().is_some_and(|editor| {
        editor.id == form_id
            && editor.form.owner() == ticket.owner()
            && editor.form.model_revision() == ticket.model_revision()
    });
    match outcome {
        Ok(result) => {
            // A committed mutation remains successful even if its old form
            // was retired while work ran. Never close a replacement editor.
            let owns_preview = matches
                && state.editor.as_ref().is_some_and(|editor| {
                    native_viewport::interface_preview_revision(world) == editor.preview_revision
                });
            let completion = if matches {
                let mut editor = state.editor.take().unwrap();
                let restoration = if owns_preview {
                    move_copy::restore(&mut editor, world)
                } else {
                    Ok(())
                };
                let view = if editor.form.kind().has_plane_references() {
                    plane_view(world, &result.context, false, None)
                } else {
                    Ok(())
                };
                let form =
                    editor
                        .form
                        .apply_succeeded(ticket, &result.context, result.engine_revision);
                form.and(view).and(restoration)
            } else {
                Ok(())
            };
            let clear = if owns_preview {
                bridge.with_native_document_owner(engine, &result.context, || {
                    native_viewport::apply_interface_preview(
                        world,
                        &result.context.document_id,
                        ViewportPreview::default(),
                    )
                })
            } else {
                Ok(())
            };
            let mut value = finish_mutation(engine, bridge, world, ticket.operation(), result);
            value["preview_error"] = json!(clear.err());
            value["form_error"] = json!(completion.err());
            value["form_retired"] = json!(!matches);
            Ok(value)
        }
        Err(error) => {
            if matches {
                let editor = state.editor.as_mut().unwrap();
                let current = bridge.native_document_receipt(engine, ticket.owner());
                if current
                    .as_ref()
                    .is_ok_and(|receipt| *receipt == editor.snapshot.receipt)
                {
                    editor.form.apply_failed(
                        ticket,
                        &editor.snapshot.model(editor.form.parameter_sketch()),
                        error.clone(),
                    )?;
                }
                // Keep the stale editor receipt until synchronize() sees
                // the freshly installed owner/model. It can then retire the
                // obsolete guide only if this editor still owns that preview.
            }
            Err(error)
        }
    }
}
