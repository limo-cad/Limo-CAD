//! Detached project replay through the existing interpreter and snapshot command.
use super::{
    frames::Frames, PreparedProjectPreview, ProjectPreviewCoverage, ProjectPreviewResult,
    ProjectPreviewStage,
};
use limo_cad_project_file::ProjectArchive;
use limo_cad_script::{ProjectReplayHost, RunProgress};
use serde_json::{json, Value};

/// Execute on a serialized preview preparation worker. The caller checks its
/// cancellation/deadline in `check`; it is called before engine creation, around
/// baseline loading, around each operation, and before returning the report.
/// Synchronous kernel calls cannot be interrupted by these cooperative checks.
/// Generated baseline/final frames carry solid geometry and assembly placement.
/// Authored exports remain in the separate report; domain visualization is not
/// implied by successful script execution. The native adapter owns localization
/// of stage labels and validates renderer-specific geometry/instance budgets.
pub fn run_prepared_project_preview(
    prepared: PreparedProjectPreview,
    mut check: impl FnMut() -> Result<(), String>,
) -> Result<ProjectPreviewResult, String> {
    check()?;
    let (script, archive, options) = prepared.into_parts();
    super::validate(script.script())?;
    check()?;
    let mut server = crate::CadServer::new()?;
    check()?;
    server.script_running = true;
    let mut host = DetachedHost {
        server: &mut server,
        archive: Some(archive),
        baseline_attempted: false,
        baseline_loaded: false,
        frames: Frames::default(),
        unrendered_domain_intents: std::collections::BTreeSet::new(),
        check: &mut check,
    };
    let report = script.run(&mut host, options)?;
    host.ensure_detached()?;
    (host.check)()?;
    host.capture(ProjectPreviewStage::Final)?;
    (host.check)()?;
    // This server is private to this request. No native finish/presentation
    // acknowledgement, session refresh, file write or publication is required.
    Ok(ProjectPreviewResult {
        report,
        frames: host.frames.values,
        coverage: ProjectPreviewCoverage::SolidGeometryAndAssembly,
        unrendered_domain_intents: host.unrendered_domain_intents.into_iter().collect(),
    })
}

