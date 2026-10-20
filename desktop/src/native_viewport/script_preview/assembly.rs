//! Admission and bounds use the production renderer's occurrence transforms.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn validate(frame: &Frame) -> Result<(), String> {
    let Some(solution) = &frame.assembly else {
        return Ok(());
    };
    if !solution.solved {
        return Err(
            "Assembly preview requires a solved assembly; inspect its diagnostics in the design"
                .into(),
        );
    }
    if solution.body_poses.len() > 256
        || solution.instance_body_poses.len() > 256
        || solution.occurrence_poses.len() > 256
        || solution.diagnostics.len() > 256
    {
        return Err("Assembly preview exceeds its pose budget".into());
    }
    let bodies = frame
        .scene
        .bodies
        .iter()
        .map(|body| body.id.0)
        .collect::<BTreeSet<_>>();
    if bodies.len() != frame.scene.bodies.len() {
        return Err("Preview contains duplicate body identities".into());
    }
    let mut legacy = BTreeSet::new();
    for pose in &solution.body_poses {
        if !bodies.contains(&pose.body_id.0) || !legacy.insert(pose.body_id.0) {
            return Err("Invalid preview body pose identity".into());
        }
        validate_pose(pose.translation, pose.rotation)?;
    }
    let mut instances = BTreeSet::new();
    let mut components = BTreeMap::new();
    for pose in &solution.instance_body_poses {
        if !bodies.contains(&pose.body_id.0)
            || !instances.insert((pose.occurrence_id.0, pose.body_id.0))
            || components
                .insert(pose.occurrence_id.0, pose.component_id.0)
                .is_some_and(|old| old != pose.component_id.0)
        {
            return Err("Invalid preview occurrence identity".into());
        }
        validate_pose(pose.translation, pose.rotation)?;
    }
    let mut occurrences = BTreeSet::new();
    for pose in &solution.occurrence_poses {
        if !occurrences.insert(pose.occurrence_id.0) {
            return Err("Duplicate preview occurrence pose".into());
        }
        validate_pose(pose.translation, pose.rotation)?;
    }
    Ok(())
}

fn validate_pose(translation: [f64; 3], rotation: [f64; 4]) -> Result<(), String> {
    let rotation = Quat::from_xyzw(
        rotation[0] as f32,
        rotation[1] as f32,
        rotation[2] as f32,
        rotation[3] as f32,
    );
    let length = rotation.length_squared();
    if translation
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 10_000.0)
        || !rotation.is_finite()
        || !length.is_finite()
        || length <= f32::MIN_POSITIVE
    {
        return Err("Invalid preview assembly transform".into());
    }
    Ok(())
}

pub(super) fn transforms(
    solution: Option<&limo_cad_sketch::AssemblySolutionDto>,
    body_id: u64,
) -> Vec<Transform> {
    let Some(solution) = solution else {
        return vec![Transform::IDENTITY];
    };
    if solution.instance_body_poses.is_empty() {
        return vec![instance_body_pose_transform(
            &[],
            &solution.body_poses,
            body_id,
            None,
        )];
    }
    solution
        .instance_body_poses
        .iter()
        .filter(|pose| pose.body_id.0 == body_id && pose.visible)
        .map(|pose| {
            instance_body_pose_transform(
                &solution.instance_body_poses,
                &solution.body_poses,
                body_id,
                Some(pose.occurrence_id.0),
            )
        })
        .collect()
}
