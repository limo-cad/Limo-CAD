//! Topology editors display the actual feature input in an isolated kernel. Opening
//! and cancelling never move the live history cursor or dirty the document.
use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

pub(crate) struct Stage {
    pub(super) engine: AppState,
    source: String,
    input_index: usize,
    restore_index: usize,
    usable: AtomicBool,
    receipt: DocumentReceipt,
    feature_id: u64,
}
impl Stage {
    fn new(engine: &AppState, receipt: &DocumentReceipt, id: u64) -> Result<Self, String> {
        let document = engine.document_snapshot();
        let input_index = document
            .features
            .iter()
            .position(|f| f.id.0 == id)
            .ok_or("The feature no longer exists")?;
        let source = parse_engine_envelope(engine.engine_call("project_export_model", ""))?
            .as_str()
            .ok_or("Engine omitted the edit snapshot")?
            .to_owned();
        let stage = Self {
            engine: AppState::new(),
            source,
            input_index,
            restore_index: document.rollback_index,
            usable: AtomicBool::new(false),
            receipt: receipt.clone(),
            feature_id: id,
        };
        parse_engine_envelope(
            stage
                .engine
                .bind_project_session(&receipt.owner.document_id),
        )?;
        stage.reset()?;
        Ok(stage)
    }
    fn reset(&self) -> Result<(), String> {
        self.usable.store(false, Ordering::Release);
        parse_engine_envelope(self.engine.project_load(&json!(self.source).to_string()))?;
        parse_engine_envelope(
            self.engine
                .solid_set_rollback(&json!({"rollback_index":self.input_index}).to_string()),
        )?;
        self.usable.store(true, Ordering::Release);
        Ok(())
    }
    pub(super) fn prepare_commit(
        &self,
        operation: &str,
        arguments: &Value,
    ) -> Result<Value, String> {
        if !self.usable.swap(false, Ordering::AcqRel) {
            return Err("Reopen the feature: its prepared model is no longer available".into());
        }
        let result = (|| {
            super::super::super::dispatch_inbox_on_engine(&self.engine, operation, arguments)?;
            let result = parse_engine_envelope(
                self.engine
                    .solid_set_rollback(&json!({"rollback_index":self.restore_index}).to_string()),
            )?;
            let scene = self.engine.viewport_snapshot().2;
            if !scene.errors.is_empty() {
                return Err(format!(
                    "The edited feature could not rebuild: {:?}",
                    scene.errors
                ));
            }
            Ok(result)
        })();
        match result {
            Ok(value) => Ok(value),
            Err(error) => match self.reset() {
                Ok(()) => Err(error),
                Err(recovery) => Err(format!(
                    "{error}. Reopen the feature: its preview could not be restored ({recovery})"
                )),
            },
        }
    }
}

impl SessionBridgeState {
    pub(crate) fn apply_native_prepared_edit_at(
        &self,
        engine: &AppState,
        owner: &DocumentContext,
        revision: u64,
        operation: &str,
        arguments: &Value,
        stage: &Stage,
        validate: impl FnOnce() -> Result<(), String>,
    ) -> Result<super::super::NativeMutationResult, String> {
        use crate::session_bridge::{bump_engine_revision, native_history::HistoryState};
        if !matches!(
            operation,
            "solid_edit_fillet"
                | "solid_edit_chamfer"
                | "solid_edit_shell"
                | "solid_edit_external_thread"
                | "solid_edit_hole"
                | "solid_edit_move_copy"
                | "solid_edit_combine"
                | "construction_plane_edit_offset"
                | "construction_plane_edit_midplane"
                | "construction_plane_edit_at_angle"
                | "solid_edit_mirror"
                | "solid_edit_split_body"
                | "solid_edit_rectangular_pattern"
                | "solid_edit_circular_pattern"
        ) || arguments["feature_id"].as_u64() != Some(stage.feature_id)
        {
            return Err("This prepared model belongs to another feature edit".into());
        }
        let mut publishers = self
            .publishers
            .lock()
            .map_err(|_| "Session publisher lock poisoned")?;
        let publisher = publishers
            .get_mut(&owner.window_id)
            .ok_or("Native window no longer exists")?;
        check_owner(publisher, engine, owner)?;
        let current = DocumentReceipt {
            owner: owner.clone(),
            revision: publisher.active_mut().engine_revision,
        };
        if current != stage.receipt || revision != current.revision {
            return Err("The design changed; reopen the feature before applying".into());
        }
        validate()?;
        let next = revision
            .checked_add(1)
            .ok_or("Session revision exhausted")?;
        let mut history = publisher.active_mut().native_history.clone();
        history.record_edit(
            &HistoryState {
                context: owner.clone(),
                engine_revision: revision,
            },
            stage.source.clone(),
            HistoryState {
                context: owner.clone(),
                engine_revision: next,
            },
        )?;
        let value = stage.prepare_commit(operation, arguments)?;
        engine.install_prepared_document(&stage.engine)?;
        if let Err(error) = bump_engine_revision(
            publisher.active_mut(),
            &owner.window_id,
            Some(&owner.document_id),
            &self.process_instance_id,
        ) {
            eprintln!("Prepared edit committed; publication needs retry: {error}");
        }
        publisher.active_mut().native_history = history;
        Ok(super::super::NativeMutationResult {
            context: owner.clone(),
            engine_revision: next,
            value,
        })
    }
}

