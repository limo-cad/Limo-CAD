//! Selected genuine B-reps for read-only section work outside live guards.
use super::*;
use limo_cad_core::BodyId;
use limo_cad_occt::section_review::{SectionReview, SectionReviewRequest};
use std::sync::Weak;

struct SourceProof {
    owner: String,
    scene: Weak<SolidSceneDto>,
    geometry_revision: u64,
    metadata_revision: u64,
    body: BodyId,
    identity: limo_cad_occt::KernelBodyGeometryIdentity,
}

pub struct PreparedSectionReview {
    kernel: limo_cad_occt::DetachedSectionKernel,
    request: SectionReviewRequest,
    source: SourceProof,
}

pub struct FinishedSectionReview {
    report: SectionReview,
    source: SourceProof,
}
impl PreparedSectionReview {
    pub fn inspect(self) -> Result<FinishedSectionReview, String> {
        Ok(FinishedSectionReview {
            report: self
                .kernel
                .inspect(&self.request)
                .map_err(|e| e.to_string())?,
            source: self.source,
        })
    }
}
impl FinishedSectionReview {
    pub fn report(&self) -> &SectionReview {
        &self.report
    }
    /// CPU presentation may take the cutaway without discarding source proof.
    pub fn report_mut(&mut self) -> &mut SectionReview {
        &mut self.report
    }
    pub fn into_report(self) -> SectionReview {
        self.report
    }
}
impl NativeEngineHost {
    pub fn prepare_section_review(&self, payload: &str) -> Result<PreparedSectionReview, String> {
        // Preserve the synchronous parse -> lock -> request/scene validation order.
        let request: SectionReviewRequest =
            serde_json::from_str(payload).map_err(|e| format!("bad request payload: {e}"))?;
        let workspace = self.inner.lock().map_err(|_| "engine lock poisoned")?;
        let inner = workspace.active();
        let scene = inner.manager.solid_scene_snapshot();
        limo_cad_occt::section_review::validate_source(&scene, &request)
            .map_err(|e| e.to_string())?;
        let identity = *inner
            .kernel
            .body_geometry_identities()
            .map_err(|e| e.to_string())?
            .get(&request.body_id)
            .ok_or("Section body has no live native geometry identity")?;
        let kernel = inner
            .kernel
            .detached_section_snapshot(request.body_id)
            .map_err(|source| {
                limo_cad_occt::section_review::SectionReviewError::Kernel {
                    body_id: request.body_id,
                    plane: request.plane,
                    offset_mm: request.offset_mm,
                    source,
                }
                .to_string()
            })?;
        let source = SourceProof {
            owner: workspace.verification_owner_key(),
            scene: Arc::downgrade(&scene),
            geometry_revision: inner.geometry_revision,
            metadata_revision: inner.viewport_revision,
            body: request.body_id,
            identity,
        };
        Ok(PreparedSectionReview {
            kernel,
            request,
            source,
        })
    }
    /// Attached callers retain their publisher/ticket receipt through adoption.
    pub fn validate_section_review(&self, finished: &FinishedSectionReview) -> Result<(), String> {
        let workspace = self.inner.lock().map_err(|_| "engine lock poisoned")?;
        let source = &finished.source;
        if workspace.verification_owner_key() != source.owner {
            return Err("The owning section document was replaced during inspection".into());
        }
        let inner = workspace.active();
        if inner.geometry_revision != source.geometry_revision
            || inner.viewport_revision != source.metadata_revision
            || !std::ptr::eq(
                source.scene.as_ptr(),
                Arc::as_ptr(&inner.manager.solid_scene_snapshot()),
            )
        {
            return Err("The section source changed during inspection".into());
        }
        if inner
            .kernel
            .body_geometry_identities()
            .map_err(|e| e.to_string())?
            .get(&source.body)
            != Some(&source.identity)
        {
            return Err(
                "The native geometry changed or was replaced during section inspection".into(),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "section_snapshot_tests.rs"]
mod tests;
