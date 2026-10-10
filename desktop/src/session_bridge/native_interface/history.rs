//! Sketch and solid application Undo/Redo through the shared live dispatcher.
//! Placement changes share the bounded edit snapshots, so their Undo restores
//! assembly intent instead of deleting an unrelated source feature. Additional
//! workspace reducers must register their own mutation boundaries here.

use super::super::{
    bump_engine_revision, dispatch_inbox_on_engine, dispatch_project_replacement,
    native_history::{self, HistoryState, RedoStep, UndoStep},
    parse_engine_envelope, retire_project_publisher, ProjectPublisher, SessionBridgeState,
    WindowPublisher,
};
use super::{check_owner, context, NativeMutationResult};
use crate::state::AppState;
use limo_cad_interface::DocumentContext;
use serde_json::{json, Value};

fn state(owner: &DocumentContext, project: &ProjectPublisher) -> HistoryState {
    HistoryState {
        context: owner.clone(),
        engine_revision: project.engine_revision,
    }
}

fn active_sketch_history(engine: &AppState) -> Result<Option<(bool, bool)>, String> {
    let sketch = parse_engine_envelope(engine.engine_call("active_sketch", ""))?;
    if sketch.is_null() {
        return Ok(None);
    }
    let undo = sketch["can_undo"]
        .as_bool()
        .ok_or("Active sketch omitted Undo availability")?;
    let redo = sketch["can_redo"]
        .as_bool()
        .ok_or("Active sketch omitted Redo availability")?;
    Ok(Some((undo, redo)))
}

struct MetadataPresentation {
    active_view: Option<String>,
    visibility: Value,
}

