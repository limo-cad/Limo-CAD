use super::constraints::Relation;
use super::*;
use limo_cad_sketch::{
    ConstraintId, DimensionRequest, DragPhase, EditDimensionRequest, EntityDto, EntityId,
    MovePointRequest,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModifyTool {
    Trim,
    Extend,
    Break,
}
impl ModifyTool {
    pub fn label(self) -> &'static str {
        match self {
            Self::Trim => "Trim",
            Self::Extend => "Extend",
            Self::Break => "Break",
        }
    }
    pub fn operation(self) -> &'static str {
        match self {
            Self::Trim => "sketch_trim",
            Self::Extend => "sketch_extend",
            Self::Break => "sketch_break",
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum InteractionCommand {
    Select,
    Delete,
    Relation(Relation),
    Modify(ModifyTool),
    Dimension,
    EditDimension(ConstraintId),
    DeleteDimension,
    DimensionReference,
    RepositionDimension,
    ConstraintInfo(ConstraintId),
    DeleteConstraint,
    DimensionText(String),
    ApplyDimension,
    CancelDimension,
    DimensionActions,
    Menu(Option<&'static str>),
    Form(super::forms::FormKind),
    FormValue { id: u64, index: usize, text: String },
    FormOption { id: u64 },
    ApplyForm { id: u64 },
    CancelForm { id: u64 },
}
#[derive(Default)]
pub(super) struct Interaction {
    pub selection: Vec<EntityId>,
    pub relation: Option<Relation>,
    pub modify: Option<ModifyTool>,
    pub dimension: Option<String>,
    pub dimension_position: Option<SketchPoint>,
    pub dimension_id: Option<ConstraintId>,
    pub dimension_reference: bool,
    pub dimension_actions: bool,
    pub reposition_dimension: Option<ConstraintId>,
    pub constraint: Option<limo_cad_sketch::ConstraintDto>,
    pub menu: Option<&'static str>,
    pub form: Option<super::forms::ModifyForm>,
}
impl Interaction {
    /// Changing tool keeps the user's geometry selection, but never carries a
    /// dimension/constraint/form draft into a different interaction mode.
    fn retain_selection(&mut self) {
        *self = Self {
            selection: std::mem::take(&mut self.selection),
            ..Default::default()
        };
    }
    pub fn instruction(&self) -> Option<String> {
        if self.reposition_dimension.is_some() {
            return Some("Click to place the dimension (Escape cancels)".into());
        }
        if let Some(relation) = self.relation {
            Some(format!(
                "{}: select geometry (Escape cancels)",
                relation.label()
            ))
        } else if let Some(tool) = self.modify {
            Some(format!("{}: click a curve (Escape cancels)", tool.label()))
        } else if self.dimension.is_some() {
            Some(
                if self.dimension_id.is_some() {
                    "Edit the dimension, or reposition its label"
                } else {
                    "Select geometry, then click to place its dimension"
                }
                .into(),
            )
        } else if let Some(form) = &self.form {
            Some(form.kind.instruction().into())
        } else if !self.selection.is_empty() {
            Some(format!(
                "{} selected · Shift-click adds/removes · Delete removes",
                self.selection.len()
            ))
        } else {
            None
        }
    }
}

pub(super) fn present(
    world: &mut World,
    owner: &DocumentContext,
    state: &Interaction,
) -> Result<(), String> {
    let (_, _, mut view, _) = native_viewport::interface_view_snapshot(world);
    let selected: Vec<_> = state.selection.iter().map(|id| id.0).collect();
    let related = state
        .constraint
        .as_ref()
        .map(|c| {
            c.constraint
                .referenced_entities()
                .into_iter()
                .map(|id| id.0)
                .collect()
        })
        .unwrap_or_default();
    if view.selected_sketch_entity_ids != selected
        || view.constraint_related_sketch_entity_ids != related
    {
        view.selected_sketch_entity_ids = selected;
        view.constraint_related_sketch_entity_ids = related;
        view.hovered_sketch_entity_id = None;
        native_viewport::apply_interface_view(world, &owner.document_id, None, Some(view))?;
    }
    Ok(())
}
fn mutate(world: &mut World, editor: &Editor, mut command: Prepared) -> Result<Value, String> {
    if command.operation == "sketch_polygon" {
        snapping::attach(&mut command.arguments, editor.draft.snap_context);
    }
    queue_mutation(
        world,
        editor.stamp.as_ref().ok_or("No active sketch")?.clone(),
        command.operation,
        command.arguments,
        Completion::Modify,
    )
}

#[derive(Resource, Default)]
struct HoverCache(
    Option<(Stamp, SketchDto)>,
    Option<(Option<u64>, Option<[u32; 2]>)>,
);

pub(super) fn hover(
    world: &mut World,
    services: &NativeServices,
    owner: &DocumentContext,
    editor: &Editor,
    cursor: Option<Vec2>,
    canvas: InterfaceRect,
) -> Result<(), String> {
    let Some(stamp) = editor.stamp.as_ref().filter(|s| s.sketch.is_some()) else {
        return Ok(());
    };
    let mut cache = world.remove_resource::<HoverCache>().unwrap_or_default();
    let result = (|| {
        if cache.0.as_ref().is_none_or(|(old, _)| old != stamp) {
            cache.0 = active(&services.engine)?.map(|sketch| (stamp.clone(), sketch));
            cache.1 = None;
        }
        let hit = match (cursor, cache.0.as_ref()) {
            (Some(cursor), Some((_, sketch))) => selection::hit(
                &sketch.entities,
                [cursor.x - canvas.x as f32, cursor.y - canvas.y as f32],
                editor.interaction.modify.is_none(),
                |p| {
                    native_viewport::interface_world_point(
                        world,
                        &owner.document_id,
                        sketch.basis.to_3d([p.x, p.y]),
                    )
                    .ok()
                    .flatten()
                },
            ),
            _ => None,
        }
        .map(|id| id.0);
        let (_, _, mut view, _) = native_viewport::interface_view_snapshot(world);
        if view.hovered_sketch_entity_id != hit {
            view.hovered_sketch_entity_id = hit;
            native_viewport::apply_interface_view(world, &owner.document_id, None, Some(view))?;
        }
        if editor.interaction.modify == Some(ModifyTool::Trim) {
            let key = (hit, cursor.map(|p| [p.x.to_bits(), p.y.to_bits()]));
            if cache.1 != Some(key) {
                cache.1 = Some(key);
                let preview = match (hit, cursor, cache.0.as_ref()) {
                    (Some(id), Some(p), Some((_, sketch))) => {
                        native_viewport::interface_sketch_point(
                            world,
                            &owner.document_id,
                            [p.x - canvas.x as f32, p.y - canvas.y as f32],
                            sketch.basis,
                        )?
                        .and_then(|p| {
                            modify_preview::trim(&services.engine, sketch, EntityId(id), p).ok()
                        })
                        .unwrap_or_default()
                    }
                    _ => ViewportPreview::default(),
                };
                native_viewport::apply_interface_preview(world, &owner.document_id, preview)?;
            }
        } else {
            cache.1 = None;
        }
        Ok(())
    })();
    world.insert_resource(cache);
    result
}

pub(super) fn execute(
    world: &mut World,
    engine: &AppState,
    bridge: &SessionBridgeState,
    owner: &DocumentContext,
    editor: &mut Editor,
    command: InteractionCommand,
    validate: impl FnOnce() -> Result<(), String>,
) -> Result<Value, String> {
    bridge.with_native_document_owner(engine, owner, validate)?;
    if matches!(command, InteractionCommand::EditDimension(id) if annotations::editing_dimension(editor, id))
    {
        // Repeated activation belongs to the same retained edit: restore its
        // field focus without replacing a pending expression or generation.
        panel::retry_dimension_focus(world);
        return Ok(json!({"handled":true,"dimension_refocused":true}));
    }
    if matches!(
        command,
        InteractionCommand::EditDimension(_) | InteractionCommand::ConstraintInfo(_)
    ) && annotations::geometry_tool_active(editor)
    {
        return Err("Finish or cancel the active tool before editing an annotation".into());
    }
    let sketch = active(engine)?.ok_or("Start or edit a sketch first")?;
    editor.draft.select(None);
    editor.press = None;
    editor.error.clear();
    editor.interaction.menu = None;
    clear_preview(world, engine, bridge, owner)?;
    match command {
        InteractionCommand::Select => editor.interaction = Default::default(),
        InteractionCommand::Menu(menu) => editor.interaction.menu = menu,
        InteractionCommand::Modify(tool) => {
            editor.interaction = Interaction {
                modify: Some(tool),
                ..Default::default()
            }
        }
        InteractionCommand::Delete => {
            if editor.interaction.selection.is_empty() {
                return Err("Select geometry to delete".into());
            }
            return mutate(
                world,
                editor,
                Prepared {
                    operation: "sketch_delete_entities",
                    arguments: json!({"entity_ids":editor.interaction.selection}),
                },
            );
        }
        InteractionCommand::Relation(relation) => {
            editor.interaction.retain_selection();
            if let Some(command) = relation.prepare(&sketch, &editor.interaction.selection)? {
                return mutate(world, editor, command);
            }
            editor.interaction.relation = Some(relation);
        }
        InteractionCommand::Dimension => {
            editor.form_serial = editor
                .form_serial
                .checked_add(1)
                .ok_or("Editor generation exhausted")?;
            editor.interaction.retain_selection();
            editor.interaction.dimension = Some(String::new());
            editor.interaction.dimension_position = None;
            editor.interaction.dimension_id = None;
        }
        InteractionCommand::EditDimension(id) => {
            editor.form_serial = editor
                .form_serial
                .checked_add(1)
                .ok_or("Editor generation exhausted")?;
            let dim = sketch
                .dimensions
                .iter()
                .find(|d| d.constraint_id == id)
                .ok_or("The dimension no longer exists")?;
            editor.interaction = Interaction {
                dimension_id: Some(id),
                dimension_reference: dim.mode == limo_cad_sketch::DimensionMode::Reference,
                dimension: Some(
                    dim.param_expression
                        .clone()
                        .unwrap_or_else(|| dim.value.to_string()),
                ),
                dimension_position: Some(dim.text_pos),
                selection: dim.entities.clone(),
                ..Default::default()
            };
        }
        InteractionCommand::DeleteDimension => {
            let id = editor
                .interaction
                .dimension_id
                .ok_or("Select a dimension to delete")?;
            return mutate(
                world,
                editor,
                Prepared {
                    operation: "sketch_delete_dimension",
                    arguments: json!({"constraint_id":id}),
                },
            );
        }
        InteractionCommand::DimensionReference => {
            let id = editor
                .interaction
                .dimension_id
                .ok_or("Select a dimension")?;
            let dim = sketch
                .dimensions
                .iter()
                .find(|d| d.constraint_id == id)
                .ok_or("Dimension changed")?;
            let mode = if dim.mode == limo_cad_sketch::DimensionMode::Driving {
                "reference"
            } else {
                "driving"
            };
            return mutate(
                world,
                editor,
                Prepared {
                    operation: "sketch_set_dimension_mode",
                    arguments: json!({"constraint_id":id,"mode":mode}),
                },
            );
        }
        InteractionCommand::RepositionDimension => {
            let id = editor
                .interaction
                .dimension_id
                .ok_or("Select a dimension")?;
            editor.interaction = Interaction {
                reposition_dimension: Some(id),
                ..Default::default()
            };
        }
        InteractionCommand::ConstraintInfo(id) => {
            let constraint = *sketch
                .constraints
                .iter()
                .find(|c| c.id == id)
                .ok_or("Constraint changed")?;
            editor.interaction = Interaction {
                constraint: Some(constraint),
                ..Default::default()
            };
        }
        InteractionCommand::DeleteConstraint => {
            let id = editor
                .interaction
                .constraint
                .as_ref()
                .ok_or("Select a constraint")?
                .id;
            return mutate(
                world,
                editor,
                Prepared {
                    operation: "sketch_delete_constraint",
                    arguments: json!({"constraint_id":id}),
                },
            );
        }
        InteractionCommand::DimensionText(text) => {
            if editor.interaction.dimension_reference {
                return Err(
                    "Reference dimensions measure geometry; switch to Driving to change its size"
                        .into(),
                );
            }
            if text.len() > 1024 {
                return Err("Dimension expression is too long".into());
            }
            *editor
                .interaction
                .dimension
                .as_mut()
                .ok_or("Dimension editor is closed")? = text;
        }
        InteractionCommand::CancelDimension => {
            editor.interaction.dimension = None;
            editor.interaction.dimension_position = None;
            editor.interaction.dimension_id = None;
            editor.interaction.dimension_actions = false;
        }
        InteractionCommand::DimensionActions => {
            editor.interaction.dimension_actions = !editor.interaction.dimension_actions;
        }
        InteractionCommand::ApplyDimension => {
            if editor.interaction.dimension_reference {
                editor.interaction.retain_selection();
                return Ok(json!({"closed":true}));
            }
            if editor.interaction.selection.is_empty() {
                return Err("Select one or two entities to dimension".into());
            }
            let text = editor
                .interaction
                .dimension
                .as_ref()
                .ok_or("Dimension editor is closed")?;
            let position = editor
                .interaction
                .dimension_position
                .ok_or("Click in the sketch to place the dimension")?;
            if let Some(id) = editor.interaction.dimension_id {
                let request = EditDimensionRequest {
                    constraint_id: id,
                    text: text.clone(),
                };
                return mutate(
                    world,
                    editor,
                    Prepared {
                        operation: "sketch_edit_dimension",
                        arguments: serde_json::to_value(request).map_err(|e| e.to_string())?,
                    },
                );
            }
            let request = DimensionRequest {
                entities: editor.interaction.selection.clone(),
                text_pos: position,
                value_text: (!text.trim().is_empty()).then(|| text.clone()),
            };
            return mutate(
                world,
                editor,
                Prepared {
                    operation: "sketch_add_dimension",
                    arguments: serde_json::to_value(request).map_err(|e| e.to_string())?,
                },
            );
        }
        InteractionCommand::Form(kind) => {
            editor.form_serial = editor
                .form_serial
                .checked_add(1)
                .ok_or("Form identities exhausted")?;
            editor.interaction.retain_selection();
            editor.interaction.form = Some(super::forms::ModifyForm::new(editor.form_serial, kind));
        }
        InteractionCommand::FormValue { id, index, text } => {
            if text.len() > 1024 {
                return Err("The expression is too long".into());
            }
            let form = editor
                .interaction
                .form
                .as_mut()
                .filter(|f| f.id == id)
                .ok_or("The sketch form changed")?;
            *form.values.get_mut(index).ok_or("Unknown sketch field")? = text;
        }
        InteractionCommand::FormOption { id } => {
            let form = editor
                .interaction
                .form
                .as_mut()
                .filter(|f| f.id == id)
                .ok_or("The sketch form changed")?;
            form.option = !form.option;
        }
        InteractionCommand::CancelForm { id } => {
            if editor.interaction.form.as_ref().is_none_or(|f| f.id != id) {
                return Err("The sketch form changed".into());
            }
            editor.interaction.form = None;
        }
        InteractionCommand::ApplyForm { id } => {
            let form = editor
                .interaction
                .form
                .as_ref()
                .filter(|f| f.id == id)
                .ok_or("The sketch form changed")?;
            let command = form.request(
                &sketch,
                &editor.interaction.selection,
                engine.document_units(),
            )?;
            return mutate(world, editor, command);
        }
    }
    present(world, owner, &editor.interaction)?;
    modify_preview::refresh_form(world, engine, owner, editor, &sketch)?;
    Ok(
        json!({"handled":true,"selection":editor.interaction.selection,"instruction":editor.interaction.instruction()}),
    )
}

pub(super) fn pointer(
    world: &mut World,
    (services, owner): (&NativeServices, &DocumentContext),
    editor: &mut Editor,
    (start, end, canvas): (Vec2, Vec2, InterfaceRect),
    shift: bool,
    ctrl: bool,
) -> Result<Value, String> {
    let sketch = active(&services.engine)?.ok_or("No active sketch")?;
    let local = |p: Vec2| [p.x - canvas.x as f32, p.y - canvas.y as f32];
    let at = local(start);
    let mut hit = selection::hit(
        &sketch.entities,
        at,
        editor.interaction.modify.is_none(),
        |p| {
            native_viewport::interface_world_point(
                world,
                &owner.document_id,
                sketch.basis.to_3d([p.x, p.y]),
            )
            .ok()
            .flatten()
        },
    );
    let mut end_point = native_viewport::interface_sketch_point(
        world,
        &owner.document_id,
        local(end),
        sketch.basis,
    )?
    .ok_or("The sketch is edge-on; use Look At")?;
    if editor
        .interaction
        .form
        .as_ref()
        .is_some_and(|form| form.kind == FormKind::Polygon)
    {
        let context = snapping::context(world, sketch.basis, end_point);
        end_point = snapping::acquire(&services.engine, &editor.draft, end_point, ctrl, context)?
            .snapped_to;
        editor.draft.snap_context = Some(context);
        hit = None;
    }
    if let Some(id) = editor.interaction.reposition_dimension {
        return mutate(
            world,
            editor,
            Prepared {
                operation: "sketch_move_dimension",
                arguments: json!({"constraint_id":id,"text_pos":end_point}),
            },
        );
    }
    if start.distance(end) > 3. {
        if editor.interaction.modify.is_some()
            || editor.interaction.relation.is_some()
            || editor.interaction.dimension.is_some()
            || editor.interaction.form.is_some()
        {
            return Err("Finish or cancel the active tool before dragging geometry".into());
        }
        if let Some(id) = hit {
            if sketch
                .entities
                .iter()
                .any(|e| matches!(e,EntityDto::Point{id:p,..} if *p==id))
            {
                let request = MovePointRequest {
                    point_id: id,
                    to_raw: end_point,
                    ctrl_held: ctrl,
                    phase: DragPhase::Single,
                };
                return mutate(
                    world,
                    editor,
                    Prepared {
                        operation: "sketch_move_point",
                        arguments: serde_json::to_value(request).map_err(|e| e.to_string())?,
                    },
                );
            }
            let origin = native_viewport::interface_sketch_point(
                world,
                &owner.document_id,
                at,
                sketch.basis,
            )?
            .ok_or("Cannot resolve drag origin")?;
            let ids = if editor.interaction.selection.contains(&id) {
                editor.interaction.selection.clone()
            } else {
                vec![id]
            };
            return mutate(
                world,
                editor,
                Prepared {
                    operation: "sketch_move_copy",
                    arguments: json!({"entity_ids":ids,"dx":end_point.x-origin.x,"dy":end_point.y-origin.y,"copy":false}),
                },
            );
        }
        return Ok(json!({"handled":true,"selection":editor.interaction.selection}));
    }
    if let Some(tool) = editor.interaction.modify {
        let id = hit.ok_or("Click a visible curve")?;
        let arguments = if tool == ModifyTool::Break {
            json!({"entity":id,"at":end_point})
        } else {
            json!({"entity":id,"click":end_point})
        };
        return mutate(
            world,
            editor,
            Prepared {
                operation: tool.operation(),
                arguments,
            },
        );
    }
    if let Some(id) = hit {
        let previous = editor.interaction.selection.clone();
        let accumulating = shift
            || editor.interaction.relation.is_some()
            || editor.interaction.dimension.is_some()
            || editor.interaction.form.is_some();
        if !accumulating {
            editor.interaction.selection.clear();
        }
        if let Some(index) = editor.interaction.selection.iter().position(|v| *v == id) {
            if shift {
                editor.interaction.selection.remove(index);
            }
        } else {
            editor.interaction.selection.push(id);
        }
        if let Some(relation) = editor.interaction.relation {
            let prepared = relation.prepare(&sketch, &editor.interaction.selection);
            if prepared.is_err() {
                editor.interaction.selection = previous;
            }
            if let Some(command) = prepared? {
                return mutate(world, editor, command);
            }
        }
    } else if let Some(form) = &mut editor.interaction.form {
        form.point = Some(end_point);
        if form.kind == FormKind::Polygon {
            let scale = match services.engine.document_units() {
                limo_cad_core::UnitSystem::Mm => 1.,
                limo_cad_core::UnitSystem::Cm => 10.,
                limo_cad_core::UnitSystem::In => 25.4,
            };
            form.values[0] = (end_point.x / scale).to_string();
            form.values[1] = (end_point.y / scale).to_string();
        }
    } else if editor.interaction.dimension.is_some() && !editor.interaction.selection.is_empty() {
        editor.interaction.dimension_position = Some(end_point);
    } else if !shift {
        editor.interaction.selection.clear();
    }
    present(world, owner, &editor.interaction)?;
    editor.error.clear();
    modify_preview::refresh_form(world, &services.engine, owner, editor, &sketch)?;
    Ok(
        json!({"handled":true,"selection":editor.interaction.selection,"dimension_placed":editor.interaction.dimension_position.is_some()}),
    )
}
