//! Pure sampler CSV export from coherent required authored assembly and scene.
use super::*;
use limo_cad_sketch::{AssemblyDocumentDto, MotionPathRequestDto};
use std::sync::Weak;
struct SourceProof {
    owner: String,
    scene: Weak<SolidSceneDto>,
    geometry_revision: u64,
    metadata_revision: u64,
    // Required solver/sampler input, never a manager/history/model clone.
    assembly: Arc<AssemblyDocumentDto>,
}
pub struct PreparedMotionPathExport {
    scene: Arc<SolidSceneDto>,
    request: MotionPathRequestDto,
    source: SourceProof,
}
pub struct FinishedMotionPathExport {
    csv: String,
    source: SourceProof,
}
impl PreparedMotionPathExport {
    pub fn export(self) -> Result<FinishedMotionPathExport, String> {
        // Preserve the original study/rate/count validation, sampling, row order
        // and derivatives. No live engine or native shapes are used here.
        let csv = self
            .source
            .assembly
            .export_motion_path_csv(self.request, &self.scene)?;
        Ok(FinishedMotionPathExport {
            csv,
            source: self.source,
        })
    }
}
impl FinishedMotionPathExport {
    pub fn csv(&self) -> &str {
        &self.csv
    }
    /// Move the wire payload while retaining opaque source proof for final validation.
    pub fn take_csv(&mut self) -> String {
        std::mem::take(&mut self.csv)
    }
    pub fn into_csv(self) -> String {
        self.csv
    }
}
impl NativeEngineHost {
    pub fn prepare_motion_path_export(
        &self,
        payload: &str,
    ) -> Result<PreparedMotionPathExport, String> {
        // Preserve the existing native read-only dispatch's poison and parse order.
        let workspace = self.inner.lock().expect("engine lock poisoned");
        let request: MotionPathRequestDto = serde_json::from_str(payload)
            .map_err(|error| format!("bad request payload: {error}"))?;
        let inner = workspace.active();
        let scene = inner.manager.solid_scene_snapshot();
        let source = SourceProof {
            owner: workspace.verification_owner_key(),
            scene: Arc::downgrade(&scene),
            geometry_revision: inner.geometry_revision,
            metadata_revision: inner.viewport_revision,
            assembly: Arc::new(inner.manager.assembly_document_ref().clone()),
        };
        Ok(PreparedMotionPathExport {
            scene,
            request,
            source,
        })
    }
    pub fn validate_motion_path_export(
        &self,
        finished: &FinishedMotionPathExport,
    ) -> Result<(), String> {
        let workspace = self.inner.lock().expect("engine lock poisoned");
        let source = &finished.source;
        if workspace.verification_owner_key() != source.owner {
            return Err("The owning motion path document was replaced during export".into());
        }
        let inner = workspace.active();
        if inner.manager.assembly_document_ref() != source.assembly.as_ref() {
            return Err("The motion path inputs changed during export".into());
        }
        if inner.geometry_revision != source.geometry_revision
            || inner.viewport_revision != source.metadata_revision
            || !std::ptr::eq(
                source.scene.as_ptr(),
                Arc::as_ptr(&inner.manager.solid_scene_snapshot()),
            )
        {
            return Err("The motion path scene changed during export".into());
        }
        Ok(())
    }
    pub fn assembly_export_motion_path_csv(&self, payload: &str) -> String {
        match self
            .prepare_motion_path_export(payload)
            .and_then(|prepared| prepared.export())
            .and_then(|finished| {
                self.validate_motion_path_export(&finished)?;
                Ok(finished.into_csv())
            }) {
            Ok(csv) => ok_json(csv),
            Err(error) => err_json(error),
        }
    }
}
#[cfg(test)]
#[path = "motion_path_snapshot_tests.rs"]
mod tests;
