//! Generated geometry frames remain separate from authored script exports.
use limo_cad_sketch::AssemblySolutionDto;
use limo_cad_solid::SolidSceneDto;
use serde::Serialize;
use serde_json::Value;
use std::io::{self, Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectPreviewStage {
    Baseline,
    Final,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectPreviewCoverage {
    /// Solid tessellation and assembly placement only. Does not qualify sketches,
    /// drawings, CAM/FEM/BIM results, annotations or other authored visuals.
    SolidGeometryAndAssembly,
}

#[derive(Debug, Serialize)]
pub struct ProjectPreviewFrame {
    pub stage: ProjectPreviewStage,
    pub scene: SolidSceneDto,
    pub assembly: AssemblySolutionDto,
}

#[derive(Debug)]
pub struct ProjectPreviewResult {
    pub report: Value,
    pub frames: Vec<ProjectPreviewFrame>,
    pub coverage: ProjectPreviewCoverage,
    /// Retained domain payloads are not computed analysis or rendered overlays.
    pub unrendered_domain_intents: Vec<String>,
}

#[derive(Default)]
pub(super) struct Frames {
    pub values: Vec<ProjectPreviewFrame>,
    bytes: usize,
}
impl Frames {
    pub fn capture(
        &mut self,
        scene: &SolidSceneDto,
        assembly: AssemblySolutionDto,
        stage: ProjectPreviewStage,
    ) -> Result<(), String> {
        if !scene.errors.is_empty() || !assembly.solved {
            return Err("Preview frame requires successful geometry and a solved assembly".into());
        }
        if self
            .values
            .last()
            .is_some_and(|last| last.scene == *scene && last.assembly == assembly)
        {
            return Ok(());
        }
        if self.values.len() >= 2 {
            return Err("Package preview supports only baseline and final generated frames".into());
        }
        // Count serialization directly into a bounded sink before cloning the
        // geometry. No intermediate JSON value/string or encoded frame buffer.
        #[derive(Serialize)]
        struct BorrowedFrame<'a> {
            stage: ProjectPreviewStage,
            scene: &'a SolidSceneDto,
            assembly: &'a AssemblySolutionDto,
        }
        let mut budget = ByteBudget {
            bytes: self.bytes,
            limit: 16 * 1024 * 1024,
        };
        serde_json::to_writer(
            &mut budget,
            &BorrowedFrame {
                stage,
                scene,
                assembly: &assembly,
            },
        )
        .map_err(|error| format!("Package preview frame budget: {error}"))?;
        self.values.push(ProjectPreviewFrame {
            stage,
            scene: scene.clone(),
            assembly,
        });
        self.bytes = budget.bytes;
        Ok(())
    }
}

struct ByteBudget {
    bytes: usize,
    limit: usize,
}
impl Write for ByteBudget {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let next = self
            .bytes
            .checked_add(bytes.len())
            .filter(|next| *next <= self.limit)
            .ok_or_else(|| io::Error::other("Generated preview frames exceed 16 MiB"))?;
        self.bytes = next;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_byte_budget_is_exact_and_failed_write_does_not_advance() {
        let mut budget = ByteBudget { bytes: 0, limit: 3 };
        budget.write_all(&[1, 2, 3]).unwrap();
        assert!(budget.write_all(&[4]).is_err());
        assert_eq!(budget.bytes, 3);
        budget.write_all(&[]).unwrap();
    }
    #[test]
    fn captured_frames_are_immutable_and_duplicate_geometry_is_omitted() {
        let mut scene: SolidSceneDto = serde_json::from_value(serde_json::json!({
            "bodies":[{"id":1,"name":"Triangle","feature_id":2,"faces":[],"edges":[],"mesh":{
                "positions":[0,0,0,1,0,0,0,1,0],"normals":[0,0,1,0,0,1,0,0,1],"indices":[0,1,2]}}],"errors":[]
        })).unwrap();
        let assembly = AssemblySolutionDto {
            solved: true,
            ..Default::default()
        };
        let mut frames = Frames::default();
        frames
            .capture(&scene, assembly.clone(), ProjectPreviewStage::Baseline)
            .unwrap();
        let bytes = frames.bytes;
        frames
            .capture(&scene, assembly.clone(), ProjectPreviewStage::Final)
            .unwrap();
        assert_eq!(frames.values.len(), 1);
        assert_eq!(frames.bytes, bytes);
        scene.bodies[0].mesh.positions[0] = 2.0;
        let mut changed = assembly;
        changed.diagnostics.push(serde_json::from_value(serde_json::json!({
            "kind":"free_component","message":"Unconstrained occurrence","joint_id":null,"body_id":null
        })).unwrap());
        frames
            .capture(&scene, changed, ProjectPreviewStage::Final)
            .unwrap();
        assert_eq!(frames.values[0].scene.bodies[0].mesh.positions[0], 0.0);
        assert_eq!(frames.values[1].scene.bodies[0].mesh.positions[0], 2.0);
        assert!(frames.values[0].assembly.diagnostics.is_empty());
        assert_eq!(frames.values[1].assembly.diagnostics.len(), 1);
        assert_eq!(frames.values[1].stage, ProjectPreviewStage::Final);
    }
}
