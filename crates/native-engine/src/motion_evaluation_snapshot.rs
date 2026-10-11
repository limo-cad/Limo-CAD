//! Required immutable authored motion inputs and genuine selected native shapes.
use super::*;
use limo_cad_core::BodyId;
use limo_cad_sketch::{
    AssemblyDocumentDto, EvaluateMotionStudyRequestDto, MotionStudyEvaluationDto,
    SampleMotionStudyRequestDto,
};
use std::{collections::BTreeMap, sync::Weak};
struct SourceProof {
    owner: String,
    scene: Weak<SolidSceneDto>,
    geometry_revision: u64,
    metadata_revision: u64,
    // Required sampler intent, not a SketchManager or model/history copy.
    // Existing sampling validates the authored assembly and clones it one frame
    // at a time; retain its exact input rather than weaken those checks by pruning.
    assembly: Arc<AssemblyDocumentDto>,
    identities: BTreeMap<BodyId, Option<limo_cad_occt::KernelBodyGeometryIdentity>>,
}
pub struct PreparedMotionEvaluation {
    kernel: limo_cad_occt::DetachedInterferenceKernel,
    scene: Arc<SolidSceneDto>,
    request: EvaluateMotionStudyRequestDto,
    source: SourceProof,
}
pub struct FinishedMotionEvaluation {
    report: MotionStudyEvaluationDto,
    source: SourceProof,
}
impl PreparedMotionEvaluation {
    pub fn evaluate(self) -> Result<FinishedMotionEvaluation, String> {
        let report =
            self.kernel
                .evaluate_motion(&self.source.assembly, &self.scene, &self.request)?;
        Ok(FinishedMotionEvaluation {
            report,
            source: self.source,
        })
    }
}
impl FinishedMotionEvaluation {
    pub fn report(&self) -> &MotionStudyEvaluationDto {
        &self.report
    }
    pub fn into_report(self) -> MotionStudyEvaluationDto {
        self.report
    }
}
impl NativeEngineHost {
    pub fn prepare_motion_evaluation(
        &self,
        payload: &str,
    ) -> Result<PreparedMotionEvaluation, String> {
        let request: EvaluateMotionStudyRequestDto =
            serde_json::from_str(payload).map_err(|e| format!("bad request payload: {e}"))?;
        let workspace = self.inner.lock().map_err(|_| "engine lock poisoned")?;
        let inner = workspace.active();
        let document = inner.manager.assembly_document_ref();
        let scene = inner.manager.solid_scene_snapshot();
        // The original evaluator samples the requested final time first.
        // Preserve that validation priority before preparing any native copies.
        let first = document.sample_motion_study(
            SampleMotionStudyRequestDto {
                study_id: request.study_id,
                time_seconds: request.time_seconds,
            },
            &scene,
        )?;
        if !first.solution.solved {
            return Err("Cannot preview an unsolved motion sample".into());
        }
        // Only enabled contact pairs can perform exact work. Their placed
        // bodies may be hidden; contact lookup deliberately does not filter them.
        let selected: std::collections::BTreeSet<_> = document
            .contact_sets
            .iter()
            .filter(|contact| contact.enabled)
            .flat_map(|contact| [contact.body_a, contact.body_b])
            .collect();
        // Broadphase/sample/no-contact outcomes still depend on the original
        // scene and every sampled/contact source, including currently absent shapes.
        let sources: std::collections::BTreeSet<_> = scene
            .bodies
            .iter()
            .map(|body| body.id)
            .chain(
                first
                    .solution
                    .instance_body_poses
                    .iter()
                    .map(|pose| pose.body_id),
            )
            .chain(
                document
                    .contact_sets
                    .iter()
                    .flat_map(|contact| [contact.body_a, contact.body_b]),
            )
            .collect();
        let mut identities = BTreeMap::new();
        let mut copies = Vec::new();
        if !sources.is_empty() {
            let current = inner
                .kernel
                .body_geometry_identities()
                .map_err(|e| e.to_string())?;
            for id in sources {
                identities.insert(id, current.get(&id).copied());
            }
            // A missing shape that never enters an exact pair remained valid.
            // Keep it absent: unchanged exact work errors only if it is needed.
            copies.extend(selected.into_iter().filter(|id| current.contains_key(id)));
        }
        let kernel = inner
            .kernel
            .detached_interference_snapshot(&copies)
            .map_err(|e| e.to_string())?;
        let source = SourceProof {
            owner: workspace.verification_owner_key(),
            scene: Arc::downgrade(&scene),
            geometry_revision: inner.geometry_revision,
            metadata_revision: inner.viewport_revision,
            assembly: Arc::new(document.clone()),
            identities,
        };
        Ok(PreparedMotionEvaluation {
            kernel,
            scene,
            request,
            source,
        })
    }
    pub fn validate_motion_evaluation(
        &self,
        finished: &FinishedMotionEvaluation,
    ) -> Result<(), String> {
        let workspace = self.inner.lock().map_err(|_| "engine lock poisoned")?;
        let source = &finished.source;
        if workspace.verification_owner_key() != source.owner {
            return Err("The owning motion document was replaced during inspection".into());
        }
        let inner = workspace.active();
        // Motion laws/study/joint/placement inputs can change without a render stamp.
        if inner.manager.assembly_document_ref() != source.assembly.as_ref() {
            return Err("The motion evaluation inputs changed during inspection".into());
        }
        if inner.geometry_revision != source.geometry_revision
            || inner.viewport_revision != source.metadata_revision
            || !std::ptr::eq(
                source.scene.as_ptr(),
                Arc::as_ptr(&inner.manager.solid_scene_snapshot()),
            )
        {
            return Err("The motion evaluation source changed during inspection".into());
        }
        if !source.identities.is_empty() {
            let current = inner
                .kernel
                .body_geometry_identities()
                .map_err(|e| e.to_string())?;
            if source
                .identities
                .iter()
                .any(|(id, identity)| current.get(id).copied() != *identity)
            {
                return Err(
                    "The native geometry changed or was replaced during motion evaluation".into(),
                );
            }
        }
        Ok(())
    }
}
#[cfg(test)]
#[path = "motion_evaluation_snapshot_tests.rs"]
mod tests;
