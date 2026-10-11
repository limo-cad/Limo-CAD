//! Required immutable authored motion inputs and genuine selected native shapes.
use super::*;
use limo_cad_core::BodyId;
use limo_cad_sketch::{
    AssemblyDocumentDto, InterferenceCheckRequestDto, SampleMotionStudyRequestDto,
    SweptCollisionReportDto,
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
pub struct PreparedSweptCollisionReview {
    kernel: limo_cad_occt::DetachedInterferenceKernel,
    scene: Arc<SolidSceneDto>,
    request: SweptCollisionRequestDto,
    source: SourceProof,
}
pub struct FinishedSweptCollisionReview {
    report: SweptCollisionReportDto,
    source: SourceProof,
}
impl PreparedSweptCollisionReview {
    pub fn inspect(self) -> Result<FinishedSweptCollisionReview, String> {
        let report =
            self.kernel
                .inspect_swept(&self.source.assembly, &self.scene, &self.request)?;
        Ok(FinishedSweptCollisionReview {
            report,
            source: self.source,
        })
    }
}
impl FinishedSweptCollisionReview {
    pub fn report(&self) -> &SweptCollisionReportDto {
        &self.report
    }
    pub fn into_report(self) -> SweptCollisionReportDto {
        self.report
    }
}
impl NativeEngineHost {
    pub fn prepare_swept_collision_review(
        &self,
        payload: &str,
    ) -> Result<PreparedSweptCollisionReview, String> {
        let request: SweptCollisionRequestDto =
            serde_json::from_str(payload).map_err(|e| format!("bad request payload: {e}"))?;
        let workspace = self.inner.lock().map_err(|_| "engine lock poisoned")?;
        let inner = workspace.active();
        let document = inner.manager.assembly_document_ref();
        limo_cad_occt::validate_swept_source(document, &request)?;
        let scene = inner.manager.solid_scene_snapshot();
        // Preserve original first-sample validation before any native copy.
        let first = document.sample_motion_study(
            SampleMotionStudyRequestDto {
                study_id: request.study_id,
                time_seconds: 0.,
            },
            &scene,
        )?;
        if !first.solution.solved {
            return Err("Cannot inspect interference in an unsolved motion sample".into());
        }
        let query = InterferenceCheckRequestDto {
            occurrence_ids: vec![],
            clearance_threshold_mm: request.clearance_threshold_mm,
        };
        limo_cad_occt::validate_interference_source(
            &scene,
            &first.solution.instance_body_poses,
            &query,
        )?;
        limo_cad_sketch::broad_phase_interference_pairs(
            &scene,
            &first.solution.instance_body_poses,
            &query,
        )?;
        let selected: std::collections::BTreeSet<_> = first
            .solution
            .instance_body_poses
            .iter()
            .filter(|pose| pose.visible)
            .map(|pose| pose.body_id)
            .collect();
        let sources: std::collections::BTreeSet<_> = scene
            .bodies
            .iter()
            .map(|body| body.id)
            .chain(selected.iter().copied())
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
        Ok(PreparedSweptCollisionReview {
            kernel,
            scene,
            request,
            source,
        })
    }
    pub fn validate_swept_collision_review(
        &self,
        finished: &FinishedSweptCollisionReview,
    ) -> Result<(), String> {
        let workspace = self.inner.lock().map_err(|_| "engine lock poisoned")?;
        let source = &finished.source;
        if workspace.verification_owner_key() != source.owner {
            return Err("The owning swept document was replaced during inspection".into());
        }
        let inner = workspace.active();
        // Motion laws/study/joint/placement inputs can change without a render stamp.
        if inner.manager.assembly_document_ref() != source.assembly.as_ref() {
            return Err("The swept motion inputs changed during inspection".into());
        }
        if inner.geometry_revision != source.geometry_revision
            || inner.viewport_revision != source.metadata_revision
            || !std::ptr::eq(
                source.scene.as_ptr(),
                Arc::as_ptr(&inner.manager.solid_scene_snapshot()),
            )
        {
            return Err("The swept source changed during inspection".into());
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
                    "The native geometry changed or was replaced during swept inspection".into(),
                );
            }
        }
        Ok(())
    }
}
#[cfg(test)]
#[path = "swept_snapshot_tests.rs"]
mod tests;
