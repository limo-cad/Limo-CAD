//! A Hole edits source geometry while its transient guides follow one picked instance.

use super::*;
use limo_cad_sketch::AssemblyTransformDto;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Placement {
    body: u64,
    occurrence: Option<u64>,
    pose: AssemblyTransformDto,
}

impl Placement {
    pub(super) fn for_body(world: &World, snapshot: &Snapshot, body: u64) -> Result<Self, String> {
        let view = native_viewport::interface_view(world).2;
        let selected = view.selected_occurrence_id.filter(|id| {
            view.instance_body_poses
                .iter()
                .any(|p| p.body_id.0 == body && p.occurrence_id.0 == *id && p.visible)
        });
        let occurrence = if selected.is_some() {
            selected
        } else {
            let mut instances = view
                .instance_body_poses
                .iter()
                .filter(|p| p.body_id.0 == body && p.visible);
            let first = instances.next().map(|p| p.occurrence_id.0);
            if instances.next().is_some() {
                return Err("Select the component instance before editing its hole".into());
            }
            first
        };
        Self::capture(world, snapshot, body, occurrence)
    }

    pub(super) fn capture(
        world: &World,
        snapshot: &Snapshot,
        body: u64,
        occurrence: Option<u64>,
    ) -> Result<Self, String> {
        let (document, _, view, _) = native_viewport::interface_view(world);
        if document != snapshot.receipt.owner.document_id {
            return Err("The rendered design is not current".into());
        }
        if view.hidden_body_ids.contains(&body) {
            return Err("The selected hole body is no longer visible".into());
        }
        let pose = if let Some(id) = occurrence {
            let instance = view
                .instance_body_poses
                .iter()
                .find(|p| p.body_id.0 == body && p.occurrence_id.0 == id && p.visible)
                .ok_or("The selected component instance is no longer visible")?;
            let structure = &snapshot.assembly.component_structure;
            if !structure
                .occurrences
                .iter()
                .any(|o| o.id.0 == id && o.component_id == instance.component_id)
                || !structure
                    .definitions
                    .iter()
                    .any(|d| d.id == instance.component_id && d.body_ids.contains(&BodyId(body)))
            {
                return Err("The selected instance no longer belongs to this body".into());
            }
            AssemblyTransformDto {
                translation: instance.translation,
                rotation: instance.rotation,
            }
        } else {
            if view.instance_body_poses.iter().any(|p| p.body_id.0 == body) {
                return Err("Select an explicit component instance for the hole".into());
            }
            view.body_poses
                .iter()
                .find(|p| p.body_id.0 == body)
                .map(|p| AssemblyTransformDto {
                    translation: p.translation,
                    rotation: p.rotation,
                })
                .unwrap_or_default()
        };
        let norm = pose.rotation.iter().map(|v| v * v).sum::<f64>();
        if !pose
            .translation
            .iter()
            .chain(&pose.rotation)
            .all(|v| v.is_finite())
            || !norm.is_finite()
            || (norm - 1.).abs() > 1e-6
        {
            return Err("The selected component placement is not a finite rigid pose".into());
        }
        Ok(Self {
            body,
            occurrence,
            pose,
        })
    }

    pub(super) fn validate(self, world: &World, snapshot: &Snapshot) -> Result<(), String> {
        if Self::capture(world, snapshot, self.body, self.occurrence)? != self {
            return Err("The component placement changed; select the hole support again".into());
        }
        Ok(())
    }

    pub(super) fn matches(self, body: u64, occurrence: Option<u64>) -> bool {
        self.body == body && self.occurrence == occurrence
    }

    pub(super) fn local_point(self, world: [f64; 3]) -> [f64; 3] {
        self.pose.inverse().transform_point(world)
    }

    pub(super) fn world_point(self, local: [f64; 3]) -> [f64; 3] {
        self.pose.transform_point(local)
    }

    pub(super) fn preview(self, preview: &mut ViewportPreview) -> Result<(), String> {
        let points = |values: &mut Arc<Vec<f32>>| -> Result<(), String> {
            for point in Arc::make_mut(values).as_chunks_mut::<3>().0 {
                *point = self.world_point(point.map(f64::from)).map(|v| v as f32);
                if !point.iter().all(|v| v.is_finite()) {
                    return Err("The placed hole preview exceeds the renderer's range".into());
                }
            }
            Ok(())
        };
        for line in &mut preview.lines {
            points(&mut line.segments)?;
        }
        for fill in &mut preview.triangles {
            points(&mut fill.positions)?;
            for normal in Arc::make_mut(&mut fill.normals).as_chunks_mut::<3>().0 {
                let vector = AssemblyTransformDto {
                    translation: [0.; 3],
                    ..self.pose
                }
                .transform_point(normal.map(f64::from));
                *normal = vector.map(|v| v as f32);
            }
        }
        for layer in &mut preview.points {
            points(&mut layer.positions)?;
        }
        for arrow in &mut preview.arrows {
            arrow.start = self
                .world_point(arrow.start.map(f64::from))
                .map(|v| v as f32);
            arrow.end = self.world_point(arrow.end.map(f64::from)).map(|v| v as f32);
            if !arrow.start.iter().chain(&arrow.end).all(|v| v.is_finite()) {
                return Err("The placed hole preview exceeds the renderer's range".into());
            }
        }
        Ok(())
    }
}