fn prepare_history_restore(
    engine: &AppState,
    current: &str,
    target: &str,
    preserve_visibility: bool,
) -> Result<(String, Option<MetadataPresentation>), String> {
    let mut current: Value = serde_json::from_str(current).map_err(|e| e.to_string())?;
    let mut target: Value = serde_json::from_str(target).map_err(|e| e.to_string())?;
    let current_intent = current
        .as_object_mut()
        .ok_or("History model must be an object")?
        .remove("print_intent")
        .unwrap_or_default();
    let mut target_intent = target
        .as_object_mut()
        .ok_or("History model must be an object")?
        .remove("print_intent")
        .unwrap_or_default();
    // Allocation is monotonic within this document, even when Undo removes an
    // orphan handoff that reserved occurrence identities.
    let floor_path = "/assembly/component_structure/next_occurrence_id";
    let floor = current
        .pointer(floor_path)
        .and_then(Value::as_u64)
        .unwrap_or(1)
        .max(
            target
                .pointer(floor_path)
                .and_then(Value::as_u64)
                .unwrap_or(1),
        );
    if let Some(value) = current.pointer_mut(floor_path) {
        *value = json!(floor);
    }
    if let Some(value) = target.pointer_mut(floor_path) {
        *value = json!(floor);
    }
    if let (Some(current_views), Some(target_views)) = (current.get("views"), target.get("views")) {
        let current_views: Vec<limo_cad_sketch::NamedViewConfigurationDto> =
            serde_json::from_value(current_views.clone()).map_err(|e| e.to_string())?;
        let mut historical: Vec<limo_cad_sketch::NamedViewConfigurationDto> =
            serde_json::from_value(target_views.clone()).map_err(|e| e.to_string())?;
        limo_cad_sketch::normalize_named_view_history_ids(&current_views, &mut historical)?;
        target["views"] = serde_json::to_value(historical).map_err(|e| e.to_string())?;
    }
    if preserve_visibility {
        let datum_ids = |model: &Value| -> Result<std::collections::BTreeSet<u64>, String> {
            let Some(planes) = model.get("datum_planes") else {
                return Ok(Default::default());
            };
            planes
                .as_array()
                .ok_or("History datum planes must be an array")?
                .iter()
                .map(|plane| {
                    plane["datum_id"]
                        .as_u64()
                        .ok_or_else(|| "History datum plane omitted its stable identity".into())
                })
                .collect()
        };
        let current_ids = datum_ids(&current)?;
        let target_ids = datum_ids(&target)?;
        let mut visibility = current
            .get("visibility")
            .ok_or("Current history model omitted project visibility")?
            .clone();
        let hidden_datums = |visibility: &Value| -> Result<Vec<u64>, String> {
            let visibility = visibility
                .as_object()
                .ok_or("History visibility must be an object")?;
            serde_json::from_value(
                visibility
                    .get("hidden_datum_plane_ids")
                    .cloned()
                    .unwrap_or_else(|| json!([])),
            )
            .map_err(|e| e.to_string())
        };
        let current_hidden = hidden_datums(&visibility)?;
        let historical_hidden = hidden_datums(
            &target
                .get("visibility")
                .cloned()
                .unwrap_or_else(|| json!({})),
        )?;
        // Surviving planes retain their current eye state. A restored plane
        // recovers its snapshot state, which removal had scrubbed from current.
        let hidden: std::collections::BTreeSet<_> = current_hidden
            .into_iter()
            .filter(|id| current_ids.contains(id) && target_ids.contains(id))
            .chain(
                historical_hidden
                    .into_iter()
                    .filter(|id| target_ids.contains(id) && !current_ids.contains(id)),
            )
            .collect();
        visibility["hidden_datum_plane_ids"] = json!(hidden);
        target["visibility"] = visibility;
    }
    let metadata_only = current == target;
    if target_intent.is_null() {
        target_intent = serde_json::to_value(limo_cad_core::PrintIntentDocumentDto::default())
            .map_err(|e| e.to_string())?;
    }
    if !current_intent["source_document_id"].is_null()
        && !target_intent["source_document_id"].is_null()
        && current_intent["source_document_id"] != target_intent["source_document_id"]
    {
        return Err("History snapshot belongs to another manufacturing source document".into());
    }
    if target_intent["source_document_id"].is_null()
        && !current_intent["source_document_id"].is_null()
    {
        target_intent["source_document_id"] = current_intent["source_document_id"].clone();
    }
    target
        .as_object_mut()
        .unwrap()
        .insert("print_intent".into(), target_intent);
    let model_json = serde_json::to_string(&target).map_err(|e| e.to_string())?;
    if !metadata_only {
        return Ok((model_json, None));
    }
    let views = parse_engine_envelope(engine.engine_call("named_views", "{}"))?;
    Ok((
        model_json,
        Some(MetadataPresentation {
            active_view: views["active"].as_str().map(str::to_string),
            visibility: parse_engine_envelope(engine.engine_call("project_visibility", "{}"))?,
        }),
    ))
}

fn restore_metadata_presentation(
    engine: &AppState,
    presentation: Option<MetadataPresentation>,
    mut value: Value,
) -> Result<Value, String> {
    if let Some(presentation) = presentation {
        if let Some(name) = presentation.active_view {
            parse_engine_envelope(
                engine.engine_call("recall_named_view", &json!({"name":name}).to_string()),
            )?;
            parse_engine_envelope(engine.engine_call(
                "project_set_visibility",
                &presentation.visibility.to_string(),
            ))?;
        }
        value
            .as_object_mut()
            .ok_or("History restore omitted its receipt")?
            .insert("print_metadata_history".into(), json!(true));
    }
    Ok(value)
}

fn mutate(
    engine: &AppState,
    publisher: &mut WindowPublisher,
    owner: &DocumentContext,
    process_instance_id: &str,
    operation: &str,
    arguments: &Value,
) -> Result<Value, String> {
    publisher
        .active_mut()
        .engine_revision
        .checked_add(1)
        .ok_or("Session engine revision exhausted")?;
    let value = dispatch_inbox_on_engine(engine, operation, arguments)?;
    if bump_engine_revision(
        publisher.active_mut(),
        &owner.window_id,
        Some(&owner.document_id),
        process_instance_id,
    )
    .is_err()
    {
        eprintln!("Native history could not publish engine revision; publication will retry");
    }
    Ok(value)
}

