//! Native interaction state over the shared CAD engine. Unfinished gestures
//! are scoped to one document incarnation, engine revision and active sketch.

mod annotations;
mod measurement;
pub(crate) mod mechanism;
mod solid;
use crate::session_bridge::native_interface::controller::assembly::joint;
mod dynamic;
pub(crate) mod support;
pub(crate) use dynamic::SizeField;
mod modify_preview;
mod palette;
pub(crate) use palette::PaletteCommand;
mod constraints;
mod forms;
pub(crate) mod mcp;
pub(crate) mod selection;
mod sketch;
mod snapping;
pub(crate) use forms::FormKind;
mod interaction;
pub(crate) mod panel;
pub(crate) use interaction::{InteractionCommand, ModifyTool};

use crate::{
    native_viewport::{
        self,
        interface_shell::{spawn_button, InterfaceCamera, InterfaceControl, NativeInterfaceHandle},
        ui::ViewportUiAssets,
        winit_host::NativeHostInput,
        ViewportCamera, ViewportPreview,
    },
    session_bridge::{
        native_interface::{
            bind_command,
            controller::{worker, NativeServices},
            finish_mutation, NativeCommand,
        },
        SessionBridgeState,
    },
    state::AppState,
};
use bevy::{ecs::system::SystemState, input::ButtonState, prelude::*, window::WindowEvent};
use limo_cad_core::{PlaneBasis, PlaneRef};
use limo_cad_interface::{DocumentContext, Rect as InterfaceRect};
use limo_cad_sketch::{CircleMode, RectangleMode, SketchDto, SlotMode, Vec2 as SketchPoint};
use serde_json::{json, Value};
pub(crate) use sketch::CreateTool;
use sketch::{Draft, Prepared};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum EditorCommand {
    Support(support::Command),
    Begin(PlaneRef),
    Edit(String),
    Finish,
    Tool(CreateTool),
    Cancel,
    Complete,
    Interaction(InteractionCommand),
    Palette(PaletteCommand),
    Size {
        generation: u64,
        field: SizeField,
        text: String,
    },
}

#[derive(Clone, Debug, PartialEq)]
struct Stamp {
    owner: DocumentContext,
    revision: u64,
    sketch: Option<String>,
    basis: Option<PlaneBasis>,
}

#[derive(Resource, Default)]
struct Editor {
    stamp: Option<Stamp>,
    draft: Draft,
    press: Option<(DocumentContext, Vec2)>,
    controls: HashMap<String, Entity>,
    error: String,
    creation_error: Option<String>,
    interaction: interaction::Interaction,
    form_serial: u64,
    support: support::Picker,
}

fn initialize(world: &mut World) {
    world.init_resource::<Editor>();
}

pub(crate) fn active(engine: &AppState) -> Result<Option<SketchDto>, String> {
    let result: Value = serde_json::from_str(&engine.engine_call("active_sketch", ""))
        .map_err(|e| e.to_string())?;
    if result["ok"] != true {
        return Err(format!("Cannot inspect active sketch: {}", result["error"]));
    }
    serde_json::from_value(result["value"].clone()).map_err(|e| e.to_string())
}

fn stamp(
    engine: &AppState,
    bridge: &SessionBridgeState,
    owner: &DocumentContext,
    prior: Option<&Stamp>,
) -> Result<Stamp, String> {
    bridge.with_native_document_receipt(engine, owner, |revision| {
        if let Some(prior) =
            prior.filter(|prior| prior.owner == *owner && prior.revision == revision)
        {
            return Ok(prior.clone());
        }
        let sketch = active(engine)?;
        Ok(Stamp {
            owner: owner.clone(),
            revision,
            basis: sketch.as_ref().map(|sketch| sketch.basis),
            sketch: sketch.map(|sketch| sketch.name),
        })
    })
}

fn synchronize_stamp(editor: &mut Editor, next: Stamp) -> bool {
    if editor.stamp.as_ref() == Some(&next) {
        return false;
    }
    editor.draft.select(None);
    editor.press = None;
    editor.error.clear();
    editor.creation_error = None;
    editor.interaction = Default::default();
    editor.support = Default::default();
    editor.stamp = Some(next);
    true
}

pub(crate) fn cancel_pointer(world: &mut World) {
    if let Some(mut editor) = world.get_resource_mut::<Editor>() {
        editor.press = None;
    }
    mechanism::cancel(world);
    crate::session_bridge::native_interface::feature::cancel_pointer(world);
}

pub(crate) fn status(world: &World) -> Option<String> {
    let editor = world.get_resource::<Editor>()?;
    if !editor.error.is_empty() {
        Some(editor.error.clone())
    } else if editor.support.active {
        Some(
            crate::native_viewport::localization::translate(world, "sketch.pickPlanePrompt").into(),
        )
    } else if editor.draft.tool.is_some() {
        Some(editor.draft.instruction().into())
    } else {
        editor.interaction.instruction()
    }
}

pub(crate) fn creation_context(world: &World) -> Option<(DocumentContext, String)> {
    let editor = world.get_resource::<Editor>()?;
    editor.draft.tool?;
    let stamp = editor.stamp.as_ref()?;
    Some((stamp.owner.clone(), stamp.sketch.clone()?))
}

pub(crate) fn creation_error_context(
    world: &World,
    error: &str,
) -> Option<(DocumentContext, String)> {
    let editor = world.get_resource::<Editor>()?;
    if editor.error != error || editor.creation_error.as_deref() != Some(error) {
        return None;
    }
    creation_context(world)
}

fn creation_failed(editor: &mut Editor, error: &str) {
    editor.error = error.into();
    editor.creation_error = Some(error.into());
}

pub(crate) fn synchronize_selection_readout(world: &mut World, visible: bool) {
    measurement::synchronize(world, visible);
}
pub(crate) fn selection_caption(world: &World) -> Option<String> {
    measurement::caption(world)
}

