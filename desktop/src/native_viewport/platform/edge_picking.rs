//! Screen-sized topology picking, with exact rendered-face occlusion. A face
//! raycast cannot select ordinary edges: their tessellation has zero area.
use super::*;

fn project(
    basis: &CameraProjectionBasis,
    viewport: (f32, f32),
    point: Vec3,
) -> Option<(Vec2, f32)> {
    let offset = point - basis.origin;
    let depth = offset.dot(basis.forward);
    if !depth.is_finite() || depth < 1e-4 {
        return None;
    }
    let pixel = Vec2::new(
        (offset.dot(basis.right) / (depth * basis.tangent * basis.aspect) + 1.) * 0.5 * viewport.0,
        (1. - offset.dot(basis.up) / (depth * basis.tangent)) * 0.5 * viewport.1,
    );
    pixel.is_finite().then_some((pixel, depth))
}
#[cfg(test)]
fn pick(
    scene: &SolidSceneDto,
    camera: ViewportCamera,
    viewport: (f32, f32),
    cursor: [f32; 2],
    hidden: &[u64],
    poses: &[BodyPoseDto],
    instances: &[InstanceBodyPoseDto],
) -> Option<NativePick> {
    pick_edges(
        scene,
        (camera, viewport, cursor),
        hidden,
        poses,
        instances,
        NativePickPurpose::RefinableEdge,
    )
}
pub(super) fn pick_edges(
    scene: &SolidSceneDto,
    (camera, viewport, cursor): (ViewportCamera, (f32, f32), [f32; 2]),
    hidden: &[u64],
    poses: &[BodyPoseDto],
    instances: &[InstanceBodyPoseDto],
    purpose: NativePickPurpose,
) -> Option<NativePick> {
    let straight = purpose == NativePickPurpose::StraightEdge;
    let vertices = purpose == NativePickPurpose::Vertex;
    let basis = camera_projection(camera, viewport)?;
    let cursor = Vec2::from_array(cursor);
    if !cursor.is_finite() {
        return None;
    }
    let mut candidates = Vec::new();
    for body in scene
        .bodies
        .iter()
        .filter(|body| !hidden.contains(&body.id.0))
    {
        let placements = if instances.is_empty() {
            vec![(None, body_pose_transform(poses, body.id.0))]
        } else {
            instances
                .iter()
                .filter(|p| p.body_id == body.id && p.visible)
                .map(|p| {
                    (
                        Some(p.occurrence_id.0),
                        instance_body_pose_transform(
                            instances,
                            poses,
                            body.id.0,
                            Some(p.occurrence_id.0),
                        ),
                    )
                })
                .collect()
        };
        for (occurrence, transform) in placements {
            for edge in body.edges.iter().filter(|edge| {
                vertices
                    || purpose == NativePickPurpose::Edge
                    || if straight {
                        edge_is_straight(edge)
                    } else {
                        edge.refinable
                    }
            }) {
                let mut best = None;
                let count = if vertices {
                    usize::from(!edge.points.is_empty()) * 2
                } else {
                    edge.points.len().saturating_sub(1)
                };
                for index in 0..count {
                    let pair = if vertices {
                        let p = &edge.points[if index == 0 { 0 } else { edge.points.len() - 1 }];
                        [p, p]
                    } else {
                        [&edge.points[index], &edge.points[index + 1]]
                    };
                    let [mut a, mut b] = pair.map(|p| {
                        transform.transform_point(Vec3::new(p.x as f32, p.y as f32, p.z as f32))
                    });
                    if !a.is_finite() || !b.is_finite() {
                        continue;
                    }
                    let da = (a - basis.origin).dot(basis.forward);
                    let db = (b - basis.origin).dot(basis.forward);
                    if da < 1e-4 && db < 1e-4 {
                        continue;
                    }
                    if da < 1e-4 {
                        a = a.lerp(b, (1e-4 - da) / (db - da));
                    }
                    if db < 1e-4 {
                        b = b.lerp(a, (1e-4 - db) / (da - db));
                    }
                    let Some((sa, da)) = project(&basis, viewport, a) else {
                        continue;
                    };
                    let Some((sb, db)) = project(&basis, viewport, b) else {
                        continue;
                    };
                    let delta = sb - sa;
                    let t = if delta.length_squared() > 1e-10 {
                        ((cursor - sa).dot(delta) / delta.length_squared()).clamp(0., 1.)
                    } else {
                        0.
                    };
                    let screen = sa + delta * t;
                    let distance = cursor.distance_squared(screen);
                    if distance > if vertices { 81. } else { 49. } {
                        continue;
                    }
                    let world = a.lerp(b, t * da / ((1. - t) * db + t * da));
                    let depth = basis.origin.distance(world);
                    if best.as_ref().is_none_or(|(old, old_depth, _, _)| {
                        distance < *old || (distance == *old && depth < *old_depth)
                    }) {
                        best = Some((distance, depth, screen, world));
                    }
                }
                if let Some((distance, depth, screen, world)) = best {
                    candidates.push((
                        distance, depth, screen, world, body.id.0, occurrence, edge.id.0,
                    ));
                }
            }
        }
    }
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    for (_, depth, screen, world, body, occurrence, edge) in candidates {
        let face = pick_occt_scene(
            scene,
            (camera, viewport, screen.x, screen.y),
            hidden,
            poses,
            instances,
            NativePickPurpose::Geometry,
        );
        let tolerance = (depth * 1e-5).max(1e-4);
        if face
            .as_ref()
            .is_some_and(|face| face.distance < f64::from(depth - tolerance))
        {
            continue;
        }
        return Some(NativePick {
            body_id: body,
            occurrence_id: occurrence,
            face_id: face
                .filter(|f| f.body_id == body && f.occurrence_id == occurrence)
                .map(|f| f.face_id)
                .unwrap_or(0),
            edge_id: Some(edge),
            point: world.to_array(),
            distance: f64::from(depth),
            connector_kind: None,
            connector_origin: None,
            connector_primary_axis: None,
            connector_secondary_axis: None,
            connector_radius: None,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn scene() -> SolidSceneDto {
        serde_json::from_value(json!({"bodies":[{"id":1,"feature_id":1,"name":"Plate",
            "mesh":{"positions":[-5.,-5.,1.,5.,-5.,1.,5.,5.,1.,-5.,5.,1.],"normals":[],"indices":[0,1,2,0,2,3]},
            "faces":[{"id":10,"key":"front","first_index":0,"index_count":6}],
            "edges":[{"id":1,"key":"hidden","points":[{"x":-2.,"y":0.,"z":0.},{"x":2.,"y":0.,"z":0.}]},
                {"id":2,"key":"visible","points":[{"x":-2.,"y":0.,"z":1.},{"x":2.,"y":0.,"z":1.}]}]}],"errors":[]})).unwrap()
    }
    fn camera() -> ViewportCamera {
        ViewportCamera {
            position: [0., 0., 20.],
            target: [0., 0., 0.],
            up: [0., 1., 0.],
            vertical_fov_degrees: 45.,
        }
    }
    #[test]
    fn vertex_snap_has_pixel_tolerance_and_rejects_occluded_or_hidden_points() {
        let mut scene = scene();
        for edge in &mut scene.bodies[0].edges {
            edge.refinable = false;
        }
        let viewport = (800., 600.);
        let basis = camera_projection(camera(), viewport).unwrap();
        let (screen, _) = project(&basis, viewport, Vec3::new(-2., 0., 1.)).unwrap();
        let pick = |scene: &SolidSceneDto, cursor: [f32; 2], hidden: &[u64]| {
            pick_edges(
                scene,
                (camera(), viewport, cursor),
                hidden,
                &[],
                &[],
                NativePickPurpose::Vertex,
            )
        };
        let hit = pick(&scene, [screen.x - 6., screen.y + 1.], &[]).unwrap();
        assert_eq!(hit.point, [-2., 0., 1.]);
        assert!(pick(&scene, [screen.x - 10., screen.y], &[]).is_none());
        assert!(pick(&scene, screen.to_array(), &[1]).is_none());
        scene.bodies[0].edges.retain(|e| e.id.0 == 1);
        let (rear, _) = project(&basis, viewport, Vec3::new(-2., 0., 0.)).unwrap();
        assert!(
            pick(&scene, rear.to_array(), &[]).is_none(),
            "Hidden vertices cannot snap through the front face"
        );
    }
    #[test]
    fn picks_visible_edges_with_pixel_tolerance_and_rejects_occluded_hidden_and_ineligible_edges() {
        let mut scene = scene();
        for y in [300., 305.] {
            assert_eq!(
                pick(&scene, camera(), (800., 600.), [400., y], &[], &[], &[])
                    .unwrap()
                    .edge_id,
                Some(2)
            );
        }
        assert!(pick(&scene, camera(), (800., 600.), [400., 308.], &[], &[], &[]).is_none());
        assert!(pick(&scene, camera(), (800., 600.), [400., 300.], &[1], &[], &[]).is_none());
        scene.bodies[0].edges[1].refinable = false;
        assert!(
            pick(&scene, camera(), (800., 600.), [400., 300.], &[], &[], &[]).is_none(),
            "the rear edge must remain occluded when the front edge is ineligible"
        );
    }
    #[test]
    fn respects_occurrence_placement_and_visibility() {
        let scene = scene();
        let mut pose = InstanceBodyPoseDto {
            occurrence_id: limo_cad_sketch::OccurrenceId(42),
            component_id: limo_cad_sketch::ComponentId(7),
            body_id: scene.bodies[0].id,
            translation: [10., 0., 0.],
            rotation: [0., 0., 0., 1.],
            visible: true,
        };
        let mut camera = camera();
        camera.position[0] = 10.;
        camera.target[0] = 10.;
        let hit = pick(
            &scene,
            camera,
            (800., 600.),
            [400., 300.],
            &[],
            &[],
            &[pose],
        )
        .unwrap();
        assert_eq!(hit.occurrence_id, Some(42));
        assert_eq!(hit.edge_id, Some(2));
        assert!((hit.point[0] - 10.).abs() < 1e-4);
        pose.visible = false;
        assert!(pick(
            &scene,
            camera,
            (800., 600.),
            [400., 300.],
            &[],
            &[],
            &[pose]
        )
        .is_none());
    }
}
