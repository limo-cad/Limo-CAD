//! Typed composite boundary for native persistence and history adapters.
use super::*;
use limo_cad_project_file::ProjectSnapshot;

impl NativeEngineHost {
    /// JSON-command adapter for the shared bounded CBOR snapshot codec.
    /// Decode failures retain the same unchanged marker as preparation errors.
    pub fn project_load_snapshot_encoded(&self, payload: &str) -> String {
        let snapshot = serde_json::from_str::<String>(payload)
            .map_err(|error| error.to_string())
            .and_then(|encoded| {
                ProjectSnapshot::decode_base64(&encoded).map_err(|error| error.to_string())
            });
        match snapshot {
            Ok(snapshot) => self.project_load_snapshot(&snapshot),
            Err(error) => unchanged_project_load_error(error),
        }
    }
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

    /// Read a transient model projection under the engine lock without copying
    /// domain payloads. Model-scoped preconditions and inspection may use this;
    /// saving, replacement, history and replay require `project_snapshot`.
    pub fn project_model_projection(&self) -> Result<String, String> {
        let workspace = self.inner.lock().map_err(|_| "Engine lock poisoned")?;
        workspace
            .active()
            .manager
            .project_model_projection()
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
        assert_eq!(
            host.project_model_projection().unwrap(),
            candidate.model_json()
        );
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
    #[test]
    fn encoded_load_retains_domains_and_marks_decode_errors_unchanged() {
        let host = NativeEngineHost::new();
        let original = host.project_snapshot().unwrap();
        let snapshot = ProjectSnapshot::new(
            original.model_json().to_owned(),
            vec![SnapshotIntent {
                name: "intent/encoded.cbor".into(),
                payload: vec![0, 255, 7],
            }],
        )
        .unwrap();
        let payload = serde_json::to_string(&snapshot.encode_base64().unwrap()).unwrap();
        assert_eq!(
            result(host.project_load_snapshot_encoded(&payload))["ok"],
            true
        );
        assert_eq!(host.project_snapshot().unwrap(), snapshot);
        let revision = host.geometry_revision();
        for invalid in ["{}", "\"***\"", "\"gA==\""] {
            let rejected = result(host.project_load_snapshot_encoded(invalid));
            assert_eq!(rejected["data"]["project_load_state"], "unchanged");
            assert_eq!(host.project_snapshot().unwrap(), snapshot);
            assert_eq!(host.geometry_revision(), revision);
        }
    }
}