fn clear_preview(
    world: &mut World,
    engine: &AppState,
    bridge: &SessionBridgeState,
    owner: &DocumentContext,
) -> Result<(), String> {
    // A sketch cursor/focus transition does not own a solid feature's draft.
    // The feature picker removes its own hover while retaining its preview.
    if crate::session_bridge::native_interface::feature::panel(world).is_some() {
        return Ok(());
    }
    bridge.with_native_document_owner(engine, owner, || {
        native_viewport::apply_interface_preview(
            world,
            &owner.document_id,
            ViewportPreview::default(),
        )
    })
}

fn preview(
    world: &mut World,
    engine: &AppState,
    bridge: &SessionBridgeState,
    owner: &DocumentContext,
    editor: &mut Editor,
    raw: SketchPoint,
    ctrl: bool,
) -> Result<(), String> {
    use native_viewport::{
        ViewportColorRole, ViewportLineLayer, ViewportPointLayer, ViewportSnapKind,
        ViewportSnapMarker,
    };
    let Some(basis) = editor.stamp.as_ref().and_then(|stamp| stamp.basis) else {
        return Ok(());
    };
    bridge.with_native_document_owner(engine, owner, || {
        let context = snapping::context(world, basis, raw);
        editor.draft.snap_context = Some(context);
        let acquired = snapping::acquire(engine, &editor.draft, raw, ctrl, context)
            .inspect_err(|error| creation_failed(editor, error))?;
        let mut cursor = acquired.snapped_to;
        let mut snap = acquired.snap;
        if editor.draft.tool == Some(CreateTool::Line) && !editor.draft.points.is_empty() {
            let value = dynamic::line_preview(engine, &editor.draft, raw, ctrl)
                .inspect_err(|error| creation_failed(editor, error))?;
            cursor = value.snapped_to;
            snap = value.snap;
        }
        use limo_cad_sketch::SnapTarget;
        let kind = match snap {
            SnapTarget::None => None,
            SnapTarget::Grid => Some(ViewportSnapKind::Grid),
            SnapTarget::Origin => Some(ViewportSnapKind::Origin),
            SnapTarget::Point { .. } => Some(ViewportSnapKind::Point),
            SnapTarget::Midpoint { .. } => Some(ViewportSnapKind::Midpoint),
            SnapTarget::ReferenceMidpoint { .. } => Some(ViewportSnapKind::ReferenceMidpoint),
            SnapTarget::Curve { .. }
            | SnapTarget::Intersection { .. }
            | SnapTarget::ProjectedEdge { .. } => Some(ViewportSnapKind::Curve),
        };
        let mut marker = kind.map(|kind| ViewportSnapMarker {
            position: basis.to_3d([cursor.x, cursor.y]).map(|v| v as f32),
            kind,
        });
        editor.draft.cursor = Some(raw);
        let resolved = dynamic::preview_points(engine, &editor.draft, raw, ctrl)
            .inspect_err(|error| creation_failed(editor, error))?;
        if let Some(points) = resolved {
            cursor = points[1];
        }
        cursor = dynamic::slot_cursor(&editor.draft, cursor)
            .inspect_err(|error| creation_failed(editor, error))?;
        if marker.as_ref().is_some_and(|marker| {
            let resolved = basis.to_3d([cursor.x, cursor.y]).map(|v| v as f32);
            Vec3::from_array(marker.position).distance(Vec3::from_array(resolved)) > 1e-5
        }) {
            marker = None;
        }
        let color = [1.; 4];
        let color_role = ViewportColorRole::SketchPreview;
        let outline = if let Some(points) = &resolved {
            editor.draft.outline_with_points(&points[..1], cursor)
        } else {
            editor.draft.outline(cursor)
        };
        let segments = outline
            .into_iter()
            .flatten()
            .flat_map(|point| basis.to_3d([point.x, point.y]).map(|v| v as f32))
            .collect::<Vec<_>>()
            .into();
        let (_, camera, _, size) = native_viewport::interface_view(world);
        let distance = Vec3::from_array(camera.position).distance(Vec3::from_array(
            basis.to_3d([cursor.x, cursor.y]).map(|v| v as f32),
        ));
        let radius = (distance * (camera.vertical_fov_degrees.to_radians() * 0.5).tan() * 4.
            / size[1].max(1.))
        .max(0.01);
        let positions = editor
            .draft
            .points
            .iter()
            .chain(std::iter::once(&cursor))
            .flat_map(|p| basis.to_3d([p.x, p.y]).map(|v| v as f32))
            .collect::<Vec<_>>()
            .into();
        native_viewport::apply_interface_preview(
            world,
            &owner.document_id,
            ViewportPreview {
                lines: vec![ViewportLineLayer {
                    color,
                    color_role,
                    width: 2.,
                    segments,
                    ..default()
                }],
                points: vec![ViewportPointLayer {
                    color,
                    color_role,
                    radius,
                    hollow: true,
                    positions,
                }],
                marker,
                ..default()
            },
        )?;
        editor.draft.resolved_preview = Some([
            resolved
                .map(|points| points[0])
                .unwrap_or_else(|| editor.draft.points.first().copied().unwrap_or(cursor)),
            cursor,
        ]);
        if editor.creation_error.as_deref() == Some(editor.error.as_str()) {
            editor.error.clear();
            editor.creation_error = None;
        }
        Ok(())
    })
}

fn committed_feedback(output: &mut Value, editor: &mut Editor, followup: Result<(), String>) {
    output["committed"] = json!(true);
    if let Err(error) = followup {
        editor.error = error.clone();
        editor.creation_error = None;
        editor.stamp = None;
        editor.draft.select(None);
        editor.press = None;
        output["presentation_pending"] = json!(true);
        output["presentation_error"] = json!(error);
    } else {
        editor.error.clear();
    }
}

