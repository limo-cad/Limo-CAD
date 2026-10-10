use super::*;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use limo_cad_export::{bambu_project, BambuExportRequest, MeshInstance};
use serde_json::json;

pub(super) fn inspect_template(payload: &str) -> String {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Request {
        template_base64: String,
    }
    let result = (|| -> Result<_, String> {
        if payload.len() > 180 * 1024 * 1024 {
            return Err("Bambu template payload exceeds 128 MiB".into());
        }
        let request: Request = serde_json::from_str(payload).map_err(|e| e.to_string())?;
        let bytes = BASE64
            .decode(request.template_base64)
            .map_err(|e| e.to_string())?;
        bambu_project::inspect_bambu_template(&bytes).map_err(|e| e.to_string())
    })();
    match result {
        Ok(result) => ok_json(result),
        Err(error) => err_json(error),
    }
}

impl NativeEngineHost {
    /// Preview and write share the same writer and ownership fence. Neither operation slices or prints.
    pub fn bambu_project(&self, payload: &str, preview: bool) -> String {
        let result = (|| -> Result<_, String> {
            if payload.len() > 192 * 1024 * 1024 {
                return Err("Bambu export payload exceeds 192 MiB".into());
            }
            let request: BambuExportRequest = serde_json::from_str(payload)
                .map_err(|e| format!("Invalid Bambu project request: {e}"))?;
            let workspace = self.inner.lock().map_err(|_| "Engine lock poisoned")?;
            let inner = workspace.active();
            request
                .validate(
                    &inner
                        .manager
                        .export_project_model()
                        .map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
            inner
                .manager
                .validate_print_height_export_view(request.export.named_view.as_deref())
                .map_err(|e| e.to_string())?;
            if !inner.manager.solid_scene_ref().errors.is_empty() {
                return Err("Resolve timeline errors before Bambu project export".into());
            }
            let template = BASE64
                .decode(&request.template_base64)
                .map_err(|e| format!("Invalid template base64: {e}"))?;
            let solution = inner
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
            inner
                .manager
                .solid_scene_ref()
                .require_complete_display_mesh(&request.export.body_ids)?;
            let mut meshes = inner
                .kernel
                .tessellate_bodies(&request.export)
                .map_err(|e| e.to_string())?;
            for mesh in &mut meshes {
                if let Some(body) = inner
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
                &inner.manager.body_appearances(),
                &instances,
                &inner.manager.assembly_document_ref().component_structure,
                &inner.manager.print_intent(),
                &request.project,
            )
            .map_err(|e| e.to_string())?;
            if !preview {
                limo_cad_export::slicer_verification::local_slicer_service().note_owned_export(
                    &workspace.verification_owner_key(),
                    &exported.report.source_document_id,
                    &exported.report.output_sha256,
                )?;
            }
            let source_layout = serde_json::to_value(&solution).map_err(|e| e.to_string())?;
            let mut response = json!({"format":"3mf","export_mode":"bambu_project","byte_length":exported.bytes.len(),"report":exported.report,"preview":preview,"requires_reslicing":true,"source_layout":source_layout,"source_session_id":workspace.active_session_id,"source_geometry_revision":inner.geometry_revision});
            if !preview {
                response["encoding"] = json!("base64");
                response["bytes_base64"] = json!(BASE64.encode(exported.bytes));
            }
            Ok(response)
        })();
        match result {
            Ok(result) => ok_json(result),
            Err(error) => err_json(error),
        }
    }
}
