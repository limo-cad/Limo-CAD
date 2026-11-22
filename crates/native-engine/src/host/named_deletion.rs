//! Deleting a feature on a kernel that names topology after the features
//! that made it.
//!
//! Open CASCADE numbers the faces and edges of a body, and the numbers move
//! when anything before them in the history changes. Deleting a finish there
//! needs proof that the finishes after it still mean the same edges, and the
//! proof only reaches as far as [`finish_deletion`](super::finish_deletion)
//! says: the last two features, a history with nothing rolled back, no
//! drawing, assembly or CAM data that might hold a number.
//!
//! On the Limo kernel a name stays with what it names. Any feature can be
//! deleted from anywhere in the history, and what comes after replays by the
//! names it already has. The one thing that can still go wrong is real: a
//! later feature was applied to something the deleted one made. That is
//! found by trying the deletion out on a copy of the project first; if a
//! feature that was sound would stop building, nothing is changed and the
//! reply says which.
use super::*;
use limo_cad_core::FeatureStatus;

impl NativeEngineHost {
    /// Whether the kernel of the active project names faces and edges in a
    /// way that survives edits of the history before them.
    pub(super) fn kernel_names_survive_edits(&self) -> bool {
        self.inner
            .lock()
            .expect("engine lock poisoned")
            .active()
            .kernel
            .uses_limo_kernel()
    }

    pub(super) fn delete_by_name(&self, payload: &str) -> String {
        // Not while a sketch is being edited or another project is about to
        // take this one's place: the same boundary the proof-based deletion
        // keeps.
        let boundary = self
            .inner
            .lock()
            .expect("engine lock poisoned")
            .active()
            .manager
            .validate_sketch_session_retention_boundary();
        if let Err(error) = boundary {
            return err_json(error.to_string());
        }
        if let Some(refusal) = self.deletion_breaks_a_later_feature(payload) {
            return err_json(refusal);
        }
        self.with_request(payload, |manager, request: DeleteFeatureRequest| {
            manager.prepare_delete_feature_with_stable_names(request)
        })
    }

    /// The deletion tried out on a copy of the project: what to say if a
    /// feature that builds now would no longer build. `None` when nothing
    /// breaks, and also when the trial itself cannot be made, which leaves
    /// the real deletion to report whatever is wrong.
    fn deletion_breaks_a_later_feature(&self, payload: &str) -> Option<String> {
        let request: DeleteFeatureRequest = serde_json::from_str(payload).ok()?;
        let snapshot = {
            let workspace = self.inner.lock().expect("engine lock poisoned");
            workspace.active().manager.export_project_snapshot().ok()?
        };
        let trial = || -> Result<Option<String>, String> {
            let mut manager = SketchManager::new();
            let mut kernel = OcctKernel::new().map_err(|e| e.to_string())?;
            let mut build =
                |manager: &mut SketchManager,
                 plan: limo_cad_solid::RecomputePlanDto|
                 -> Result<Vec<limo_cad_solid::KernelFeatureErrorDto>, String> {
                    let (scene, verified) = kernel
                        .recompute_with_supports(&plan, &manager.history_support_queries())
                        .map_err(|e| e.to_string())?;
                    let errors = scene.errors.clone();
                    manager
                        .commit_solid_with_verified_supports(
                            limo_cad_solid::CommitKernelRequest {
                                transaction_id: plan.transaction_id,
                                scene,
                            },
                            &verified,
                        )
                        .map_err(|e| e.to_string())?;
                    Ok(errors)
                };
            let plan = manager
                .prepare_load_project(snapshot.model_json().to_owned())
                .map_err(|e| e.to_string())?;
            build(&mut manager, plan)?;
            let sound: BTreeSet<limo_cad_core::FeatureId> = manager
                .document_dto()
                .features
                .iter()
                .filter(|f| matches!(f.status, FeatureStatus::Ok))
                .map(|f| f.id)
                .collect();
            let plan = manager
                .prepare_delete_feature_with_stable_names(request)
                .map_err(|e| e.to_string())?;
            let errors = build(&mut manager, plan)?;
            let document = manager.document_dto();
            let Some(broken) = document
                .features
                .iter()
                .find(|f| sound.contains(&f.id) && !matches!(f.status, FeatureStatus::Ok))
            else {
                return Ok(None);
            };
            let why = errors
                .iter()
                .find(|e| e.feature_id == broken.id)
                .map(|e| e.message.clone())
                .or_else(|| match &broken.status {
                    FeatureStatus::Error { message } => Some(message.clone()),
                    FeatureStatus::Ok => None,
                })
                .unwrap_or_default();
            Ok(Some(format!(
                "Cannot delete this feature: '{}' was applied to something it made, which has no survivor without it ({why}). Delete or re-apply '{}' first; nothing was changed",
                broken.name, broken.name
            )))
        };
        trial().ok().flatten()
    }
}