struct Prepared {
    stage: Arc<Stage>,
    snapshot: Snapshot,
    original: ViewportModel,
    form: SolidForm,
}
fn prepare(
    engine: &AppState,
    receipt: DocumentReceipt,
    kind: SolidFormKind,
    id: u64,
) -> Result<Prepared, String> {
    let original = model_snapshot(engine);
    if original.active_sketch.is_some() {
        return Err("Finish the sketch before editing a solid feature".into());
    }
    let stage = Arc::new(Stage::new(engine, &receipt, id)?);
    let definitions = parse_engine_envelope(stage.engine.engine_call(
        match kind {
            SolidFormKind::Hole => "hole_definitions",
            SolidFormKind::Fillet => "fillet_definitions",
            SolidFormKind::Chamfer => "chamfer_definitions",
            SolidFormKind::MoveCopy
            | SolidFormKind::ExternalThread
            | SolidFormKind::Shell
            | SolidFormKind::Combine
            | SolidFormKind::Mirror
            | SolidFormKind::SplitBody
            | SolidFormKind::RectangularPattern
            | SolidFormKind::CircularPattern => "body_feature_definitions",
            SolidFormKind::OffsetPlane | SolidFormKind::Midplane | SolidFormKind::AnglePlane => {
                "datum_plane_definitions"
            }
            _ => return Err("This feature has no topology editor".into()),
        },
        "",
    ))?;
    let definition = definitions
        .as_array()
        .and_then(|items| items.iter().find(|d| d["feature_id"].as_u64() == Some(id)))
        .ok_or("The feature no longer exists")?;
    let snapshot = Snapshot::capture(&stage.engine, receipt)?;
    let form = if kind == SolidFormKind::MoveCopy {
        SolidForm::edit_move(definition, &snapshot.model(None))?
    } else if kind == SolidFormKind::Hole {
        SolidForm::edit_hole(definition, &snapshot.model(None))?
    } else if kind == SolidFormKind::ExternalThread {
        SolidForm::edit_thread(definition, &snapshot.model(None))?
    } else if kind.is_pattern() {
        SolidForm::edit_pattern(definition, &snapshot.model(None))?
    } else if kind.is_body_plane() {
        SolidForm::edit_body_plane(definition, &snapshot.model(None))?
    } else if kind.is_plane() {
        SolidForm::edit_plane(definition, &snapshot.model(None))?
    } else if kind == SolidFormKind::Combine {
        SolidForm::edit_combine(definition, &snapshot.model(None))?
    } else if kind == SolidFormKind::Shell {
        SolidForm::edit_shell(definition, &snapshot.model(None))?
    } else {
        SolidForm::edit_edges(kind, definition, &snapshot.model(None))?
    };
    Ok(Prepared {
        stage,
        snapshot,
        original,
        form,
    })
}
fn install(
    world: &mut World,
    state: &mut NativeFeature,
    prepared: Prepared,
) -> Result<Value, String> {
    if state.editor.is_some() {
        return Err("Finish or cancel the open feature first".into());
    }
    let id = state
        .last_id
        .checked_add(1)
        .ok_or("Feature identities exhausted")?;
    let pick_target = Some(if prepared.form.kind().selects_bodies() {
        SolidField::Bodies
    } else if prepared.form.kind().is_plane() {
        SolidField::FirstPlane
    } else if prepared.form.kind() == SolidFormKind::Combine {
        SolidField::TargetBody
    } else if prepared.form.kind() == SolidFormKind::Hole {
        SolidField::HolePositions
    } else if prepared.form.kind() == SolidFormKind::ExternalThread {
        SolidField::Cylinder
    } else if prepared.form.kind() == SolidFormKind::Shell {
        SolidField::Faces
    } else {
        SolidField::Edges
    });
    let mut editor = Editor {
        id,
        form: prepared.form,
        snapshot: prepared.snapshot,
        previous_preview: native_viewport::interface_preview_snapshot(world),
        preview_revision: native_viewport::interface_preview_revision(world),
        preview_notice: None,
        pick_target,
        choice_field: None,
        stage: Some(prepared.stage),
        original_view: Some(prepared.original),
        hovered_edge: None,
        hovered_face: None,
        hovered_body: None,
        hovered_occurrence: None,
        hovered_plane: None,
        hovered_point: None,
        move_view: None,
        move_hover: None,
        move_drag: None,
        offset_drag: None,
    };
    native_viewport::apply_interface_edit_model(world, editor.snapshot.viewport.clone())?;
    if let Err(error) = update_preview(&mut editor, world) {
        native_viewport::apply_interface_model(world, editor.original_view.take().unwrap())?;
        return Err(error);
    }
    state.editor = Some(editor);
    state.last_id = id;
    Ok(json!({"form_id":id,"opened":true}))
}
pub(super) fn begin(
    engine: &AppState,
    bridge: &SessionBridgeState,
    world: &mut World,
    owner: &DocumentContext,
    kind: SolidFormKind,
    id: u64,
    validate: impl FnOnce() -> Result<(), String>,
    state: &mut NativeFeature,
) -> Result<Value, String> {
    if state.editor.is_some() {
        return Err("Finish or cancel the open feature first".into());
    }
    let receipt = with_receipt(bridge, engine, owner, |receipt| {
        validate()?;
        Ok(receipt)
    })?;
    {
        use super::super::controller::worker;
        if worker::available(world) {
            let transfer = Arc::new(Mutex::new(None));
            let output = transfer.clone();
            return worker::enqueue_document_io(
                world,
                "prepare_feature_edit".into(),
                move |services, guard| {
                    with_receipt(
                        &services.bridge,
                        &services.engine,
                        &receipt.owner,
                        |current| {
                            if current != receipt {
                                return Err("The design changed before the editor opened".into());
                            }
                            guard.validate()?;
                            let prepared = prepare(&services.engine, current.clone(), kind, id)?;
                            *transfer
                                .lock()
                                .map_err(|_| "Edit preparation lock poisoned")? = Some(prepared);
                            Ok(super::super::NativeMutationResult {
                                context: current.owner,
                                engine_revision: current.revision,
                                value: Value::Null,
                            })
                        },
                    )
                },
                move |world, services, result| {
                    let result = result?;
                    let prepared = output
                        .lock()
                        .map_err(|_| "Edit preparation lock poisoned")?
                        .take()
                        .ok_or("Edit preparation was lost")?;
                    let mut state = world.remove_resource::<NativeFeature>().unwrap_or_default();
                    let value = with_receipt(
                        &services.bridge,
                        &services.engine,
                        &result.context,
                        |receipt| {
                            if receipt != prepared.snapshot.receipt {
                                return Err("The design changed before the editor opened".into());
                            }
                            crate::native_editor::support::cancel(world, &receipt.owner)?;
                            install(world, &mut state, prepared)
                        },
                    );
                    world.insert_resource(state);
                    value
                },
            );
        }
    }
    with_receipt(bridge, engine, owner, |current| {
        if current != receipt {
            return Err("The design changed before the editor opened".into());
        }
        install(world, state, prepare(engine, current, kind, id)?)
    })
}
