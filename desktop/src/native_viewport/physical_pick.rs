//! Bounded immutable input to the ordinary physical face picker. Preparing,
//! raycasting and extracting face previews belong on a cancellable worker.
//! Every visible face remains an occluder, including faces outside CAM scope.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) const MAX_TRIANGLES: usize = 65_536;
pub(crate) const MAX_POINTS: usize = 131_072;
pub(crate) const MAX_INSTANCES: usize = 2_048;
pub(crate) const MAX_FACES: usize = 16_384;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Instance {
    pub body_id: u64,
    pub occurrence_id: Option<u64>,
    pub transform: Transform,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Snapshot {
    pub owner: String,
    pub revision: u64,
    pub instances: Vec<Instance>,
}
pub(crate) fn snapshot(world: &World, hidden: &[u64]) -> Result<Snapshot, String> {
    if section_view::active(world) {
        return Err("Close section inspection before selecting source faces".into());
    }
    let model = world.resource::<ModelResource>();
    if model.document.scene.bodies.len() > MAX_INSTANCES
        || model.instance_body_poses.len() > MAX_INSTANCES
        || model.body_poses.len() > MAX_INSTANCES
    {
        return Err("Viewport picking exceeds its body budget; use the geometry fields".into());
    }
    let mut instances = Vec::new();
    for body in &model.document.scene.bodies {
        if hidden.contains(&body.id.0) {
            continue;
        }
        for occurrence_id in visible_body_occurrences(model, body.id.0) {
            let transform = instance_body_pose_transform(
                &model.instance_body_poses,
                &model.body_poses,
                body.id.0,
                occurrence_id,
            );
            if !transform.translation.is_finite()
                || !transform.rotation.is_finite()
                || !transform.scale.is_finite()
            {
                return Err("Visible geometry has an invalid transform".into());
            }
            instances.push(Instance {
                body_id: body.id.0,
                occurrence_id,
                transform,
            });
            if instances.len() > MAX_INSTANCES {
                return Err(
                    "Viewport picking exceeds its occurrence budget; use the geometry fields"
                        .into(),
                );
            }
        }
    }
    Ok(Snapshot {
        owner: model.session_id.clone(),
        revision: model.geometry_revision,
        instances,
    })
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Ray {
    pub camera: ViewportCamera,
    pub viewport: [f32; 2],
    pub point: [f32; 2],
}
pub(crate) struct Prepared {
    scene: Arc<SolidSceneDto>,
    pub snapshot: Snapshot,
}
fn cancelled(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        Err("Geometry picking was cancelled".into())
    } else {
        Ok(())
    }
}
impl Prepared {
    pub(crate) fn new(
        scene: Arc<SolidSceneDto>,
        snapshot: Snapshot,
        cancel: &AtomicBool,
    ) -> Result<Self, String> {
        if snapshot.instances.len() > MAX_INSTANCES {
            return Err("Viewport occurrence budget exceeded".into());
        }
        let mut points = 0usize;
        let mut triangles = 0usize;
        let mut faces = 0usize;
        let mut seen = std::collections::HashSet::new();
        for instance in &snapshot.instances {
            cancelled(cancel)?;
            if !instance.transform.translation.is_finite()
                || !instance.transform.rotation.is_finite()
                || !instance.transform.scale.is_finite()
            {
                return Err("Visible geometry has an invalid transform".into());
            }
            let body = scene
                .bodies
                .iter()
                .find(|body| body.id.0 == instance.body_id)
                .ok_or("Visible body is unavailable")?;
            faces = faces
                .checked_add(body.faces.len())
                .ok_or("Geometry face budget overflow")?;
            if faces > MAX_FACES {
                return Err(
                    "Viewport picking exceeds its face budget; use the geometry fields".into(),
                );
            }
            if seen.insert(body.id) {
                points = points
                    .checked_add(body.mesh.positions.len() / 3)
                    .ok_or("Geometry point budget overflow")?;
                if points > MAX_POINTS {
                    return Err(
                        "Viewport picking exceeds its source point budget; use the geometry fields"
                            .into(),
                    );
                }
                if body.mesh.positions.len() % 3 != 0
                    || body.mesh.positions.iter().any(|v| !v.is_finite())
                {
                    return Err("Visible geometry has invalid positions".into());
                }
            }
            for face in &body.faces {
                cancelled(cancel)?;
                let start = face.first_index as usize;
                let count = face.index_count as usize;
                let end = start
                    .checked_add(count)
                    .ok_or("Invalid visible face range")?;
                if !start.is_multiple_of(3)
                    || !count.is_multiple_of(3)
                    || end > body.mesh.indices.len()
                {
                    return Err("Invalid visible face range".into());
                }
                triangles = triangles
                    .checked_add(count / 3)
                    .ok_or("Geometry triangle budget overflow")?;
                if triangles > MAX_TRIANGLES {
                    return Err("Viewport picking exceeds its physical triangle budget; use the geometry fields".into());
                }
                for index in &body.mesh.indices[start..end] {
                    let point = mesh_position(body, *index).ok_or("Invalid visible face index")?;
                    if !instance.transform.transform_point(point).is_finite() {
                        return Err("Visible geometry exceeds the finite viewport range".into());
                    }
                }
            }
        }
        Ok(Self { scene, snapshot })
    }
    pub(crate) fn pick(&self, ray: Ray, cancel: &AtomicBool) -> Result<Option<NativePick>, String> {
        let (origin, direction, factor) = camera_pick_ray(
            ray.camera,
            (ray.viewport[0], ray.viewport[1]),
            ray.point[0],
            ray.point[1],
        )
        .ok_or("The viewport camera is unavailable")?;
        let precise_ray = (origin.as_dvec3(), direction.as_dvec3().normalize());
        let mut best = None;
        for instance in &self.snapshot.instances {
            cancelled(cancel)?;
            let body = self
                .scene
                .bodies
                .iter()
                .find(|body| body.id.0 == instance.body_id)
                .unwrap();
            pick_body(
                body,
                instance.occurrence_id,
                instance.transform,
                precise_ray,
                factor,
                &mut best,
                NativePickPurpose::Geometry,
            );
        }
        Ok(best)
    }
    pub(crate) fn face_triangles(
        &self,
        body_id: u64,
        face_id: u64,
        cancel: &AtomicBool,
    ) -> Result<Vec<f32>, String> {
        let body = self
            .scene
            .bodies
            .iter()
            .find(|body| body.id.0 == body_id)
            .ok_or("Picked body is unavailable")?;
        let face = body
            .faces
            .iter()
            .find(|face| face.id.0 == face_id)
            .ok_or("Picked face is unavailable")?;
        let start = face.first_index as usize;
        let end = start
            .checked_add(face.index_count as usize)
            .filter(|end| *end <= body.mesh.indices.len())
            .ok_or("Invalid picked face range")?;
        let mut result = Vec::new();
        for instance in self
            .snapshot
            .instances
            .iter()
            .filter(|instance| instance.body_id == body_id)
        {
            cancelled(cancel)?;
            for index in &body.mesh.indices[start..end] {
                result.extend(
                    instance
                        .transform
                        .transform_point(
                            mesh_position(body, *index).ok_or("Invalid picked face index")?,
                        )
                        .to_array(),
                );
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
#[path = "physical_pick/tests.rs"]
mod tests;