impl SessionBridgeState {
    pub(crate) fn native_history_available(
        &self,
        engine: &AppState,
        expected: &DocumentContext,
    ) -> Result<(bool, bool), String> {
        let mut publishers = self
            .publishers
            .lock()
            .map_err(|_| "Session publisher lock poisoned")?;
        let publisher = publishers
            .get_mut(&expected.window_id)
            .ok_or("Native interface window is no longer available")?;
        check_owner(publisher, engine, expected)?;
        let project = publisher.active_mut();
        let before = state(expected, project);
        project.native_history.observe(&before)?;
        if let Some(available) = active_sketch_history(engine)? {
            return Ok(available);
        }
        let (rollback_index, feature_count) = engine.document_history_position();
        let undo = project.native_history.peek_edit_undo(&before).is_some()
            || native_history::undo_step(rollback_index, feature_count)?.is_some();
        let redo = project
            .native_history
            .redo_step(&before, rollback_index, feature_count)?
            .is_some();
        Ok((undo, redo))
    }

    pub(crate) fn apply_native_history(
        &self,
        engine: &AppState,
        expected: &DocumentContext,
        redo: bool,
        validate_control: impl FnOnce() -> Result<(), String>,
    ) -> Result<NativeMutationResult, String> {
        self.apply_native_history_guarded(engine, expected, None, redo, validate_control)
    }

    /// A worker history intent is tied to the same exact revision captured
    /// by its observed control. Check it under the existing mutation fence.
    pub(crate) fn apply_native_history_at(
        &self,
        engine: &AppState,
        expected: &DocumentContext,
        expected_revision: u64,
        redo: bool,
        validate_control: impl FnOnce() -> Result<(), String>,
    ) -> Result<NativeMutationResult, String> {
        self.apply_native_history_guarded(
            engine,
            expected,
            Some(expected_revision),
            redo,
            validate_control,
        )
    }

    fn apply_native_history_guarded(
        &self,
        engine: &AppState,
        expected: &DocumentContext,
        expected_revision: Option<u64>,
        redo: bool,
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
            return Err("The design changed before this history action could run".into());
        }
        validate_control()?;
        let project = publisher.active_mut();
        let before = state(expected, project);
        project.native_history.observe(&before)?;
        let nothing = if redo {
            "There is nothing to redo"
        } else {
            "There is nothing to undo"
        };