struct DetachedHost<'a, F> {
    server: &'a mut crate::CadServer,
    archive: Option<ProjectArchive>,
    baseline_attempted: bool,
    baseline_loaded: bool,
    frames: Frames,
    unrendered_domain_intents: std::collections::BTreeSet<String>,
    check: &'a mut F,
}
impl<F> DetachedHost<'_, F> {
    fn ensure_detached(&self) -> Result<(), String> {
        if self.server.attached_document_id.is_some()
            || self.server.desktop_binding.is_some()
            || self.server.loaded_publication.is_some()
            || self.server.failed_attachment.is_some()
        {
            return Err("Preview host must remain detached from desktop documents".into());
        }
        Ok(())
    }
}
impl<F: FnMut() -> Result<(), String>> DetachedHost<'_, F> {
    fn capture(&mut self, stage: ProjectPreviewStage) -> Result<(), String> {
        self.ensure_detached()?;
        (self.check)()?;
        let assembly = self.server.manager.assembly_solution();
        (self.check)()?;
        self.frames
            .capture(self.server.manager.solid_scene_ref(), assembly, stage)?;
        self.unrendered_domain_intents.extend(
            self.server
                .manager
                .project_intents()
                .iter()
                .map(|intent| intent.name.clone()),
        );
        (self.check)()?;
        Ok(())
    }
}
impl<F: FnMut() -> Result<(), String>> ProjectReplayHost for DetachedHost<'_, F> {
    fn validate_call(&mut self, group: &str, operation: &str) -> Result<(), String> {
        self.ensure_detached()?;
        (self.check)()?;
        crate::interface::validate_script_call(group, operation)?;
        if !super::supported(operation) {
            return Err(format!(
                "Operation {operation} is not available in isolated preview"
            ));
        }
        Ok(())
    }

    fn load_project_baseline(&mut self) -> Result<(), String> {
        self.ensure_detached()?;
        (self.check)()?;
        if self.baseline_attempted {
            return Err("Preview baseline was already submitted".into());
        }
        self.baseline_attempted = true;
        let snapshot = self
            .archive
            .as_ref()
            .ok_or("Preview archive is unavailable")?
            .snapshot()
            .map_err(|error| error.to_string())?;
        let encoded = snapshot
            .encode_base64()
            .map_err(|error| error.to_string())?;
        (self.check)()?;
        // Reuse the same domain validation, kernel preparation and paired commit
        // as ordinary snapshot loading. This internal baseline operation is not
        // admitted as an authored preview call. Keep the full archive alive for
        // the request, including ancillary and unknown intent entries.
        self.server.call_tool(
            "cad_load_project_snapshot",
            json!({"snapshot_cbor_base64":encoded}),
        )?;
        self.ensure_detached()?;
        (self.check)()?;
        if !self.server.manager.solid_scene_ref().errors.is_empty() {
            return Err("Preview baseline contains failed geometry".into());
        }
        self.capture(ProjectPreviewStage::Baseline)?;
        self.baseline_loaded = true;
        Ok(())
    }

    fn call(
        &mut self,
        name: &str,
        arguments: Value,
        progress: RunProgress,
    ) -> Result<Value, String> {
        self.ensure_detached()?;
        (self.check)()?;
        if !self.baseline_loaded {
            return Err("Preview baseline has not been loaded".into());
        }
        if name != "cad_interface" || arguments["action"] != "execute" {
            return Err("Preview supports only admitted interface operations".into());
        }
        self.validate_call(
            arguments["group"]
                .as_str()
                .ok_or("Preview group is missing")?,
            arguments["operation"]
                .as_str()
                .ok_or("Preview operation is missing")?,
        )?;
        let result = self
            .server
            .dispatch_script_call(name, arguments, progress)?;
        self.ensure_detached()?;
        (self.check)()?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use limo_cad_project_file::{ReplayPackage, SaveMetadata};
    fn archive() -> ProjectArchive {
        ProjectArchive::new(
            limo_cad_sketch::SketchManager::new()
                .export_project_model()
                .unwrap(),
            SaveMetadata {
                application_version: "test",
                saved_at: "2026-10-17T00:00:00Z",
            },
        )
        .unwrap()
    }
    fn prepared() -> PreparedProjectPreview {
        let source =
            json!({"version":2,"name":"Detached baseline","starting_state":"project","steps":[]})
                .to_string();
        let package = ReplayPackage::new(source.clone(), archive().encode().unwrap()).unwrap();
        super::super::prepare_project_preview_source(&package, &source).unwrap()
    }
    #[test]
    fn cancellation_and_empty_baseline_use_shared_project_interpreter() {
        assert_eq!(
            run_prepared_project_preview(prepared(), || Err("cancelled".into())).unwrap_err(),
            "cancelled"
        );
        let report = run_prepared_project_preview(prepared(), || Ok(())).unwrap();
        assert_eq!(report.report["project_baseline_loaded"], true);
        assert_eq!(report.report["steps_completed"], 0);
        assert_eq!(report.frames.len(), 1);
        assert_eq!(report.frames[0].stage, ProjectPreviewStage::Baseline);
        assert_eq!(
            report.coverage,
            ProjectPreviewCoverage::SolidGeometryAndAssembly
        );
    }
    #[test]
    fn detached_baseline_preserves_unknown_intent_and_rejects_resubmission() {
        let mut archive = archive();
        archive
            .set_intent("intent/future.cbor", Some(vec![0, 255, 7]))
            .unwrap();
        let mut server = crate::CadServer::new().unwrap();
        let mut check = || Ok(());
        let mut host = DetachedHost {
            server: &mut server,
            archive: Some(archive),
            baseline_attempted: false,
            baseline_loaded: false,
            frames: Frames::default(),
            unrendered_domain_intents: std::collections::BTreeSet::new(),
            check: &mut check,
        };
        host.load_project_baseline().unwrap();
        assert_eq!(
            host.server
                .manager
                .export_project_snapshot()
                .unwrap()
                .intents()[0]
                .payload,
            [0, 255, 7]
        );
        assert!(host
            .load_project_baseline()
            .unwrap_err()
            .contains("already submitted"));
        host.ensure_detached().unwrap();
        assert!(host
            .validate_call(
                crate::interface::group_for("cad_attach").unwrap(),
                "cad_attach"
            )
            .is_err());
    }
    #[test]
    fn invalid_known_domain_does_not_replace_detached_model() {
        let mut archive = archive();
        archive
            .set_intent("intent/thermal-studies.cbor", Some(vec![255]))
            .unwrap();
        let mut server = crate::CadServer::new().unwrap();
        let before = server.manager.export_project_snapshot().unwrap();
        let mut check = || Ok(());
        let mut host = DetachedHost {
            server: &mut server,
            archive: Some(archive),
            baseline_attempted: false,
            baseline_loaded: false,
            frames: Frames::default(),
            unrendered_domain_intents: std::collections::BTreeSet::new(),
            check: &mut check,
        };
        assert!(host.load_project_baseline().is_err());
        assert!(!host.baseline_loaded);
        assert_eq!(
            host.server.manager.export_project_snapshot().unwrap(),
            before
        );
        assert!(host
            .load_project_baseline()
            .unwrap_err()
            .contains("already submitted"));
    }
    #[test]
    fn cancellation_after_baseline_prevents_later_commands() {
        let cancelled = std::cell::Cell::new(false);
        let mut check = || {
            if cancelled.get() {
                Err("cancelled".into())
            } else {
                Ok(())
            }
        };
        let mut server = crate::CadServer::new().unwrap();
        let mut host = DetachedHost {
            server: &mut server,
            archive: Some(archive()),
            baseline_attempted: false,
            baseline_loaded: false,
            frames: Frames::default(),
            unrendered_domain_intents: std::collections::BTreeSet::new(),
            check: &mut check,
        };
        host.load_project_baseline().unwrap();
        let before = host.server.manager.export_project_snapshot().unwrap();
        cancelled.set(true);
        let result = host.call("cad_interface", json!({"action":"execute","group":crate::interface::group_for("sketch_begin").unwrap(),"operation":"sketch_begin","arguments":{}}), RunProgress { steps_completed:0, step_count:1 });
        assert_eq!(result.unwrap_err(), "cancelled");
        assert_eq!(
            host.server.manager.export_project_snapshot().unwrap(),
            before
        );
        host.ensure_detached().unwrap();
    }
    #[test]
    fn generated_frames_do_not_overwrite_authored_exports_or_hide_domain_scope() {
        let source = json!({"version":2,"name":"Separate exports","starting_state":"project",
            "steps":[],"exports":{"preview_frames":"authored marker"}})
        .to_string();
        let mut archive = archive();
        archive
            .set_intent("intent/future.cbor", Some(vec![0, 255, 7]))
            .unwrap();
        let bytes = archive.encode().unwrap();
        let package = ReplayPackage::new(source.clone(), bytes.clone()).unwrap();
        let prepared = super::super::prepare_project_preview_source(&package, &source).unwrap();
        let result = run_prepared_project_preview(prepared, || Ok(())).unwrap();
        assert_eq!(
            result.report["exports"]["preview_frames"],
            "authored marker"
        );
        assert_eq!(result.frames.len(), 1);
        assert_eq!(result.unrendered_domain_intents, vec!["intent/future.cbor"]);
        assert_eq!(
            result.coverage,
            ProjectPreviewCoverage::SolidGeometryAndAssembly
        );
        assert_eq!(package.project_bytes(), bytes);
    }
}