/// Controls and the native canvas share this exact owner-checked action path.
pub(crate) fn execute(
    world: &mut World,
    engine: &AppState,
    bridge: &SessionBridgeState,
    owner: &DocumentContext,
    command: EditorCommand,
    validate: impl FnOnce() -> Result<(), String>,
) -> Result<Value, String> {
    if worker::busy(world) {
        return Err("Wait for the current modeling operation to finish".into());
    }
    let mut validate = Some(validate);
    if matches!(
        &command,
        EditorCommand::Begin(_) | EditorCommand::Edit(_) | EditorCommand::Support(_)
    ) {
        bridge.with_native_document_owner(engine, owner, || validate.take().unwrap()())?;
        crate::session_bridge::native_interface::controller::named_views::ensure_source_ready(
            world, engine, bridge, owner,
        )?;
    }
    let mut validate = || validate.take().map_or(Ok(()), |validate| validate());
    initialize(world);
    let next = stamp(
        engine,
        bridge,
        owner,
        world.resource::<Editor>().stamp.as_ref(),
    )?;
    world.resource_scope(|world, mut editor: Mut<Editor>| {
        synchronize_stamp(&mut editor, next);
        match command {
            EditorCommand::Support(command) => {
                support::execute(world, engine, bridge, owner, &mut editor, command, validate)
            }
            EditorCommand::Edit(name) => {
                if editor
                    .stamp
                    .as_ref()
                    .is_some_and(|stamp| stamp.sketch.is_some())
                {
                    return Err("Finish the current sketch before editing another".into());
                }
                validate()?;
                let occurrence_id = native_viewport::interface_navigation_source(world)
                    .1
                    .selected_occurrence_id;
                queue_mutation(
                    world,
                    editor.stamp.as_ref().unwrap().clone(),
                    "sketch_edit",
                    json!({"name":name,"occurrence_id":occurrence_id}),
                    Completion::Begin,
                )
            }
            EditorCommand::Begin(plane) => {
                if editor
                    .stamp
                    .as_ref()
                    .is_some_and(|stamp| stamp.sketch.is_some())
                {
                    return Err("Finish the current sketch before starting another".into());
                }
                validate()?;
                queue_mutation(
                    world,
                    editor.stamp.as_ref().unwrap().clone(),
                    "sketch_begin",
                    json!({"plane":plane}),
                    Completion::Begin,
                )
            }
            EditorCommand::Finish => {
                if editor
                    .stamp
                    .as_ref()
                    .is_none_or(|stamp| stamp.sketch.is_none())
                {
                    return Err("There is no sketch to finish".into());
                }
                validate()?;
                queue_mutation(
                    world,
                    editor.stamp.as_ref().unwrap().clone(),
                    "sketch_finish",
                    json!({}),
                    Completion::Finish,
                )
            }
            EditorCommand::Tool(tool) => {
                bridge.with_native_document_owner(engine, owner, validate)?;
                if editor
                    .stamp
                    .as_ref()
                    .is_none_or(|stamp| stamp.sketch.is_none())
                {
                    return Err("Start or edit a sketch to draw geometry".into());
                }
                editor.draft.select(Some(tool));
                editor.interaction = Default::default();
                editor.error.clear();
                clear_preview(world, engine, bridge, owner)?;
                Ok(json!({"active_tool":tool.label(),"instruction":editor.draft.instruction()}))
            }
            EditorCommand::Cancel => {
                bridge.with_native_document_owner(engine, owner, validate)?;
                editor.draft.escape();
                if editor
                    .stamp
                    .as_ref()
                    .is_none_or(|stamp| stamp.sketch.is_none())
                {
                    solid::clear(world, owner)?;
                }
                editor.error.clear();
                editor.press = None;
                editor.support = Default::default();
                support::present(world, owner, &editor.support)?;
                editor.interaction = Default::default();
                interaction::present(world, owner, &editor.interaction)?;
                clear_preview(world, engine, bridge, owner)?;
                Ok(json!({"active_tool":editor.draft.tool.map(CreateTool::label)}))
            }
            EditorCommand::Complete => {
                if let Some(command) = editor.draft.complete()? {
                    commit(world, engine, bridge, &mut editor, command, validate)
                } else {
                    bridge.with_native_document_owner(engine, owner, validate)?;
                    let tool = editor.draft.tool;
                    editor.draft.select(tool);
                    clear_preview(world, engine, bridge, owner)?;
                    Ok(json!({"complete":true}))
                }
            }
            EditorCommand::Interaction(command) => {
                interaction::execute(world, engine, bridge, owner, &mut editor, command, validate)
            }
            EditorCommand::Palette(command) => {
                palette::execute(world, engine, bridge, owner, &mut editor, command, validate)
            }
            EditorCommand::Size {
                generation,
                field,
                text,
            } => dynamic::set(
                world,
                (engine, bridge, owner),
                &mut editor,
                generation,
                field,
                text,
                validate,
            ),
        }
    })
}

pub(crate) fn confirm_size(
    world: &mut World,
    engine: &AppState,
    bridge: &SessionBridgeState,
    owner: &DocumentContext,
    generation: u64,
    validate: impl FnOnce() -> Result<(), String>,
) -> Result<Value, String> {
    bridge.with_native_document_owner(engine, owner, || {
        if world
            .get_resource::<Editor>()
            .is_none_or(|editor| editor.draft.generation != generation)
        {
            return Err("This drawing gesture changed".into());
        }
        Ok(())
    })?;
    execute(
        world,
        engine,
        bridge,
        owner,
        EditorCommand::Complete,
        validate,
    )
}

fn commit(
    world: &mut World,
    _engine: &AppState,
    _bridge: &SessionBridgeState,
    editor: &mut Editor,
    mut command: Prepared,
    validate: impl FnOnce() -> Result<(), String>,
) -> Result<Value, String> {
    let expected = editor
        .stamp
        .as_ref()
        .ok_or("Sketch gesture has no document owner")?
        .clone();
    validate()?;
    snapping::attach(&mut command.arguments, editor.draft.snap_context);
    queue_mutation(
        world,
        expected,
        command.operation,
        command.arguments,
        Completion::Primitive,
    )
}

#[derive(Clone, Copy)]
enum Completion {
    Begin,
    Finish,
    Primitive,
    Modify,
    Settings,
}

