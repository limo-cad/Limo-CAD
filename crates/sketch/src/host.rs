//! Host dispatch: the single JSON entry point both engine hosts (native
//! commands, wasm-bindgen exports) funnel through, so native and browser
//! behavior are identical by construction.
//!
//! Every method takes a JSON-string payload and returns the JSON envelope
//! (`ok_json`/`err_json`, with optional structured `data` for conflict
//! reports). Native UI and MCP hosts use the same operation names.

use limo_cad_solid::{
    BodyFeatureRequestDto, CommitKernelRequest, DatumPlaneRequest, DeleteFeatureRequest,
    EditBodyFeatureRequest, EditDatumPlaneRequest, EditExtrudeRequest, EditHoleRequest,
    EditLoftRequest, EditRevolveRequest, EditRibRequest, EditSolidChamferRequest,
    EditSolidFilletRequest, EditSweepRequest, ExtrudeRequest, HoleRequest, LoftRequest,
    ReorderFeatureRequest, RevolveRequest, RibRequest, SetRollbackRequest, SolidChamferRequest,
    SolidFilletRequest, SweepRequest,
};
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::constraint::Constraint;
use crate::dto::{
    err_json, ok_json, Arc3PointRequest, ArcCenterRequest, BeginSketchRequest, BreakRequest,
    ChamferRequest, CircleRequest, CircularPatternRequest, ConstraintBatchRequest,
    DeleteConstraintRequest, DeleteDimensionRequest, DeleteEntitiesRequest, DeleteEntityRequest,
    DimensionRequest, EditDimensionRequest, EvalExpressionRequest, ExtendRequest, FilletRequest,
    LockedCircleRequest, LockedRectangleRequest, LockedSegmentRequest, MidpointLineRequest,
    MirrorRequest, MoveCopyRequest, MoveDimensionRequest, MovePointRequest, OffsetRequest,
    PointRequest, PolygonRequest, RectangleRequest, RectangularPatternRequest, ScaleRequest,
    SegmentRequest, SetDimensionModeRequest, SetDimensionStyleRequest, SetGridSnapRequest,
    SetGridStepRequest, SlotRequest, SplineRequest, ToggleFixBatchRequest, TrimRequest,
};
use crate::manager::SketchManager;
use crate::plane::PlaneRef;
use crate::session::SessionError;
use crate::{JointId, SetJointMotionRequestDto};
mod print_heights;
mod print_intent;
mod print_modifiers;

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum BeginSketchPayload {
    Options(BeginSketchRequest),
    Plane(PlaneRef),
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum EditSketchPayload {
    Name(String),
    Options(crate::EditSketchRequest),
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum DocumentNamePayload {
    Name(String),
    Guarded {
        name: String,
        expected_model_json: String,
    },
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SetNamedViewsPayload {
    views: Vec<crate::NamedViewConfigurationDto>,
    #[serde(default)]
    expected_model_json: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportViewSolutionPayload {
    #[serde(default)]
    name: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RecallNamedViewPayload {
    name: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RenameNamedViewPayload {
    name: String,
    new_name: String,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RenameSolidFeaturePayload {
    feature_id: limo_cad_core::FeatureId,
    name: String,
}

#[derive(serde::Deserialize)]
struct ProjectExportPayload {
    expected_model_json: String,
    save_name: Option<String>,
}

fn require_project_snapshot(
    manager: &SketchManager,
    expected: &str,
) -> Result<String, SessionError> {
    let current = manager.export_project_model()?;
    let expected: serde_json::Value = serde_json::from_str(expected)
        .map_err(|error| SessionError::Solid(format!("invalid expected project model: {error}")))?;
    let actual: serde_json::Value =
        serde_json::from_str(&current).map_err(|error| SessionError::Solid(error.to_string()))?;
    if actual != expected {
        return Err(SessionError::Solid(
            "The document changed while saving. Start Save again.".into(),
        ));
    }
    Ok(current)
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum CamPlanPayload {
    Setup(u64),
    Through {
        setup_id: u64,
        #[serde(default)]
        through_operation_id: Option<u64>,
    },
}

/// Dispatch observation through an immutable manager reference. A returned
/// response is a read even when validation rejects its payload.
pub fn handle_read_only(manager: &SketchManager, method: &str, payload: &str) -> Option<String> {
    if viewport_preview(method) && viewport_payload(payload).is_some() {
        return None;
    }
    match method {
        "print_intent_get" | "print_intent_effective" => {
            return print_intent::handle_read_only(manager, method, payload)
        }
        "print_intent_height_binding" => {
            return print_heights::handle_read_only(manager, method, payload)
        }
        "print_modifier_effective" => {
            return print_modifiers::handle_read_only(manager, method, payload)
        }
        _ => {}
    }
    Some(match method {
        "document" => ok_json(manager.document_dto()),
        "project_export_model" if payload.is_empty() || payload == "null" => {
            to_json(manager.export_project_model())
        }
        "project_export_model" => with_payload(payload, |request: ProjectExportPayload| {
            let current = require_project_snapshot(manager, &request.expected_model_json)?;
            let Some(name) = request.save_name else {
                return Ok(current);
            };
            let name = name.trim();
            if name.is_empty() {
                return Err(SessionError::Solid("document name cannot be empty".into()));
            }

            let mut saved: serde_json::Value = serde_json::from_str(&current)
                .map_err(|error| SessionError::Solid(error.to_string()))?;
            saved["document"]["name"] = name.into();
            Ok(saved.to_string())
        }),
        "finished_sketches" => ok_json(manager.finished_sketches()),
        "active_sketch" => ok_json(manager.active_snapshot()),
        "profile_catalog" => ok_json(manager.profile_catalog()),
        "solid_scene" => ok_json(manager.solid_scene_ref()),
        "body_appearances" => ok_json(manager.body_appearances()),
        "project_visibility" => ok_json(manager.project_visibility()),
        "named_views" => ok_json(manager.named_views()),
        "named_view_solution" => with_payload(payload, |request: ExportViewSolutionPayload| {
            manager.export_view_solution(request.name.as_deref())
        }),
        "drawing_document" => ok_json(manager.drawing_document_ref()),
        "assembly_document" => ok_json(manager.assembly_document_ref()),
        "assembly_solution" => ok_json(manager.assembly_solution()),
        "assembly_preview_joint" => with_payload(payload, |request| manager.preview_joint(request)),
        "assembly_preview_joint_update" => {
            with_payload(payload, |request| manager.preview_joint_update(request))
        }
        "assembly_preview_joint_motion" => {
            with_payload(payload, |request: SetJointMotionRequestDto| {
                manager.preview_joint_motion(request)
            })
        }
        "assembly_preview_joint_coordinates" => with_payload(payload, |request| {
            manager.preview_joint_coordinates(request)
        }),
        "assembly_preview_mechanism_drag" => {
            with_payload(payload, |request| manager.preview_mechanism_drag(request))
        }
        "assembly_sample_motion_study" => {
            with_payload(payload, |request| manager.sample_motion_study(request))
        }
        "assembly_export_motion_path_csv" => {
            with_payload(payload, |request| manager.export_motion_path_csv(request))
        }
        "assembly_interference_check" => with_payload(payload, |request| {
            manager.approximate_interference_check(request)
        }),
        "assembly_evaluate_motion_study" => with_payload(payload, |request| {
            manager.approximate_motion_study_evaluation(request)
        }),
        "assembly_swept_collision_check" => with_payload(payload, |request| {
            manager.approximate_swept_collision_check(request)
        }),
        "geometry_edge_chain" => {
            with_payload(payload, |request| manager.geometry_edge_chain(request))
        }
        "cam_chamfer_geometry" => {
            with_payload(payload, |request| manager.cam_chamfer_geometry(request))
        }
        "cam_document" => ok_json(manager.cam_document()),
        "cam_cutter_mesh" => {
            with_payload(payload, |geometry: limo_cad_cam::CamCutterGeometryDto| {
                limo_cad_cam::cutter_mesh(geometry).map_err(crate::SessionError::Solid)
            })
        }
        "cam_toolpath_statuses" => to_json(manager.cam_toolpath_statuses()),
        "cam_plan" => with_payload(payload, |request: CamPlanPayload| match request {
            CamPlanPayload::Setup(setup_id) => manager.cam_plan(setup_id),
            CamPlanPayload::Through {
                setup_id,
                through_operation_id,
            } => match through_operation_id {
                Some(operation_id) => manager.cam_plan_through(setup_id, operation_id),
                None => manager.cam_plan(setup_id),
            },
        }),
        "cam_post" => with_payload(payload, |request| manager.cam_post(request)),
        "cam_analyze_nbpost" => {
            with_payload(payload, |request| manager.cam_analyze_nbpost(request))
        }
        "cam_simulate" => with_payload(payload, |request| manager.cam_simulate(request)),
        "cam_simulate_gcode" => {
            with_payload(payload, |request| manager.cam_simulate_gcode(request))
        }
        "cam_post_events" => {
            with_payload(payload, |setup_id: u64| manager.cam_post_events(setup_id))
        }
        "extrude_definitions" => ok_json(manager.extrude_definitions()),
        "revolve_definitions" => ok_json(manager.revolve_definitions()),
        "sweep_definitions" => ok_json(manager.sweep_definitions()),
        "loft_definitions" => ok_json(manager.loft_definitions()),
        "rib_definitions" => ok_json(manager.rib_definitions()),
        "fillet_definitions" => ok_json(manager.fillet_definitions()),
        "chamfer_definitions" => ok_json(manager.chamfer_definitions()),
        "hole_definitions" => ok_json(manager.hole_definitions()),
        "datum_plane_definitions" => ok_json(manager.datum_plane_definitions()),
        "body_feature_definitions" => ok_json(manager.body_feature_definitions()),
        "preview_creation_point" => {
            with_payload(payload, |r: crate::dto::CreationPointPreviewRequest| {
                manager.preview_creation_point(r)
            })
        }
        "preview_segment" => with_payload(payload, |r: SegmentRequest| manager.preview_segment(r)),
        "preview_creation" => with_payload(payload, |r: crate::dto::CreationPreviewRequest| {
            manager.preview_creation(r)
        }),
        "eval_expression" => with_payload(payload, |r: EvalExpressionRequest| {
            manager.eval_expression(r)
        }),
        "preview_segment_locked" => with_payload(payload, |r: LockedSegmentRequest| {
            manager.preview_segment_locked(r)
        }),
        "preview_rectangle_locked" => with_payload(payload, |r: LockedRectangleRequest| {
            manager.preview_rectangle_locked(r)
        }),
        "preview_circle_locked" => with_payload(payload, |r: LockedCircleRequest| {
            manager.preview_circle_locked(r)
        }),
        "fillet_preview" => with_payload(payload, |r: FilletRequest| manager.fillet_preview(&r)),
        "chamfer_preview" => with_payload(payload, |r: ChamferRequest| manager.chamfer_preview(r)),
        "offset_preview" => with_payload(payload, |r: OffsetRequest| manager.offset_preview(&r)),
        "trim_preview" => with_payload(payload, |r: TrimRequest| manager.trim_preview(&r)),
        _ => return None,
    })
}

fn viewport_preview(method: &str) -> bool {
    matches!(
        method,
        "preview_creation_point"
            | "preview_segment"
            | "preview_segment_locked"
            | "preview_creation"
            | "preview_rectangle_locked"
            | "preview_circle_locked"
    )
}
fn viewport_creation(method: &str) -> bool {
    matches!(
        method,
        "add_line"
            | "add_line_locked"
            | "add_line_midpoint"
            | "add_point"
            | "add_rectangle"
            | "add_rectangle_locked"
            | "add_circle"
            | "add_circle_locked"
            | "add_arc_3pt"
            | "add_arc_center"
            | "add_slot"
            | "add_spline"
            | "polygon_create"
    )
}
fn viewport_payload(payload: &str) -> Option<serde_json::Value> {
    let value: serde_json::Value = serde_json::from_str(payload).ok()?;
    value.get("viewport_snap").map(|_| value.clone())
}
fn dispatch_viewport(
    manager: &mut SketchManager,
    method: &str,
    mut value: serde_json::Value,
) -> String {
    let context = value
        .as_object_mut()
        .unwrap()
        .remove("viewport_snap")
        .unwrap();
    let context = match serde_json::from_value(context) {
        Ok(context) => context,
        Err(error) => return err_json(format!("bad viewport snap context: {error}")),
    };
    match manager.with_viewport_snap(context, |manager| {
        handle(manager, method, &value.to_string())
    }) {
        Ok(response) => response,
        Err(error) => err_json(error.to_string()),
    }
}

/// Read-only previews may temporarily scope runtime snap distances under the
/// engine lock. They must bypass geometry-cache invalidation and model fences.
pub fn handle_viewport_preview(
    manager: &mut SketchManager,
    method: &str,
    payload: &str,
) -> Option<String> {
    if !viewport_preview(method) {
        return None;
    }
    viewport_payload(payload).map(|value| dispatch_viewport(manager, method, value))
}

/// Dispatch one engine call. Unknown methods and malformed payloads yield
/// an error envelope, never a panic.
pub fn handle(manager: &mut SketchManager, method: &str, payload: &str) -> String {
    if viewport_creation(method) || viewport_preview(method) {
        if let Some(value) = viewport_payload(payload) {
            return dispatch_viewport(manager, method, value);
        }
    }
    if let Some(response) = handle_read_only(manager, method, payload) {
        return response;
    }
    if matches!(
        method,
        "print_intent_height_binding"
            | "print_intent_upsert_height_range"
            | "print_intent_upsert_layer_profile"
            | "print_intent_remove_height"
            | "print_intent_rebind_height"
    ) {
        return print_heights::handle(manager, method, payload);
    }
    if method.starts_with("print_intent_") {
        return print_intent::handle(manager, method, payload);
    }
    if method.starts_with("print_modifier_") {
        return print_modifiers::handle(manager, method, payload);
    }
    match method {
        "solid_rename_feature" => with_payload(payload, |request: RenameSolidFeaturePayload| {
            manager.rename_solid_feature(request.feature_id, request.name)
        }),
        "document_set_name" => with_payload(payload, |request: DocumentNamePayload| {
            let name = match request {
                DocumentNamePayload::Name(name) => name,
                DocumentNamePayload::Guarded {
                    name,
                    expected_model_json,
                } => {
                    require_project_snapshot(manager, &expected_model_json)?;
                    name
                }
            };
            manager.set_document_name(name)
        }),
        "project_prepare_new" => to_json(manager.prepare_new_project()),
        "project_prepare_load" => {
            with_payload(payload, |model: String| manager.prepare_load_project(model))
        }
        "begin_sketch" => with_payload(payload, |request: BeginSketchPayload| match request {
            BeginSketchPayload::Options(options) => manager.begin_sketch_with_options(options),
            BeginSketchPayload::Plane(plane) => manager.begin_sketch(plane),
        }),
        "end_sketch" => to_json(manager.end_sketch()),
        "edit_sketch" => with_payload(payload, |request: EditSketchPayload| match request {
            EditSketchPayload::Name(name) => manager.edit_sketch(&name),
            EditSketchPayload::Options(request) => match request.occurrence_id {
                Some(occurrence) => manager.edit_sketch_in_occurrence(&request.name, occurrence),
                None => manager.edit_sketch(&request.name),
            },
        }),
        "project_set_visibility" => with_payload(payload, |visibility| {
            manager.set_project_visibility(visibility)
        }),
        "clear_named_view" => ok_json(manager.clear_named_view()),
        "upsert_named_view" => with_payload(payload, |view| manager.upsert_named_view(view)),
        "rename_named_view" => with_payload(payload, |request: RenameNamedViewPayload| {
            manager.rename_named_view(request.name, request.new_name)
        }),
        "delete_named_view" => with_payload(payload, |request: RecallNamedViewPayload| {
            manager.delete_named_view(request.name)
        }),
        "set_named_views" => with_payload(payload, |request: SetNamedViewsPayload| {
            limo_cad_solid::check_export_model_snapshot(
                request.expected_model_json.as_deref(),
                &manager.export_project_model()?,
            )
            .map_err(|e| crate::SessionError::Solid(e.into()))?;
            manager.set_named_views(request.views)
        }),
        "recall_named_view" => with_payload(payload, |request: RecallNamedViewPayload| {
            manager.recall_named_view(request.name)
        }),
        "construction_set_visibility" => with_payload(payload, |request| {
            manager.set_construction_visibility(request)
        }),
        "drawing_apply" => with_payload(payload, |command| manager.drawing_command(command)),
        "drawing_create_sheet" => with_payload(payload, |r| {
            manager.drawing_command(crate::drawing_commands::DrawingCommand::CreateSheet(r))
        }),
        "drawing_select_sheet" => with_payload(payload, |r| {
            manager.drawing_command(crate::drawing_commands::DrawingCommand::SelectSheet(r))
        }),
        "drawing_delete_sheet" => with_payload(payload, |r| {
            manager.drawing_command(crate::drawing_commands::DrawingCommand::DeleteSheet(r))
        }),
        "drawing_update_view" => with_payload(payload, |r| {
            manager.drawing_command(crate::drawing_commands::DrawingCommand::UpdateView(r))
        }),
        "drawing_delete_view" => with_payload(payload, |r| {
            manager.drawing_command(crate::drawing_commands::DrawingCommand::DeleteView(r))
        }),
        "drawing_add_annotation" => with_payload(payload, |r| {
            manager.drawing_command(crate::drawing_commands::DrawingCommand::AddAnnotation(r))
        }),
        "drawing_update_annotation" => with_payload(payload, |r| {
            manager.drawing_command(crate::drawing_commands::DrawingCommand::UpdateAnnotation(r))
        }),
        "drawing_delete_annotation" => with_payload(payload, |r| {
            manager.drawing_command(crate::drawing_commands::DrawingCommand::DeleteAnnotation(r))
        }),
        "drawing_create_template" => with_payload(payload, |r| {
            manager.drawing_command(crate::drawing_commands::DrawingCommand::CreateTemplate(r))
        }),
        "drawing_apply_template" => with_payload(payload, |r| {
            manager.drawing_command(crate::drawing_commands::DrawingCommand::ApplyTemplate(r))
        }),
        "drawing_delete_template" => with_payload(payload, |r| {
            manager.drawing_command(crate::drawing_commands::DrawingCommand::DeleteTemplate(r))
        }),
        "drawing_add_revision" => with_payload(payload, |r| {
            manager.drawing_command(crate::drawing_commands::DrawingCommand::AddRevision(r))
        }),
        "drawing_set_release" => with_payload(payload, |r| {
            manager.drawing_command(crate::drawing_commands::DrawingCommand::SetRelease(r))
        }),
        "drawing_add_view" => with_payload(payload, |r| {
            manager.drawing_command(crate::drawing_commands::DrawingCommand::AddView(r))
        }),
        "drawing_add_linear_dimension" => with_payload(payload, |r| {
            manager.drawing_command(crate::drawing_commands::DrawingCommand::AddLinearDimension(
                r,
            ))
        }),
        "drawing_add_radial_dimension" => with_payload(payload, |r| {
            manager.drawing_command(crate::drawing_commands::DrawingCommand::AddRadialDimension(
                r,
            ))
        }),
        "drawing_add_angular_dimension" => with_payload(payload, |r| {
            manager.drawing_command(crate::drawing_commands::DrawingCommand::AddAngularDimension(r))
        }),
        "drawing_set_bom" => with_payload(payload, |r| {
            manager.drawing_command(crate::drawing_commands::DrawingCommand::SetBom(r))
        }),
        "drawing_add_note" => with_payload(payload, |r| {
            manager.drawing_command(crate::drawing_commands::DrawingCommand::AddNote(r))
        }),
        "drawing_set_document" => {
            with_payload(payload, |drawing| manager.set_drawing_document(drawing))
        }
        "assembly_set_document" => {
            with_payload(payload, |document| manager.set_assembly_document(document))
        }
        "assembly_create_component" => {
            with_payload(payload, |request| manager.create_component(request))
        }
        "assembly_update_component" => {
            with_payload(payload, |request| manager.update_component(request))
        }
        "assembly_create_occurrence" => {
            with_payload(payload, |request| manager.create_occurrence(request))
        }
        "assembly_update_occurrence" => {
            with_payload(payload, |request| manager.update_occurrence(request))
        }
        "assembly_duplicate_occurrence" => {
            with_payload(payload, |request| manager.duplicate_occurrence(request))
        }
        "assembly_remove_occurrence" => {
            with_payload(payload, |request| manager.remove_occurrence(request))
        }
        "assembly_set_occurrence_grounded" => {
            with_payload(payload, |request| manager.set_occurrence_grounded(request))
        }
        "assembly_set_occurrence_pose" => {
            with_payload(payload, |request| manager.set_occurrence_pose(request))
        }
        "assembly_create_joint" => with_payload(payload, |request| manager.create_joint(request)),
        "assembly_update_joint" => with_payload(payload, |request| manager.update_joint(request)),
        "assembly_delete_joint" => with_payload(payload, |id: JointId| manager.delete_joint(id)),
        "assembly_set_joint_enabled" => {
            with_payload(payload, |request| manager.set_joint_enabled(request))
        }
        "assembly_set_joint_motion" => {
            with_payload(payload, |request: SetJointMotionRequestDto| {
                manager.set_joint_motion(request)
            })
        }
        "assembly_create_gear_relation" => {
            with_payload(payload, |request| manager.create_gear_relation(request))
        }
        "assembly_update_gear_relation" => {
            with_payload(payload, |request| manager.update_gear_relation(request))
        }
        "assembly_delete_gear_relation" => {
            with_payload(payload, |id| manager.delete_gear_relation(id))
        }
        "assembly_set_joint_coordinates" => {
            with_payload(payload, |request| manager.set_joint_coordinates(request))
        }
        "assembly_apply_joint_motions" => {
            with_payload(payload, |request| manager.apply_joint_motions(request))
        }
        "assembly_create_position" => {
            with_payload(payload, |request| manager.create_assembly_position(request))
        }
        "assembly_update_position" => with_payload(payload, |position| {
            manager.update_assembly_position(position)
        }),
        "assembly_delete_position" => {
            with_payload(payload, |id| manager.delete_assembly_position(id))
        }
        "assembly_apply_position" => {
            with_payload(payload, |id| manager.apply_assembly_position(id))
        }
        "assembly_create_motion_study" => {
            with_payload(payload, |request| manager.create_motion_study(request))
        }
        "assembly_update_motion_study" => {
            with_payload(payload, |study| manager.update_motion_study(study))
        }
        "assembly_delete_motion_study" => {
            with_payload(payload, |id| manager.delete_motion_study(id))
        }
        "assembly_create_contact_set" => {
            with_payload(payload, |request| manager.create_contact_set(request))
        }
        "assembly_update_contact_set" => {
            with_payload(payload, |contact| manager.update_contact_set(contact))
        }
        "assembly_delete_contact_set" => with_payload(payload, |id| manager.delete_contact_set(id)),
        "assembly_set_grounded_body" => {
            with_payload(payload, |body_id| manager.set_grounded_body(body_id))
        }
        "cam_set_document" => with_payload(payload, |cam| manager.set_cam_document(cam)),
        "cam_regenerate_operation" => with_payload(payload, |operation_id: u64| {
            manager.cam_regenerate_operation(operation_id)
        }),
        "cam_regenerate_setup" => with_payload(payload, |setup_id: u64| {
            manager.cam_regenerate_setup(setup_id)
        }),
        "set_body_appearance" => {
            with_payload(payload, |appearance: limo_cad_core::BodyAppearance| {
                manager.set_body_appearance(appearance)
            })
        }
        "datum_plane_create" => with_payload(payload, |r: DatumPlaneRequest| {
            manager.create_datum_plane(r)
        }),
        "datum_plane_edit" => with_payload(payload, |r: EditDatumPlaneRequest| {
            manager.edit_datum_plane(r)
        }),
        "solid_prepare_body_feature" => with_payload(payload, |r: BodyFeatureRequestDto| {
            manager.prepare_body_feature(r)
        }),
        "solid_prepare_edit_body_feature" => with_payload(payload, |r: EditBodyFeatureRequest| {
            manager.prepare_edit_body_feature(r)
        }),
        "solid_prepare_extrude" => {
            with_payload(payload, |r: ExtrudeRequest| manager.prepare_extrude(r))
        }
        "solid_prepare_edit_extrude" => with_payload(payload, |r: EditExtrudeRequest| {
            manager.prepare_edit_extrude(r)
        }),
        "solid_prepare_revolve" => {
            with_payload(payload, |r: RevolveRequest| manager.prepare_revolve(r))
        }
        "solid_prepare_edit_revolve" => with_payload(payload, |r: EditRevolveRequest| {
            manager.prepare_edit_revolve(r)
        }),
        "solid_prepare_sweep" => with_payload(payload, |r: SweepRequest| manager.prepare_sweep(r)),
        "solid_prepare_edit_sweep" => {
            with_payload(payload, |r: EditSweepRequest| manager.prepare_edit_sweep(r))
        }
        "solid_prepare_loft" => with_payload(payload, |r: LoftRequest| manager.prepare_loft(r)),
        "solid_prepare_edit_loft" => {
            with_payload(payload, |r: EditLoftRequest| manager.prepare_edit_loft(r))
        }
        "solid_prepare_rib" => with_payload(payload, |r: RibRequest| manager.prepare_rib(r)),
        "solid_prepare_edit_rib" => {
            with_payload(payload, |r: EditRibRequest| manager.prepare_edit_rib(r))
        }
        "solid_prepare_fillet" => with_payload(payload, |r: SolidFilletRequest| {
            manager.prepare_solid_fillet(r)
        }),
        "solid_prepare_edit_fillet" => with_payload(payload, |r: EditSolidFilletRequest| {
            manager.prepare_edit_solid_fillet(r)
        }),
        "solid_prepare_chamfer" => with_payload(payload, |r: SolidChamferRequest| {
            manager.prepare_solid_chamfer(r)
        }),
        "solid_prepare_edit_chamfer" => with_payload(payload, |r: EditSolidChamferRequest| {
            manager.prepare_edit_solid_chamfer(r)
        }),
        "solid_prepare_hole" => with_payload(payload, |r: HoleRequest| manager.prepare_hole(r)),
        "solid_prepare_edit_hole" => {
            with_payload(payload, |r: EditHoleRequest| manager.prepare_edit_hole(r))
        }
        "solid_prepare_recompute" => to_json(manager.prepare_recompute()),
        "solid_prepare_set_rollback" => with_payload(payload, |r: SetRollbackRequest| {
            manager.prepare_set_rollback(r)
        }),
        "solid_prepare_delete_feature" => with_payload(payload, |r: DeleteFeatureRequest| {
            manager.prepare_delete_feature(r)
        }),
        "solid_prepare_reorder_feature" => with_payload(payload, |r: ReorderFeatureRequest| {
            manager.prepare_reorder_feature(r)
        }),
        "solid_commit" => with_payload(payload, |r: CommitKernelRequest| manager.commit_solid(r)),
        "add_line" => with_payload(payload, |r: SegmentRequest| manager.add_line(r)),
        "add_line_locked" => with_payload(payload, |r: LockedSegmentRequest| {
            manager.add_line_locked(r)
        }),
        "add_point" => with_payload(payload, |r: PointRequest| manager.add_point(r)),
        "add_line_midpoint" => with_payload(payload, |r: MidpointLineRequest| {
            manager.add_line_midpoint(r)
        }),
        "add_rectangle" => with_payload(payload, |r: RectangleRequest| manager.add_rectangle(r)),
        "add_rectangle_locked" => with_payload(payload, |r: LockedRectangleRequest| {
            manager.add_rectangle_locked(r)
        }),
        "add_circle" => with_payload(payload, |r: CircleRequest| manager.add_circle(r)),
        "add_circle_locked" => with_payload(payload, |r: LockedCircleRequest| {
            manager.add_circle_locked(r)
        }),
        "add_slot" => with_payload(payload, |r: SlotRequest| manager.add_slot(r)),
        "add_spline" => with_payload(payload, |r: SplineRequest| manager.add_spline(r)),
        "add_arc_3pt" => with_payload(payload, |r: Arc3PointRequest| manager.add_arc_3pt(r)),
        "add_arc_center" => with_payload(payload, |r: ArcCenterRequest| manager.add_arc_center(r)),
        "add_constraint" => with_payload(payload, |c: Constraint| manager.add_constraint(c)),
        "add_constraints" => with_payload(payload, |r: ConstraintBatchRequest| {
            manager.add_constraints(r)
        }),
        "add_dimension" => with_payload(payload, |r: DimensionRequest| manager.add_dimension(r)),
        "edit_dimension" => {
            with_payload(payload, |r: EditDimensionRequest| manager.edit_dimension(r))
        }
        "set_dimension_mode" => with_payload(payload, |r: SetDimensionModeRequest| {
            manager.set_dimension_mode(r)
        }),
        "move_dimension" => {
            with_payload(payload, |r: MoveDimensionRequest| manager.move_dimension(r))
        }
        "delete_dimension" => with_payload(payload, |r: DeleteDimensionRequest| {
            manager.delete_dimension(r.constraint_id)
        }),
        "delete_constraint" => with_payload(payload, |r: DeleteConstraintRequest| {
            manager.delete_constraint(r.constraint_id)
        }),
        "set_dimension_style" => with_payload(payload, |r: SetDimensionStyleRequest| {
            manager.set_dimension_style(r)
        }),
        "fillet_lines" => with_payload(payload, |r: FilletRequest| manager.fillet_lines(r)),
        "chamfer_lines" => with_payload(payload, |r: ChamferRequest| manager.chamfer_lines(r)),
        "offset_curve" => with_payload(payload, |r: OffsetRequest| manager.offset_curve(r)),
        "trim_entity" => with_payload(payload, |r: TrimRequest| manager.trim_entity(r)),
        "extend_entity" => with_payload(payload, |r: ExtendRequest| manager.extend_entity(r)),
        "break_curve" => with_payload(payload, |r: BreakRequest| manager.break_curve(r)),
        "mirror_entities" => with_payload(payload, |r: MirrorRequest| manager.mirror_entities(r)),
        "rectangular_pattern" => with_payload(payload, |r: RectangularPatternRequest| {
            manager.rectangular_pattern(r)
        }),
        "circular_pattern" => with_payload(payload, |r: CircularPatternRequest| {
            manager.circular_pattern(r)
        }),
        "move_copy_entities" => {
            with_payload(payload, |r: MoveCopyRequest| manager.move_copy_entities(r))
        }
        "scale_entities" => with_payload(payload, |r: ScaleRequest| manager.scale_entities(r)),
        "polygon_create" => with_payload(payload, |r: PolygonRequest| manager.polygon_create(r)),
        "toggle_fix" => with_payload(payload, |r: DeleteEntityRequest| {
            manager.toggle_fix(r.entity_id)
        }),
        "toggle_fix_entities" => with_payload(payload, |r: ToggleFixBatchRequest| {
            manager.toggle_fix_entities(r)
        }),
        "move_point" => with_payload(payload, |r: MovePointRequest| manager.move_point(r)),
        "delete_entity" => with_payload(payload, |r: DeleteEntityRequest| {
            manager.delete_entity(r.entity_id)
        }),
        "delete_entities" => with_payload(payload, |r: DeleteEntitiesRequest| {
            manager.delete_entities(&r.entity_ids)
        }),
        "undo" => to_json(manager.undo()),
        "redo" => to_json(manager.redo()),
        "set_grid_snap" => with_payload(payload, |r: SetGridSnapRequest| manager.set_grid_snap(r)),
        "set_grid_step" => with_payload(payload, |r: SetGridStepRequest| manager.set_grid_step(r)),
        other => err_json(format!("unknown engine method: {other}")),
    }
}

fn with_payload<T, R, F>(payload: &str, f: F) -> String
where
    T: DeserializeOwned,
    R: Serialize,
    F: FnOnce(T) -> Result<R, SessionError>,
{
    match serde_json::from_str::<T>(payload) {
        Ok(request) => to_json(f(request)),
        Err(e) => err_json(format!("bad request payload: {e}")),
    }
}

fn to_json<R: Serialize>(result: Result<R, SessionError>) -> String {
    match result {
        Ok(value) => ok_json(value),
        Err(e) => match &e {
            SessionError::OverConstrained {
                rejected,
                conflicts_with,
            } => serde_json::json!({
                "ok": false,
                "error": e.to_string(),
                "data": {
                    "reason": "conflict",
                    "rejected": rejected,
                    "conflicts_with": conflicts_with,
                },
            })
            .to_string(),
            SessionError::RedundantConstraint {
                rejected,
                implied_by,
            } => serde_json::json!({
                "ok": false,
                "error": e.to_string(),


                "data": {
                    "reason": "redundant",
                    "rejected": rejected,
                    "conflicts_with": implied_by,
                },
            })
            .to_string(),
            _ => err_json(e.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn value(response: String) -> Value {
        let envelope: Value = serde_json::from_str(&response).unwrap();
        assert_eq!(envelope["ok"], true, "{envelope}");
        envelope["value"].clone()
    }

    #[test]
    fn guarded_save_copy_and_name_adoption_preserve_the_captured_model() {
        let mut manager = SketchManager::new();
        value(handle(&mut manager, "document_set_name", r#""Original""#));
        let original = value(handle(&mut manager, "project_export_model", ""));
        let model: Value = serde_json::from_str(original.as_str().unwrap()).unwrap();
        let request = json!({
            "expected_model_json": serde_json::to_string_pretty(&model).unwrap(),
            "save_name": "  Saved copy  "
        });
        let saved = value(handle(
            &mut manager,
            "project_export_model",
            &request.to_string(),
        ));
        let mut expected = model.clone();
        expected["document"]["name"] = json!("Saved copy");
        assert_eq!(
            serde_json::from_str::<Value>(saved.as_str().unwrap()).unwrap(),
            expected
        );
        assert_eq!(
            value(handle(&mut manager, "project_export_model", "")),
            original,
            "serializing Save As must not rename or rewrite live document history"
        );
        value(handle(
            &mut manager,
            "document_set_name",
            &json!({
                "name": "Saved copy", "expected_model_json": original
            })
            .to_string(),
        ));
        assert_eq!(manager.document_dto().name, "Saved copy");
        for (method, payload) in [
            ("project_export_model", request),
            (
                "document_set_name",
                json!({"name":"Stale save", "expected_model_json":original}),
            ),
        ] {
            let response: Value =
                serde_json::from_str(&handle(&mut manager, method, &payload.to_string())).unwrap();
            assert_eq!(response["ok"], false);
            assert!(response["error"]
                .as_str()
                .unwrap()
                .contains("document changed"));
        }
        assert_eq!(manager.document_dto().name, "Saved copy");
        let current = value(handle(&mut manager, "project_export_model", ""));
        let invalid: Value = serde_json::from_str(&handle(
            &mut manager,
            "project_export_model",
            &json!({"expected_model_json": current, "save_name":"   "}).to_string(),
        ))
        .unwrap();
        assert_eq!(invalid["ok"], false);
        assert_eq!(
            value(handle(&mut manager, "project_export_model", "")),
            current
        );
    }
}
