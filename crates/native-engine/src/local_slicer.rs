use super::*;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use limo_cad_export::{
    bambu_project::BambuProjectReport,
    slicer_verification::{local_slicer_service, LocalSlicerStartRequest, VerificationIdentity},
};
use serde_json::Value;

impl NativeEngineHost {
    pub fn start_local_slicer_verification(&self, payload: &str) -> String {
        let result = (|| -> Result<_, String> {
            if payload.len() > 192 * 1024 * 1024 {
                return Err("Local verification request exceeds the project payload limit".into());
            }
            let request: LocalSlicerStartRequest =
                serde_json::from_str(payload).map_err(|e| e.to_string())?;
            let written: Value = serde_json::from_str(&self.bambu_project(
                &serde_json::to_string(&request.project).map_err(|e| e.to_string())?,
                false,
            ))
            .map_err(|e| e.to_string())?;
            if written["ok"] != true {
                return Err(written["error"]
                    .as_str()
                    .unwrap_or("Bambu project preparation failed")
                    .into());
            }
            let written = &written["value"];
            let bytes = BASE64
                .decode(
                    written["bytes_base64"]
                        .as_str()
                        .ok_or("Writer returned no project bytes")?,
                )
                .map_err(|e| e.to_string())?;
            let report: BambuProjectReport =
                serde_json::from_value(written["report"].clone()).map_err(|e| e.to_string())?;
            let workspace = self.inner.lock().map_err(|_| "Engine lock poisoned")?;
            let inner = workspace.active();
            let model = inner
                .manager
                .export_project_model()
                .map_err(|e| e.to_string())?;
            // Reject intervening edits between writing the reviewed copy and starting the worker.
            request
                .project
                .validate(&model)
                .map_err(|e| e.to_string())?;
            let layout = serde_json::to_value(
                inner
                    .manager
                    .export_view_solution(request.project.export.named_view.as_deref())
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            if written["source_session_id"].as_str() != Some(workspace.active_session_id.as_str())
                || written["source_layout"] != layout
                || written["source_geometry_revision"].as_u64() != Some(inner.geometry_revision)
            {
                return Err("The owning tab or presentation layout changed while preparing validation; review the project again".into());
            }
            let exported_layout =
                limo_cad_export::slicer_verification::resolved_bambu_layout(&report);
            let mut identity = VerificationIdentity::from_owned_export(
                &bytes,
                &model,
                &layout,
                &exported_layout,
                report.refresh_reference.profile_sha256,
                report.source_document_id,
                request.project.export.named_view.clone(),
            )?;
            identity.source_geometry_revision = Some(inner.geometry_revision);
            local_slicer_service().start_with_warnings(
                bytes,
                identity,
                report.template.plate_count as u32,
                request.options,
                workspace.verification_owner_key(),
                report.warnings,
            )
        })();
        match result {
            Ok(value) => ok_json(value),
            Err(error) => err_json(error),
        }
    }

    pub fn local_slicer_status(&self, payload: &str, cancel: bool) -> String {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Request {
            job_id: u64,
        }
        let result = (|| -> Result<_, String> {
            let request: Request = serde_json::from_str(payload).map_err(|e| e.to_string())?;
            let workspace = self.inner.lock().map_err(|_| "Engine lock poisoned")?;
            if cancel {
                return serde_json::to_value(
                    local_slicer_service()
                        .cancel_owned(request.job_id, &workspace.verification_owner_key())?,
                )
                .map_err(|error| error.to_string());
            }
            let manager = &workspace.active().manager;
            let source = manager
                .print_intent()
                .source_document_id
                .ok_or("Current CAD document has no persistent print identity")?;
            let model = manager.export_project_model().map_err(|e| e.to_string())?;
            let mut report = local_slicer_service().poll_owned(
                request.job_id,
                &source,
                &model,
                &workspace.verification_owner_key(),
                cancel,
            )?;
            report.check_current_geometry_revision(workspace.active().geometry_revision);
            report.check_current_layout(
                manager
                    .export_view_solution(report.identity.named_view.as_deref())
                    .map_err(|e| e.to_string())
                    .and_then(|layout| serde_json::to_value(layout).map_err(|e| e.to_string())),
            );
            if report.stale {
                local_slicer_service().note_owned_stale(
                    request.job_id,
                    &source,
                    &workspace.verification_owner_key(),
                )?;
            }
            serde_json::to_value(report).map_err(|error| error.to_string())
        })();
        match result {
            Ok(value) => ok_json(value),
            Err(error) => err_json(error),
        }
    }
}

#[cfg(test)]
mod verification_ownership_tests {
    use super::*;
    #[test]
    fn accepted_settings_edit_then_snapshot_restore_without_poll_keeps_evidence_stale() {
        let host = NativeEngineHost::new();
        let mut document = limo_cad_core::PrintIntentDocumentDto::default();
        document.defaults.wall_count = Some(2);
        let before = host
            .inner
            .lock()
            .unwrap()
            .active()
            .manager
            .export_project_model()
            .unwrap();
        let set: Value = serde_json::from_str(
            &host.engine_call(
                "print_intent_set_document",
                &serde_json::json!({
                    "document":document,"expected_model_json":before,
                })
                .to_string(),
            ),
        )
        .unwrap();
        assert_eq!(set["ok"], true);
        let (model, source, owner) = {
            let workspace = host.inner.lock().unwrap();
            (
                workspace.active().manager.export_project_model().unwrap(),
                workspace
                    .active()
                    .manager
                    .print_intent()
                    .source_document_id
                    .unwrap(),
                workspace.verification_owner_key(),
            )
        };
        let bytes = b"owned native settings lifecycle".to_vec();
        let captured = VerificationIdentity::from_owned_export(
            &bytes,
            &model,
            &serde_json::json!({}),
            &serde_json::json!({}),
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
                    executable: std::env::temp_dir().join("missing-native-settings-lifecycle.exe"),
                    timeout_seconds_per_plate: 1,
                },
                owner.clone(),
            )
            .unwrap();
        document = host.inner.lock().unwrap().active().manager.print_intent();
        document.defaults.wall_count = Some(6);
        let rejected: Value = serde_json::from_str(
            &host.engine_call(
                "print_intent_set_document",
                &serde_json::json!({
                    "document":document,"expected_model_json":"stale snapshot",
                })
                .to_string(),
            ),
        )
        .unwrap();
        assert_eq!(rejected["ok"], false);
        assert!(!service.poll(started.job_id, None).unwrap().stale);
        let edited: Value = serde_json::from_str(
            &host.engine_call(
                "print_intent_set_document",
                &serde_json::json!({
                    "document":document,"expected_model_json":model,
                })
                .to_string(),
            ),
        )
        .unwrap();
        assert_eq!(edited["ok"], true);
        let restored_reply: Value =
            serde_json::from_str(&host.project_load(&serde_json::to_string(&model).unwrap()))
                .unwrap();
        assert_eq!(restored_reply["ok"], true, "{restored_reply}");
        let restored = host
            .inner
            .lock()
            .unwrap()
            .active()
            .manager
            .export_project_model()
            .unwrap();
        assert_eq!(restored, model);
        let report = service
            .poll_owned(started.job_id, &source, &restored, &owner, false)
            .unwrap();
        assert!(report.stale);
        assert_eq!(report.identity, captured);
    }