fn queue_mutation(
    world: &mut World,
    expected: Stamp,
    operation: &'static str,
    arguments: Value,
    kind: Completion,
) -> Result<Value, String> {
    worker::enqueue_operation(
        world,
        expected.owner.clone(),
        expected.revision,
        operation.into(),
        arguments,
        move |world, services, result| {
            let engine = &services.engine;
            let bridge = &services.bridge;
            world.resource_scope(|world, mut editor: Mut<Editor>| {
                let result = match result {
                    Ok(result) => result,
                    Err(error) => {
                        editor.error = error.clone();
                        editor.creation_error = matches!(kind, Completion::Primitive)
                            .then(|| error.clone());
                        if editor.stamp.as_ref() == Some(&expected)
                            && matches!(operation, "sketch_add_dimension" | "sketch_edit_dimension")
                        {
                            panel::retry_dimension_focus(world);
                        }
                        return Err(error);
                    }
                };
                let owner = result.context.clone();
                let same_gesture = editor.stamp.as_ref() == Some(&expected);
                let accepted = if same_gesture {
                    match kind {
                        Completion::Settings => Ok(()),
                        Completion::Primitive => editor.draft.accepted(&result.value),
                        Completion::Begin | Completion::Finish => {
                            editor.draft.select(None);
                            editor.interaction = Default::default();
                            editor.support = Default::default();
                            Ok(())
                        }
                        Completion::Modify => {
                            editor.interaction.selection.clear();
                            editor.interaction.relation = None;
                            editor.interaction.form = None;
                            editor.interaction.dimension = None;
                            editor.interaction.dimension_position = None;
                            editor.interaction.dimension_id = None;
                            editor.interaction.dimension_reference = false;
                            editor.interaction.reposition_dimension = None;
                            editor.interaction.constraint = None;
                            Ok(())
                        }
                    }
                } else {
                    Err("The sketch interaction changed while modeling finished".into())
                };
                let mut output = finish_mutation(engine, bridge, world, operation, result);
                let followup = (|| {
                    accepted?;
                    editor.stamp = Some(stamp(engine, bridge, &owner, None)?);
                    interaction::present(world,&owner,&editor.interaction)?;
                    support::present(world, &owner, &editor.support)?;
                    if matches!(kind, Completion::Begin) {
                        look_at_sketch(world, engine, bridge, &owner, true)?;
                    } else if matches!(kind, Completion::Finish) {
                        let receipt = bridge.native_document_receipt(engine, &owner)?;
                        crate::session_bridge::native_interface::restore_model_view(world, &owner, receipt.revision)?;
                    }
                    clear_preview(world, engine, bridge, &owner)
                })();
                committed_feedback(&mut output, &mut editor, followup);
                if matches!(kind, Completion::Finish)
                    && engine.with_document(|document| document.features().features.iter().any(|f| !matches!(f.kind, limo_cad_core::FeatureKind::Sketch | limo_cad_core::FeatureKind::ConstructionPlane)))
                {
                    let receipt = bridge.native_document_receipt(engine, &owner)?;
                    let pending = worker::enqueue_operation(world, receipt.owner, receipt.revision, "solid_recompute".into(), json!({}), |world, services, result| {
                        match result {
                            Ok(result) => {
                                let owner = result.context.clone();
                                let mut value = finish_mutation(&services.engine, &services.bridge, world, "solid_recompute", result);
                                let receipt = services.bridge.native_document_receipt(&services.engine, &owner)?;
                                crate::session_bridge::native_interface::restore_model_view(world, &owner, receipt.revision)?;
                                value["committed"] = json!(true);
                                Ok(value)
                            }
                            Err(error) => Ok(json!({"committed":true,"model_error":format!("Sketch saved, but dependent solids could not be rebuilt: {error}")})),
                        }
                    })?;
                    output["mutation_pending"] = json!(true);
                    output["mutation_id"] = pending["mutation_id"].clone();
                }
                Ok(output)
            })
        },
    )
}

fn look_at_sketch(
    world: &mut World,
    engine: &AppState,
    bridge: &SessionBridgeState,
    owner: &DocumentContext,
    fit_authored: bool,
) -> Result<(), String> {
    bridge.with_native_document_owner(engine, owner, || {
        let Some(sketch) = active(engine)? else {
            return Err("The new sketch is no longer active".into());
        };
        let (_, camera, _, size) = native_viewport::interface_view(world);
        let center = Vec3::from_array(sketch.basis.origin.map(|v| v as f32));
        let normal = Vec3::from_array(sketch.basis.normal.map(|v| v as f32));
        let distance = Vec3::from_array(camera.position)
            .distance(Vec3::from_array(camera.target))
            .max(100.);
        let mut camera = ViewportCamera {
            position: (center + normal * distance).to_array(),
            target: center.to_array(),
            up: sketch.basis.v.map(|v| v as f32),
            ..camera
        };
        if fit_authored {
            if let Some(fitted) = crate::session_bridge::native_interface::fit_sketch_begin_camera(
                &sketch,
                camera,
                size,
                palette::WIDTH,
            )? {
                camera = fitted;
            }
        }
        native_viewport::apply_interface_view(world, &owner.document_id, Some(camera), None)
    })
}

