//! Selected genuine B-reps for read-only section work outside live guards.
use super::*;
use limo_cad_core::BodyId;
use limo_cad_occt::section_review::{SectionReview, SectionReviewRequest};
use std::sync::Weak;

#[derive(Clone)]
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
    source_errors: Vec<limo_cad_solid::KernelFeatureErrorDto>,
}

pub struct FinishedSectionReview {
    report: SectionReview,
    source: SourceProof,
}
impl PreparedSectionReview {
    pub fn inspect(self) -> Result<FinishedSectionReview, String> {
        self.inspect_at(&self.request)
    }
    /// Reuse one selected-body copy while a live inspection moves its plane.
    /// Every result retains the original private source proof for adoption.
    pub fn inspect_at(
        &self,
        request: &SectionReviewRequest,
    ) -> Result<FinishedSectionReview, String> {
        request.validate()?;
        if request.body_id != self.source.body {
            return Err("The section request does not match its copied source body".into());
        }
        Ok(FinishedSectionReview {
            report: self
                .kernel
                .inspect(request)
                .map_err(|e| e.to_string())?
                .with_source_errors(self.source_errors.clone()),
            source: self.source.clone(),
        })
    }
    /// Exact retained-side preview, including bodies outside the section plane.
    pub fn inspect_retained_at(
        &self,
        request: &SectionReviewRequest,
    ) -> Result<FinishedSectionReview, String> {
        request.validate()?;
        if request.body_id != self.source.body || !request.include_cutaway {
            return Err("The cutaway request does not match its copied source body".into());
        }
        Ok(FinishedSectionReview {
            report: self
                .kernel
                .inspect_retained(request)
                .map_err(|e| e.to_string())?
                .with_source_errors(self.source_errors.clone()),
            source: self.source.clone(),
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
        self.prepare_section_reviews(&[request])?
            .pop()
            .ok_or("Section source missing".into())
    }
    /// Capture all selected source bodies under one engine guard.
    pub fn prepare_section_reviews(
        &self,
        requests: &[SectionReviewRequest],
    ) -> Result<Vec<PreparedSectionReview>, String> {
        if requests.is_empty() || requests.len() > 128 {
            return Err("Choose between 1 and 128 section bodies".into());
        }
        let workspace = self.inner.lock().map_err(|_| "engine lock poisoned")?;
        let inner = workspace.active();
        let scene = inner.manager.solid_scene_snapshot();
        let identities = inner
            .kernel
            .body_geometry_identities()
            .map_err(|e| e.to_string())?;
        let mut ids = std::collections::BTreeSet::new();
        requests
            .iter()
            .map(|request| {
                if !ids.insert(request.body_id) {
                    return Err("Choose each section body once".into());
                }
                limo_cad_occt::section_review::validate_source(&scene, request)
                    .map_err(|e| e.to_string())?;
                let kernel_error = |source| {
                    limo_cad_occt::section_review::SectionReviewError::Kernel {
                        body_id: request.body_id,
                        plane: request.plane,
                        offset_mm: request.offset_mm,
                        source,
                    }
                    .to_string()
                };
                let identity = *identities.get(&request.body_id).ok_or_else(|| {
                    kernel_error(limo_cad_occt::OcctError(
                        "Section body has no live native geometry identity".into(),
                    ))
                })?;
                let kernel = inner
                    .kernel
                    .detached_section_snapshot(request.body_id)
                    .map_err(kernel_error)?;
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
                    request: request.clone(),
                    source,
                    source_errors: scene.errors.clone(),
                })
            })
            .collect()
    }
    /// Attached callers retain their publisher/ticket receipt through adoption.
    pub fn validate_section_review(&self, finished: &FinishedSectionReview) -> Result<(), String> {
        self.validate_section_reviews(std::slice::from_ref(finished))
    }
    /// Adopt the entire selection coherently; one stale source rejects the batch.
    pub fn validate_section_reviews(
        &self,
        finished: &[FinishedSectionReview],
    ) -> Result<(), String> {
        if finished.is_empty() {
            return Err("Section result has no source bodies".into());
        }
        let workspace = self.inner.lock().map_err(|_| "engine lock poisoned")?;
        let identities = workspace
            .active()
            .kernel
            .body_geometry_identities()
            .map_err(|e| e.to_string())?;
        for finished in finished {
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
            if identities.get(&source.body) != Some(&source.identity) {
                return Err(
                    "The native geometry changed or was replaced during section inspection".into(),
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "section_snapshot_tests.rs"]
mod tests;
