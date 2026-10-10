//! Assembly placement keeps reusable part history intact. Every transform is
//! resolved in world space, then converted to the selected instance's parent.
use super::*;
use limo_cad_sketch::{AssemblyDocumentDto, AssemblyTransformDto, OccurrenceId};
use std::collections::HashSet;

fn connected(assembly: &AssemblyDocumentDto, id: u64) -> HashSet<u64> {
    let mut ids = HashSet::from([id]);
    loop {
        let before = ids.len();
        for joint in assembly.joints.iter().filter(|j| j.enabled) {
            if let (Some(a), Some(b)) = (
                joint.advanced.connector_a_occurrence_id,
                joint.advanced.connector_b_occurrence_id,
            ) {
                if ids.contains(&a.0) || ids.contains(&b.0) {
                    ids.extend([a.0, b.0]);
                }
            }
        }
        if ids.len() == before {
            break;
        }
    }
    ids
}

impl SolidForm {
    pub(crate) fn move_is_component(&self) -> bool {
        self.move_copy.as_ref().is_some_and(|f| f.component)
    }
    pub(crate) fn move_occurrence(&self) -> Option<u64> {
        self.move_copy
            .as_ref()
            .filter(|f| f.component)
            .and_then(|f| f.occurrence)
    }
    pub(crate) fn smart_move_occurrence(
        model: &FormModel<'_>,
        selected: Option<u64>,
        bodies: &[u64],
    ) -> Option<u64> {
        let a = model.assembly?;
        if let Some(id) = selected.filter(|id| {
            a.component_structure
                .occurrences
                .iter()
                .any(|o| o.id.0 == *id)
        }) {
            return Some(id);
        }
        if bodies.len() != 1 {
            return None;
        }
        let mut matches = a.component_structure.occurrences.iter().filter(|o| {
            a.component_structure
                .definitions
                .iter()
                .any(|d| d.id == o.component_id && d.body_ids.iter().any(|b| b.0 == bodies[0]))
        });
        let o = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        (o.grounded || connected(a, o.id.0).len() > 1).then_some(o.id.0)
    }
    pub(crate) fn set_move_occurrence(
        &mut self,
        id: Option<u64>,
        model: &FormModel<'_>,
    ) -> Result<(), String> {
        self.editing(model)?;
        if self.feature.is_some() {
            return Err("A body feature cannot move an assembly instance".into());
        }
        let a = model.assembly.ok_or("Assembly structure is unavailable")?;
        if id.is_some_and(|id| {
            !a.component_structure
                .occurrences
                .iter()
                .any(|o| o.id.0 == id)
        }) {
            return Err("The selected component no longer exists".into());
        }
        let f = self.move_copy.as_mut().ok_or("Open Move/Copy first")?;
        f.component = true;
        f.occurrence = id;
        if !f.manual_pivot {
            let ids = subtree(a, id.into_iter().collect());
            let mut min = DVec3::splat(f64::INFINITY);
            let mut max = DVec3::splat(f64::NEG_INFINITY);
            if let Some(solution) = model.assembly_solution {
                for pose in solution
                    .instance_body_poses
                    .iter()
                    .filter(|p| ids.contains(&p.occurrence_id.0))
                {
                    if let Some(body) = model.scene.bodies.iter().find(|b| b.id == pose.body_id) {
                        let q = DQuat::from_array(pose.rotation);
                        let t = DVec3::from_array(pose.translation);
                        for p in body.mesh.positions.as_chunks::<3>().0 {
                            let p = q * DVec3::new(p[0] as f64, p[1] as f64, p[2] as f64) + t;
                            min = min.min(p);
                            max = max.max(p);
                        }
                    }
                }
            }
            if min.is_finite() && max.is_finite() {
                f.pivot = vector(
                    ((min + max) * 0.5).to_array(),
                    DimensionKind::Length,
                    model.document.settings.units,
                );
            }
        }
        self.changed();
        Ok(())
    }
    pub(crate) fn move_occurrence_targets(
        &self,
        model: &FormModel<'_>,
    ) -> Result<HashSet<u64>, String> {
        let f = self.move_copy.as_ref().ok_or("Open Move/Copy first")?;
        let id = f.occurrence.ok_or("Select a component to move or copy")?;
        let a = model.assembly.ok_or("Assembly structure is unavailable")?;
        let occurrence = a
            .component_structure
            .occurrences
            .iter()
            .find(|o| o.id.0 == id)
            .ok_or("The selected component no longer exists")?;
        let connected = connected(a, id);
        if !f.copy && connected.len() > 1 {
            let anchor = a
                .component_structure
                .occurrences
                .iter()
                .filter(|o| {
                    o.parent_occurrence_id == occurrence.parent_occurrence_id
                        && connected.contains(&o.id.0)
                })
                .min_by_key(|o| (!o.grounded, o.id.0));
            if let Some(anchor) = anchor.filter(|o| o.id.0 != id) {
                return Err(format!(
                    "Move {} to place this connected assembly, or create a copy",
                    anchor.name
                ));
            }
        }
        Ok(subtree(
            a,
            if f.copy {
                HashSet::from([id])
            } else {
                connected
            },
        ))
    }
    pub(super) fn move_occurrence_payload(
        &self,
        model: &FormModel<'_>,
        request: &MoveCopyBodyRequest,
    ) -> Result<(&'static str, Value), String> {
        let id = self.move_occurrence().ok_or("Select a component")?;
        let a = model.assembly.ok_or("Assembly structure is unavailable")?;
        let occurrence = a
            .component_structure
            .occurrences
            .iter()
            .find(|o| o.id.0 == id)
            .ok_or("The component no longer exists")?;
        let solution = model
            .assembly_solution
            .ok_or("The component placement is unavailable")?;
        let world = solution
            .occurrence_poses
            .iter()
            .find(|p| p.occurrence_id.0 == id)
            .ok_or("The component has no solved placement")?;
        let delta = DQuat::from_array(request.rotation);
        let pivot = xyz(request.pivot);
        let mut translation = pivot
            + delta * (DVec3::from_array(world.translation) - pivot)
            + xyz(request.translation);
        let mut rotation = delta * DQuat::from_array(world.rotation);
        if let Some(parent) = occurrence.parent_occurrence_id {
            let parent = solution
                .occurrence_poses
                .iter()
                .find(|p| p.occurrence_id == parent)
                .ok_or("The parent has no solved placement")?;
            let inverse = DQuat::from_array(parent.rotation).inverse();
            translation = inverse * (translation - DVec3::from_array(parent.translation));
            rotation = inverse * rotation;
        }
        let local_pose = AssemblyTransformDto {
            translation: translation.to_array(),
            rotation: rotation.normalize().to_array(),
        };
        if request.copy {
            Ok((
                "assembly_duplicate_occurrence",
                json!(limo_cad_sketch::DuplicateOccurrenceRequestDto {
                    occurrence_id: OccurrenceId(id),
                    parent_occurrence_id: occurrence.parent_occurrence_id,
                    local_pose: Some(local_pose)
                }),
            ))
        } else {
            Ok((
                "assembly_set_occurrence_pose",
                json!(limo_cad_sketch::SetOccurrencePoseRequestDto {
                    occurrence_id: OccurrenceId(id),
                    local_pose
                }),
            ))
        }
    }
}

fn subtree(a: &AssemblyDocumentDto, mut ids: HashSet<u64>) -> HashSet<u64> {
    loop {
        let before = ids.len();
        for o in &a.component_structure.occurrences {
            if o.parent_occurrence_id
                .is_some_and(|parent| ids.contains(&parent.0))
            {
                ids.insert(o.id.0);
            }
        }
        if ids.len() == before {
            return ids;
        }
    }
}
