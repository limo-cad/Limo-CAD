//! Typed composite boundary for native persistence and history adapters.
use super::*;
use limo_cad_project_file::ProjectSnapshot;

impl NativeEngineHost {
    /// Capture model and domain intent under one engine lock. Callers retain
    /// their document-owner/revision fence through capture and save preparation.
    /// This does not capture active sketch editing state or derived results.
    pub fn project_snapshot(&self) -> Result<ProjectSnapshot, String> {
        let workspace = self.inner.lock().map_err(|_| "Engine lock poisoned")?;
        workspace
            .active()
            .manager
            .export_project_snapshot()
            .map_err(|error| error.to_string())
    }

    /// Load paired model/domain intent through the same kernel transaction as
    /// legacy project loading. A preparation rejection carries the existing
    /// unchanged marker; callers must preserve normal replacement retirement
    /// semantics for later kernel errors and successful loads.
    ///
    /// The desktop caller must hold its owner/revision fence. This method does
    /// not publish a revision, change file paths, or manage desktop history.
    pub fn project_load_snapshot(&self, snapshot: &ProjectSnapshot) -> String {
        self.execute_with_prepare_error(
            |manager| manager.prepare_load_project_snapshot(snapshot),
            unchanged_project_load_error,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use limo_cad_project_file::SnapshotIntent;

    fn result(raw: String) -> serde_json::Value {
        serde_json::from_str(&raw).unwrap()
    }

    #[test]
    fn snapshot_load_captures_paired_model_and_opaque_intent() {
        let host = NativeEngineHost::new();
        let donor = NativeEngineHost::new();
        assert_eq!(
            result(donor.engine_call("document_set_name", "\"Snapshot donor\""))["ok"],
            true
        );
        let model = donor.project_snapshot().unwrap();
        let candidate = ProjectSnapshot::new(
            model.model_json().to_owned(),
            vec![SnapshotIntent {
                name: "intent/future-study.cbor".into(),
                payload: vec![1, 0, 255],
            }],
        )
        .unwrap();
        assert_eq!(result(host.project_load_snapshot(&candidate))["ok"], true);
        assert_eq!(host.project_snapshot().unwrap(), candidate);
        // An incomplete legacy export must still refuse to discard domain data.
        assert_eq!(
            result(host.engine_call("project_export_model", ""))["ok"],
            false
        );
    }

    #[test]
    fn rejected_snapshot_load_marks_unchanged_and_preserves_prior_intent() {
        let host = NativeEngineHost::new();
        let initial = host.project_snapshot().unwrap();
        let prior = ProjectSnapshot::new(
            initial.model_json().to_owned(),
            vec![SnapshotIntent {
                name: "intent/retained.cbor".into(),
                payload: vec![42],
            }],
        )
        .unwrap();
        assert_eq!(result(host.project_load_snapshot(&prior))["ok"], true);
        let revision = host.geometry_revision();
        let invalid = ProjectSnapshot::new(
            prior.model_json().to_owned(),
            vec![SnapshotIntent {
                name: limo_cad_fem::THERMAL_STUDY_ENTRY.into(),
                payload: vec![0xff],
            }],
        )
        .unwrap();
        let rejected = result(host.project_load_snapshot(&invalid));
        assert_eq!(rejected["ok"], false);
        assert_eq!(rejected["data"]["project_load_state"], "unchanged");
        assert_eq!(host.project_snapshot().unwrap(), prior);
        assert_eq!(host.geometry_revision(), revision);
        // Valid retry replaces the entire paired state, including intent removal.
        assert_eq!(result(host.project_load_snapshot(&initial))["ok"], true);
        assert_eq!(host.project_snapshot().unwrap(), initial);
    }
}
