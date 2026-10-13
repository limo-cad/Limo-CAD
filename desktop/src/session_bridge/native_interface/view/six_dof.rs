//! The release shell's object-motion camera convention on the native camera DTO.
use super::super::controller::six_dof::Motion;
use super::*;
use bevy::prelude::{Quat, Resource};

#[derive(Resource, Default)]
struct Pivot {
    stamp: Option<(String, [u32; 2])>,
    point: Option<Vec3>,
}

fn pivot(world: &mut World, session: &str) -> Option<Vec3> {
    let (stamp, _) = native_viewport::interface_navigation_source(world);
    if let Some(cache) = world.get_resource::<Pivot>() {
        if cache
            .stamp
            .as_ref()
            .is_some_and(|(id, saved)| id == session && *saved == stamp)
        {
            return cache.point;
        }
    }
    let (_, presentation) = native_viewport::interface_navigation_source(world);
    let geometry = native_viewport::interface_geometry(world);
    let bounds = target_bounds(world, geometry, presentation, Target::Solids).or_else(|| {
        target_bounds(
            world,
            native_viewport::interface_geometry(world),
            presentation,
            Target::All,
        )
    });
    let point = bounds.map(|bounds| bounds.min * 0.5 + bounds.max * 0.5);
    world.insert_resource(Pivot {
        stamp: Some((session.into(), stamp)),
        point,
    });
    point
}

pub(in super::super) fn apply(
    world: &mut World,
    owner: &DocumentContext,
    motion: Motion,
    seconds: f32,
    speed: f32,
) -> Result<(), String> {
    let (session, camera) = native_viewport::interface_camera_snapshot(world);
    if owner.document_id != session {
        return Err("The rendered document is not current".into());
    }
    let center = pivot(world, &session).unwrap_or(Vec3::from_array(camera.target));
    let camera = move_camera(camera, center, motion, seconds, speed)?;
    native_viewport::apply_interface_view(world, &session, Some(camera), None)?;
    super::cancel(
        world,
        "Camera transition interrupted by 3D mouse navigation",
    );
    Ok(())
}

fn move_camera(
    mut camera: ViewportCamera,
    pivot: Vec3,
    motion: Motion,
    seconds: f32,
    speed: f32,
) -> Result<ViewportCamera, String> {
    if !pivot.is_finite()
        || !seconds.is_finite()
        || !speed.is_finite()
        || motion
            .translation
            .iter()
            .chain(&motion.rotation)
            .any(|n| !n.is_finite())
    {
        return Err("3D mouse motion must be finite".into());
    }
    let (target, _, frame) = super::motion::pose(camera)?;
    let mut position = Vec3::from_array(camera.position);
    let mut target = target;
    let right = frame * Vec3::X;
    let up = frame * Vec3::Y;
    let forward = frame * Vec3::NEG_Z;
    let translation = motion.translation.map(|v| v.clamp(-1., 1.));
    let rotation = motion.rotation.map(|v| v.clamp(-1., 1.));
    let dt = seconds.clamp(0.001, 0.05);
    let speed = speed.clamp(0.25, 3.);
    let translation_speed = position.distance(pivot).max(1.) * 0.9 * dt * speed;
    let delta = (-right * translation[0] - up * translation[2]) * translation_speed;
    position += delta;
    target += delta;
    if translation[1] != 0. {
        let offset = position - target;
        let distance = offset.length();
        if distance > 1e-6 {
            let next = (distance * (translation[1] * 0.9 * dt * speed).exp()).clamp(2., 5_000.);
            position = target + offset * (next / distance);
        }
    }
    let angle = 1.65 * dt * speed;
    let turn = Quat::from_axis_angle(up, -rotation[2] * angle)
        * Quat::from_axis_angle(right, -rotation[0] * angle)
        * Quat::from_axis_angle(forward, -rotation[1] * angle);
    camera.position = (pivot + turn * (position - pivot)).to_array();
    camera.target = (pivot + turn * (target - pivot)).to_array();
    camera.up = (turn * Vec3::from_array(camera.up)).normalize().to_array();
    super::motion::pose(camera)?;
    Ok(camera)
}

#[cfg(test)]
mod tests;
