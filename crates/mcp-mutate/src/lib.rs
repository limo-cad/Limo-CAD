//! Shared MCP mutate name → engine method + payload mapping.
//!
//! Used by `nbcad-mcp` (`cad_submit` accept-list / ToolSpec sync tests) and by
//! the Tauri session bridge inbox dispatcher so both sides agree on every
//! modeling mutate. Inspect/export/control tools are intentionally absent.

use serde_json::{json, Value};

/// Reads served directly by the owning desktop engine. In-progress sketches
/// are absent from the completed snapshot; assembly metadata needs no second
/// geometry reconstruction. This shared allowlist never dispatches mutations.
pub fn is_live_engine_query(method: &str) -> bool {
    matches!(
        method,
        "active_sketch"
            | "drawing_export"
            | "drawing_projection"
            | "assembly_document"
            | "eval_expression"
            | "preview_segment"
            | "preview_segment_locked"
            | "fillet_preview"
            | "offset_preview"
            | "trim_preview"
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PayloadKind {
    Empty,
    Object,
    Field(&'static str),
    DatumSource(&'static str),
    EditDatumSource(&'static str),
    BodyFeature(&'static str),
    EditBodyFeature(&'static str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionKind {
    Direct,
    SolidReplay,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MutateSpec {
    pub name: &'static str,
    pub engine_method: &'static str,
    pub payload: PayloadKind,
    pub execution: ExecutionKind,
}

/// Every modeling mutate that `cad_submit` may enqueue and the UI inbox may apply.
pub static MUTATES: &[MutateSpec] = &[
    MutateSpec {
        name: "drawing_create_sheet",
        engine_method: "drawing_create_sheet",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "drawing_select_sheet",
        engine_method: "drawing_select_sheet",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "drawing_delete_sheet",
        engine_method: "drawing_delete_sheet",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "drawing_add_view",
        engine_method: "drawing_add_view",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "drawing_add_linear_dimension",
        engine_method: "drawing_add_linear_dimension",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "drawing_add_radial_dimension",
        engine_method: "drawing_add_radial_dimension",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "drawing_add_angular_dimension",
        engine_method: "drawing_add_angular_dimension",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "drawing_set_bom",
        engine_method: "drawing_set_bom",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "drawing_add_note",
        engine_method: "drawing_add_note",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "cad_set_document_name",
        engine_method: "document_set_name",
        payload: PayloadKind::Field("name"),
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "cad_load_project_model",
        engine_method: "project_prepare_load",
        payload: PayloadKind::Field("model_json"),
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "cad_new_project",
        engine_method: "project_prepare_new",
        payload: PayloadKind::Empty,
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "sketch_begin",
        engine_method: "begin_sketch",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_finish",
        engine_method: "end_sketch",
        payload: PayloadKind::Empty,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_edit",
        engine_method: "edit_sketch",
        payload: PayloadKind::Field("name"),
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_add_line",
        engine_method: "add_line",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_add_line_locked",
        engine_method: "add_line_locked",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_add_midpoint_line",
        engine_method: "add_line_midpoint",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_add_point",
        engine_method: "add_point",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_add_rectangle",
        engine_method: "add_rectangle",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_add_rectangle_locked",
        engine_method: "add_rectangle_locked",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_add_circle",
        engine_method: "add_circle",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_add_circle_locked",
        engine_method: "add_circle_locked",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_add_arc_3pt",
        engine_method: "add_arc_3pt",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_add_arc_center",
        engine_method: "add_arc_center",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_add_slot",
        engine_method: "add_slot",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_add_spline",
        engine_method: "add_spline",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_add_constraint",
        engine_method: "add_constraint",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_add_constraints",
        engine_method: "add_constraints",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_add_dimension",
        engine_method: "add_dimension",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_edit_dimension",
        engine_method: "edit_dimension",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_move_dimension",
        engine_method: "move_dimension",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_delete_dimension",
        engine_method: "delete_dimension",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_fillet",
        engine_method: "fillet_lines",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_chamfer",
        engine_method: "chamfer_lines",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_offset",
        engine_method: "offset_curve",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_trim",
        engine_method: "trim_entity",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_extend",
        engine_method: "extend_entity",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_break",
        engine_method: "break_curve",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_mirror",
        engine_method: "mirror_entities",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_rectangular_pattern",
        engine_method: "rectangular_pattern",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_circular_pattern",
        engine_method: "circular_pattern",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_move_copy",
        engine_method: "move_copy_entities",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_scale",
        engine_method: "scale_entities",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_polygon",
        engine_method: "polygon_create",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_move_point",
        engine_method: "move_point",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_toggle_fix",
        engine_method: "toggle_fix_entities",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_delete_entities",
        engine_method: "delete_entities",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_undo",
        engine_method: "undo",
        payload: PayloadKind::Empty,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_redo",
        engine_method: "redo",
        payload: PayloadKind::Empty,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_set_grid_snap",
        engine_method: "set_grid_snap",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_set_grid_step",
        engine_method: "set_grid_step",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "sketch_set_dimension_style",
        engine_method: "set_dimension_style",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "construction_plane_offset",
        engine_method: "datum_plane_create",
        payload: PayloadKind::DatumSource("offset"),
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "construction_plane_edit_offset",
        engine_method: "datum_plane_edit",
        payload: PayloadKind::EditDatumSource("offset"),
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "construction_plane_midplane",
        engine_method: "datum_plane_create",
        payload: PayloadKind::DatumSource("midplane"),
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "construction_plane_edit_midplane",
        engine_method: "datum_plane_edit",
        payload: PayloadKind::EditDatumSource("midplane"),
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "construction_plane_at_angle",
        engine_method: "datum_plane_create",
        payload: PayloadKind::DatumSource("at_angle"),
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "construction_plane_edit_at_angle",
        engine_method: "datum_plane_edit",
        payload: PayloadKind::EditDatumSource("at_angle"),
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "solid_extrude",
        engine_method: "solid_prepare_extrude",
        payload: PayloadKind::Object,
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_edit_extrude",
        engine_method: "solid_prepare_edit_extrude",
        payload: PayloadKind::Object,
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_revolve",
        engine_method: "solid_prepare_revolve",
        payload: PayloadKind::Object,
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_edit_revolve",
        engine_method: "solid_prepare_edit_revolve",
        payload: PayloadKind::Object,
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_sweep",
        engine_method: "solid_prepare_sweep",
        payload: PayloadKind::Object,
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_edit_sweep",
        engine_method: "solid_prepare_edit_sweep",
        payload: PayloadKind::Object,
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_loft",
        engine_method: "solid_prepare_loft",
        payload: PayloadKind::Object,
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_edit_loft",
        engine_method: "solid_prepare_edit_loft",
        payload: PayloadKind::Object,
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_rib",
        engine_method: "solid_prepare_rib",
        payload: PayloadKind::Object,
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_edit_rib",
        engine_method: "solid_prepare_edit_rib",
        payload: PayloadKind::Object,
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_fillet",
        engine_method: "solid_prepare_fillet",
        payload: PayloadKind::Object,
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_edit_fillet",
        engine_method: "solid_prepare_edit_fillet",
        payload: PayloadKind::Object,
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_chamfer",
        engine_method: "solid_prepare_chamfer",
        payload: PayloadKind::Object,
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_edit_chamfer",
        engine_method: "solid_prepare_edit_chamfer",
        payload: PayloadKind::Object,
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_hole",
        engine_method: "solid_prepare_hole",
        payload: PayloadKind::Object,
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_edit_hole",
        engine_method: "solid_prepare_edit_hole",
        payload: PayloadKind::Object,
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_shell",
        engine_method: "solid_prepare_body_feature",
        payload: PayloadKind::BodyFeature("shell"),
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_edit_shell",
        engine_method: "solid_prepare_edit_body_feature",
        payload: PayloadKind::EditBodyFeature("shell"),
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_move_copy",
        engine_method: "solid_prepare_body_feature",
        payload: PayloadKind::BodyFeature("move_copy"),
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_edit_move_copy",
        engine_method: "solid_prepare_edit_body_feature",
        payload: PayloadKind::EditBodyFeature("move_copy"),
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_mirror",
        engine_method: "solid_prepare_body_feature",
        payload: PayloadKind::BodyFeature("mirror"),
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_edit_mirror",
        engine_method: "solid_prepare_edit_body_feature",
        payload: PayloadKind::EditBodyFeature("mirror"),
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_rectangular_pattern",
        engine_method: "solid_prepare_body_feature",
        payload: PayloadKind::BodyFeature("rectangular_pattern"),
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_edit_rectangular_pattern",
        engine_method: "solid_prepare_edit_body_feature",
        payload: PayloadKind::EditBodyFeature("rectangular_pattern"),
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_circular_pattern",
        engine_method: "solid_prepare_body_feature",
        payload: PayloadKind::BodyFeature("circular_pattern"),
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_edit_circular_pattern",
        engine_method: "solid_prepare_edit_body_feature",
        payload: PayloadKind::EditBodyFeature("circular_pattern"),
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_combine",
        engine_method: "solid_prepare_body_feature",
        payload: PayloadKind::BodyFeature("combine"),
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_edit_combine",
        engine_method: "solid_prepare_edit_body_feature",
        payload: PayloadKind::EditBodyFeature("combine"),
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_split_body",
        engine_method: "solid_prepare_body_feature",
        payload: PayloadKind::BodyFeature("split_body"),
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_edit_split_body",
        engine_method: "solid_prepare_edit_body_feature",
        payload: PayloadKind::EditBodyFeature("split_body"),
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_import_step",
        engine_method: "solid_prepare_body_feature",
        payload: PayloadKind::BodyFeature("import_step"),
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_edit_import_step",
        engine_method: "solid_prepare_edit_body_feature",
        payload: PayloadKind::EditBodyFeature("import_step"),
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_recompute",
        engine_method: "solid_prepare_recompute",
        payload: PayloadKind::Empty,
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_set_rollback",
        engine_method: "solid_prepare_set_rollback",
        payload: PayloadKind::Object,
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_delete_feature",
        engine_method: "solid_prepare_delete_feature",
        payload: PayloadKind::Object,
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "solid_reorder_feature",
        engine_method: "solid_prepare_reorder_feature",
        payload: PayloadKind::Object,
        execution: ExecutionKind::SolidReplay,
    },
    MutateSpec {
        name: "assembly_create_component",
        engine_method: "assembly_create_component",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "assembly_update_component",
        engine_method: "assembly_update_component",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "assembly_create_occurrence",
        engine_method: "assembly_create_occurrence",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "assembly_update_occurrence",
        engine_method: "assembly_update_occurrence",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "assembly_set_occurrence_pose",
        engine_method: "assembly_set_occurrence_pose",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "assembly_set_occurrence_grounded",
        engine_method: "assembly_set_occurrence_grounded",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "assembly_create_joint",
        engine_method: "assembly_create_joint",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "assembly_update_joint",
        engine_method: "assembly_update_joint",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "assembly_delete_joint",
        engine_method: "assembly_delete_joint",
        payload: PayloadKind::Field("joint_id"),
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "assembly_set_joint_enabled",
        engine_method: "assembly_set_joint_enabled",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "assembly_set_joint_motion",
        engine_method: "assembly_set_joint_motion",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "assembly_create_gear_relation",
        engine_method: "assembly_create_gear_relation",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "assembly_update_gear_relation",
        engine_method: "assembly_update_gear_relation",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "assembly_delete_gear_relation",
        engine_method: "assembly_delete_gear_relation",
        payload: PayloadKind::Field("relation_id"),
        execution: ExecutionKind::Direct,
    },
    MutateSpec {
        name: "set_body_appearance",
        engine_method: "set_body_appearance",
        payload: PayloadKind::Object,
        execution: ExecutionKind::Direct,
    },
];

pub fn mutate_specs() -> &'static [MutateSpec] {
    MUTATES
}

pub fn lookup_mutate(name: &str) -> Option<&'static MutateSpec> {
    MUTATES.iter().find(|spec| spec.name == name)
}

pub fn is_inbox_mutate(name: &str) -> bool {
    lookup_mutate(name).is_some()
}

/// Encode MCP tool arguments into the host/engine payload string.
pub fn encode_payload(kind: PayloadKind, arguments: &Value) -> Result<String, String> {
    match kind {
        PayloadKind::Empty => Ok(String::new()),
        PayloadKind::Object => serde_json::to_string(arguments)
            .map_err(|error| format!("could not encode arguments: {error}")),
        PayloadKind::Field(field) => {
            let value = arguments
                .get(field)
                .ok_or_else(|| format!("missing required argument '{field}'"))?;
            serde_json::to_string(value)
                .map_err(|error| format!("could not encode '{field}': {error}"))
        }
        PayloadKind::DatumSource(kind) => {
            let mut source = arguments
                .as_object()
                .cloned()
                .ok_or_else(|| "tool arguments must be an object".to_string())?;
            source.insert("type".to_string(), Value::String(kind.to_string()));
            let name = source.remove("name");
            let mut payload = json!({ "source": source });
            if let Some(name) = name {
                payload["name"] = name;
            }
            serde_json::to_string(&payload)
                .map_err(|error| format!("could not encode construction plane: {error}"))
        }
        PayloadKind::EditDatumSource(kind) => {
            let mut fields = arguments
                .as_object()
                .cloned()
                .ok_or_else(|| "tool arguments must be an object".to_string())?;
            let feature_id = fields
                .remove("feature_id")
                .ok_or_else(|| "missing required argument 'feature_id'".to_string())?;
            fields.insert("type".to_string(), Value::String(kind.to_string()));
            serde_json::to_string(&json!({
                "feature_id": feature_id,
                "plane": { "source": fields }
            }))
            .map_err(|error| format!("could not encode construction plane edit: {error}"))
        }
        PayloadKind::BodyFeature(kind) => serde_json::to_string(&json!({
            "type": kind,
            "request": arguments
        }))
        .map_err(|error| format!("could not encode body feature: {error}")),
        PayloadKind::EditBodyFeature(kind) => {
            let feature_id = arguments
                .get("feature_id")
                .ok_or_else(|| "missing required argument 'feature_id'".to_string())?;
            let request = arguments
                .get("request")
                .cloned()
                .unwrap_or_else(|| json!({}));
            serde_json::to_string(&json!({
                "feature_id": feature_id,
                "feature": { "type": kind, "request": request }
            }))
            .map_err(|error| format!("could not encode body feature edit: {error}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutate_names_are_unique_and_nonempty() {
        let mut seen = std::collections::BTreeSet::new();
        for spec in MUTATES {
            assert!(!spec.name.is_empty());
            assert!(!spec.engine_method.is_empty());
            assert!(seen.insert(spec.name), "duplicate mutate {}", spec.name);
        }
        assert!(!MUTATES.is_empty());
    }

    #[test]
    fn sketch_add_line_locked_maps_to_engine_method() {
        let spec = lookup_mutate("sketch_add_line_locked").expect("present");
        assert_eq!(spec.engine_method, "add_line_locked");
        assert_eq!(spec.execution, ExecutionKind::Direct);
        assert_eq!(spec.payload, PayloadKind::Object);
    }

    #[test]
    fn solid_import_step_and_move_copy_are_in_lookup_mutate() {
        let import = lookup_mutate("solid_import_step").expect("present");
        assert_eq!(import.engine_method, "solid_prepare_body_feature");
        assert_eq!(import.execution, ExecutionKind::SolidReplay);
        assert_eq!(import.payload, PayloadKind::BodyFeature("import_step"));
        let edit_import = lookup_mutate("solid_edit_import_step").expect("present");
        assert_eq!(
            edit_import.payload,
            PayloadKind::EditBodyFeature("import_step")
        );
        let move_copy = lookup_mutate("solid_move_copy").expect("present");
        assert_eq!(move_copy.engine_method, "solid_prepare_body_feature");
        assert_eq!(move_copy.execution, ExecutionKind::SolidReplay);
        assert_eq!(move_copy.payload, PayloadKind::BodyFeature("move_copy"));
        let edit_move = lookup_mutate("solid_edit_move_copy").expect("present");
        assert_eq!(edit_move.payload, PayloadKind::EditBodyFeature("move_copy"));
        assert!(lookup_mutate("cad_script").is_none());
        assert!(lookup_mutate("cad_compare_solids").is_none());
        let create = lookup_mutate("assembly_create_component").expect("present");
        assert_eq!(create.engine_method, "assembly_create_component");
        assert_eq!(create.execution, ExecutionKind::Direct);
        assert!(lookup_mutate("assembly_document").is_none());
        assert!(lookup_mutate("assembly_solution").is_none());
    }

    #[test]
    fn assembly_create_joint_is_in_lookup_mutate() {
        let create = lookup_mutate("assembly_create_joint").expect("present");
        assert_eq!(create.engine_method, "assembly_create_joint");
        assert_eq!(create.execution, ExecutionKind::Direct);
        assert_eq!(create.payload, PayloadKind::Object);
        let update = lookup_mutate("assembly_update_joint").expect("present");
        assert_eq!(update.engine_method, "assembly_update_joint");
        assert_eq!(update.execution, ExecutionKind::Direct);
        assert_eq!(update.payload, PayloadKind::Object);
        assert!(lookup_mutate("assembly_document").is_none());
        assert!(lookup_mutate("assembly_solution").is_none());
        let delete = lookup_mutate("assembly_delete_joint").expect("present");
        assert_eq!(delete.engine_method, "assembly_delete_joint");
        assert_eq!(delete.execution, ExecutionKind::Direct);
        assert_eq!(delete.payload, PayloadKind::Field("joint_id"));
        assert_eq!(
            encode_payload(delete.payload, &json!({"joint_id": 42})).unwrap(),
            "42"
        );
        assert!(encode_payload(delete.payload, &json!({"id": 42})).is_err());
    }

    #[test]
    fn encode_object_and_field_payloads() {
        let object = encode_payload(PayloadKind::Object, &json!({"x": 1})).unwrap();
        assert!(object.contains("x"));
        let field = encode_payload(PayloadKind::Field("name"), &json!({"name": "Part"})).unwrap();
        assert!(field.contains("Part"));
        let err = encode_payload(PayloadKind::Field("name"), &json!({})).unwrap_err();
        assert!(err.contains("missing required argument 'name'"));
    }

    #[test]
    fn assembly_create_component_is_in_lookup_mutate() {
        let spec = lookup_mutate("assembly_create_component").expect("present");
        assert_eq!(spec.engine_method, "assembly_create_component");
        assert_eq!(spec.execution, ExecutionKind::Direct);
        assert_eq!(spec.payload, PayloadKind::Object);
        assert!(lookup_mutate("assembly_document").is_none());
        assert!(lookup_mutate("assembly_solution").is_none());
        for name in [
            "assembly_update_component",
            "assembly_create_occurrence",
            "assembly_update_occurrence",
            "assembly_set_occurrence_pose",
            "assembly_set_occurrence_grounded",
        ] {
            assert!(lookup_mutate(name).is_some(), "{name}");
        }
    }
}