/// Called by the central controller in original OS event order, interleaved
/// with the exact native control actions generated by those events.
pub(crate) fn process_one(
    world: &mut World,
    handle: &NativeInterfaceHandle,
    services: &NativeServices,
    event: &NativeHostInput,
) -> Result<Value, String> {
    if worker::busy(world) {
        return Err("Wait for the current modeling operation to finish".into());
    }
    initialize(world);
    let Some(frame) = handle.frame() else {
        return Ok(json!({"handled":false}));
    };
    if mechanism::pointer(world, handle, services, event)? {
        world.resource_mut::<Editor>().press = None;
        return Ok(json!({"handled":true,"mechanism_drag":true}));
    }
    if frame.modal_stack.is_empty() && event.context.as_ref() == Some(&frame.context) {
        if matches!(&event.event, WindowEvent::KeyboardInput(key)
            if key.state == ButtonState::Pressed && key.logical_key == bevy::input::keyboard::Key::Escape)
            && !event.modifiers.ctrl
            && !event.modifiers.meta
            && !event.modifiers.alt
        {
            if let Some(panel) = crate::session_bridge::native_interface::feature::panel(world) {
                return crate::session_bridge::native_interface::feature::reduce(
                    &services.engine,
                    &services.bridge,
                    world,
                    &frame.context,
                    &crate::session_bridge::native_interface::feature::FeatureCommand::Control {
                        form_id: panel.form_id,
                        action: panel.choice_field.map_or(
                            crate::session_bridge::native_interface::feature::FeatureControl::Cancel,
                            crate::session_bridge::native_interface::feature::FeatureControl::Field,
                        ),
                    },
                    &limo_cad_interface::ControlInput::Click,
                    || Ok(()),
                );
            }
            use crate::session_bridge::native_interface::controller::{named_views, print_intent};
            if print_intent::active(world) {
                return print_intent::cancel(
                    world,
                    &services.engine,
                    &services.bridge,
                    &frame.context,
                );
            }
            if named_views::active(world) {
                return named_views::cancel(
                    world,
                    &services.engine,
                    &services.bridge,
                    &frame.context,
                );
            }
        }
        use crate::session_bridge::native_interface::feature::manipulator::{self, Pointer};
        let phase = match &event.event {
            WindowEvent::CursorMoved(_) => Some(Pointer::Move),
            WindowEvent::MouseButtonInput(b) if b.button == MouseButton::Left => {
                Some(if b.state == ButtonState::Pressed {
                    Pointer::Press
                } else {
                    Pointer::Release
                })
            }
            WindowEvent::CursorLeft(_) => Some(Pointer::Cancel),
            WindowEvent::WindowFocused(e) if !e.focused => Some(Pointer::Cancel),
            _ => None,
        };
        if let Some(phase) = phase {
            let point = event.cursor.and_then(|p| {
                let canvas = frame.canvases.iter().find(|c| c.name == "viewport")?;
                let a = canvas.bounds;
                if matches!(phase, Pointer::Press)
                    && (handle.owns_pointer([p.x as f64, p.y as f64])
                        || p.x < a.x as f32
                        || p.y < a.y as f32
                        || p.x >= (a.x + a.width) as f32
                        || p.y >= (a.y + a.height) as f32)
                {
                    return None;
                }
                Some([p.x - a.x as f32, p.y - a.y as f32])
            });
            if manipulator::pointer(world, services, &frame.context, phase, point)? {
                return Ok(json!({"handled":true,"offset_drag":true}));
            }
        }
    }
    if frame.modal_stack.is_empty()
        && event.context.as_ref() == Some(&frame.context)
        && joint::active(world)
    {
        if matches!(&event.event, WindowEvent::KeyboardInput(k) if k.state==ButtonState::Pressed && k.key_code==KeyCode::Escape)
        {
            return joint::cancel(world, &services.engine, &services.bridge, &frame.context);
        }
        if let WindowEvent::CursorMoved(moved) = &event.event {
            if let Some(c) = frame.canvases.iter().find(|c| c.name == "viewport") {
                let p = moved.position;
                let a = c.bounds;
                let inside = p.x >= a.x as f32
                    && p.y >= a.y as f32
                    && p.x < (a.x + a.width) as f32
                    && p.y < (a.y + a.height) as f32
                    && !handle.owns_pointer([p.x as f64, p.y as f64]);
                if let Some(value) = joint::canvas(
                    world,
                    services,
                    &frame.context,
                    inside.then_some([p.x - a.x as f32, p.y - a.y as f32]),
                    false,
                )? {
                    return Ok(value);
                }
            }
        }
    }
    let next = stamp(
        &services.engine,
        &services.bridge,
        &frame.context,
        world.resource::<Editor>().stamp.as_ref(),
    )?;
    world.resource_scope(|world, mut editor: Mut<Editor>| {
        synchronize_stamp(&mut editor, next);
        let mut result = json!({"handled":false});
        if !frame.modal_stack.is_empty() {
            editor.press = None;
            return Ok(result);
        }
        if event.context.as_ref() != Some(&frame.context) {
            editor.press = None;
            return Ok(result);
        }
        match &event.event {
            WindowEvent::WindowFocused(event) if !event.focused => {
                editor.press = None;
                clear_preview(world, &services.engine, &services.bridge, &frame.context)?;
            }
            WindowEvent::CursorLeft(_) => {
                editor.press = None;
                clear_preview(world, &services.engine, &services.bridge, &frame.context)?;
                support::hover(world, &frame.context, &mut editor.support, None)?;
                crate::session_bridge::native_interface::feature::hover_references(world,services,&frame.context,None)?;
                solid::hover(world, services, &frame.context, None)?;
            }
            WindowEvent::CursorMoved(moved) if editor.support.active => {
                if let Some(canvas) = frame.canvases.iter().find(|c| c.name == "viewport") {
                    let p = moved.position;
                    let a = canvas.bounds;
                    let inside = f64::from(p.x) >= a.x && f64::from(p.x) < a.x+a.width &&
                        f64::from(p.y) >= a.y && f64::from(p.y) < a.y+a.height && !handle.owns_pointer([f64::from(p.x),f64::from(p.y)]);
                    support::hover(world, &frame.context, &mut editor.support, inside.then_some([p.x-a.x as f32,p.y-a.y as f32]))?;
                    result = json!({"handled":true,"hover":inside});
                }
            }
            WindowEvent::CursorMoved(moved) if editor.draft.tool.is_none() && editor.stamp.as_ref().is_none_or(|s|s.sketch.is_none()) => {
                if let Some(canvas)=frame.canvases.iter().find(|c|c.name=="viewport") {
                    let p=moved.position;let a=canvas.bounds;
                    let inside=f64::from(p.x)>=a.x&&f64::from(p.x)<a.x+a.width&&f64::from(p.y)>=a.y&&f64::from(p.y)<a.y+a.height&&!handle.owns_pointer([f64::from(p.x),f64::from(p.y)]);
                    let handled=crate::session_bridge::native_interface::feature::hover_references(world,services,&frame.context,inside.then_some([p.x-a.x as f32,p.y-a.y as f32]))?;
                    if !handled {
                        solid::hover(world, services, &frame.context, inside.then_some([p.x-a.x as f32,p.y-a.y as f32]))?;
                    }
                    result=json!({"handled":true,"hover":inside});
                }
            }
            WindowEvent::CursorMoved(moved) if editor.draft.tool.is_some() || editor.interaction.form.as_ref().is_some_and(|form| form.kind == FormKind::Polygon) => {
                let Some(basis) = editor.stamp.as_ref().and_then(|stamp| stamp.basis) else { return Ok(result); };
                let Some(canvas) = frame.canvases.iter().find(|canvas| canvas.name == "viewport") else { return Ok(result); };
                let cursor = moved.position;
                if handle.owns_pointer([f64::from(cursor.x),f64::from(cursor.y)]) {return Ok(result);}
                if f64::from(cursor.x) < canvas.bounds.x || f64::from(cursor.x) >= canvas.bounds.x+canvas.bounds.width
                    || f64::from(cursor.y) < canvas.bounds.y || f64::from(cursor.y) >= canvas.bounds.y+canvas.bounds.height {
                    clear_preview(world, &services.engine, &services.bridge, &frame.context)?;
                    return Ok(result);
                }
                if let Some(point) = native_viewport::interface_sketch_point(world, &frame.context.document_id,
                    [cursor.x-canvas.bounds.x as f32,cursor.y-canvas.bounds.y as f32],basis)? {
                    preview(world,&services.engine,&services.bridge,&frame.context,&mut editor,point,event.modifiers.ctrl)?;
                    result = json!({"handled":true,"preview":true,"instruction":editor.draft.instruction()});
                }
            }
            WindowEvent::CursorMoved(moved) if editor.stamp.as_ref().is_some_and(|s|s.sketch.is_some()) => {
                if let Some(canvas) = frame.canvases.iter().find(|c| c.name == "viewport") {
                    let p = moved.position;
                    let a = canvas.bounds;
                    let inside = f64::from(p.x) >= a.x && f64::from(p.x) < a.x+a.width &&
                        f64::from(p.y) >= a.y && f64::from(p.y) < a.y+a.height && !handle.owns_pointer([f64::from(p.x),f64::from(p.y)]);
                    interaction::hover(world, services, &frame.context, &editor, inside.then_some(p), a)?;
                    result = json!({"handled":true,"hover":inside});
                }
            }
            WindowEvent::KeyboardInput(key)
                if key.state == ButtonState::Pressed
                    && !key.repeat
                    && !event.modifiers.ctrl
                    && !event.modifiers.meta
                    && !event.modifiers.alt =>
            {
                match key.key_code {
                    KeyCode::Escape => {
                        editor.draft.escape();
                        if editor.stamp.as_ref().is_none_or(|stamp| stamp.sketch.is_none()) {
                            solid::clear(world, &frame.context)?;
                        }
                        editor.interaction=Default::default();
                        interaction::present(world,&frame.context,&editor.interaction)?;
                        editor.error.clear();
                        editor.press = None;
                        editor.support = Default::default();
                        support::present(world, &frame.context, &editor.support)?;
                        clear_preview(world, &services.engine, &services.bridge, &frame.context)?;
                        result = json!({"handled":true,"cancelled":true});
                    }
                    KeyCode::Delete | KeyCode::Backspace if !editor.interaction.selection.is_empty() => {
                        return interaction::execute(world,&services.engine,&services.bridge,&frame.context,&mut editor,InteractionCommand::Delete,||Ok(()));
                    }
                    KeyCode::Enter | KeyCode::NumpadEnter => match editor.draft.complete() {
                        Ok(Some(command)) => {
                            match commit(
                                world,
                                &services.engine,
                                &services.bridge,
                                &mut editor,
                                command,
                                || Ok(()),
                            ) {
                                Ok(value) => result = value,
                                Err(error) => { editor.creation_error = None; editor.error = error.clone(); return Err(error); }
                            }
                        }
                        Err(error) => { creation_failed(&mut editor, &error); return Err(error); }
                        _ => {}
                    },
                    _ => {}
                }
            }
            WindowEvent::MouseButtonInput(button) if button.button == MouseButton::Left => {
                let Some(cursor) = event.cursor else {
                    editor.press = None;
                    return Ok(result);
                };
                let Some(canvas) = frame
                    .canvases
                    .iter()
                    .find(|canvas| canvas.name == "viewport")
                else {
                    return Ok(result);
                };
                let in_canvas = f64::from(cursor.x) >= canvas.bounds.x
                    && f64::from(cursor.x) < canvas.bounds.x + canvas.bounds.width
                    && f64::from(cursor.y) >= canvas.bounds.y
                    && f64::from(cursor.y) < canvas.bounds.y + canvas.bounds.height;
                if button.state == ButtonState::Pressed {
                    editor.press = in_canvas.then(|| (frame.context.clone(), cursor));
                } else {
                    let Some((owner, start)) = editor.press.take() else {
                        return Ok(result);
                    };
                    if owner != frame.context || !in_canvas {
                        return Ok(result);
                    }
                    if editor.draft.tool.is_none() && editor.stamp.as_ref().is_some_and(|s|s.sketch.is_some()) {
                        return interaction::pointer(world,
        (services, &owner),
        &mut editor,
        (start, cursor, canvas.bounds),
        event.modifiers.shift,
        event.modifiers.ctrl);
                    }
                    if start.distance(cursor)>3. {return Ok(result);}
                    if editor.support.active {
                        return support::pick(world, services, &owner, &mut editor,
                            [cursor.x-canvas.bounds.x as f32,cursor.y-canvas.bounds.y as f32]);
                    }
                    if let Some(value)=joint::canvas(world,services,&owner,Some([cursor.x-canvas.bounds.x as f32,cursor.y-canvas.bounds.y as f32]),true)? {return Ok(value);}
                    if let Some(value) = crate::session_bridge::native_interface::feature::handle_canvas_pick(
                        world, services, &owner,
                        [cursor.x-canvas.bounds.x as f32,cursor.y-canvas.bounds.y as f32],
                    )? {
                        return Ok(value);
                    }
                    let Some(basis) = editor.stamp.as_ref().and_then(|stamp| stamp.basis) else {
                        return solid::select(world, services, &owner, [cursor.x-canvas.bounds.x as f32,cursor.y-canvas.bounds.y as f32], event.modifiers.shift || event.modifiers.ctrl || event.modifiers.meta);
                    };
                    if editor.draft.tool.is_none() {
                        return Ok(result);
                    }
                    let point = native_viewport::interface_sketch_point(
                        world,
                        &owner.document_id,
                        [
                            cursor.x - canvas.bounds.x as f32,
                            cursor.y - canvas.bounds.y as f32,
                        ],
                        basis,
                    )?;
                    if let Some(raw) = point {
                        let context = snapping::context(world, basis, raw);
                        let acquired = services.bridge.with_native_document_owner(&services.engine, &owner, || {
                            snapping::acquire(&services.engine, &editor.draft, raw, event.modifiers.ctrl, context)
                        })?;
                        editor.draft.snap_context = Some(context);
                        let point = snapping::pick(&editor.draft, raw, &acquired);
                        match editor.draft.prepare(point, event.modifiers.ctrl) {
                            Ok(Some(command)) => {
                                match commit(
                                    world,
                                    &services.engine,
                                    &services.bridge,
                                    &mut editor,
                                    command,
                                    || Ok(()),
                                ) {
                                    Ok(value) => result = value,
                                    Err(error) => { editor.creation_error = None; editor.error = error.clone(); return Err(error); }
                                }
                            }
                            Ok(None) => {
                                editor.error.clear();
                                preview(world,&services.engine,&services.bridge,&owner,&mut editor,point,event.modifiers.ctrl)?;
                                result = json!({"handled":true,"picks":editor.draft.points.len(),"instruction":editor.draft.instruction()});
                            }
                            Err(error) => { creation_failed(&mut editor, &error); return Err(error); }
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(result)
    })
}

pub(crate) fn synchronize_controls(
    world: &mut World,
    _handle: &NativeInterfaceHandle,
    services: &NativeServices,
    owner: &DocumentContext,
    area: InterfaceRect,
    canvas: InterfaceRect,
) -> Result<(), String> {
    if worker::busy(world) {
        return Ok(());
    }
    initialize(world);
    let next = stamp(
        &services.engine,
        &services.bridge,
        owner,
        world.resource::<Editor>().stamp.as_ref(),
    )?;
    let mut cameras = world.query_filtered::<Entity, With<InterfaceCamera>>();
    let Ok(camera) = cameras.single(world) else {
        return Ok(());
    };
    let assets = world.resource::<ViewportUiAssets>().clone();
    let theme = crate::native_viewport::ui::theme(world);
    world.resource_scope(|world, mut editor: Mut<Editor>| {
        synchronize_stamp(&mut editor, next);
        support::present(world, owner, &editor.support)?;
        interaction::present(world, owner, &editor.interaction)?;
        let rows: Vec<(String, EditorCommand)> =
            if editor.stamp.as_ref().is_some_and(|s| s.sketch.is_some()) {
                let mut rows = [
                    CreateTool::Line,
                    CreateTool::Arc3Point,
                    CreateTool::Rectangle(RectangleMode::TwoPoint),
                    CreateTool::Circle(CircleMode::CenterDiameter),
                    CreateTool::Spline,
                    CreateTool::Slot(SlotMode::CenterToCenter),
                ]
                .into_iter()
                .map(|tool| tool.primary_variant(editor.draft.tool))
                .map(|tool| (tool.label().to_owned(), EditorCommand::Tool(tool)))
                .collect::<Vec<_>>();
                for command in [
                    InteractionCommand::Form(FormKind::MoveCopy),
                    InteractionCommand::Modify(ModifyTool::Trim),
                    InteractionCommand::Modify(ModifyTool::Extend),
                    InteractionCommand::Form(FormKind::Offset),
                    InteractionCommand::Form(FormKind::Fillet),
                    InteractionCommand::Dimension,
                    InteractionCommand::Form(FormKind::Mirror),
                    InteractionCommand::Form(FormKind::RectangularPattern),
                    InteractionCommand::Form(FormKind::CircularPattern),
                    InteractionCommand::Relation(constraints::Relation::Coincident),
                    InteractionCommand::Relation(constraints::Relation::Align),
                    InteractionCommand::Relation(constraints::Relation::Tangent),
                    InteractionCommand::Relation(constraints::Relation::Parallel),
                    InteractionCommand::Relation(constraints::Relation::Perpendicular),
                    InteractionCommand::Select,
                ] {
                    let label = match &command {
                        InteractionCommand::Form(t) => t.label(),
                        InteractionCommand::Modify(t) => t.label(),
                        InteractionCommand::Relation(t) => t.label(),
                        InteractionCommand::Dimension => "Sketch Dimension",
                        _ => "Select",
                    };
                    rows.push((label.into(), EditorCommand::Interaction(command)));
                }
                rows.push(("Finish sketch".into(), EditorCommand::Finish));
                if editor.draft.tool.is_some() {
                    rows.push(("Cancel tool".into(), EditorCommand::Cancel));
                }
                if editor.draft.tool == Some(CreateTool::Spline) {
                    rows.push(("Finish spline".into(), EditorCommand::Complete));
                }
                rows
            } else {
                vec![(
                    "Create Sketch".into(),
                    EditorCommand::Support(support::Command::Start),
                )]
            };
        editor.controls.retain(|label, entity| {
            if rows.iter().any(|row| &row.0 == label) {
                true
            } else {
                world.despawn(*entity);
                false
            }
        });
        let completion_reserve = panel::completion_reserve(editor.draft.tool);
        let groups = panel::ribbon_groups(area, completion_reserve);
        let mut slots: HashMap<&str, usize> = HashMap::new();
        let active_sketch = editor
            .stamp
            .as_ref()
            .is_some_and(|stamp| stamp.sketch.is_some());
        for (label, command) in rows {
            let finish = matches!(command, EditorCommand::Finish | EditorCommand::Complete);
            let width = 48.;
            let group_key = panel::group(&command)
                .strip_prefix("sketch/")
                .unwrap_or("draw");
            let group = groups
                .iter()
                .find(|group| group.key == group_key)
                .unwrap_or(&groups[0]);
            let slot = slots.entry(group_key).or_default();
            let x = if active_sketch {
                group.left + (group.width - group.count as f32 * 50.) / 2. + *slot as f32 * 50.
            } else {
                area.x as f32
            };
            let y = area.y as f32;
            let visible = finish
                || !active_sketch
                || (*slot < group.count
                    && x + width <= (area.x + area.width - f64::from(completion_reserve)) as f32);
            *slot += 1;
            use crate::native_viewport::interface_shell::ribbon;
            let node = if finish {
                ribbon::finish_node(
                    if matches!(command, EditorCommand::Complete) {
                        156.
                    } else if area.x + area.width <= 1400. {
                        8.
                    } else {
                        12.
                    },
                    area.y as f32 + 23.5,
                    area.x + area.width <= 1400.,
                )
            } else {
                ribbon::node(x, y, width)
            };
            let entity = if let Some(entity) = editor.controls.get(&label) {
                *entity
            } else {
                let mut system = SystemState::<Commands>::new(world);
                let entity = {
                    let mut commands = system.get_mut(world).map_err(|e| e.to_string())?;
                    spawn_button(
                        &mut commands,
                        camera,
                        node.clone(),
                        InterfaceControl::button(panel::group(&command), &label),
                        theme,
                        &assets,
                    )
                };
                system.apply(world);
                let icon = panel::icon(&command);
                ribbon::decorate(world, entity, icon);
                bind_command(world, entity, NativeCommand::Sketch(command.clone()))?;
                editor.controls.insert(label.clone(), entity);
                entity
            };
            if world.get::<Node>(entity) != Some(&node) {
                world.entity_mut(entity).insert(node);
            }
            let build_open = crate::session_bridge::native_interface::feature::panel(world)
                .is_some()
                || joint::active(world);
            let mut control = world
                .get_mut::<InterfaceControl>(entity)
                .ok_or("Sketch control was removed")?;
            control.visible = visible;
            control.selected = match command {
                EditorCommand::Support(support::Command::Start) => Some(editor.support.active),
                EditorCommand::Tool(tool) => Some(editor.draft.tool == Some(tool)),
                EditorCommand::Interaction(InteractionCommand::Modify(tool)) => {
                    Some(editor.interaction.modify == Some(tool))
                }
                EditorCommand::Interaction(InteractionCommand::Relation(relation)) => {
                    Some(editor.interaction.relation == Some(relation))
                }
                EditorCommand::Interaction(InteractionCommand::Form(kind)) => Some(
                    editor
                        .interaction
                        .form
                        .as_ref()
                        .is_some_and(|f| f.kind == kind),
                ),
                _ => None,
            };
            control.disabled = (matches!(command, EditorCommand::Complete)
                && editor.draft.points.len() < 2)
                || (matches!(command, EditorCommand::Support(_) | EditorCommand::Begin(_))
                    && build_open);
        }
        panel::synchronize(world, camera, &mut editor, area, canvas)?;
        support::synchronize(world, camera, &editor, canvas)?;
        dynamic::synchronize(world, camera, &services.engine, owner, &editor, canvas)?;
        palette::synchronize(world, camera, services, owner, &editor, canvas)?;
        annotations::synchronize(world, camera, services, owner, &editor, canvas)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_successful_commit_clears_a_stale_gesture_error() {
        let owner = DocumentContext {
            window_id: "main".into(),
            document_id: "a".into(),
            epoch: 1,
        };
        let mut editor = Editor::default();
        let current = Stamp {
            owner,
            revision: 2,
            sketch: Some("Sketch1".into()),
            basis: None,
        };
        synchronize_stamp(&mut editor, current.clone());
        editor.error = "This segment has no length".into();
        assert!(!synchronize_stamp(&mut editor, current.clone()));
        assert!(!editor.error.is_empty());

        let mut output = json!({});
        committed_feedback(&mut output, &mut editor, Ok(()));
        assert_eq!(output["committed"], json!(true));
        assert!(
            editor.error.is_empty(),
            "a committed gesture must not keep reporting an earlier failure"
        );
    }

    #[test]
    fn replacing_or_mutating_a_document_retires_unfinished_gestures() {
        let owner = DocumentContext {
            window_id: "main".into(),
            document_id: "a".into(),
            epoch: 1,
        };
        let mut editor = Editor::default();
        let original = Stamp {
            owner: owner.clone(),
            revision: 2,
            sketch: Some("Sketch1".into()),
            basis: None,
        };
        synchronize_stamp(&mut editor, original.clone());
        editor.draft.select(Some(CreateTool::Line));
        editor.draft.prepare(SketchPoint::ZERO, false).unwrap();
        assert!(!synchronize_stamp(&mut editor, original.clone()));
        assert_eq!(editor.draft.points.len(), 1);
        let newer = Stamp {
            revision: 3,
            ..original.clone()
        };
        assert!(synchronize_stamp(&mut editor, newer));
        assert!(editor.draft.points.is_empty());
        assert!(editor.draft.tool.is_none());
        let replaced = Stamp {
            owner: DocumentContext { epoch: 2, ..owner },
            ..original
        };
        assert!(synchronize_stamp(&mut editor, replaced));
    }
}
