use super::*;
use limo_cad_export::{bambu_project, BambuExportRequest, MeshInstance};

pub fn specs() -> Vec<ToolSpec> {
    let template = json!({"type":"string","maxLength":188743680,"description":"Base64 bytes of a complete saved Bambu project. Its source is never overwritten."});
    let id = json!({"type":"integer","minimum":1});
    let binding = binding_schema();
    let reference = refresh_reference_schema();
    let export = object_schema(
        json!({
            "expected_model_json":{"type":"string","minLength":1},
            "body_ids":{"type":"array","maxItems":4096,"uniqueItems":true,"items":id},
            "scope":{"const":"assembly"},"named_view":{"type":["string","null"]},
            "linear_deflection":{"type":"number","exclusiveMinimum":0},"angular_deflection":{"type":"number","exclusiveMinimum":0},
            "include_appearance":{"const":true},"slicer_target":{"type":"string","enum":["standard","bambu_studio"]}
        }),
        &["expected_model_json"],
    );
    let request = object_schema(
        json!({"template_base64":template,"export":export,"project":object_schema(json!({
        "source_document_id":{"type":"string","minLength":36,"maxLength":36},
        "bindings":{"type":"array","maxItems":4096,"items":binding},
        "placement":{"type":"string","enum":["resolved_scene","template"],"default":"resolved_scene"},
        "refresh_reference":reference,
        "start_reviewed_native_baseline":{"type":"boolean","default":false,"description":"Explicitly adopt the inspected native archive using complete reviewed current numeric bindings. Requires nonempty bindings and no refresh_reference; does not infer missing identities or change automatic refresh."},
        "accept_native_setting_changes":{"type":"boolean","default":false,"description":"Explicitly accept reviewed native edits to the five managed settings as the new inherited baseline; ambiguity and changed profile identity remain errors."},
        "allow_template_appearance":{"type":"boolean","default":false,"description":"Explicitly retain reviewed template filament/color mapping when it differs from CAD appearance."}
    }), &["source_document_id"])}),
        &["template_base64", "export", "project"],
    );
    vec![
        ToolSpec::direct(
            "bambu_template_inspect",
            "Inspect a complete Bambu template",
            "Validate a saved Bambu project and inspect full printer/process/filament/support mappings plus object, instance and normal-volume IDs for deliberate stable CAD bindings. A bed envelope or thin inherited profile is insufficient. No slicing, print or cloud command is performed.",
            "bambu_template_inspect",
            Payload::Object,
            object_schema(json!({"template_base64":template}), &["template_base64"]),
        ),
        ToolSpec::direct(
            "bambu_project_preview",
            "Preview explicit Bambu project export",
            "Run the shared Rust project writer and readback without returning file bytes. Review effective settings, stable identity bindings, profile provenance, filament/nozzle/support mapping, differences, preserved grouping and invalidated stale slicing data. Foreign templates require explicit bindings; refresh uses the source namespace manifest. Metadata readback does not prove slicing or physical performance.",
            "bambu_project_preview",
            Payload::Object,
            request.clone(),
        ),
        ToolSpec::direct(
            "solid_export_bambu_project",
            "Export an unsliced Bambu project",
            "Explicit separate project mode over a complete saved Bambu template. Return base64 bytes and the same report as preview, with matched meshes replaced, requested typed settings applied and stale G-code/caches removed. Preserve the selected profile and logical filament/support assignments; physical AMS trays remain explicit. Reslicing is required. Portable solid_export_3mf remains the default. No printer/cloud commands are sent.",
            "solid_export_bambu_project",
            Payload::Object,
            request,
        ),
    ]
}

