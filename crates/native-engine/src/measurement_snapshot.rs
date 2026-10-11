//! Exact measurement runs on selected independent native shapes outside the document lock.
use super::*;
use limo_cad_core::BodyId;
use limo_cad_occt::measurement::{
    MeasurementReport, MeasurementRequest, ResolvedMeasurementSelection,
};
use std::{collections::BTreeMap, sync::Weak};

struct SourceProof {
    owner: String,
    scene: Weak<SolidSceneDto>,
    geometry_revision: u64,
    metadata_revision: u64,
    bodies: Vec<BodyPoseDto>,
    instances: Vec<InstanceBodyPoseDto>,
    identities: BTreeMap<BodyId, limo_cad_occt::KernelBodyGeometryIdentity>,
}

pub struct PreparedMeasurementReview {
    kernel: limo_cad_occt::DetachedMeasurementKernel,
    selections: Vec<ResolvedMeasurementSelection>,
    source: SourceProof,
}
pub struct FinishedMeasurementReview {
    report: MeasurementReport,
    source: SourceProof,
}
impl PreparedMeasurementReview {
    pub fn inspect(self) -> Result<FinishedMeasurementReview, String> {
        Ok(FinishedMeasurementReview {
            report: self
                .kernel
                .inspect(&self.selections)
                .map_err(|e| e.to_string())?,
            source: self.source,
        })
    }
}
impl FinishedMeasurementReview {
    pub fn report(&self) -> &MeasurementReport {
        &self.report
    }
    pub fn into_report(self) -> MeasurementReport {
        self.report
    }
}
impl NativeEngineHost {
    pub fn prepare_measurement_review(
        &self,
        request: &MeasurementRequest,
    ) -> Result<PreparedMeasurementReview, String> {
        let workspace = self.inner.lock().map_err(|_| "engine lock poisoned")?;
        let inner = workspace.active();
        let scene = inner.manager.solid_scene_snapshot();
        let placement = inner
            .manager
            .presentation_solution()
            .map_err(|e| e.to_string())?;
        if !placement.solved {
            return Err("Cannot measure an unsolved assembly".into());
        }
        let selections = limo_cad_occt::measurement::resolve(
            &scene,
            &placement.body_poses,
            &placement.instance_body_poses,
            request,
        )?;
        let current = inner
            .kernel
            .body_geometry_identities()
            .map_err(|e| e.to_string())?;
        let mut identities = BTreeMap::new();
        for selection in &selections {
            identities.insert(
                selection.body_id,
                *current
                    .get(&selection.body_id)
                    .ok_or("The measurement body has no native geometry")?,
            );
        }
        let ids: Vec<_> = identities.keys().copied().collect();
        let kernel = inner
            .kernel
            .detached_measurement_snapshot(&ids)
            .map_err(|e| e.to_string())?;
        Ok(PreparedMeasurementReview {
            kernel,
            selections,
            source: SourceProof {
                owner: workspace.verification_owner_key(),
                scene: Arc::downgrade(&scene),
                geometry_revision: inner.geometry_revision,
                metadata_revision: inner.viewport_revision,
                bodies: placement.body_poses,
                instances: placement.instance_body_poses,
                identities,
            },
        })
    }

    pub fn validate_measurement_review(
        &self,
        finished: &FinishedMeasurementReview,
    ) -> Result<(), String> {
        let workspace = self.inner.lock().map_err(|_| "engine lock poisoned")?;
        let source = &finished.source;
        if workspace.verification_owner_key() != source.owner {
            return Err("The measurement document was replaced during inspection".into());
        }
        let inner = workspace.active();
        let placement = inner
            .manager
            .presentation_solution()
            .map_err(|e| e.to_string())?;
        if !placement.solved
            || placement.body_poses != source.bodies
            || placement.instance_body_poses != source.instances
            || inner.geometry_revision != source.geometry_revision
            || inner.viewport_revision != source.metadata_revision
            || !std::ptr::eq(
                source.scene.as_ptr(),
                Arc::as_ptr(&inner.manager.solid_scene_snapshot()),
            )
        {
            return Err("The measurement geometry or placement changed during inspection".into());
        }
        let current = inner
            .kernel
            .body_geometry_identities()
            .map_err(|e| e.to_string())?;
        if source
            .identities
            .iter()
            .any(|(id, identity)| current.get(id) != Some(identity))
        {
            return Err("The native measurement geometry was replaced during inspection".into());
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "measurement_snapshot_tests.rs"]
mod tests;
