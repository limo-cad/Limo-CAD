//! Named export façade shared by UI and MCP.

use limo_cad_core::BodyAppearance;

use crate::slicer::SlicerTarget;
use crate::stl::write_stl;
use crate::threemf::write_3mf;
use crate::{ExportError, MeshExportRequest, TriangleMesh};

/// Shared manufacturing export entry points (UI + MCP).
#[derive(Debug, Default)]
pub struct ExportFacade;

impl ExportFacade {
    pub fn export_stl(meshes: &[TriangleMesh]) -> Result<Vec<u8>, ExportError> {
        write_stl(meshes)
    }

    pub fn export_3mf(
        meshes: &[TriangleMesh],
        appearances: &[BodyAppearance],
        request: &MeshExportRequest,
    ) -> Result<Vec<u8>, ExportError> {
        write_3mf(
            meshes,
            appearances,
            request.include_appearance,
            request.slicer_target,
        )
    }

    pub fn export_3mf_for_target(
        meshes: &[TriangleMesh],
        appearances: &[BodyAppearance],
        include_appearance: bool,
        target: SlicerTarget,
    ) -> Result<Vec<u8>, ExportError> {
        write_3mf(meshes, appearances, include_appearance, target)
    }
}
