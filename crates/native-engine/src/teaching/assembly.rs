//! Bounded assembly placement shared by Frame admission, fitting and playback.
use super::Frame;
use limo_cad_sketch::AssemblySolutionDto;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
pub struct Pose {
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
}
impl Pose {
    const IDENTITY: Self = Self {
        translation: [0.; 3],
        rotation: [0., 0., 0., 1.],
    };
    fn new(translation: [f64; 3], rotation: [f64; 4]) -> Self {
        let rotation = rotation.map(|v| v as f32);
        let inverse = rotation.iter().map(|v| v * v).sum::<f32>().sqrt().recip();
        Self {
            translation: translation.map(|v| v as f32),
            rotation: rotation.map(|v| v * inverse),
        }
    }
    pub fn transform_point(self, point: [f32; 3]) -> [f32; 3] {
        let [x, y, z, w] = self.rotation;
        let [px, py, pz] = point;
        let t = [
            2. * (y * pz - z * py),
            2. * (z * px - x * pz),
            2. * (x * py - y * px),
        ];
        [
            px + w * t[0] + y * t[2] - z * t[1] + self.translation[0],
            py + w * t[1] + z * t[0] - x * t[2] + self.translation[1],
            pz + w * t[2] + x * t[1] - y * t[0] + self.translation[2],
        ]
    }
}

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
        .map(|b| b.id.0)
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
    let rotation = rotation.map(|v| v as f32);
    let length = rotation.iter().map(|v| v * v).sum::<f32>();
    if translation
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 10_000.)
        || rotation.iter().any(|v| !v.is_finite())
        || !length.is_finite()
        || length <= f32::MIN_POSITIVE
    {
        return Err("Invalid preview assembly transform".into());
    }
    Ok(())
}

/// Apply after Frame validation. Instance poses take precedence over legacy
/// definition poses; a definition with only hidden instances produces no draws.
pub fn body_poses(solution: Option<&AssemblySolutionDto>, body: u64) -> Vec<Pose> {
    let Some(solution) = solution else {
        return vec![Pose::IDENTITY];
    };
    if solution.instance_body_poses.is_empty() {
        return vec![solution
            .body_poses
            .iter()
            .find(|p| p.body_id.0 == body)
            .map(|p| Pose::new(p.translation, p.rotation))
            .unwrap_or(Pose::IDENTITY)];
    }
    solution
        .instance_body_poses
        .iter()
        .filter(|p| p.body_id.0 == body && p.visible)
        .map(|p| Pose::new(p.translation, p.rotation))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame() -> Frame {
        serde_json::from_value(serde_json::json!({"caption":"Package","scene":{"bodies":[{
            "id":1,"name":"Stock","feature_id":2,"faces":[],"edges":[],"mesh":{
            "positions":[0,0,0,10,0,0,0,10,0],"normals":[0,0,1,0,0,1,0,0,1],"indices":[0,1,2]}}],"errors":[]},
            "assembly":{"solved":true,"diagnostics":[],"body_poses":[{"body_id":1,"translation":[9000,0,0],"rotation":[0,0,0,1]}],
            "instance_body_poses":[{"occurrence_id":1,"component_id":1,"body_id":1,"translation":[100,0,0],"rotation":[0,0,1,1],"visible":true},
            {"occurrence_id":2,"component_id":1,"body_id":1,"translation":[300,0,0],"rotation":[0,0,0,1],"visible":true},
            {"occurrence_id":3,"component_id":1,"body_id":1,"translation":[9000,0,0],"rotation":[0,0,0,1],"visible":false}]}}
        )).unwrap()
    }
    #[test]
    fn package_occurrences_share_validation_placement_and_fitting() {
        let frame = frame();
        let bounds = super::super::validate_frames(std::slice::from_ref(&frame)).unwrap();
        let poses = body_poses(frame.assembly.as_ref(), 1);
        assert_eq!(poses.len(), 2);
        let point = poses[0].transform_point([10., 0., 0.]);
        assert!((point[0] - 100.).abs() < 0.001 && (point[1] - 10.).abs() < 0.001);
        assert!(bounds.center[0] > 100. && bounds.center[0] < 300.);
        assert!(
            bounds.radius < 150.,
            "Hidden/legacy poses must not expand instance fitting"
        );
    }
    #[test]
    fn package_assembly_rejects_bad_ownership_transforms_and_drawn_budgets() {
        let mut frame = frame();
        let poses = &mut frame.assembly.as_mut().unwrap().instance_body_poses;
        poses[0].body_id.0 = 77;
        assert!(super::super::validate_frames(&[frame.clone()]).is_err());
        poses_reset(&mut frame);
        frame.assembly.as_mut().unwrap().instance_body_poses[0].rotation = [0.; 4];
        assert!(super::super::validate_frames(&[frame.clone()]).is_err());
        poses_reset(&mut frame);
        frame.assembly.as_mut().unwrap().instance_body_poses[0].translation =
            [10_000., 10_000., 0.];
        assert!(super::super::validate_frames(&[frame.clone()]).is_err());
        poses_reset(&mut frame);
        let pose = frame.assembly.as_ref().unwrap().instance_body_poses[0].clone();
        frame.assembly.as_mut().unwrap().instance_body_poses = (0..256)
            .map(|i| {
                let mut p = pose.clone();
                p.occurrence_id.0 = i + 1;
                p
            })
            .collect();
        frame.scene.bodies[0].mesh.indices = [0, 1, 2].repeat(400);
        assert!(super::super::validate_frames(&[frame])
            .unwrap_err()
            .contains("instances exceed"));
    }
    fn poses_reset(value: &mut Frame) {
        *value = frame();
    }
}
