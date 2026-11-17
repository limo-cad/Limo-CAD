//! Owned portable mesh export inputs. Synchronous MCP contracts remain unchanged.
use super::*;
use limo_cad_export::{MeshExportRequest, MeshExportScope, MeshInstance};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub enum PortableMeshFormat {
    Stl,
    ThreeMf,
}

pub struct PreparedPortableMeshExport {
    kernel: limo_cad_occt::DetachedMeshKernel,
    request: MeshExportRequest,
    format: PortableMeshFormat,
    names: BTreeMap<limo_cad_core::BodyId, String>,
    appearances: Vec<BodyAppearance>,
    structure: limo_cad_sketch::ComponentStructureDto,
    solution: limo_cad_sketch::AssemblySolutionDto,
    owner_key: String,
    identities: BTreeMap<limo_cad_core::BodyId, limo_cad_occt::KernelBodyGeometryIdentity>,
}

pub struct FinishedPortableMeshExport {
    bytes: Vec<u8>,
    layout_has_issues: bool,
    model: String,
    owner_key: String,
    identities: BTreeMap<limo_cad_core::BodyId, limo_cad_occt::KernelBodyGeometryIdentity>,
}

impl PreparedPortableMeshExport {
    pub fn build(mut self) -> Result<FinishedPortableMeshExport, String> {
        let mut meshes = self
            .kernel
            .tessellate(&self.request)
            .map_err(|e| e.to_string())?;
        for mesh in &mut meshes {
            if let Some(name) = self.names.get(&mesh.body_id) {
                mesh.name.clone_from(name);
            }
        }
        // Use the same captured intent and newly tessellated selected meshes as
        // the portable writer; a fresh layout check must not reenter live OCCT.
        let layout_has_issues = if matches!(self.format, PortableMeshFormat::ThreeMf)
            && self.request.scope == MeshExportScope::Assembly
        {
            !limo_cad_export::analyze_print_layout(
                &meshes,
                &self.structure,
                &self.solution,
                self.request
                    .print_bed
                    .as_ref()
                    .ok_or("Portable layout omitted its captured bed")?,
            )
            .map_err(|e| e.to_string())?
            .issues
            .is_empty()
        } else {
            false
        };
        let bytes = match self.format {
            PortableMeshFormat::Stl => {
                let instances: Vec<_> = self
                    .solution
                    .instance_body_poses
                    .iter()
                    .map(|pose| MeshInstance {
                        body_id: pose.body_id,
                        occurrence_id: pose.occurrence_id.0,
                        translation: pose.translation,
                        rotation: pose.rotation,
                        visible: pose.visible,
                    })
                    .collect();
                let meshes =
                    limo_cad_export::prepare_export_meshes(&meshes, &instances, self.request.scope)
                        .map_err(|e| e.to_string())?;
                limo_cad_export::write_stl(&meshes).map_err(|e| e.to_string())?
            }
            PortableMeshFormat::ThreeMf => limo_cad_export::write_3mf_scene(
                &meshes,
                &self.appearances,
                &self.request,
                &self.structure,
                &self.solution,
            )
            .map_err(|e| e.to_string())?,
        };
        Ok(FinishedPortableMeshExport {
            bytes,
            layout_has_issues,
            model: self
                .request
                .expected_model_json
                .take()
                .ok_or("Portable export omitted its validated model")?,
            owner_key: self.owner_key,
            identities: self.identities,
        })
    }
}
impl FinishedPortableMeshExport {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn layout_has_issues(&self) -> bool {
        self.layout_has_issues
    }
}

