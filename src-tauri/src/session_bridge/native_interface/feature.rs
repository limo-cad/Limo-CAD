//! Native profile-feature transaction: retained fields, real shared engine commands,
//! and transient renderer data all belong to one exact document revision.

use std::collections::{HashMap, HashSet};

use bevy::prelude::{Resource, World};
use nbcad_core::{BodyId, DocumentDto};
use nbcad_interface::{ControlInput, DocumentContext};
use nbcad_solid::{
    ExtrudeDefinitionDto, LoftDefinitionDto, PathRefDto, PlanarFaceSourceDto, ProfileRefDto,
    RevolveDefinitionDto, SweepDefinitionDto,
};
use serde_json::{json, Value};

use super::{check_owner, finish_mutation, model_snapshot, workspace::DocumentReceipt};
use crate::{
    native_forms::{DimensionKind, FormModel, ParameterValue, ProfileSource, SolidForm},
    native_viewport::{self, ViewportModel, ViewportPreview},
    session_bridge::{parse_engine_envelope, SessionBridgeState},
    state::AppState,
};

pub(crate) use crate::native_forms::{SolidField, SolidFieldView, SolidFormKind};

mod apply;
pub(super) mod editing;
pub(crate) mod manipulator;
mod move_copy;
pub(crate) mod panel;
mod picking;
mod preview;
pub(crate) use picking::{handle_canvas_pick, hover_references};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum FeatureCommand {
    Open {
        kind: SolidFormKind,
        feature_id: Option<u64>,
    },
    Control {
        form_id: u64,
        action: FeatureControl,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FeatureControl {
    Field(SolidField),
    Pick(SolidField),
    Clear(SolidField),
    Scroll(i32),
    Choose {
        field: SolidField,
        option: usize,
    },
    Apply,
    Cancel,
}

/// References are accepted only from the currently rendered source model.
/// Occurrence picks must be resolved to the edited component by the caller;
/// a placed assembly face is not a source-local face with the same integer ID.
#[derive(Clone, Debug)]
pub(crate) enum FeaturePick {
    Plane(nbcad_core::PlaneRef),
    AxisEdge(BodyId, nbcad_core::EdgeId),
    OccurrenceEdge(BodyId, nbcad_core::EdgeId, u64),
    Faces {
        body: Option<BodyId>,
        faces: Vec<nbcad_core::FaceId>,
    },
    Edges {
        body: Option<BodyId>,
        edges: Vec<nbcad_core::EdgeId>,
    },
    Profiles(Vec<ProfileRefDto>),
    Bodies(Vec<BodyId>),
    Occurrence(u64),
    Face(PlanarFaceSourceDto),
    MovePoint([f64; 3]),
    HoleSupport {
        face: PlanarFaceSourceDto,
        point: Option<[f64; 3]>,
    },
    HolePosition {
        point: [f64; 3],
        reference: Option<nbcad_solid::SketchPointRefDto>,
    },
    AxisLine {
        sketch_name: String,
        entity_id: u64,
    },
    Path(PathRefDto),
}

pub(crate) struct FeaturePanel {
    pub title: String,
    pub kind: SolidFormKind,
    pub form_id: u64,
    pub fields: Vec<SolidFieldView>,
    pub can_apply: bool,
    pub busy: bool,
    pub error: Option<String>,
    pub preview_notice: Option<String>,
    pub notes: Vec<String>,
    pub pick_target: Option<SolidField>,
    pub choice_field: Option<SolidField>,
}

struct Snapshot {
    receipt: DocumentReceipt,
    document: DocumentDto,
    viewport: ViewportModel,
    parameters: HashMap<String, Vec<ParameterValue>>,
    source_occurrences: HashSet<(u64, u64)>,
    assembly: nbcad_sketch::AssemblyDocumentDto,
    assembly_solution: nbcad_sketch::AssemblySolutionDto,
}

impl Snapshot {
    fn capture(engine: &AppState, receipt: DocumentReceipt) -> Result<Self, String> {
        let viewport = model_snapshot(engine);
        let assembly: nbcad_sketch::AssemblyDocumentDto = serde_json::from_value(
            parse_engine_envelope(engine.engine_call("assembly_document", ""))?,
        )
        .map_err(|e| e.to_string())?;
        let structure = &assembly.component_structure;
        let assembly_solution = serde_json::from_value(parse_engine_envelope(
            engine.engine_call("assembly_solution", ""),
        )?)
        .map_err(|e| e.to_string())?;
        let mut counts = HashMap::new();
        for pose in &viewport.instance_body_poses {
            *counts.entry(pose.body_id.0).or_insert(0usize) += 1;
        }
        let promoted: HashSet<_> = structure
            .definitions
            .iter()
            .filter(|d| d.promoted)
            .flat_map(|d| d.body_ids.iter().map(move |b| (d.id.0, b.0)))
            .collect();
        let roots: HashSet<_> = structure
            .occurrences
            .iter()
            .filter(|o| o.parent_occurrence_id.is_none())
            .map(|o| o.id.0)
            .collect();
        let source_occurrences = viewport
            .instance_body_poses
            .iter()
            .filter(|pose| {
                pose.visible
                    && pose
                        .translation
                        .iter()
                        .all(|v| v.is_finite() && v.abs() < 1e-9)
                    && pose.rotation.iter().all(|v| v.is_finite())
                    && pose.rotation[..3].iter().all(|v| v.abs() < 1e-9)
                    && (pose.rotation[3].abs() - 1.).abs() < 1e-9
                    && counts.get(&pose.body_id.0) == Some(&1)
                    && promoted.contains(&(pose.component_id.0, pose.body_id.0))
                    && roots.contains(&pose.occurrence_id.0)
            })
            .map(|p| (p.body_id.0, p.occurrence_id.0))
            .collect();
        let parameters = viewport
            .finished_sketches
            .iter()
            .map(|sketch| {
                let values = sketch
                    .dimensions
                    .iter()
                    .filter_map(|dimension| {
                        Some(ParameterValue {
                            name: dimension.param_name.clone()?,
                            kind: if dimension.kind == "angle" {
                                DimensionKind::Angle
                            } else {
                                DimensionKind::Length
                            },
                            value: dimension.value,
                        })
                    })
                    .collect();
                (sketch.name.clone(), values)
            })
            .collect();
        Ok(Self {
            receipt,
            document: engine.document_snapshot(),
            viewport,
            parameters,
            source_occurrences,
            assembly,
            assembly_solution,
        })
    }

    fn source_local(&self, body: u64, occurrence: Option<u64>) -> bool {
        occurrence.is_none_or(|id| self.source_occurrences.contains(&(body, id)))
    }

    fn model(&self, source: Option<&str>) -> FormModel<'_> {
        let parameters = match source {
            Some(sketch_name) => self
                .parameters
                .get(sketch_name)
                .map(Vec::as_slice)
                .unwrap_or(&[]),
            _ => &[],
        };
        FormModel {
            owner: &self.receipt.owner,
            engine_revision: self.receipt.revision,
            document: &self.document,
            profiles: &self.viewport.profile_catalog,
            scene: &self.viewport.scene,
            datum_planes: &self.viewport.datum_planes,
            parameters,
            assembly: Some(&self.assembly),
            assembly_solution: Some(&self.assembly_solution),
        }
    }
}

struct Editor {
    id: u64,
    form: SolidForm,
    snapshot: Snapshot,
    previous_preview: ViewportPreview,
    preview_revision: u64,
    preview_notice: Option<String>,
    pick_target: Option<SolidField>,
    choice_field: Option<SolidField>,
    stage: Option<std::sync::Arc<editing::Stage>>,
    original_view: Option<ViewportModel>,
    hovered_edge: Option<(BodyId, nbcad_core::EdgeId)>,
    hovered_face: Option<(BodyId, nbcad_core::FaceId)>,
    hovered_body: Option<BodyId>,
    hovered_occurrence: Option<u64>,
    hovered_plane: Option<nbcad_core::PlaneRef>,
    hovered_point: Option<[f64; 3]>,
    move_view: Option<move_copy::View>,
    move_hover: Option<move_copy::Handle>,
    move_drag: Option<move_copy::Drag>,
    offset_drag: Option<manipulator::Drag>,
}

#[derive(Resource, Default)]
struct NativeFeature {
    last_id: u64,
    editor: Option<Editor>,
}

/// The small receipt and effects are fenced together. Do not nest another
/// publisher-locking bridge operation inside `apply`.
fn with_receipt<T>(
    bridge: &SessionBridgeState,
    engine: &AppState,
    owner: &DocumentContext,
    apply: impl FnOnce(DocumentReceipt) -> Result<T, String>,
) -> Result<T, String> {
    let publishers = bridge
        .publishers
        .lock()
        .map_err(|_| "Session publisher lock poisoned")?;
    let publisher = publishers
        .get(&owner.window_id)
        .ok_or("Native window no longer exists")?;
    check_owner(publisher, engine, owner)?;
    apply(DocumentReceipt {
        owner: owner.clone(),
        revision: publisher.by_project[&owner.document_id].engine_revision,
    })
}

fn check_revision(editor: &Editor, receipt: &DocumentReceipt) -> Result<(), String> {
    if editor.snapshot.receipt != *receipt {
        return Err(
            "The design changed; cancel and reopen the feature with its current references".into(),
        );
    }
    Ok(())
}

pub(crate) fn panel(world: &World) -> Option<FeaturePanel> {
    let editor = world.get_resource::<NativeFeature>()?.editor.as_ref()?;
    let model = editor.snapshot.model(editor.form.parameter_sketch());
    Some(FeaturePanel {
        title: format!(
            "{}{}{}",
            if editor.form.is_feature_edit() {
                "Edit "
            } else {
                ""
            },
            if matches!(
                editor.form.kind(),
                SolidFormKind::Fillet | SolidFormKind::Chamfer
            ) {
                "Solid "
            } else {
                ""
            },
            editor.form.kind().label()
        ),
        kind: editor.form.kind(),
        form_id: editor.id,
        fields: editor.form.fields(&model),
        can_apply: editor.form.can_apply(&model),
        busy: editor.form.is_busy(),
        error: editor.form.engine_error().map(str::to_owned),
        preview_notice: editor.preview_notice.clone(),
        notes: editor.form.feature_notes(),
        pick_target: editor.pick_target,
        choice_field: editor.choice_field,
    })
}

/// A form cannot follow a model change, tab switch or project replacement.
/// Call after the native scene refresh and before publishing its controls.
pub(crate) fn synchronize(
    engine: &AppState,
    bridge: &SessionBridgeState,
    world: &mut World,
    owner: &DocumentContext,
) -> Result<(), String> {
    let mut state = world.remove_resource::<NativeFeature>().unwrap_or_default();
    let result = with_receipt(bridge, engine, owner, |receipt| {
        if state
            .editor
            .as_ref()
            .is_some_and(|editor| editor.snapshot.receipt != receipt)
        {
            let editor = state.editor.take().unwrap();
            if native_viewport::interface_preview_revision(world) == editor.preview_revision {
                if editor.form.kind().has_plane_references()
                    && editor.snapshot.receipt.owner.document_id == owner.document_id
                {
                    plane_view(world, owner, false, None)?;
                }
                native_viewport::apply_interface_preview(
                    world,
                    &owner.document_id,
                    ViewportPreview::default(),
                )?;
            }
        }
        if let Some(editor) = state
            .editor
            .as_mut()
            .filter(|e| move_copy::needs_refresh(e, world))
        {
            update_preview(editor, world)?;
        }
        Ok(())
    });
    world.insert_resource(state);
    result
}

fn plane_view(
    world: &mut World,
    owner: &DocumentContext,
    enabled: bool,
    hovered: Option<nbcad_core::PlaneRef>,
) -> Result<(), String> {
    use native_viewport::{ViewportMode, ViewportOriginPlane};
    use nbcad_core::{OriginPlane, PlaneRef};
    let (id, _, mut view, _) = native_viewport::interface_view_snapshot(world);
    if id != owner.document_id {
        return Ok(());
    }
    let old = view.clone();
    if enabled {
        view.mode = ViewportMode::PickPlane;
    } else if view.mode == ViewportMode::PickPlane {
        view.mode = ViewportMode::Solid;
    }
    view.hovered_origin_plane = None;
    view.hovered_datum_plane_id = None;
    view.hovered_face_id = None;
    match hovered {
        Some(PlaneRef::OriginPlane { plane }) => {
            view.hovered_origin_plane = Some(match plane {
                OriginPlane::Xy => ViewportOriginPlane::Xy,
                OriginPlane::Xz => ViewportOriginPlane::Xz,
                OriginPlane::Yz => ViewportOriginPlane::Yz,
            })
        }
        Some(PlaneRef::DatumPlane { datum_id }) => view.hovered_datum_plane_id = Some(datum_id.0),
        Some(PlaneRef::PlanarFace { face_id }) => view.hovered_face_id = Some(face_id.0),
        None => (),
    }
    if old != view {
        native_viewport::apply_interface_view(world, &owner.document_id, None, Some(view))?;
    }
    Ok(())
}

fn update_preview(editor: &mut Editor, world: &mut World) -> Result<(), String> {
    if editor.form.kind() == SolidFormKind::MoveCopy {
        let next = move_copy::preview(editor, world)?;
        native_viewport::apply_interface_preview(
            world,
            &editor.snapshot.receipt.owner.document_id,
            next,
        )?;
        editor.preview_revision = native_viewport::interface_preview_revision(world);
        editor.preview_notice = None;
        return Ok(());
    }
    let model = editor.snapshot.model(editor.form.parameter_sketch());
    if editor.form.kind().has_plane_references() {
        let enabled = editor.form.kind().is_plane()
            || matches!(editor.pick_target, Some(SolidField::FirstPlane));
        plane_view(
            world,
            model.owner,
            enabled,
            if enabled { editor.hovered_plane } else { None },
        )?;
    }
    // Invalid text remains the actual field draft, but must not leave a
    // stale last-valid extrusion appearing to describe the invalid input.
    let (mut next, mut notice) = if editor.form.kind() != SolidFormKind::Extrude {
        match preview::references(&editor.form, &model, &editor.snapshot.viewport) {
            Ok(value) => (value, None),
            Err(e) => (ViewportPreview::default(), Some(e)),
        }
    } else {
        match editor.form.prepare_preview(&model) {
            Ok(ticket) => match preview::build(ticket.request(), &editor.snapshot.viewport) {
                Ok(next) if editor.form.accepts_preview(&ticket, &model) => (next, None),
                Ok(_) => return Err("Extrude preview was superseded".into()),
                Err(error) => (ViewportPreview::default(), Some(error)),
            },
            Err(_) => (ViewportPreview::default(), None),
        }
    };
    if let Some((body, edge)) = editor.hovered_edge {
        if let Some(edge) = model
            .scene
            .bodies
            .iter()
            .find(|b| b.id == body)
            .and_then(|b| b.edges.iter().find(|e| e.id == edge))
            .filter(|e| e.points.len() <= 100_001)
        {
            let segments: Vec<f32> = edge
                .points
                .windows(2)
                .flat_map(|pair| {
                    pair.iter()
                        .flat_map(|p| [p.x as f32, p.y as f32, p.z as f32])
                })
                .collect();
            if segments.len() <= 600_000 && segments.iter().all(|v| v.is_finite()) {
                next.lines.push(native_viewport::ViewportLineLayer {
                    color: [1., 0.66, 0.25, 1.],
                    width: 3.,
                    segments,
                    ..Default::default()
                });
            }
        }
    }
    if let Some((body, face)) = editor.hovered_face {
        if !editor
            .form
            .selected_faces()
            .is_some_and(|(b, ids)| b == body && ids.contains(&face))
        {
            match preview::face_fill(model.scene, body, &[face], [1., 0.65, 0.2, 0.25]) {
                Ok(fill) => next.triangles.push(fill),
                Err(error) => notice = Some(error),
            }
        }
    }
    if let Some(body) = editor.hovered_body {
        if !editor.form.selected_bodies().contains(&body)
            && !editor
                .form
                .combine_bodies(SolidField::TargetBody)
                .contains(&body)
            && !editor
                .form
                .combine_bodies(SolidField::ToolBodies)
                .contains(&body)
        {
            match preview::body_fill(model.scene, body, [1., 0.7, 0.35, 0.15]) {
                Ok(fill) => next.triangles.push(fill),
                Err(error) => notice = Some(error),
            }
        }
    }
    if let Some(point) = editor
        .hovered_point
        .filter(|_| editor.pick_target == Some(SolidField::HolePositions))
    {
        let (_, camera, _, _) = native_viewport::interface_view_snapshot(world);
        let distance = (0..3)
            .map(|i| (camera.position[i] as f64 - point[i]).powi(2))
            .sum::<f64>()
            .sqrt();
        next.points.push(native_viewport::ViewportPointLayer {
            color: [1., 0.7, 0.2, 1.],
            color_role: Default::default(),
            radius: (distance * 0.003) as f32,
            hollow: false,
            positions: point.into_iter().map(|v| v as f32).collect(),
        });
    }
    native_viewport::apply_interface_preview(world, &model.owner.document_id, next)?;
    editor.preview_revision = native_viewport::interface_preview_revision(world);
    editor.preview_notice = notice;
    Ok(())
}

fn selected_source(world: &World, snapshot: &Snapshot) -> Result<Option<FeaturePick>, String> {
    let (_, _, presentation, _) = native_viewport::interface_view_snapshot(world);
    if presentation.selected_occurrence_id.is_some()
        && presentation
            .selected_body_ids
            .iter()
            .any(|body| !snapshot.source_local(*body, presentation.selected_occurrence_id))
    {
        return Err(
            "Open the component for editing before extruding an assembly occurrence".into(),
        );
    }
    if !presentation.selected_profiles.is_empty() {
        return Ok(Some(FeaturePick::Profiles(presentation.selected_profiles)));
    }
    match (
        presentation.selected_body_ids.as_slice(),
        presentation.selected_face_ids.as_slice(),
    ) {
        ([body], [face]) => Ok(Some(FeaturePick::Face(PlanarFaceSourceDto {
            body_id: BodyId(*body),
            face_id: nbcad_core::FaceId(*face),
        }))),
        _ => Ok(None),
    }
}

fn apply_pick(editor: &mut Editor, pick: FeaturePick) -> Result<(), String> {
    let model = editor.snapshot.model(editor.form.parameter_sketch());
    match (editor.pick_target, pick) {
        (Some(SolidField::Bodies), FeaturePick::Occurrence(id)) => {
            editor.form.set_move_occurrence(Some(id), &model)
        }
        (Some(SolidField::Bodies), FeaturePick::Bodies(bodies)) => {
            editor.form.set_bodies(bodies, &model)
        }
        (
            Some(field @ (SolidField::FirstPlane | SolidField::SecondPlane)),
            FeaturePick::Plane(plane),
        ) => {
            editor
                .form
                .set_plane_reference(field, Some(plane), &model)?;
            editor.pick_target = if field == SolidField::FirstPlane
                && editor.form.kind() == SolidFormKind::Midplane
            {
                Some(SolidField::SecondPlane)
            } else if field == SolidField::FirstPlane
                && editor.form.kind() == SolidFormKind::AnglePlane
            {
                Some(SolidField::AxisEdge)
            } else {
                None
            };
            Ok(())
        }
        (Some(field), FeaturePick::OccurrenceEdge(body, edge, id))
            if field.is_straight_reference() && editor.form.move_is_component() =>
        {
            editor
                .form
                .set_move_edge(field, Some((body, edge)), Some(id), &model)?;
            editor.pick_target = None;
            Ok(())
        }
        (Some(field), FeaturePick::AxisEdge(body, edge)) if field.is_straight_reference() => {
            if editor.form.kind() == SolidFormKind::MoveCopy {
                editor
                    .form
                    .set_move_edge(field, Some((body, edge)), None, &model)?;
            } else if editor.form.kind().is_pattern() {
                editor
                    .form
                    .set_pattern_edge(field, Some((body, edge)), &model)?;
            } else {
                editor.form.set_plane_axis(Some((body, edge)), &model)?;
            }
            editor.pick_target = None;
            Ok(())
        }
        (
            Some(field @ (SolidField::TargetBody | SolidField::ToolBodies)),
            FeaturePick::Bodies(bodies),
        ) => editor.form.set_combine_bodies(field, bodies, &model),
        (Some(field), FeaturePick::MovePoint(point)) if field.is_move_point() => {
            editor.form.set_move_point(field, point, &model)?;
            if field == SolidField::FromPoint {
                editor.pick_target = Some(SolidField::ToPoint);
            }
            Ok(())
        }
        (Some(SolidField::HoleSupport), FeaturePick::HoleSupport { face, point }) => {
            editor.form.set_hole_support(Some(face), point, &model)?;
            editor.pick_target = Some(SolidField::HolePositions);
            Ok(())
        }
        (Some(SolidField::HolePositions), FeaturePick::HolePosition { point, reference }) => {
            editor.form.set_hole_position(point, reference, &model)
        }
        (Some(SolidField::Cylinder), FeaturePick::Face(face)) => {
            editor.form.set_thread_face(Some(face), &model)?;
            editor.pick_target = None;
            Ok(())
        }
        (Some(SolidField::Faces), FeaturePick::Faces { body, faces }) => {
            editor.form.set_faces(body, faces, &model)
        }
        (Some(SolidField::Edges), FeaturePick::Edges { body, edges }) => {
            editor.form.set_edges(body, edges, &model)
        }
        (Some(SolidField::Source), FeaturePick::Profiles(profiles)) => {
            editor.form.set_profiles(profiles, &model)
        }
        (Some(field @ (SolidField::Path | SolidField::Guide)), FeaturePick::Path(path)) => editor
            .form
            .set_path(field, (!path.entity_ids.is_empty()).then_some(path), &model),
        (Some(SolidField::Source), FeaturePick::Face(face)) => {
            editor.form.set_source(ProfileSource::Face(face), &model)
        }
        (Some(SolidField::Targets), FeaturePick::Bodies(bodies)) => {
            editor.form.set_targets(bodies, &model)
        }
        (Some(SolidField::StopFace), FeaturePick::Face(face)) => {
            editor.form.set_stop_face(Some(face), &model)
        }
        (
            Some(SolidField::AxisLine),
            FeaturePick::AxisLine {
                sketch_name,
                entity_id,
            },
        ) => editor.form.set_axis(Some((sketch_name, entity_id)), &model),
        _ => Err("This selection does not match the active feature reference field".into()),
    }
}

pub(crate) fn accept_pick(
    engine: &AppState,
    bridge: &SessionBridgeState,
    world: &mut World,
    owner: &DocumentContext,
    form_id: u64,
    pick: FeaturePick,
    validate_control: impl FnOnce() -> Result<(), String>,
) -> Result<Value, String> {
    let mut state = world.remove_resource::<NativeFeature>().unwrap_or_default();
    let result = with_receipt(bridge, engine, owner, |receipt| {
        validate_control()?;
        let editor = state
            .editor
            .as_mut()
            .filter(|editor| editor.id == form_id)
            .ok_or("The feature form changed")?;
        check_revision(editor, &receipt)?;
        apply_pick(editor, pick)?;
        update_preview(editor, world)?;
        Ok(json!({"form_id":form_id,"reference_accepted":true}))
    });
    world.insert_resource(state);
    result
}

pub(crate) fn reduce(
    engine: &AppState,
    bridge: &SessionBridgeState,
    world: &mut World,
    owner: &DocumentContext,
    command: &FeatureCommand,
    input: &ControlInput,
    validate_control: impl FnOnce() -> Result<(), String>,
) -> Result<Value, String> {
    if matches!(input, ControlInput::Key(key) if key.key == "Escape" && !key.ctrl && !key.meta && !key.alt && !key.shift)
    {
        if let FeatureCommand::Control { form_id, .. } = command {
            // The focused field's original binding/owner still authorize
            // this input; Escape closes that same form, not a later editor.
            return reduce(
                engine,
                bridge,
                world,
                owner,
                &FeatureCommand::Control {
                    form_id: *form_id,
                    action: FeatureControl::Cancel,
                },
                &ControlInput::Click,
                validate_control,
            );
        }
    }
    let field_edit = matches!(
        command,
        FeatureCommand::Control {
            action: FeatureControl::Field(_),
            ..
        }
    );
    if (field_edit
        && !matches!(
            input,
            ControlInput::SetValue(_)
                | ControlInput::Click
                | ControlInput::DoubleClick
                | ControlInput::Key(_)
        ))
        || (!field_edit && !super::is_activation(input))
    {
        return Err("This input does not match the feature control".into());
    }
    let mut state = world.remove_resource::<NativeFeature>().unwrap_or_default();
    let result = reduce_owned(
        engine,
        bridge,
        world,
        owner,
        command,
        input,
        validate_control,
        &mut state,
    );
    world.insert_resource(state);
    result
}

fn reduce_owned(
    engine: &AppState,
    bridge: &SessionBridgeState,
    world: &mut World,
    owner: &DocumentContext,
    command: &FeatureCommand,
    input: &ControlInput,
    validate_control: impl FnOnce() -> Result<(), String>,
    state: &mut NativeFeature,
) -> Result<Value, String> {
    if let FeatureCommand::Open { kind, feature_id } = command {
        if let Some(feature_id) = feature_id.filter(|_| {
            matches!(
                kind,
                SolidFormKind::Fillet
                    | SolidFormKind::Chamfer
                    | SolidFormKind::MoveCopy
                    | SolidFormKind::Hole
                    | SolidFormKind::ExternalThread
                    | SolidFormKind::Shell
                    | SolidFormKind::Combine
                    | SolidFormKind::OffsetPlane
                    | SolidFormKind::Midplane
                    | SolidFormKind::AnglePlane
                    | SolidFormKind::Mirror
                    | SolidFormKind::SplitBody
                    | SolidFormKind::RectangularPattern
                    | SolidFormKind::CircularPattern
            )
        }) {
            return editing::begin(
                engine,
                bridge,
                world,
                owner,
                *kind,
                feature_id,
                validate_control,
                state,
            );
        }
        return with_receipt(bridge, engine, owner, |receipt| {
            validate_control()?;
            if state.editor.is_some() {
                return Err("Finish or cancel the open feature form first".into());
            }
            let id = state
                .last_id
                .checked_add(1)
                .ok_or("Feature form identities exhausted")?;
            let snapshot = Snapshot::capture(engine, receipt)?;
            if snapshot.viewport.active_sketch.is_some() {
                return Err("Finish the active sketch before opening a solid feature".into());
            }
            if snapshot.viewport.session_id != owner.document_id
                || native_viewport::interface_view_snapshot(world).0 != owner.document_id
            {
                return Err("The rendered design is not current".into());
            }
            let model = snapshot.model(None);
            crate::native_editor::support::cancel(world, owner)?;
            let form = if let (SolidFormKind::Rib, Some(id)) = (kind, feature_id) {
                let definitions: Vec<nbcad_solid::RibDefinitionDto> = serde_json::from_value(
                    parse_engine_envelope(engine.engine_call("rib_definitions", ""))?,
                )
                .map_err(|e| e.to_string())?;
                SolidForm::edit_rib(
                    definitions
                        .iter()
                        .find(|d| d.feature_id.0 == *id)
                        .ok_or("The selected Rib no longer exists")?,
                    &model,
                )?
            } else if let (SolidFormKind::Sweep, Some(id)) = (kind, feature_id) {
                let definitions: Vec<SweepDefinitionDto> = serde_json::from_value(
                    parse_engine_envelope(engine.engine_call("sweep_definitions", ""))?,
                )
                .map_err(|e| e.to_string())?;
                SolidForm::edit_sweep(
                    definitions
                        .iter()
                        .find(|d| d.feature_id.0 == *id)
                        .ok_or("The selected Sweep no longer exists")?,
                    &model,
                )?
            } else if let (SolidFormKind::Loft, Some(id)) = (kind, feature_id) {
                let definitions: Vec<LoftDefinitionDto> = serde_json::from_value(
                    parse_engine_envelope(engine.engine_call("loft_definitions", ""))?,
                )
                .map_err(|e| e.to_string())?;
                SolidForm::edit_loft(
                    definitions
                        .iter()
                        .find(|d| d.feature_id.0 == *id)
                        .ok_or("The selected Loft no longer exists")?,
                    &model,
                )?
            } else if let (SolidFormKind::Revolve, Some(id)) = (kind, feature_id) {
                let definitions: Vec<RevolveDefinitionDto> = serde_json::from_value(
                    parse_engine_envelope(engine.engine_call("revolve_definitions", ""))?,
                )
                .map_err(|e| e.to_string())?;
                let definition = definitions
                    .iter()
                    .find(|d| d.feature_id.0 == *id)
                    .ok_or("The selected Revolve no longer exists")?;
                SolidForm::edit_revolve(definition, &model)?
            } else if let Some(id) = feature_id {
                let definitions: Vec<ExtrudeDefinitionDto> = serde_json::from_value(
                    parse_engine_envelope(engine.engine_call("extrude_definitions", ""))?,
                )
                .map_err(|error| error.to_string())?;
                let definition = definitions
                    .iter()
                    .find(|definition| definition.feature_id.0 == *id)
                    .ok_or("The selected Extrude feature no longer exists")?;
                SolidForm::edit(definition, &model)?
            } else {
                SolidForm::new_kind(*kind, &model)
            };
            let mut editor = Editor {
                id,
                form,
                snapshot,
                previous_preview: native_viewport::interface_preview_snapshot(world),
                preview_revision: native_viewport::interface_preview_revision(world),
                preview_notice: None,
                pick_target: Some(if kind.selects_bodies() {
                    SolidField::Bodies
                } else if kind.is_plane() {
                    SolidField::FirstPlane
                } else if matches!(kind, SolidFormKind::Fillet | SolidFormKind::Chamfer) {
                    SolidField::Edges
                } else if *kind == SolidFormKind::Combine {
                    SolidField::TargetBody
                } else if *kind == SolidFormKind::Hole {
                    SolidField::HoleSupport
                } else if *kind == SolidFormKind::ExternalThread {
                    SolidField::Cylinder
                } else if *kind == SolidFormKind::Shell {
                    SolidField::Faces
                } else if *kind == SolidFormKind::Rib {
                    SolidField::Path
                } else {
                    SolidField::Source
                }),
                choice_field: None,
                stage: None,
                original_view: None,
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
            if feature_id.is_none()
                && matches!(kind, SolidFormKind::Fillet | SolidFormKind::Chamfer)
            {
                let (_, _, presentation, _) = native_viewport::interface_view_snapshot(world);
                if presentation.selected_occurrence_id.is_some()
                    && presentation.selected_body_ids.iter().any(|body| {
                        !editor
                            .snapshot
                            .source_local(*body, presentation.selected_occurrence_id)
                    })
                {
                    return Err("Open the component before refining its edges".into());
                }
                if let [body] = presentation.selected_body_ids.as_slice() {
                    if !presentation.selected_edge_ids.is_empty() {
                        apply_pick(
                            &mut editor,
                            FeaturePick::Edges {
                                body: Some(BodyId(*body)),
                                edges: presentation
                                    .selected_edge_ids
                                    .iter()
                                    .copied()
                                    .map(nbcad_core::EdgeId)
                                    .collect(),
                            },
                        )?;
                    }
                }
            } else if feature_id.is_none() && *kind == SolidFormKind::Combine {
                let (_, _, presentation, _) = native_viewport::interface_view_snapshot(world);
                let bodies: Vec<_> = presentation
                    .selected_body_ids
                    .iter()
                    .copied()
                    .filter(|body| {
                        editor
                            .snapshot
                            .source_local(*body, presentation.selected_occurrence_id)
                    })
                    .map(BodyId)
                    .collect();
                if let Some(target) = bodies.first() {
                    apply_pick(&mut editor, FeaturePick::Bodies(vec![*target]))?;
                    if bodies.len() > 1 {
                        editor.pick_target = Some(SolidField::ToolBodies);
                        apply_pick(&mut editor, FeaturePick::Bodies(bodies[1..].to_vec()))?;
                    }
                }
            } else if feature_id.is_none()
                && matches!(kind, SolidFormKind::ExternalThread | SolidFormKind::Hole)
            {
                let (_, _, presentation, _) = native_viewport::interface_view_snapshot(world);
                if let ([body], [face]) = (
                    presentation.selected_body_ids.as_slice(),
                    presentation.selected_face_ids.as_slice(),
                ) {
                    if editor
                        .snapshot
                        .source_local(*body, presentation.selected_occurrence_id)
                    {
                        apply_pick(
                            &mut editor,
                            if *kind == SolidFormKind::Hole {
                                FeaturePick::HoleSupport {
                                    face: PlanarFaceSourceDto {
                                        body_id: BodyId(*body),
                                        face_id: nbcad_core::FaceId(*face),
                                    },
                                    point: presentation
                                        .selected_surface_point
                                        .map(|p| [p.x, p.y, p.z]),
                                }
                            } else {
                                FeaturePick::Face(PlanarFaceSourceDto {
                                    body_id: BodyId(*body),
                                    face_id: nbcad_core::FaceId(*face),
                                })
                            },
                        )?;
                    }
                }
            } else if feature_id.is_none() && *kind == SolidFormKind::Shell {
                let (_, _, presentation, _) = native_viewport::interface_view_snapshot(world);
                if let [body] = presentation.selected_body_ids.as_slice() {
                    if editor
                        .snapshot
                        .source_local(*body, presentation.selected_occurrence_id)
                        && !presentation.selected_face_ids.is_empty()
                    {
                        apply_pick(
                            &mut editor,
                            FeaturePick::Faces {
                                body: Some(BodyId(*body)),
                                faces: presentation
                                    .selected_face_ids
                                    .iter()
                                    .copied()
                                    .map(nbcad_core::FaceId)
                                    .collect(),
                            },
                        )?;
                    }
                }
            } else if feature_id.is_none()
                && *kind == SolidFormKind::MoveCopy
                && SolidForm::smart_move_occurrence(
                    &editor.snapshot.model(None),
                    native_viewport::interface_view_snapshot(world)
                        .2
                        .selected_occurrence_id,
                    &native_viewport::interface_view_snapshot(world)
                        .2
                        .selected_body_ids,
                )
                .is_some()
            {
                let p = native_viewport::interface_view_snapshot(world).2;
                let id = SolidForm::smart_move_occurrence(
                    &editor.snapshot.model(None),
                    p.selected_occurrence_id,
                    &p.selected_body_ids,
                )
                .unwrap();
                apply_pick(&mut editor, FeaturePick::Occurrence(id))?;
            } else if feature_id.is_none() && kind.selects_bodies() {
                let (_, _, presentation, _) = native_viewport::interface_view_snapshot(world);
                let mut bodies: Vec<_> = presentation
                    .selected_body_ids
                    .iter()
                    .copied()
                    .filter(|body| {
                        editor
                            .snapshot
                            .source_local(*body, presentation.selected_occurrence_id)
                    })
                    .map(BodyId)
                    .collect();
                if *kind == SolidFormKind::SplitBody {
                    bodies.truncate(1);
                }
                apply_pick(&mut editor, FeaturePick::Bodies(bodies))?;
            } else if feature_id.is_none() && !kind.is_plane() && *kind != SolidFormKind::Rib {
                if let Some(pick) = selected_source(world, &editor.snapshot)? {
                    if !matches!(pick, FeaturePick::Face(_)) || *kind == SolidFormKind::Extrude {
                        apply_pick(&mut editor, pick)?;
                    }
                }
            }
            update_preview(&mut editor, world)?;
            state.editor = Some(editor);
            state.last_id = id;
            Ok(json!({"form_id":id,"opened":true}))
        });
    }
    let FeatureCommand::Control { form_id, action } = command else {
        unreachable!()
    };
    if matches!(action, FeatureControl::Apply) {
        return apply::begin(
            engine,
            bridge,
            world,
            owner,
            *form_id,
            validate_control,
            state,
        );
    }
    let editor = state
        .editor
        .as_mut()
        .filter(|editor| editor.id == *form_id)
        .ok_or("The feature form changed")?;
    if editor.form.is_busy() {
        return Err("The feature is still applying".into());
    }
    let mut close = false;
    let result = with_receipt(bridge, engine, owner, |receipt| {
        validate_control()?;
        if matches!(action, FeatureControl::Cancel) {
            let owns_preview =
                native_viewport::interface_preview_revision(world) == editor.preview_revision;
            let can_restore = receipt == editor.snapshot.receipt && owns_preview;
            let mut model = editor.snapshot.model(editor.form.parameter_sketch());
            model.owner = &receipt.owner;
            model.engine_revision = receipt.revision;
            editor.form.cancel(&model)?;
            if editor.form.kind().has_plane_references() {
                plane_view(world, owner, false, None)?;
            }
            let restoration = if can_restore {
                move_copy::restore(editor, world)?;
                if let Some(original) = editor.original_view.take() {
                    native_viewport::apply_interface_model(world, original)?;
                }
                native_viewport::apply_interface_preview(
                    world,
                    &receipt.owner.document_id,
                    editor.previous_preview.clone(),
                )
            } else if owns_preview {
                native_viewport::apply_interface_preview(
                    world,
                    &receipt.owner.document_id,
                    ViewportPreview::default(),
                )
            } else {
                Ok(())
            };
            close = true;
            return Ok(
                json!({"form_id":form_id,"cancelled":true,"preview_restored":can_restore && restoration.is_ok(),"preview_error":restoration.err()}),
            );
        }
        check_revision(editor, &receipt)?;
        // A form action leaves the canvas: do not retain a hit from the last
        // pointer move, including when MCP focuses or edits a field directly.
        let move_had_hover = editor.move_hover.take().is_some();
        editor.move_drag = None;
        let had_hover = move_had_hover
            | editor.hovered_point.take().is_some()
            | editor.hovered_edge.take().is_some()
            | editor.hovered_face.take().is_some()
            | editor.hovered_body.take().is_some()
            | editor.hovered_occurrence.take().is_some()
            | editor.hovered_plane.take().is_some();
        if had_hover {
            update_preview(editor, world)?;
        }
        let model = editor.snapshot.model(editor.form.parameter_sketch());
        match action {
            FeatureControl::Scroll(delta) => {
                if !super::is_activation(input) {
                    return Err("Activate a panel scroll control".into());
                }
                panel::scroll_by(world, *delta as f32)?;
                return Ok(json!({"form_id":form_id,"scrolled":true}));
            }
            FeatureControl::Field(field) => {
                let row = editor
                    .form
                    .fields(&model)
                    .into_iter()
                    .find(|row| row.field == *field && row.visible && row.enabled)
                    .ok_or("This feature field is not available")?;
                match (input, row.value) {
                    (ControlInput::SetValue(value), _) => {
                        editor.form.set_value(*field, value, &model)?;
                        editor.choice_field = None;
                    }
                    (input, nbcad_interface::Field::Text { .. }) if super::is_activation(input) => {
                        return Ok(json!({"form_id":form_id,"focused":true}));
                    }
                    (input, nbcad_interface::Field::Toggle(value))
                        if super::is_activation(input) =>
                    {
                        editor.form.set_value(
                            *field,
                            if value { "false" } else { "true" },
                            &model,
                        )?;
                    }
                    (input, nbcad_interface::Field::Choice { value, options }) => {
                        if super::is_activation(input) {
                            editor.choice_field =
                                (editor.choice_field != Some(*field)).then_some(*field);
                            return Ok(
                                json!({"form_id":form_id,"choices_open":editor.choice_field.is_some()}),
                            );
                        }
                        let ControlInput::Key(key) = input else {
                            return Err("Choose an available field value".into());
                        };
                        if key.ctrl || key.meta || key.alt || key.shift {
                            return Err("This field key is not supported".into());
                        }
                        let options: Vec<_> =
                            options.iter().filter(|option| !option.disabled).collect();
                        let index = options
                            .iter()
                            .position(|option| option.value == value)
                            .unwrap_or(0);
                        let next = match key.key.as_str() {
                            "ArrowDown" => {
                                index.saturating_add(1).min(options.len().saturating_sub(1))
                            }
                            "ArrowUp" => index.saturating_sub(1),
                            "Home" => 0,
                            "End" => options.len().saturating_sub(1),
                            _ => return Err("This field key is not supported".into()),
                        };
                        let option = options
                            .get(next)
                            .ok_or("This field has no available choices")?;
                        editor.form.set_value(*field, &option.value, &model)?;
                    }
                    _ => return Err("This input does not match the feature field".into()),
                }
            }
            FeatureControl::Choose { field, option } => {
                if editor.choice_field != Some(*field)
                    && !matches!(field, SolidField::Axis | SolidField::MoveObjectType)
                {
                    return Err("This choice list is closed".into());
                }
                let row = editor
                    .form
                    .fields(&model)
                    .into_iter()
                    .find(|row| row.field == *field && row.visible && row.enabled)
                    .ok_or("This feature field is not available")?;
                let nbcad_interface::Field::Choice { options, .. } = row.value else {
                    return Err("This field has no choices".into());
                };
                let option = options
                    .get(*option)
                    .filter(|option| !option.disabled)
                    .ok_or("This field choice is not available")?;
                editor.form.set_value(*field, &option.value, &model)?;
                editor.choice_field = None;
            }
            FeatureControl::Pick(field) => {
                if !matches!(
                    field,
                    SolidField::Source
                        | SolidField::FromPoint
                        | SolidField::ToPoint
                        | SolidField::PivotPoint
                        | SolidField::HoleSupport
                        | SolidField::HolePositions
                        | SolidField::Cylinder
                        | SolidField::Faces
                        | SolidField::TargetBody
                        | SolidField::ToolBodies
                        | SolidField::FirstPlane
                        | SolidField::SecondPlane
                        | SolidField::AxisEdge
                        | SolidField::DirectionEdge
                        | SolidField::SecondDirectionEdge
                        | SolidField::Bodies
                        | SolidField::Edges
                        | SolidField::Targets
                        | SolidField::StopFace
                        | SolidField::AxisLine
                        | SolidField::Path
                        | SolidField::Guide
                ) {
                    return Err("This field is not a geometry reference".into());
                }
                editor.pick_target = Some(*field);
                editor.choice_field = None;
            }
            FeatureControl::Clear(field) => match field {
                SolidField::Bodies if editor.form.move_is_component() => {
                    editor.form.set_move_occurrence(None, &model)?
                }
                SolidField::Bodies => editor.form.set_bodies(vec![], &model)?,
                SolidField::FirstPlane | SolidField::SecondPlane => {
                    editor.form.set_plane_reference(*field, None, &model)?;
                    editor.pick_target = Some(*field);
                }
                field if field.is_straight_reference() => {
                    if editor.form.kind() == SolidFormKind::MoveCopy {
                        editor.form.set_move_edge(*field, None, None, &model)?;
                    } else if editor.form.kind().is_pattern() {
                        editor.form.set_pattern_edge(*field, None, &model)?;
                    } else {
                        editor.form.set_plane_axis(None, &model)?;
                    }
                    editor.pick_target = Some(*field);
                }
                SolidField::TargetBody | SolidField::ToolBodies => {
                    editor.form.set_combine_bodies(*field, Vec::new(), &model)?
                }
                field if field.is_move_point() => {
                    editor.form.set_move_point(*field, [0.; 3], &model)?;
                    editor.pick_target = Some(*field);
                }
                SolidField::HoleSupport => {
                    editor.form.set_hole_support(None, None, &model)?;
                    editor.pick_target = Some(*field);
                }
                SolidField::HolePositions => {
                    editor.form.clear_hole_positions(&model)?;
                    editor.pick_target = Some(*field);
                }
                SolidField::Cylinder => {
                    editor.form.set_thread_face(None, &model)?;
                    editor.pick_target = Some(*field);
                }
                SolidField::Faces => editor.form.set_faces(None, Vec::new(), &model)?,
                SolidField::Edges => editor.form.set_edges(None, Vec::new(), &model)?,
                SolidField::Source => editor.form.set_profiles(Vec::new(), &model)?,
                SolidField::Targets => editor.form.set_targets(Vec::new(), &model)?,
                SolidField::StopFace => editor.form.set_stop_face(None, &model)?,
                SolidField::AxisLine => editor.form.set_axis(None, &model)?,
                SolidField::Path | SolidField::Guide => {
                    editor.form.set_path(*field, None, &model)?
                }
                _ => return Err("This field is not a geometry reference".into()),
            },
            FeatureControl::Apply | FeatureControl::Cancel => unreachable!(),
        }
        if matches!(
            action,
            FeatureControl::Field(SolidField::MoveObjectType)
                | FeatureControl::Choose {
                    field: SolidField::MoveObjectType,
                    ..
                }
        ) {
            editor.pick_target = Some(SolidField::Bodies);
        }
        if matches!(
            action,
            FeatureControl::Field(SolidField::Axis)
                | FeatureControl::Choose {
                    field: SolidField::Axis,
                    ..
                }
        ) {
            editor.pick_target = editor
                .form
                .fields(&model)
                .iter()
                .any(|r| r.field == SolidField::AxisLine && r.visible)
                .then_some(SolidField::AxisLine);
        }
        if editor.form.kind() != SolidFormKind::Hole
            && matches!(
                action,
                FeatureControl::Field(SolidField::Extent)
                    | FeatureControl::Choose {
                        field: SolidField::Extent,
                        ..
                    }
            )
        {
            let to_face = editor
                .form
                .fields(&model)
                .iter()
                .any(|row| row.field == SolidField::StopFace && row.visible);
            editor.pick_target = if to_face {
                Some(SolidField::StopFace)
            } else if editor.form.kind() == SolidFormKind::Rib {
                Some(SolidField::Path)
            } else {
                Some(SolidField::Source)
            };
        }
        if matches!(
            action,
            FeatureControl::Field(SolidField::GuideEnabled | SolidField::CenterlineEnabled)
        ) {
            editor.pick_target = editor
                .form
                .fields(&model)
                .iter()
                .find(|r| {
                    r.visible
                        && r.field
                            == if matches!(action, FeatureControl::Field(SolidField::GuideEnabled))
                            {
                                SolidField::Guide
                            } else {
                                SolidField::Path
                            }
                })
                .map(|r| r.field)
                .or(Some(SolidField::Source));
        }
        if matches!(
            action,
            FeatureControl::Field(SolidField::MoveMode)
                | FeatureControl::Choose {
                    field: SolidField::MoveMode,
                    ..
                }
        ) {
            editor.pick_target = editor.form.move_pick_target();
        }
        if matches!(action, FeatureControl::Field(SolidField::SecondEnabled)) {
            editor.pick_target = Some(
                if editor
                    .form
                    .fields(&model)
                    .iter()
                    .any(|r| r.field == SolidField::SecondDirectionEdge && r.visible)
                {
                    SolidField::SecondDirectionEdge
                } else {
                    SolidField::DirectionEdge
                },
            );
        }
        update_preview(editor, world)?;
        Ok(json!({"form_id":form_id,"edited":true}))
    });
    if close {
        state.editor = None;
    }
    result
}

#[cfg(test)]
mod tests;
