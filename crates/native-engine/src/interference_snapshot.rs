//! Static exact interference from coherent selected native copies.
use super::*;
use limo_cad_core::BodyId;
use limo_cad_sketch::{broad_phase_interference_pairs, InterferenceReportDto};
use std::{collections::BTreeMap, sync::Weak};

struct SourceProof {
    owner: String,
    scene: Weak<SolidSceneDto>,
    geometry_revision: u64,
    metadata_revision: u64,
    poses: Vec<InstanceBodyPoseDto>,
    identities: BTreeMap<BodyId, Option<limo_cad_occt::KernelBodyGeometryIdentity>>,
}
/// Owns only selected independent B-reps and immutable inspection inputs.
pub struct PreparedInterferenceReview {
    kernel: limo_cad_occt::DetachedInterferenceKernel,
    scene: Arc<SolidSceneDto>,
    request: InterferenceCheckRequestDto,
    source: SourceProof,
}
pub struct FinishedInterferenceReview {
    report: InterferenceReportDto,
    source: SourceProof,
}
impl PreparedInterferenceReview {
    pub fn inspect(self) -> Result<FinishedInterferenceReview, String> {
        let report = self
            .kernel
            .inspect(&self.scene, &self.source.poses, &self.request)?;
        Ok(FinishedInterferenceReview {
            report,
            source: self.source,
        })
    }
}
impl FinishedInterferenceReview {
    pub fn report(&self) -> &InterferenceReportDto {
        &self.report
    }
    pub fn into_report(self) -> InterferenceReportDto {
        self.report
    }
}
impl NativeEngineHost {
    pub fn prepare_interference_review(
        &self,
        payload: &str,
    ) -> Result<PreparedInterferenceReview, String> {
        let request: InterferenceCheckRequestDto =
            serde_json::from_str(payload).map_err(|e| format!("bad request payload: {e}"))?;
        let workspace = self.inner.lock().map_err(|_| "engine lock poisoned")?;
        let inner = workspace.active();
        let solution = inner.manager.assembly_solution();
        if !solution.solved {
            return Err("Cannot inspect interference in an unsolved assembly".into());
        }
        let scene = inner.manager.solid_scene_snapshot();
        limo_cad_occt::validate_interference_source(
            &scene,
            &solution.instance_body_poses,
            &request,
        )?;
        // Preserve every repeated occurrence pose. Only candidate-pair geometry
        // was accessed by the original exact algorithm; empty reports stay valid.
        let mut identities = BTreeMap::new();
        let pairs =
            broad_phase_interference_pairs(&scene, &solution.instance_body_poses, &request)?;
        let ids: std::collections::BTreeSet<_> = pairs
            .iter()
            .flat_map(|&(a, b)| {
                [
                    solution.instance_body_poses[a].body_id,
                    solution.instance_body_poses[b].body_id,
                ]
            })
            .collect();
        let source_ids: std::collections::BTreeSet<_> = solution
            .instance_body_poses
            .iter()
            .filter(|pose| {
                pose.visible
                    && (request.occurrence_ids.is_empty()
                        || request.occurrence_ids.contains(&pose.occurrence_id))
            })
            .map(|pose| pose.body_id)
            .collect();
        if !source_ids.is_empty() {
            let current = inner
                .kernel
                .body_geometry_identities()
                .map_err(|e| e.to_string())?;
            for id in source_ids {
                // A missing non-candidate shape did not enter native exact work
                // previously. Retain its absence without adding an admission gate.
                identities.insert(id, current.get(&id).copied());
            }
        }
        let kernel = inner
            .kernel
            .detached_interference_snapshot(&ids.into_iter().collect::<Vec<_>>())
            .map_err(|e| e.to_string())?;
        let source = SourceProof {
            owner: workspace.verification_owner_key(),
            scene: Arc::downgrade(&scene),
            geometry_revision: inner.geometry_revision,
            metadata_revision: inner.viewport_revision,
            poses: solution.instance_body_poses,
            identities,
        };
        Ok(PreparedInterferenceReview {
            kernel,
            scene,
            request,
            source,
        })
    }
    pub fn validate_interference_review(
        &self,
        finished: &FinishedInterferenceReview,
    ) -> Result<(), String> {
        let workspace = self.inner.lock().map_err(|_| "engine lock poisoned")?;
        let source = &finished.source;
        if workspace.verification_owner_key() != source.owner {
            return Err("The owning interference document was replaced during inspection".into());
        }
        let inner = workspace.active();
        let solution = inner.manager.assembly_solution();
        // Placement is authoritative independently of geometry/render revisions.
        if !solution.solved || solution.instance_body_poses != source.poses {
            return Err("The interference occurrence poses changed during inspection".into());
        }
        if inner.geometry_revision != source.geometry_revision
            || inner.viewport_revision != source.metadata_revision
            || !std::ptr::eq(
                source.scene.as_ptr(),
                Arc::as_ptr(&inner.manager.solid_scene_snapshot()),
            )
        {
            return Err("The interference source changed during inspection".into());
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
                    "The native geometry changed or was replaced during interference inspection"
                        .into(),
                );
            }
        }
        Ok(())
    }
}
#[cfg(test)]
#[path = "interference_snapshot_tests.rs"]
mod tests;
