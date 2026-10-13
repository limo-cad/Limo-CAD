use super::*;
use limo_cad_assembly::{AssemblyTransformDto, OccurrenceId};

impl SketchManager {
    /// Edit shared definition geometry in one resolved occurrence's display frame.
    /// The solver, projected references and saved history retain local coordinates.
    pub fn edit_sketch_in_occurrence(
        &mut self,
        name: &str,
        occurrence_id: OccurrenceId,
    ) -> Result<SketchDto, SessionError> {
        if self.active.is_some() {
            return Err(SessionError::SketchAlreadyActive);
        }
        let sketch_feature = self
            .finished
            .iter()
            .find(|finished| finished.session.name() == name)
            .ok_or_else(|| SessionError::SketchNotFound(name.into()))?
            .feature_id;
        let occurrence = self
            .assembly
            .component_structure
            .occurrences
            .iter()
            .find(|occurrence| occurrence.id == occurrence_id)
            .ok_or_else(|| {
                SessionError::BrokenReference(format!(
                    "Occurrence {} no longer exists",
                    occurrence_id.0
                ))
            })?;
        let component = self
            .assembly
            .component_structure
            .definitions
            .iter()
            .find(|component| component.id == occurrence.component_id)
            .ok_or_else(|| {
                SessionError::BrokenReference("Occurrence definition no longer exists".into())
            })?;
        let (dependencies, writers) =
            self.timeline_dependencies_and_body_writers(&self.document.features().features);
        let depends_on_sketch = |body_id: BodyId| {
            let mut pending = writers
                .get(&body_id)
                .copied()
                .into_iter()
                .collect::<Vec<_>>();
            let mut visited = BTreeSet::new();
            while let Some(feature_id) = pending.pop() {
                if feature_id == sketch_feature {
                    return true;
                }
                if visited.insert(feature_id) {
                    pending.extend(dependencies.get(&feature_id).into_iter().flatten().copied());
                }
            }
            false
        };
        let solution = self.assembly_solution();
        let pose = solution
            .instance_body_poses
            .iter()
            .find(|pose| {
                pose.occurrence_id == occurrence_id
                    && pose.visible
                    && component.body_ids.contains(&pose.body_id)
                    && depends_on_sketch(pose.body_id)
            })
            .ok_or_else(|| {
                SessionError::Solid(format!(
                    "Sketch '{name}' does not drive a visible body in the selected part occurrence"
                ))
            })?;
        let placement = AssemblyTransformDto {
            translation: pose.translation,
            rotation: pose.rotation,
        };
        self.edit_sketch(name)?;
        let session = self.active.as_mut().ok_or(SessionError::NoActiveSketch)?;
        session.set_edit_placement(Some((occurrence_id, placement)));
        Ok(session.dto())
    }

    pub(super) fn ensure_no_component_edit(&self) -> Result<(), SessionError> {
        if self
            .active
            .as_ref()
            .is_some_and(SketchSession::editing_occurrence)
        {
            return Err(SessionError::Solid("Finish the in-place component edit before changing assembly placement or structure".into()));
        }
        Ok(())
    }
}