impl CadServer {
    pub(super) fn export_bambu_project(
        &mut self,
        arguments: Value,
        preview: bool,
    ) -> Result<Value, String> {
        let request: BambuExportRequest = serde_json::from_value(arguments)
            .map_err(|e| format!("Invalid Bambu project request: {e}"))?;
        request
            .validate(
                &self
                    .manager
                    .export_project_model()
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        self.manager
            .validate_print_height_export_view(request.export.named_view.as_deref())
            .map_err(|e| e.to_string())?;
        if !self.manager.solid_scene_ref().errors.is_empty() {
            return Err("Resolve timeline errors before Bambu project export".into());
        }
        let template = BASE64
            .decode(&request.template_base64)
            .map_err(|e| format!("Invalid template base64: {e}"))?;
        let solution = self
            .manager
            .export_view_solution(request.export.named_view.as_deref())
            .map_err(|e| e.to_string())?;
        if !solution.solved {
            return Err("Resolve assembly/layout errors before Bambu project export".into());
        }
        let instances: Vec<_> = solution
            .instance_body_poses
            .iter()
            .map(|pose| MeshInstance {
                body_id: pose.body_id,
                occurrence_id: pose.occurrence_id.0,
                translation: pose.translation,
                rotation: pose.rotation,
                visible: pose.visible,
            })
            .collect();
        self.manager
            .solid_scene_ref()
            .require_complete_display_mesh(&request.export.body_ids)?;
        let mut meshes = self
            .kernel
            .tessellate_bodies(&request.export)
            .map_err(|e| e.to_string())?;
        for mesh in &mut meshes {
            if let Some(body) = self
                .manager
                .solid_scene_ref()
                .bodies
                .iter()
                .find(|body| body.id == mesh.body_id)
            {
                mesh.name = body.name.clone();
            }
        }
        let exported = bambu_project::write_bambu_project(
            &template,
            &meshes,
            &self.manager.body_appearances(),
            &instances,
            &self.manager.assembly_document_ref().component_structure,
            &self.manager.print_intent(),
            &request.project,
        )
        .map_err(|e| e.to_string())?;
        if !preview {
            limo_cad_export::slicer_verification::local_slicer_service().note_owned_export(
                &self.verification_owner_id,
                &exported.report.source_document_id,
                &exported.report.output_sha256,
            )?;
        }
        let mut response = json!({"format":"3mf","export_mode":"bambu_project","byte_length":exported.bytes.len(),"report":exported.report,"preview":preview,"requires_reslicing":true});
        if !preview {
            response["encoding"] = json!("base64");
            response["bytes_base64"] = json!(BASE64.encode(exported.bytes));
        }
        Ok(response)
    }
}

pub fn inspect(arguments: Value) -> Result<Value, String> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        template_base64: String,
    }
    let request: Request = serde_json::from_value(arguments).map_err(|e| e.to_string())?;
    if request.template_base64.len() > 180 * 1024 * 1024 {
        return Err("Bambu template exceeds 128 MiB".into());
    }
    let bytes = BASE64
        .decode(request.template_base64)
        .map_err(|e| e.to_string())?;
    serde_json::to_value(bambu_project::inspect_bambu_template(&bytes).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

fn binding_schema() -> Value {
    let id = json!({"type":"integer","minimum":1,"maximum":9007199254740990u64});
    object_schema(
        json!({"body_id":id,"occurrence_id":id,"object_id":id,"instance_id":{"type":"integer","minimum":0},"part_id":id}),
        &[
            "body_id",
            "occurrence_id",
            "object_id",
            "instance_id",
            "part_id",
        ],
    )
}

pub(super) fn refresh_reference_schema() -> Value {
    let binding = binding_schema();
    let managed_settings = object_schema(
        json!({
            "wall_loops":{"type":"string"},
            "sparse_infill_density":{"type":"string"},
            "sparse_infill_pattern":{"type":"string","enum":["grid","gyroid","zig-zag","concentric","cubic","honeycomb","lightning"]},
            "top_shell_layers":{"type":"string"},"bottom_shell_layers":{"type":"string"}
        }),
        &[],
    );
    let hash = json!({"type":"string","pattern":"^[0-9a-f]{64}$"});
    object_schema(
        json!({
            "version":{"const":1},"source_document_id":{"type":"string","minLength":36,"maxLength":36},
            "original_template_sha256":hash,"profile_sha256":hash,"profile_identity_sha256":hash,
            "baseline_project_settings":managed_settings,"written_project_settings":managed_settings,
            "height_objects":{"type":"array","maxItems":4096,"items":print_height_tools::refresh_object_schema()},
            "modifiers":{"type":"array","maxItems":1024,"items":object_schema(json!({
                "modifier":print_modifier_tools::modifier_schema(),
                "parent_volume_uuid":{"type":"string","minLength":1,"maxLength":256},
                "target_uuid":{"type":"string","minLength":1,"maxLength":256},
                "source_mesh_center_mm":{"type":"array","minItems":3,"maxItems":3,"items":{"type":"number","minimum":-10_000_000,"maximum":10_000_000}}
            }), &["modifier","parent_volume_uuid","target_uuid","source_mesh_center_mm"])},
            "parts":{"type":"array","maxItems":4096,"items":object_schema(json!({
                "binding":binding,"target_uuid":{"type":"string","minLength":1,"maxLength":256},
                "instance_identify_id":{"type":"integer","minimum":1},
                "baseline_part_settings":managed_settings,"written_part_settings":managed_settings,
                "baseline_object_settings":{"anyOf":[managed_settings,{"type":"null"}]},
                "written_object_settings":{"anyOf":[managed_settings,{"type":"null"}]}
            }), &["binding","target_uuid","instance_identify_id","baseline_part_settings","written_part_settings"])}
        }),
        &[
            "version",
            "source_document_id",
            "original_template_sha256",
            "profile_sha256",
            "profile_identity_sha256",
            "baseline_project_settings",
            "written_project_settings",
            "parts",
        ],
    )
}

pub(super) fn handoff_schema() -> Value {
    object_schema(
        json!({"kind":{"const":"bambu_studio"},
        "name":{"type":"string","minLength":1,"maxLength":256},
        "source_label":{"type":"string","minLength":1,"maxLength":256},
        "reference":refresh_reference_schema()}),
        &["kind", "name", "source_label", "reference"],
    )
}