impl NativeEngineHost {
    /// Caller captures this under its original publisher/document receipt. The
    /// snapshot owns selected B-reps and cannot publish a file or grant authority.
    pub fn prepare_portable_mesh_export(
        &self,
        mut request: MeshExportRequest,
        format: PortableMeshFormat,
    ) -> Result<PreparedPortableMeshExport, String> {
        if request.scope == MeshExportScope::Definition
            && request
                .named_view
                .as_deref()
                .is_some_and(|name| !name.is_empty())
        {
            return Err(
                "A named layout requires assembly scope; definition scope uses source coordinates."
                    .into(),
            );
        }
        if request.expected_model_json.is_none() {
            return Err("An owned portable export requires its original model snapshot".into());
        }
        let workspace = self.inner.lock().map_err(|_| "Engine lock poisoned")?;
        let inner = workspace.active();
        request
            .check_model_snapshot(
                &inner
                    .manager
                    .project_model_projection()
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        let scene = inner.manager.solid_scene_ref();
        if !scene.errors.is_empty() {
            return Err("Resolve timeline errors before portable mesh export.".into());
        }
        scene.require_complete_display_mesh(&request.body_ids)?;
        let solution = if request.scope == MeshExportScope::Definition {
            inner.manager.assembly_solution()
        } else {
            inner
                .manager
                .export_view_solution(request.named_view.as_deref())
                .map_err(|e| e.to_string())?
        };
        if request.scope == MeshExportScope::Assembly && !solution.solved {
            return Err("Resolve assembly errors before mesh export.".into());
        }
        if matches!(format, PortableMeshFormat::ThreeMf)
            && request.scope == MeshExportScope::Assembly
            && request.print_bed.is_none()
        {
            request.print_bed = Some(
                inner
                    .manager
                    .export_print_bed(request.named_view.as_deref())
                    .map_err(|e| e.to_string())?,
            );
        }
        let available = inner.kernel.active_body_ids().map_err(|e| e.to_string())?;
        if request.body_ids.is_empty() {
            request.body_ids.extend(available.iter().copied());
        }
        request.body_ids.sort_unstable();
        request.body_ids.dedup();
        if request.body_ids.is_empty() {
            return Err("There are no active bodies to export.".into());
        }
        let current = inner
            .kernel
            .body_geometry_identities()
            .map_err(|e| e.to_string())?;
        let mut identities = BTreeMap::new();
        for id in &request.body_ids {
            if !available.contains(id) {
                return Err(format!("Selected body {} is not active.", id.0));
            }
            identities.insert(
                *id,
                *current
                    .get(id)
                    .ok_or("Export body has no live geometry identity")?,
            );
        }
        let kernel = inner
            .kernel
            .detached_mesh_snapshot(&request.body_ids)
            .map_err(|e| e.to_string())?;
        let names = scene
            .bodies
            .iter()
            .filter(|body| request.body_ids.contains(&body.id))
            .map(|body| (body.id, body.name.clone()))
            .collect();
        Ok(PreparedPortableMeshExport {
            kernel,
            request,
            format,
            names,
            appearances: if matches!(format, PortableMeshFormat::ThreeMf) {
                inner.manager.body_appearances()
            } else {
                Vec::new()
            },
            structure: if matches!(format, PortableMeshFormat::ThreeMf) {
                inner
                    .manager
                    .assembly_document_ref()
                    .component_structure
                    .clone()
            } else {
                Default::default()
            },
            solution,
            owner_key: workspace.verification_owner_key(),
            identities,
        })
    }

    /// The caller must retain the original publisher receipt and current export
    /// job lease across this validation and its atomic write. No registration or
    /// source mutation occurs; native identity is private in-process evidence.
    pub fn validate_portable_mesh_export(
        &self,
        finished: &FinishedPortableMeshExport,
    ) -> Result<(), String> {
        let workspace = self.inner.lock().map_err(|_| "Engine lock poisoned")?;
        if workspace.verification_owner_key() != finished.owner_key {
            return Err("The owning native document was replaced before portable export".into());
        }
        let inner = workspace.active();
        if inner
            .manager
            .project_model_projection()
            .map_err(|e| e.to_string())?
            != finished.model
        {
            return Err("The model changed before portable export".into());
        }
        let current = inner
            .kernel
            .body_geometry_identities()
            .map_err(|e| e.to_string())?;
        if finished
            .identities
            .iter()
            .any(|(id, identity)| current.get(id) != Some(identity))
        {
            return Err(
                "The native geometry changed or was replaced before portable export".into(),
            );
        }
        Ok(())
    }
}

/// Exact exchange uses the same owned B-rep snapshot and private publication
/// proof, without introducing a display-mesh prerequisite.
pub struct PreparedStepExport {
    kernel: limo_cad_occt::DetachedMeshKernel,
    request: StepExportRequest,
    owner_key: String,
    identities: BTreeMap<limo_cad_core::BodyId, limo_cad_occt::KernelBodyGeometryIdentity>,
}
pub struct FinishedStepExport(FinishedPortableMeshExport);
impl PreparedStepExport {
    pub fn build(mut self) -> Result<FinishedStepExport, String> {
        let bytes = self
            .kernel
            .export_step(&self.request)
            .map_err(|e| e.to_string())?;
        Ok(FinishedStepExport(FinishedPortableMeshExport {
            bytes,
            layout_has_issues: false,
            model: self
                .request
                .expected_model_json
                .take()
                .ok_or("STEP export omitted its validated model")?,
            owner_key: self.owner_key,
            identities: self.identities,
        }))
    }
}
impl FinishedStepExport {
    pub fn bytes(&self) -> &[u8] {
        self.0.bytes()
    }
}
impl NativeEngineHost {
    pub fn prepare_step_export(
        &self,
        request: StepExportRequest,
    ) -> Result<PreparedStepExport, String> {
        if request.expected_model_json.is_none() {
            return Err("An owned STEP export requires its original model snapshot".into());
        }
        let workspace = self.inner.lock().map_err(|_| "Engine lock poisoned")?;
        let inner = workspace.active();
        limo_cad_solid::check_export_model_snapshot(
            request.expected_model_json.as_deref(),
            &inner
                .manager
                .project_model_projection()
                .map_err(|e| e.to_string())?,
        )?;
        if !inner.manager.solid_scene_ref().errors.is_empty() {
            return Err("Resolve timeline errors before exporting STEP.".into());
        }
        // Preserve the exact writer's effective selection: placed occurrences
        // override requested_body_ids; otherwise an empty list means all bodies.
        let available = inner.kernel.active_body_ids().map_err(|e| e.to_string())?;
        let mut selected: Vec<_> = if !request.occurrences.is_empty() {
            request.occurrences.iter().map(|o| o.body_id).collect()
        } else if request.body_ids.is_empty() {
            available.clone()
        } else {
            request.body_ids.clone()
        };
        selected.sort_unstable();
        selected.dedup();
        if selected.is_empty() {
            return Err("There are no active bodies to export.".into());
        }
        let current = inner
            .kernel
            .body_geometry_identities()
            .map_err(|e| e.to_string())?;
        let mut identities = BTreeMap::new();
        for id in &selected {
            if !available.contains(id) {
                return Err(format!("Selected body {} is not active.", id.0));
            }
            identities.insert(
                *id,
                *current
                    .get(id)
                    .ok_or("Export body has no live geometry identity")?,
            );
        }
        let kernel = inner
            .kernel
            .detached_mesh_snapshot(&selected)
            .map_err(|e| e.to_string())?;
        Ok(PreparedStepExport {
            kernel,
            request,
            owner_key: workspace.verification_owner_key(),
            identities,
        })
    }
    /// Retain the original publisher receipt and job lease through the write.
    pub fn validate_step_export(&self, finished: &FinishedStepExport) -> Result<(), String> {
        self.validate_portable_mesh_export(&finished.0)
    }
}
