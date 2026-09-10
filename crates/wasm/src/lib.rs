//! nbcad-wasm — WASM host of the noBS CAD engine.
//!
//! Thin wasm-bindgen facade over [`SketchManager`]: one exported function
//! per engine API method, JSON-string in / JSON-string out, dispatching
//! through `nbcad_sketch::host::handle` — the exact same code path the
//! Tauri commands use, so browser and native behavior are identical by
//! construction. All payloads are the shared envelope
//! (`{"ok":true,"value":...}` / `{"ok":false,"error":"..."}`).

use wasm_bindgen::prelude::*;

use nbcad_sketch::host;
use nbcad_sketch::SketchManager;

/// Engine instance held by the frontend `WasmEngine` adapter.
#[wasm_bindgen]
pub struct WasmEngine {
    manager: SketchManager,
}

#[wasm_bindgen]
impl WasmEngine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            manager: SketchManager::new(),
        }
    }

    /// Document snapshot (name, settings, browser tree incl. sketches).
    pub fn document(&mut self) -> String {
        host::handle(&mut self.manager, "document", "")
    }

    pub fn document_set_name(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "document_set_name", payload)
    }

    pub fn project_export_model(&mut self, payload: Option<String>) -> String {
        host::handle(
            &mut self.manager,
            "project_export_model",
            payload.as_deref().unwrap_or(""),
        )
    }

    pub fn project_prepare_new(&mut self) -> String {
        host::handle(&mut self.manager, "project_prepare_new", "")
    }

    pub fn project_prepare_load(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "project_prepare_load", payload)
    }

    /// `payload`: serialized `PlaneRef`.
    pub fn begin_sketch(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "begin_sketch", payload)
    }

    pub fn end_sketch(&mut self) -> String {
        host::handle(&mut self.manager, "end_sketch", "")
    }

    /// Finished-sketch snapshots (M1d): muted 3D rendering + re-edit list.
    pub fn finished_sketches(&mut self) -> String {
        host::handle(&mut self.manager, "finished_sketches", "")
    }

    /// `payload`: sketch name (JSON string) to re-enter for editing (M1d).
    pub fn edit_sketch(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "edit_sketch", payload)
    }

    pub fn active_sketch(&mut self) -> String {
        host::handle(&mut self.manager, "active_sketch", "")
    }

    pub fn profile_catalog(&mut self) -> String {
        host::handle(&mut self.manager, "profile_catalog", "")
    }

    pub fn solid_scene(&mut self) -> String {
        host::handle(&mut self.manager, "solid_scene", "")
    }

    pub fn body_appearances(&mut self) -> String {
        host::handle(&mut self.manager, "body_appearances", "")
    }

    pub fn project_visibility(&mut self) -> String {
        host::handle(&mut self.manager, "project_visibility", "")
    }

    pub fn project_set_visibility(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "project_set_visibility", payload)
    }

    pub fn construction_set_visibility(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "construction_set_visibility", payload)
    }

    pub fn drawing_document(&mut self) -> String {
        host::handle(&mut self.manager, "drawing_document", "")
    }

    pub fn drawing_apply(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "drawing_apply", payload)
    }

    pub fn drawing_set_document(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "drawing_set_document", payload)
    }

    pub fn assembly_document(&mut self) -> String {
        host::handle(&mut self.manager, "assembly_document", "")
    }

    pub fn assembly_set_document(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_set_document", payload)
    }

    pub fn assembly_solution(&mut self) -> String {
        host::handle(&mut self.manager, "assembly_solution", "")
    }

    pub fn assembly_create_component(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_create_component", payload)
    }

    pub fn assembly_update_component(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_update_component", payload)
    }

    pub fn assembly_create_occurrence(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_create_occurrence", payload)
    }

    pub fn assembly_update_occurrence(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_update_occurrence", payload)
    }

    pub fn assembly_duplicate_occurrence(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_duplicate_occurrence", payload)
    }

    pub fn assembly_set_occurrence_grounded(&mut self, payload: &str) -> String {
        host::handle(
            &mut self.manager,
            "assembly_set_occurrence_grounded",
            payload,
        )
    }

    pub fn assembly_set_occurrence_pose(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_set_occurrence_pose", payload)
    }

    pub fn assembly_preview_joint(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_preview_joint", payload)
    }

    pub fn assembly_create_joint(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_create_joint", payload)
    }

    pub fn assembly_update_joint(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_update_joint", payload)
    }

    pub fn assembly_preview_joint_update(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_preview_joint_update", payload)
    }

    pub fn assembly_delete_joint(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_delete_joint", payload)
    }

    pub fn assembly_set_joint_enabled(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_set_joint_enabled", payload)
    }

    pub fn assembly_set_joint_motion(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_set_joint_motion", payload)
    }

    pub fn assembly_preview_joint_motion(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_preview_joint_motion", payload)
    }

    pub fn assembly_set_joint_coordinates(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_set_joint_coordinates", payload)
    }

    pub fn assembly_preview_joint_coordinates(&mut self, payload: &str) -> String {
        host::handle(
            &mut self.manager,
            "assembly_preview_joint_coordinates",
            payload,
        )
    }

    pub fn assembly_preview_mechanism_drag(&mut self, payload: &str) -> String {
        host::handle(
            &mut self.manager,
            "assembly_preview_mechanism_drag",
            payload,
        )
    }

    pub fn assembly_apply_joint_motions(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_apply_joint_motions", payload)
    }

    pub fn assembly_create_position(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_create_position", payload)
    }

    pub fn assembly_update_position(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_update_position", payload)
    }

    pub fn assembly_delete_position(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_delete_position", payload)
    }

    pub fn assembly_apply_position(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_apply_position", payload)
    }

    pub fn assembly_create_motion_study(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_create_motion_study", payload)
    }

    pub fn assembly_update_motion_study(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_update_motion_study", payload)
    }

    pub fn assembly_delete_motion_study(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_delete_motion_study", payload)
    }

    pub fn assembly_sample_motion_study(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_sample_motion_study", payload)
    }

    pub fn assembly_export_motion_path_csv(&mut self, payload: &str) -> String {
        host::handle(
            &mut self.manager,
            "assembly_export_motion_path_csv",
            payload,
        )
    }

    pub fn assembly_create_contact_set(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_create_contact_set", payload)
    }

    pub fn assembly_update_contact_set(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_update_contact_set", payload)
    }

    pub fn assembly_delete_contact_set(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_delete_contact_set", payload)
    }

    pub fn assembly_interference_check(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_interference_check", payload)
    }

    pub fn assembly_evaluate_motion_study(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_evaluate_motion_study", payload)
    }

    pub fn assembly_swept_collision_check(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_swept_collision_check", payload)
    }

    pub fn assembly_set_grounded_body(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "assembly_set_grounded_body", payload)
    }

    pub fn set_body_appearance(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "set_body_appearance", payload)
    }

    pub fn extrude_definitions(&mut self) -> String {
        host::handle(&mut self.manager, "extrude_definitions", "")
    }

    pub fn revolve_definitions(&mut self) -> String {
        host::handle(&mut self.manager, "revolve_definitions", "")
    }

    pub fn sweep_definitions(&mut self) -> String {
        host::handle(&mut self.manager, "sweep_definitions", "")
    }

    pub fn loft_definitions(&mut self) -> String {
        host::handle(&mut self.manager, "loft_definitions", "")
    }

    pub fn rib_definitions(&mut self) -> String {
        host::handle(&mut self.manager, "rib_definitions", "")
    }

    pub fn fillet_definitions(&mut self) -> String {
        host::handle(&mut self.manager, "fillet_definitions", "")
    }

    pub fn chamfer_definitions(&mut self) -> String {
        host::handle(&mut self.manager, "chamfer_definitions", "")
    }

    pub fn hole_definitions(&mut self) -> String {
        host::handle(&mut self.manager, "hole_definitions", "")
    }

    pub fn datum_plane_definitions(&mut self) -> String {
        host::handle(&mut self.manager, "datum_plane_definitions", "")
    }

    pub fn body_feature_definitions(&mut self) -> String {
        host::handle(&mut self.manager, "body_feature_definitions", "")
    }

    pub fn datum_plane_create(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "datum_plane_create", payload)
    }

    pub fn datum_plane_edit(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "datum_plane_edit", payload)
    }

    pub fn solid_prepare_body_feature(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "solid_prepare_body_feature", payload)
    }

    pub fn solid_prepare_edit_body_feature(&mut self, payload: &str) -> String {
        host::handle(
            &mut self.manager,
            "solid_prepare_edit_body_feature",
            payload,
        )
    }

    pub fn solid_prepare_extrude(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "solid_prepare_extrude", payload)
    }

    pub fn solid_prepare_edit_extrude(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "solid_prepare_edit_extrude", payload)
    }

    pub fn solid_prepare_revolve(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "solid_prepare_revolve", payload)
    }

    pub fn solid_prepare_edit_revolve(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "solid_prepare_edit_revolve", payload)
    }

    pub fn solid_prepare_sweep(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "solid_prepare_sweep", payload)
    }

    pub fn solid_prepare_edit_sweep(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "solid_prepare_edit_sweep", payload)
    }

    pub fn solid_prepare_loft(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "solid_prepare_loft", payload)
    }

    pub fn solid_prepare_edit_loft(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "solid_prepare_edit_loft", payload)
    }

    pub fn solid_prepare_rib(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "solid_prepare_rib", payload)
    }

    pub fn solid_prepare_edit_rib(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "solid_prepare_edit_rib", payload)
    }

    pub fn solid_prepare_fillet(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "solid_prepare_fillet", payload)
    }

    pub fn solid_prepare_edit_fillet(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "solid_prepare_edit_fillet", payload)
    }

    pub fn solid_prepare_chamfer(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "solid_prepare_chamfer", payload)
    }

    pub fn solid_prepare_edit_chamfer(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "solid_prepare_edit_chamfer", payload)
    }

    pub fn solid_prepare_hole(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "solid_prepare_hole", payload)
    }

    pub fn solid_prepare_edit_hole(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "solid_prepare_edit_hole", payload)
    }

    pub fn solid_prepare_recompute(&mut self) -> String {
        host::handle(&mut self.manager, "solid_prepare_recompute", "")
    }

    pub fn solid_prepare_set_rollback(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "solid_prepare_set_rollback", payload)
    }

    pub fn solid_prepare_delete_feature(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "solid_prepare_delete_feature", payload)
    }

    pub fn solid_prepare_reorder_feature(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "solid_prepare_reorder_feature", payload)
    }

    pub fn solid_commit(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "solid_commit", payload)
    }

    /// `payload`: serialized `SegmentRequest`.
    pub fn preview_segment(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "preview_segment", payload)
    }

    /// `payload`: serialized `SegmentRequest`.
    pub fn add_line(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "add_line", payload)
    }

    /// `payload`: serialized `LockedSegmentRequest`.
    pub fn preview_segment_locked(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "preview_segment_locked", payload)
    }

    /// `payload`: serialized `LockedSegmentRequest`.
    pub fn add_line_locked(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "add_line_locked", payload)
    }

    /// `payload`: serialized `PointRequest`.
    pub fn add_point(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "add_point", payload)
    }

    /// `payload`: serialized `MidpointLineRequest`.
    pub fn add_line_midpoint(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "add_line_midpoint", payload)
    }

    /// `payload`: serialized `RectangleRequest`.
    pub fn add_rectangle(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "add_rectangle", payload)
    }

    /// `payload`: serialized `LockedRectangleRequest`.
    pub fn add_rectangle_locked(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "add_rectangle_locked", payload)
    }

    /// `payload`: serialized `SlotRequest`.
    pub fn add_slot(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "add_slot", payload)
    }

    /// `payload`: serialized `SplineRequest`.
    pub fn add_spline(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "add_spline", payload)
    }

    /// `payload`: serialized `CircleRequest`.
    pub fn add_circle(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "add_circle", payload)
    }

    /// `payload`: serialized `LockedCircleRequest`.
    pub fn add_circle_locked(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "add_circle_locked", payload)
    }

    /// `payload`: serialized `Arc3PointRequest`.
    pub fn add_arc_3pt(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "add_arc_3pt", payload)
    }

    /// `payload`: serialized `ArcCenterRequest`.
    pub fn add_arc_center(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "add_arc_center", payload)
    }

    /// `payload`: serialized `Constraint`.
    pub fn add_constraint(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "add_constraint", payload)
    }

    pub fn add_constraints(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "add_constraints", payload)
    }

    /// `payload`: serialized `EvalExpressionRequest`.
    pub fn eval_expression(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "eval_expression", payload)
    }

    /// `payload`: serialized `DimensionRequest`.
    pub fn add_dimension(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "add_dimension", payload)
    }

    /// `payload`: serialized `EditDimensionRequest`.
    pub fn edit_dimension(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "edit_dimension", payload)
    }

    /// `payload`: serialized `SetDimensionModeRequest`.
    pub fn set_dimension_mode(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "set_dimension_mode", payload)
    }

    /// `payload`: serialized `MoveDimensionRequest`.
    pub fn move_dimension(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "move_dimension", payload)
    }

    /// `payload`: serialized `DeleteDimensionRequest`.
    pub fn delete_dimension(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "delete_dimension", payload)
    }

    /// `payload`: serialized `DeleteConstraintRequest`.
    pub fn delete_constraint(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "delete_constraint", payload)
    }

    /// `payload`: serialized `SetDimensionStyleRequest`.
    pub fn set_dimension_style(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "set_dimension_style", payload)
    }

    /// `payload`: serialized `FilletRequest`.
    pub fn fillet_preview(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "fillet_preview", payload)
    }

    /// `payload`: serialized `FilletRequest`.
    pub fn fillet_lines(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "fillet_lines", payload)
    }

    /// `payload`: serialized `ChamferRequest`.
    pub fn chamfer_lines(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "chamfer_lines", payload)
    }

    /// `payload`: serialized `OffsetRequest`.
    pub fn offset_preview(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "offset_preview", payload)
    }

    /// `payload`: serialized `OffsetRequest`.
    pub fn offset_curve(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "offset_curve", payload)
    }

    /// `payload`: serialized `TrimRequest`.
    pub fn trim_preview(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "trim_preview", payload)
    }

    /// `payload`: serialized `TrimRequest`.
    pub fn trim_entity(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "trim_entity", payload)
    }

    /// `payload`: serialized `ExtendRequest`.
    pub fn extend_entity(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "extend_entity", payload)
    }

    /// `payload`: serialized `BreakRequest`.
    pub fn break_curve(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "break_curve", payload)
    }

    /// `payload`: serialized `MirrorRequest`.
    pub fn mirror_entities(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "mirror_entities", payload)
    }

    /// `payload`: serialized `RectangularPatternRequest`.
    pub fn rectangular_pattern(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "rectangular_pattern", payload)
    }

    /// `payload`: serialized `CircularPatternRequest`.
    pub fn circular_pattern(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "circular_pattern", payload)
    }

    /// `payload`: serialized `MoveCopyRequest`.
    pub fn move_copy_entities(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "move_copy_entities", payload)
    }

    /// `payload`: serialized `ScaleRequest`.
    pub fn scale_entities(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "scale_entities", payload)
    }

    /// `payload`: serialized `PolygonRequest`.
    pub fn polygon_create(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "polygon_create", payload)
    }

    /// `payload`: serialized `DeleteEntityRequest`.
    pub fn toggle_fix(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "toggle_fix", payload)
    }

    pub fn toggle_fix_entities(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "toggle_fix_entities", payload)
    }

    /// `payload`: serialized `DeleteEntitiesRequest`.
    pub fn delete_entities(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "delete_entities", payload)
    }

    /// `payload`: serialized `MovePointRequest`.
    pub fn move_point(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "move_point", payload)
    }

    /// `payload`: serialized `DeleteEntityRequest`.
    pub fn delete_entity(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "delete_entity", payload)
    }

    pub fn undo(&mut self) -> String {
        host::handle(&mut self.manager, "undo", "")
    }

    pub fn redo(&mut self) -> String {
        host::handle(&mut self.manager, "redo", "")
    }

    /// `payload`: serialized `SetGridSnapRequest`.
    pub fn set_grid_snap(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "set_grid_snap", payload)
    }

    /// `payload`: serialized `SetGridStepRequest`.
    pub fn set_grid_step(&mut self, payload: &str) -> String {
        host::handle(&mut self.manager, "set_grid_step", payload)
    }
}

impl Default for WasmEngine {
    fn default() -> Self {
        Self::new()
    }
}
