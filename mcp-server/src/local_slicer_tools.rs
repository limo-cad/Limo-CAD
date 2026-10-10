use super::*;
use limo_cad_export::{
    bambu_project::BambuProjectReport,
    slicer_verification::{local_slicer_service, LocalSlicerStartRequest, VerificationIdentity},
};

pub fn specs(project_schema: Value) -> Vec<ToolSpec> {
    let request = object_schema(
        json!({"project":project_schema,"options":object_schema(json!({
        "executable":{"type":"string","minLength":1,"description":"Explicit absolute path to an installed local Bambu Studio executable. No caller-supplied CLI arguments are accepted."},
        "timeout_seconds_per_plate":{"type":"integer","minimum":1,"maximum":600,"default":120}
    }), &["executable"])}),
        &["project", "options"],
    );
    let status = object_schema(
        json!({"job_id":{"type":"integer","minimum":1}}),
        &["job_id"],
    );
    vec![
        ToolSpec::direct(
            "bambu_local_verification_start",
            "Verify in local Bambu Studio",
            "Opt-in bounded native import/slicing of a temporary copy of the reviewed explicit Bambu project. Return a job ID immediately; poll for each plate's exit status, generated toolpath hash, native estimates/material use and recorded version. Evidence is tied to current document/project/settings/layout/profile hashes and becomes stale after edits. Portable export works without this tool. This command never starts or transmits a print, contacts a printer, or replaces an active slicer project.",
            "bambu_local_verification_start",
            Payload::Object,
            request,
        ),
        ToolSpec::direct(
            "bambu_local_verification_poll",
            "Read local slicer evidence",
            "Read bounded local job evidence owned by the current CAD document. Report stale after document edits. Import, generated toolpaths and physical qualification remain separate; native estimates do not prove fit or strength.",
            "bambu_local_verification_poll",
            Payload::Object,
            status.clone(),
        ),
        ToolSpec::direct(
            "bambu_local_verification_cancel",
            "Cancel local slicer validation",
            "Cancel only this tool's temporary-copy slicer child; never stop or change the user's active slicer or printer. Remaining plates are cancelled and partial evidence is retained.",
            "bambu_local_verification_cancel",
            Payload::Object,
            status,
        ),
    ]
}

impl CadServer {
    pub(super) fn start_local_verification(&mut self, arguments: Value) -> Result<Value, String> {
        let request: LocalSlicerStartRequest =
            serde_json::from_value(arguments).map_err(|e| e.to_string())?;
        let written = self.export_bambu_project(
            serde_json::to_value(&request.project).map_err(|e| e.to_string())?,
            false,
        )?;
        let bytes = BASE64
            .decode(
                written["bytes_base64"]
                    .as_str()
                    .ok_or("Writer returned no project bytes")?,
            )
            .map_err(|e| e.to_string())?;
        let report: BambuProjectReport =
            serde_json::from_value(written["report"].clone()).map_err(|e| e.to_string())?;
        let model = self
            .manager
            .export_project_model()
            .map_err(|e| e.to_string())?;
        request
            .project
            .validate(&model)
            .map_err(|e| e.to_string())?;
        let layout = serde_json::to_value(
            self.manager
                .export_view_solution(request.project.export.named_view.as_deref())
                .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let exported_layout = limo_cad_export::slicer_verification::resolved_bambu_layout(&report);
        let identity = VerificationIdentity::from_owned_export(
            &bytes,
            &model,
            &layout,
            &exported_layout,
            report.refresh_reference.profile_sha256,
            report.source_document_id,
            request.project.export.named_view.clone(),
        )?;
        serde_json::to_value(local_slicer_service().start_with_warnings(
            bytes,
            identity,
            report.template.plate_count as u32,
            request.options,
            self.verification_owner_id.clone(),
            report.warnings,
        )?)
        .map_err(|e| e.to_string())
    }
    pub(super) fn local_verification_status(
        &mut self,
        arguments: Value,
        cancel: bool,
    ) -> Result<Value, String> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Request {
            job_id: u64,
        }
        let request: Request = serde_json::from_value(arguments).map_err(|e| e.to_string())?;
        if cancel {
            return serde_json::to_value(
                local_slicer_service().cancel_owned(request.job_id, &self.verification_owner_id)?,
            )
            .map_err(|error| error.to_string());
        }
        let source = self
            .manager
            .print_intent()
            .source_document_id
            .ok_or("Current CAD document has no persistent print identity")?;
        let model = self
            .manager
            .export_project_model()
            .map_err(|e| e.to_string())?;
        let mut report = local_slicer_service().poll_owned(
            request.job_id,
            &source,
            &model,
            &self.verification_owner_id,
            cancel,
        )?;
        report.check_current_layout(
            self.manager
                .export_view_solution(report.identity.named_view.as_deref())
                .map_err(|e| e.to_string())
                .and_then(|layout| serde_json::to_value(layout).map_err(|e| e.to_string())),
        );
        if report.stale {
            local_slicer_service().note_owned_stale(
                request.job_id,
                &source,
                &self.verification_owner_id,
            )?;
        }
        serde_json::to_value(report).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod verification_observation_tests {
    use super::*;

    #[test]
    fn settings_edit_then_snapshot_restore_without_poll_preserves_stale_evidence_and_rejects_failed_edit(
    ) {
        let mut server = CadServer::new().unwrap();
        let mut document = server.manager.print_intent();
        document.defaults.wall_count = Some(2);
        let before = server.manager.export_project_model().unwrap();
        server
            .call_tool(
                "print_intent_set_document",
                json!({"document":document,"expected_model_json":before}),
            )
            .unwrap();
        let model = server.manager.export_project_model().unwrap();
        let source = server.manager.print_intent().source_document_id.unwrap();
        let bytes = b"owned MCP observation lifecycle".to_vec();
        let captured = VerificationIdentity::from_owned_export(
            &bytes,
            &model,
            &json!({}),
            &json!({}),
            "a".repeat(64),
            source.clone(),
            None,
        )
        .unwrap();
        let service = local_slicer_service();
        let started = service
            .start(
                bytes,
                captured.clone(),
                1,
                limo_cad_export::slicer_verification::LocalSlicerOptions {
                    executable: std::env::temp_dir().join("missing-mcp-model-observation.exe"),
                    timeout_seconds_per_plate: 1,
                },
                server.verification_owner_id.clone(),
            )
            .unwrap();
        document = server.manager.print_intent();
        document.defaults.wall_count = Some(6);
        assert!(server
            .call_tool(
                "print_intent_set_document",
                json!({"document":document,"expected_model_json":"stale snapshot"})
            )
            .is_err());
        assert!(!service.poll(started.job_id, None).unwrap().stale);
        server
            .call_tool(
                "print_intent_set_document",
                json!({"document":document,"expected_model_json":model}),
            )
            .unwrap();
        server
            .call_tool("cad_load_project_model", json!({"model_json":model}))
            .unwrap();
        let restored = server.manager.export_project_model().unwrap();
        assert_eq!(restored, model);
        let report = service
            .poll_owned(
                started.job_id,
                &source,
                &restored,
                &server.verification_owner_id,
                false,
            )
            .unwrap();
        assert!(report.stale);
        assert_eq!(report.identity, captured);
    }
}