    #[test]
    fn separate_hosts_with_same_tab_ids_cannot_cancel_or_read_another_owned_job() {
        let first = NativeEngineHost::new();
        let second = NativeEngineHost::new();
        for host in [&first, &second] {
            let response: Value =
                serde_json::from_str(&host.bind_project_session("same-verification-tab")).unwrap();
            assert_eq!(response["ok"], true);
        }
        let first_owner = first.inner.lock().unwrap().verification_owner_key();
        let second_owner = second.inner.lock().unwrap().verification_owner_key();
        assert_ne!(first_owner, second_owner);
        let source = "01234567-89ab-4cde-8123-456789abcdef";
        let model = serde_json::json!({"print_intent":{"source_document_id":source}}).to_string();
        let bytes = b"owned cancellation lifecycle fixture".to_vec();
        let identity = VerificationIdentity::from_owned_export(
            &bytes,
            &model,
            &serde_json::json!({}),
            &serde_json::json!({}),
            "a".repeat(64),
            source.into(),
            None,
        )
        .unwrap();
        let service = local_slicer_service();
        let started = service
            .start(
                bytes,
                identity,
                1,
                limo_cad_export::slicer_verification::LocalSlicerOptions {
                    executable: std::env::temp_dir().join("missing-bambu-private-owner-test.exe"),
                    timeout_seconds_per_plate: 1,
                },
                first_owner.clone(),
            )
            .unwrap();
        let payload = serde_json::json!({"job_id":started.job_id}).to_string();
        let denied: Value =
            serde_json::from_str(&second.local_slicer_status(&payload, true)).unwrap();
        assert_eq!(denied["ok"], false);
        assert!(denied["error"].as_str().unwrap().contains("owning engine"));
        assert!(service
            .poll_owned(started.job_id, source, &model, &second_owner, false)
            .is_err());
        // The owning host is now blank with no original document UUID. Cancellation remains
        // authorized by its private host+tab ownership and returns no old document details.
        let receipt: Value =
            serde_json::from_str(&first.local_slicer_status(&payload, true)).unwrap();
        assert_eq!(receipt["ok"], true);
        assert_eq!(
            receipt["value"],
            serde_json::json!({"job_id":started.job_id,"cancel_requested":true})
        );
        assert_eq!(
            service
                .poll(started.job_id, None)
                .unwrap()
                .identity
                .source_document_id,
            source
        );
    }
}