        let value = if let Some((can_undo, can_redo)) = active_sketch_history(engine)? {
            if !(if redo { can_redo } else { can_undo }) {
                return Err(nothing.into());
            }
            mutate(
                engine,
                publisher,
                expected,
                &self.process_instance_id,
                if redo { "sketch_redo" } else { "sketch_undo" },
                &json!({}),
            )?
        } else if !redo
            && publisher
                .active_mut()
                .native_history
                .peek_edit_undo(&before)
                .is_some()
        {
            let ticket = publisher
                .active_mut()
                .native_history
                .peek_edit_undo(&before)
                .unwrap();
            let current = parse_engine_envelope(engine.engine_call("project_export_model", ""))?;
            let current = current
                .as_str()
                .ok_or("Engine did not return an Undo snapshot")?;
            let (model_json, presentation) = prepare_history_restore(
                engine,
                current,
                ticket.model_json(),
                publisher.active_mut().native_history.preserves_visibility(),
            )?;
            let mut replacement = ProjectPublisher::new();
            let after_owner = context(&expected.window_id, &expected.document_id, &replacement);
            let mut history = publisher.active_mut().native_history.clone();
            history.commit_edit_undo(
                ticket.clone(),
                current.to_owned(),
                state(&after_owner, &replacement),
            )?;
            let (outcome, changed) = dispatch_project_replacement(
                engine,
                "cad_load_project_model",
                &json!({"model_json":model_json}),
            );
            let outcome = outcome
                .and_then(|value| restore_metadata_presentation(engine, presentation, value));
            if changed {
                if outcome.is_ok() {
                    replacement.native_history = history;
                    replacement.native_file_epoch = publisher.active_mut().native_file_epoch;
                }
                retire_project_publisher(
                    publisher,
                    &expected.window_id,
                    &expected.document_id,
                    &self.process_instance_id,
                    replacement,
                );
            }
            outcome?
        } else {
            let document = engine.document_snapshot();
            if redo {
                match publisher
                    .active_mut()
                    .native_history
                    .redo_step(&before, document.rollback_index, document.features.len())?
                    .ok_or(nothing)?
                {
                    RedoStep::Rollback(index) => mutate(
                        engine,
                        publisher,
                        expected,
                        &self.process_instance_id,
                        "solid_set_rollback",
                        &json!({"rollback_index":index}),
                    )?,
                    RedoStep::Restore(ticket) => {
                        let current =
                            parse_engine_envelope(engine.engine_call("project_export_model", ""))?;
                        let (model_json, presentation) = prepare_history_restore(
                            engine,
                            current
                                .as_str()
                                .ok_or("Engine did not return a Redo snapshot")?,
                            ticket.model_json(),
                            publisher.active_mut().native_history.preserves_visibility(),
                        )?;
                        let mut replacement = ProjectPublisher::new();
                        let after_owner =
                            context(&expected.window_id, &expected.document_id, &replacement);
                        let mut history = publisher.active_mut().native_history.clone();
                        history.commit_redo(ticket.clone(), state(&after_owner, &replacement))?;
                        let (outcome, changed) = dispatch_project_replacement(
                            engine,
                            "cad_load_project_model",
                            &json!({"model_json":model_json}),
                        );
                        let outcome = outcome.and_then(|value| {
                            restore_metadata_presentation(engine, presentation, value)
                        });
                        if changed {
                            if outcome.is_ok() {
                                replacement.native_history = history;
                                replacement.native_file_epoch =
                                    publisher.active_mut().native_file_epoch;
                            }
                            retire_project_publisher(
                                publisher,
                                &expected.window_id,
                                &expected.document_id,
                                &self.process_instance_id,
                                replacement,
                            );
                        }
                        outcome?
                    }
                }
            } else {
                match native_history::undo_step(document.rollback_index, document.features.len())?
                    .ok_or(nothing)?
                {
                    UndoStep::Rollback(index) => mutate(
                        engine,
                        publisher,
                        expected,
                        &self.process_instance_id,
                        "solid_set_rollback",
                        &json!({"rollback_index":index}),
                    )?,
                    UndoStep::DeleteLatest => {
                        let feature = document.features.last().ok_or(nothing)?;
                        let model =
                            parse_engine_envelope(engine.engine_call("project_export_model", ""))?;
                        let model = model
                            .as_str()
                            .ok_or("Engine did not return a complete Undo model")?;
                        let history = &publisher.active_mut().native_history;
                        let ticket = history.prepare_undo(&before, model.to_owned())?;
                        let mut committed = history.clone();
                        let after = HistoryState {
                            context: expected.clone(),
                            engine_revision: before
                                .engine_revision
                                .checked_add(1)
                                .ok_or("Session engine revision exhausted")?,
                        };
                        committed.commit_undo(ticket, after)?;
                        let value = mutate(
                            engine,
                            publisher,
                            expected,
                            &self.process_instance_id,
                            "solid_delete_feature",
                            &json!({"feature_id":feature.id.0}),
                        )?;
                        publisher.active_mut().native_history = committed;
                        value
                    }
                }
            }
        };
        let project = publisher.active_mut();
        Ok(NativeMutationResult {
            context: context(&expected.window_id, &expected.document_id, project),
            engine_revision: project.engine_revision,
            value,
        })
    }
}

#[cfg(test)]
mod tests;
