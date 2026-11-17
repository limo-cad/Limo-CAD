use super::*;
use limo_cad_project_file::{ProjectSnapshot, SnapshotIntent};

fn validate_domains(snapshot: &ProjectSnapshot) -> Result<(), SessionError> {
    for intent in snapshot.intents() {
        if intent.name == limo_cad_fem::THERMAL_STUDY_ENTRY {
            limo_cad_fem::ThermalStudyDocument::decode_cbor(&intent.payload)
                .map_err(SessionError::Solid)?;
        }
    }
    Ok(())
}
impl SketchManager {
    /// Read the model component for transient geometry/metadata inspection and
    /// model-scoped preconditions, without cloning unrelated domain payloads.
    /// This is not a persistence, replacement or replay representation: those
    /// callers must use the complete project snapshot to retain domain intent.
    /// Active sketch editing and pending geometry transactions remain blocked.
    pub fn project_model_projection(&self) -> Result<String, SessionError> {
        self.solids
            .ensure_metadata_editable()
            .map_err(|e| SessionError::Solid(e.to_string()))?;
        self.export_model_content()
    }

    /// Complete authoritative intent, suitable for paired history/publication.
    /// Kernel geometry and computed analysis results remain derived.
    pub fn export_project_snapshot(&self) -> Result<ProjectSnapshot, SessionError> {
        ProjectSnapshot::new(
            self.project_model_projection()?,
            self.project_intents.clone(),
        )
        .map_err(|e| SessionError::Solid(e.to_string()))
    }
    pub fn project_intents(&self) -> &[SnapshotIntent] {
        &self.project_intents
    }

    /// Replace domain intent atomically after envelope and known-domain checks.
    /// Native callers must use the same owner/revision fence as model edits.
    /// Unknown domains are retained by snapshots; callers must include them in
    /// replacements unless the user explicitly requested their removal.
    pub fn replace_project_intents(
        &mut self,
        intents: Vec<SnapshotIntent>,
    ) -> Result<bool, SessionError> {
        if self.pending_project.is_some() {
            return Err(SessionError::Solid(
                "A project replacement is pending".into(),
            ));
        }
        self.solids
            .ensure_metadata_editable()
            .map_err(|e| SessionError::Solid(e.to_string()))?;
        let candidate = ProjectSnapshot::new(self.export_model_content()?, intents)
            .map_err(|e| SessionError::Solid(e.to_string()))?;
        validate_domains(&candidate)?;
        if candidate.intents() == self.project_intents {
            return Ok(false);
        }
        self.project_intents = candidate.intents().to_vec();
        Ok(true)
    }
    /// Validate all known domain payloads before staging geometry. Adopt the
    /// paired candidate only when the existing kernel transaction commits.
    /// Cancellation, stale commit or failure leave current intent untouched.
    pub fn prepare_load_project_snapshot(
        &mut self,
        snapshot: &ProjectSnapshot,
    ) -> Result<RecomputePlanDto, SessionError> {
        validate_domains(snapshot)?;
        let plan = self.prepare_load_project_ref(snapshot.model_json())?;
        let candidate = self
            .pending_project
            .as_mut()
            .expect("successful project preparation creates a candidate");
        candidate.manager.project_intents = snapshot.intents().to_vec();
        Ok(plan)
    }
}
