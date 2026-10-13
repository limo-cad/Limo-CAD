//! Saved presentation and printing offsets use the same occurrence hierarchy.
use super::*;

/// World-axis translation and rotation about an occurrence's current origin.
/// Children inherit their parent's movement. Exact solids and joints are unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewOccurrenceOffsetDto {
    pub occurrence_id: OccurrenceId,
    pub translation: [f64; 3],
    #[serde(default = "identity_rotation")]
    pub rotation: [f64; 4],
}

fn identity_rotation() -> [f64; 4] {
    [0., 0., 0., 1.]
}

pub fn resolve_view_layout(
    structure: &ComponentStructureDto,
    solution: &AssemblySolutionDto,
    offsets: &[ViewOccurrenceOffsetDto],
) -> Result<AssemblySolutionDto, String> {
    let mut by_id = HashMap::new();
    for offset in offsets {
        if by_id.insert(offset.occurrence_id, offset).is_some() {
            return Err("duplicate occurrence in named-view offsets".into());
        }
        if !structure
            .occurrences
            .iter()
            .any(|o| o.id == offset.occurrence_id)
        {
            return Err(format!(
                "Unknown layout occurrence {}",
                offset.occurrence_id.0
            ));
        }
        validate_transform(
            AssemblyTransformDto {
                translation: offset.translation,
                rotation: offset.rotation,
            },
            "view occurrence offset",
        )?;
        if offset.translation.iter().any(|v| v.abs() > 1e6) {
            return Err("view occurrence offset must stay within 1000000 mm".into());
        }
    }
    let originals: HashMap<_, _> = solution
        .occurrence_poses
        .iter()
        .map(|p| {
            (
                p.occurrence_id,
                RigidPose {
                    translation: p.translation,
                    rotation: p.rotation,
                },
            )
        })
        .collect();
    let parents: HashMap<_, _> = structure
        .occurrences
        .iter()
        .map(|o| (o.id, o.parent_occurrence_id))
        .collect();
    let mut deltas = HashMap::new();
    fn delta_for(
        id: OccurrenceId,
        parents: &HashMap<OccurrenceId, Option<OccurrenceId>>,
        originals: &HashMap<OccurrenceId, RigidPose>,
        offsets: &HashMap<OccurrenceId, &ViewOccurrenceOffsetDto>,
        deltas: &mut HashMap<OccurrenceId, RigidPose>,
        visiting: &mut HashSet<OccurrenceId>,
    ) -> Result<RigidPose, String> {
        if let Some(delta) = deltas.get(&id) {
            return Ok(*delta);
        }
        if !visiting.insert(id) {
            return Err("cyclic occurrence hierarchy in view layout".into());
        }
        let inherited = match parents.get(&id).copied().flatten() {
            Some(parent) => delta_for(parent, parents, originals, offsets, deltas, visiting)?,
            None => RigidPose::IDENTITY,
        };
        let delta = if let Some(offset) = offsets.get(&id) {
            let origin = originals
                .get(&id)
                .ok_or_else(|| format!("Occurrence {} has no solved pose", id.0))?;
            let pivot = inherited.compose(*origin).translation;
            let rotation = RigidPose::from_transform(AssemblyTransformDto {
                translation: [0.; 3],
                rotation: offset.rotation,
            });
            RigidPose::translation(add(pivot, offset.translation))
                .compose(rotation)
                .compose(RigidPose::translation(scale(pivot, -1.)))
                .compose(inherited)
        } else {
            inherited
        };
        visiting.remove(&id);
        deltas.insert(id, delta);
        Ok(delta)
    }
    let mut result = solution.clone();
    for pose in &mut result.occurrence_poses {
        let delta = delta_for(
            pose.occurrence_id,
            &parents,
            &originals,
            &by_id,
            &mut deltas,
            &mut HashSet::new(),
        )?;
        let moved = delta.compose(RigidPose {
            translation: pose.translation,
            rotation: pose.rotation,
        });
        pose.translation = moved.translation;
        pose.rotation = moved.rotation;
    }
    for pose in &mut result.instance_body_poses {
        let delta = delta_for(
            pose.occurrence_id,
            &parents,
            &originals,
            &by_id,
            &mut deltas,
            &mut HashSet::new(),
        )?;
        let moved = delta.compose(RigidPose {
            translation: pose.translation,
            rotation: pose.rotation,
        });
        pose.translation = moved.translation;
        pose.rotation = moved.rotation;
    }
    for pose in &mut result.body_poses {
        if let Some(instance) = result
            .instance_body_poses
            .iter()
            .find(|p| p.body_id == pose.body_id)
        {
            pose.translation = instance.translation;
            pose.rotation = instance.rotation;
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn component_validation_rejects_rotation_norm_overflow_before_normalization() {
        let mut structure: ComponentStructureDto = serde_json::from_str(
            r#"{
            "definitions":[{"id":1,"name":"Part"}],
            "occurrences":[{"id":1,"name":"Instance","component_id":1}],
            "next_component_id":2,"next_occurrence_id":2
        }"#,
        )
        .unwrap();
        structure.occurrences[0].local_pose.rotation = [0., 0., 0., 2.];
        structure.validate().unwrap();
        structure.occurrences[0].local_pose.rotation = [1e200; 4];
        assert!(structure.validate().is_err());
        structure.occurrences[0].local_pose = Default::default();
        structure.definitions[0].local_coordinate_system.rotation = [1e200; 4];
        assert!(structure.validate().is_err());
    }
    #[test]
    fn rotating_a_parent_keeps_its_multipart_children_and_repeated_sibling() {
        let structure = ComponentStructureDto {
            occurrences: [(1, None), (2, Some(1)), (3, None)]
                .into_iter()
                .map(|(id, parent)| ComponentOccurrenceDto {
                    id: OccurrenceId(id),
                    name: id.to_string(),
                    component_id: ComponentId(1),
                    parent_occurrence_id: parent.map(OccurrenceId),
                    local_pose: Default::default(),
                    visible: true,
                    grounded: false,
                })
                .collect(),
            ..Default::default()
        };
        let solution = AssemblySolutionDto {
            occurrence_poses: [(1, [10., 0., 0.]), (2, [12., 0., 0.]), (3, [30., 0., 0.])]
                .into_iter()
                .map(|(id, translation)| OccurrencePoseDto {
                    occurrence_id: OccurrenceId(id),
                    component_id: ComponentId(1),
                    translation,
                    rotation: identity_rotation(),
                })
                .collect(),
            instance_body_poses: [
                (1, 1, [10., 0., 0.]),
                (2, 2, [12., 0., 0.]),
                (3, 1, [30., 0., 0.]),
            ]
            .into_iter()
            .map(|(id, body, translation)| InstanceBodyPoseDto {
                occurrence_id: OccurrenceId(id),
                component_id: ComponentId(1),
                body_id: BodyId(body),
                translation,
                rotation: identity_rotation(),
                visible: true,
            })
            .collect(),
            solved: true,
            ..Default::default()
        };
        let half = std::f64::consts::FRAC_1_SQRT_2;
        let result = resolve_view_layout(
            &structure,
            &solution,
            &[ViewOccurrenceOffsetDto {
                occurrence_id: OccurrenceId(1),
                translation: [0., 5., 0.],
                rotation: [0., 0., half, half],
            }],
        )
        .unwrap();
        assert!((result.instance_body_poses[1].translation[0] - 10.).abs() < 1e-8);
        assert!((result.instance_body_poses[1].translation[1] - 7.).abs() < 1e-8);
        assert_eq!(
            result.instance_body_poses[2],
            solution.instance_body_poses[2]
        );
        assert_eq!(solution.instance_body_poses[1].translation, [12., 0., 0.]);
        assert!(resolve_view_layout(
            &structure,
            &solution,
            &[ViewOccurrenceOffsetDto {
                occurrence_id: OccurrenceId(1),
                translation: [0.; 3],
                rotation: [1e200; 4],
            }],
        )
        .is_err());
    }
}
