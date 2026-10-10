//! Non-destructive joint editing and topology picking over the shared solver.
use super::*;
use crate::native_forms::joint::{self as form, Connector, Form};
use crate::native_viewport::{
    NativePick, NativePickPurpose, ViewportPresentation, ViewportPreview,
};
use limo_cad_sketch::{AssemblySolutionDto, JointConnectorDto, OccurrenceId};
pub(super) mod panel;
#[cfg(test)]
pub(super) mod tests;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Field(form::Field),
    Pick(usize),
    Clear,
    Ground(usize),
    Orientation,
    ChooseKind(usize),
    Apply,
    Cancel,
    Scroll(i32),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Open(Option<u64>),
    Enabled(u64),
    Delete(u64),
    Control { id: u64, action: Action },
}
struct Editor {
    id: u64,
    owner: DocumentContext,
    revision: u64,
    assembly: Arc<AssemblyDocumentDto>,
    form: Form,
    pick: Option<usize>,
    orientation: bool,
    choice: bool,
    scroll: f32,
    max_scroll: f32,
    error: Option<String>,
    original_view: ViewportPresentation,
    original_preview: Arc<ViewportPreview>,
    preview_revision: u64,
}
#[derive(Resource, Default)]
struct State {
    serial: u64,
    editor: Option<Editor>,
    widgets: chrome::Widgets,
    bounds: Option<InterfaceRect>,
}
pub(crate) fn active(world: &World) -> bool {
    world
        .get_resource::<State>()
        .is_some_and(|s| s.editor.is_some())
}
fn restore(world: &mut World, e: &Editor) -> Result<(), String> {
    let (id, _, mut v, _) = native_viewport::interface_view_snapshot(world);
    if id != e.owner.document_id {
        return Ok(());
    }
    v.body_poses = e.original_view.body_poses.clone();
    v.instance_body_poses = e.original_view.instance_body_poses.clone();
    v.hovered_body_id = None;
    v.hovered_face_id = None;
    v.hovered_edge_id = None;
    v.hovered_occurrence_id = None;
    native_viewport::apply_interface_view(world, &id, None, Some(v))?;
    native_viewport::apply_interface_preview(world, &id, e.original_preview.clone())
}
fn markers(world: &mut World, e: &mut Editor, hover: Option<&Connector>) -> Result<(), String> {
    let mut preview = e.original_preview.as_ref().clone();
    let (_, camera, _, size) = native_viewport::interface_view(world);
    let length = (Vec3::from_array(camera.position).distance(Vec3::from_array(camera.target))
        * (camera.vertical_fov_degrees.to_radians() * 0.5).tan()
        * 64.
        / size[1].max(1.))
    .max(0.01);
    for (i, c) in e
        .form
        .connectors
        .iter()
        .enumerate()
        .filter_map(|(i, c)| c.as_ref().map(|c| (i, c)))
        .chain(hover.map(|c| (2, c)))
    {
        let transform = native_viewport::interface_body_transform(
            world,
            c.connector.body_id.0,
            Some(c.occurrence.0),
        );
        let origin =
            transform.transform_point(Vec3::from_array(c.connector.frame.origin.map(|v| v as f32)));
        let axis = (transform.rotation
            * Vec3::from_array(c.connector.frame.primary_axis.map(|v| v as f32)))
        .normalize_or_zero();
        let secondary = (transform.rotation
            * Vec3::from_array(c.connector.frame.secondary_axis.map(|v| v as f32)))
        .normalize_or_zero();
        let color = match i {
            0 => [0.42, 0.56, 1., 1.],
            1 => [1., 0.66, 0.28, 1.],
            _ => [0.83, 0.77, 1., 1.],
        };
        preview.points.push(native_viewport::ViewportPointLayer {
            color,
            color_role: Default::default(),
            radius: length * 0.08,
            hollow: false,
            positions: origin.to_array().to_vec().into(),
        });
        for (direction, scale) in [(axis, 1.), (secondary, 0.65)] {
            preview.arrows.push(native_viewport::ViewportArrow {
                start: origin.to_array(),
                end: (origin + direction * length * scale).to_array(),
                color,
                width: 1.5,
                xray: false,
            });
        }
    }
    native_viewport::apply_interface_preview(world, &e.owner.document_id, preview)?;
    e.preview_revision = native_viewport::interface_preview_revision(world);
    Ok(())
}
fn connector(world: &World, hit: NativePick) -> Result<Connector, String> {
    let occurrence = OccurrenceId(
        hit.occurrence_id
            .ok_or("Pick a visible component instance")?,
    );
    let geometry = native_viewport::interface_geometry(world);
    let body = geometry
        .scene
        .bodies
        .iter()
        .find(|b| b.id.0 == hit.body_id)
        .ok_or("The picked body no longer exists")?;
    let face = body.faces.iter().find(|f| f.id.0 == hit.face_id);
    let edge = hit
        .edge_id
        .and_then(|id| body.edges.iter().find(|e| e.id.0 == id));
    let kind = hit
        .connector_kind
        .ok_or("Pick a planar face, cylindrical surface or circular edge")?;
    let frame = json!({"origin":hit.connector_origin.ok_or("Connector has no origin")?,"primary_axis":hit.connector_primary_axis.ok_or("Connector has no axis")?,"secondary_axis":hit.connector_secondary_axis.ok_or("Connector has no reference")?});
    let connector:JointConnectorDto=serde_json::from_value(json!({"body_id":body.id,"face_id":hit.face_id,"face_key":face.map(|f|f.key.as_str()).unwrap_or(""),"edge_id":edge.map(|e|e.id),"edge_key":edge.map(|e|e.key.as_str()),"kind":kind,"radius":hit.connector_radius,"frame":frame})).map_err(|e|e.to_string())?;
    Ok(Connector {
        connector,
        occurrence,
        label: body.name.clone(),
    })
}
fn with_picked_connector(mut result: Value, picked: Option<Value>) -> Value {
    if let Some(picked) = picked {
        result["handled"] = json!(true);
        result["picked_connector"] = picked;
    }
    result
}
fn preview(
    world: &mut World,
    engine: &AppState,
    bridge: &SessionBridgeState,
    e: &mut Editor,
    a: &AssemblyDocumentDto,
    picked: Option<Value>,
) -> Result<Value, String> {
    let (operation, args) = match e.form.request(a) {
        Ok(v) => v,
        Err(error) => {
            e.error = None;
            restore(world, e)?;
            markers(world, e, None)?;
            return Ok(with_picked_connector(
                json!({"changed":true,"valid":false,"reason":error}),
                picked,
            ));
        }
    };
    let operation = if operation == "assembly_update_joint" {
        "assembly_preview_joint_update"
    } else {
        "assembly_preview_joint"
    };
    let owner = e.owner.clone();
    let revision = e.revision;
    let id = e.id;
    if worker::available(world) {
        let complete = move |world: &mut World,
                             services: &NativeServices,
                             result: Result<NativeMutationResult, String>|
              -> Result<Value, String> {
            let mut state = world
                .remove_resource::<State>()
                .ok_or("Joint editor was closed")?;
            let result = (|| {
                let e = state
                    .editor
                    .as_mut()
                    .filter(|e| e.id == id)
                    .ok_or("Joint editor was replaced")?;
                let preview_owner = e.owner.clone();
                services.bridge.with_native_document_receipt(&services.engine,&preview_owner,|rev| {
                if rev!=e.revision {return Err("The model changed during the joint preview".into());}
                match result {
                    Ok(result)=>{
                        let solution:AssemblySolutionDto=serde_json::from_value(result.value).map_err(|e|e.to_string())?;
                        let (_,_,mut view,_)=native_viewport::interface_view_snapshot(world);
                        view.body_poses=solution.body_poses.into();view.instance_body_poses=solution.instance_body_poses.into();
                        native_viewport::apply_interface_view(world,&e.owner.document_id,None,Some(view))?;
                        e.error=(!solution.solved).then(||"The proposed joint graph could not be solved; adjust the connectors or offsets".into());
                        markers(world,e,None)?;
                        Ok(json!({"preview":true,"solved":solution.solved,"diagnostics":solution.diagnostics}))
                    }
                    Err(error)=>{restore(world,e)?;e.error=Some(error.clone());markers(world,e,None)?;Ok(json!({"preview":false,"error":error}))}
                }
            })
            })();
            world.insert_resource(state);
            result.map(|result| with_picked_connector(result, picked))
        };
        worker::enqueue_query(world, owner, revision, operation.into(), args, complete)
    } else {
        let value = bridge.with_native_document_receipt(engine, &owner, |rev| {
            if rev != revision {
                return Err("The model changed before the joint preview".into());
            }
            parse_engine_envelope(engine.engine_call(operation, &args.to_string()))
        })?;
        let solution: AssemblySolutionDto =
            serde_json::from_value(value).map_err(|e| e.to_string())?;
        let (_, _, mut view, _) = native_viewport::interface_view_snapshot(world);
        view.body_poses = solution.body_poses.into();
        view.instance_body_poses = solution.instance_body_poses.into();
        native_viewport::apply_interface_view(world, &owner.document_id, None, Some(view))?;
        e.error = (!solution.solved).then(|| "Joint graph could not be solved".into());
        markers(world, e, None)?;
        Ok(with_picked_connector(
            json!({"preview":true,"solved":solution.solved}),
            picked,
        ))
    }
}
pub(crate) fn reduce(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    engine: &AppState,
    bridge: &SessionBridgeState,
    action: &NativeInterfaceAction,
    command: &Command,
) -> Result<Value, String> {
    let receipt = bridge.native_document_receipt(engine, &action.context)?;
    bridge
        .with_native_document_owner(engine, &action.context, || handle.validate_action(action))?;
    if native_viewport::interface_geometry(world)
        .active_sketch
        .is_some()
        || feature::panel(world).is_some()
    {
        return Err("Finish the current modeling command before editing joints".into());
    }
    let a = bridge.with_native_document_receipt(engine, &action.context, |revision| {
        if revision != receipt.revision {
            return Err("The assembly changed; use its refreshed controls".into());
        }
        document(engine)
    })?;
    let mut state = world.remove_resource::<State>().unwrap_or_default();
    let result = (|| {
        if let Command::Open(id) = command {
            if !super::super::super::is_activation(&action.control.input) {
                return Err("Activate the joint editor".into());
            }
            if state.editor.is_some() {
                return Err("Finish or cancel the open joint".into());
            }
            let original = id
                .map(|id| {
                    a.joints
                        .iter()
                        .find(|j| j.id.0 == id)
                        .cloned()
                        .ok_or("Joint no longer exists")
                })
                .transpose()?;
            state.serial = state
                .serial
                .checked_add(1)
                .ok_or("Joint editor identifiers exhausted")?;
            let (_, _, view, _) = native_viewport::interface_view(world);
            let mut e = Editor {
                id: state.serial,
                owner: receipt.owner.clone(),
                revision: receipt.revision,
                assembly: Arc::new(a.clone()),
                form: Form::new(&a, original, engine.document_units()),
                pick: id.is_none().then_some(0),
                orientation: false,
                choice: false,
                scroll: 0.,
                max_scroll: 0.,
                error: None,
                original_view: view.clone(),
                original_preview: native_viewport::interface_preview_snapshot(world),
                preview_revision: native_viewport::interface_preview_revision(world),
            };
            markers(world, &mut e, None)?;
            state.editor = Some(e);
            return Ok(json!({"joint_editor":state.serial}));
        }
        if let Command::Enabled(id) | Command::Delete(id) = command {
            if state.editor.is_some() {
                return Err("Finish the open joint editor".into());
            }
            if !super::super::super::is_activation(&action.control.input) {
                return Err("Activate the joint control".into());
            }
            let j = a
                .joints
                .iter()
                .find(|j| j.id.0 == *id)
                .ok_or("Joint no longer exists")?;
            return mutation(
                world,
                engine,
                bridge,
                &receipt.owner,
                receipt.revision,
                if matches!(command, Command::Delete(_)) {
                    "assembly_delete_joint"
                } else {
                    "assembly_set_joint_enabled"
                },
                json!({"joint_id":id,"enabled":!j.enabled}),
            );
        }
        let Command::Control { id, action: change } = command else {
            unreachable!()
        };
        let e = state
            .editor
            .as_mut()
            .filter(|e| e.id == *id && e.owner == receipt.owner && e.revision == receipt.revision)
            .ok_or("Joint editor changed; use its refreshed controls")?;
        let input = &action.control.input;
        let activate = super::super::super::is_activation(input);
        let mut update = true;
        match change {
            Action::Field(field) => {
                let value = match (field, input) {
                    (_, ControlInput::SetValue(value)) => Some(value.clone()),
                    (form::Field::Limited(i), _) if activate => {
                        Some((!e.form.coordinates[*i].limited).to_string())
                    }
                    (form::Field::Flipped, _) if activate => Some((!e.form.flipped).to_string()),
                    (form::Field::Kind, ControlInput::Key(key))
                        if !activate && !key.ctrl && !key.meta && !key.alt && !key.shift =>
                    {
                        let i = form::KINDS
                            .iter()
                            .position(|(k, _, _)| *k == e.form.kind)
                            .unwrap();
                        let i = match key.key.as_str() {
                            "ArrowDown" => (i + 1).min(form::KINDS.len() - 1),
                            "ArrowUp" => i.saturating_sub(1),
                            "Home" => 0,
                            "End" => form::KINDS.len() - 1,
                            _ => return Err("Unsupported joint type key".into()),
                        };
                        Some(form::KINDS[i].1.into())
                    }
                    (form::Field::Kind, _) if activate => {
                        e.choice = !e.choice;
                        None
                    }
                    (_, _) if activate => None,
                    _ => return Err("Edit the joint field with the matching control input".into()),
                };
                if let Some(value) = value {
                    e.form.set(*field, &value)?;
                } else {
                    update = false;
                }
            }
            _ if !activate => return Err("Activate the joint control".into()),
            Action::Pick(i) => {
                if *i >= 2 {
                    return Err("Unknown connector".into());
                }
                e.pick = Some(*i);
                update = false;
            }
            Action::Clear => {
                e.form.connectors = [None, None];
                e.form.ground_changed = false;
                e.form.infer_ground(&a);
                e.pick = Some(0);
            }
            Action::Ground(i) => {
                if *i >= 2 {
                    return Err("Unknown fixed component".into());
                }
                e.form.fixed = Some(*i);
                e.form.ground_changed = true;
            }
            Action::Orientation => {
                e.orientation = !e.orientation;
                update = false;
            }
            Action::ChooseKind(i) => {
                e.form.kind = form::KINDS.get(*i).ok_or("Joint type unavailable")?.0;
                e.choice = false;
            }
            Action::Scroll(delta) => {
                e.scroll = (e.scroll + *delta as f32).clamp(0., e.max_scroll);
                update = false;
            }
            Action::Cancel => {
                restore(world, e)?;
                state.editor = None;
                return Ok(json!({"cancelled":true}));
            }
            Action::Apply => {
                if let Some(error) = &e.error {
                    return Err(error.clone());
                }
                let (operation, args) = e.form.request(&a)?;
                let owner = e.owner.clone();
                let revision = e.revision;
                let id = e.id;
                let complete =
                    move |world: &mut World,
                          services: &NativeServices,
                          result: Result<NativeMutationResult, String>| {
                        match result {
                            Ok(result) => {
                                if let Some(mut state) = world.get_resource_mut::<State>() {
                                    if state.editor.as_ref().is_some_and(|e| e.id == id) {
                                        state.editor = None;
                                    }
                                }
                                native_viewport::apply_interface_preview(
                                    world,
                                    &result.context.document_id,
                                    ViewportPreview::default(),
                                )?;
                                Ok(finish_mutation(
                                    &services.engine,
                                    &services.bridge,
                                    world,
                                    operation,
                                    result,
                                ))
                            }
                            Err(error) => {
                                if let Some(mut state) = world.get_resource_mut::<State>() {
                                    if let Some(e) = state.editor.as_mut().filter(|e| e.id == id) {
                                        e.error = Some(error.clone());
                                    }
                                }
                                Err(error)
                            }
                        }
                    };
                return worker::enqueue_operation(
                    world,
                    owner,
                    revision,
                    operation.into(),
                    args,
                    complete,
                );
            }
        }
        if update {
            e.error = None;
            preview(world, engine, bridge, e, &a, None)
        } else {
            markers(world, e, None)?;
            Ok(json!({"changed":true}))
        }
    })();
    world.insert_resource(state);
    result
}
pub(crate) fn canvas(
    world: &mut World,
    services: &NativeServices,
    owner: &DocumentContext,
    point: Option<[f32; 2]>,
    click: bool,
) -> Result<Option<Value>, String> {
    let Some(mut state) = world.remove_resource::<State>() else {
        return Ok(None);
    };
    let result = (|| {
        let Some(e) = state.editor.as_mut() else {
            return Ok(None);
        };
        let receipt = services
            .bridge
            .native_document_receipt(&services.engine, owner)?;
        if e.owner != *owner || e.revision != receipt.revision {
            return Err("The joint editor belongs to an earlier model".into());
        }
        let Some(index) = e.pick else {
            return Ok(Some(json!({"handled":true})));
        };
        let hit = point
            .map(|p| {
                native_viewport::interface_pick(
                    world,
                    &owner.document_id,
                    p,
                    NativePickPurpose::JointConnector,
                )
            })
            .transpose()?
            .flatten()
            .and_then(|h| connector(world, h).ok());
        if click {
            let mut hit = hit.ok_or("Pick a planar face, cylindrical surface or circular edge")?;
            if e.form.connectors[1 - index]
                .as_ref()
                .is_some_and(|c| c.occurrence == hit.occurrence)
            {
                return Err("Pick another component instance".into());
            }
            let a = e.assembly.clone();
            if let Some(o) = a
                .component_structure
                .occurrences
                .iter()
                .find(|o| o.id == hit.occurrence)
            {
                hit.label = o.name.clone();
            }
            let picked = json!({
                "slot": if index == 0 { "a" } else { "b" },
                "occurrence_id": hit.occurrence,
                "occurrence_name": hit.label,
                "connector": hit.connector,
            });
            e.form.connectors[index] = Some(hit);
            e.form.infer_ground(&a);
            e.pick = e.form.connectors.iter().position(Option::is_none);
            let result = preview(
                world,
                &services.engine,
                &services.bridge,
                e,
                &a,
                Some(picked),
            )?;
            Ok(Some(result))
        } else {
            markers(world, e, hit.as_ref())?;
            Ok(Some(json!({"handled":true,"hover":hit.is_some()})))
        }
    })();
    world.insert_resource(state);
    result
}
pub(crate) fn cancel(
    world: &mut World,
    engine: &AppState,
    bridge: &SessionBridgeState,
    owner: &DocumentContext,
) -> Result<Value, String> {
    let Some(mut state) = world.remove_resource::<State>() else {
        return Ok(json!({"cancelled":false}));
    };
    let result = bridge.with_native_document_receipt(engine, owner, |revision| {
        let Some(e) = state.editor.take() else {
            return Ok(json!({"cancelled":false}));
        };
        if e.owner == *owner && e.revision == revision {
            restore(world, &e)?;
        } else if e.owner.document_id == owner.document_id
            && native_viewport::interface_preview_revision(world) == e.preview_revision
        {
            native_viewport::apply_interface_preview(
                world,
                &owner.document_id,
                ViewportPreview::default(),
            )?;
        }
        Ok(json!({"cancelled":true}))
    });
    world.insert_resource(state);
    result
}
pub(crate) fn scroll(world: &mut World, point: [f32; 2], delta: f32) -> bool {
    let Some(mut state) = world.get_resource_mut::<State>() else {
        return false;
    };
    let Some(b) = state.bounds else {
        return false;
    };
    if point[0] < b.x as f32
        || point[0] >= (b.x + b.width) as f32
        || point[1] < b.y as f32
        || point[1] >= (b.y + b.height) as f32
    {
        return false;
    }
    if let Some(e) = &mut state.editor {
        e.scroll = (e.scroll - delta).clamp(0., e.max_scroll);
        true
    } else {
        false
    }
}
pub(crate) fn synchronize(
    world: &mut World,
    _services: &NativeServices,
    owner: &DocumentContext,
    revision: u64,
    bounds: InterfaceRect,
) -> Result<(), String> {
    let mut state = world.remove_resource::<State>().unwrap_or_default();
    let result = (|| {
        state.widgets.begin();
        state.bounds = state.editor.as_ref().map(|_| bounds);
        if state
            .editor
            .as_ref()
            .is_some_and(|e| e.owner != *owner || e.revision != revision)
        {
            let old = state.editor.take().unwrap();
            if old.owner == *owner
                && native_viewport::interface_preview_revision(world) == old.preview_revision
            {
                native_viewport::apply_interface_preview(
                    world,
                    &owner.document_id,
                    ViewportPreview::default(),
                )?;
            }
        }
        if let Some(e) = state.editor.as_mut() {
            let a = e.assembly.clone();
            panel::paint(world, &mut state.widgets, e, &a, bounds)?;
        }
        state.widgets.finish(world);
        Ok(())
    })();
    world.insert_resource(state);
    result
}
