use super::*;
use serde_json::json;

fn scene() -> SolidSceneDto {
    serde_json::from_value(json!({"bodies":[
        {"id":1,"name":"Back","feature_id":1,"mesh":{"positions":[-10.,-10.,0.,10.,-10.,0.,0.,10.,0.],"normals":[],"indices":[0,1,2]},
         "faces":[{"id":11,"key":"surface","first_index":0,"index_count":3,"plane":null}],"edges":[]},
        {"id":2,"name":"Front occluder","feature_id":2,"mesh":{"positions":[-10.,-10.,5.,10.,-10.,5.,0.,10.,5.],"normals":[],"indices":[0,1,2]},
         "faces":[{"id":22,"key":"surface","first_index":0,"index_count":3,"plane":null}],"edges":[]}
    ],"errors":[]})).unwrap()
}
fn snapshot(ids: &[u64]) -> Snapshot {
    Snapshot {
        owner: "fixture".into(),
        revision: 1,
        instances: ids
            .iter()
            .map(|id| Instance {
                body_id: *id,
                occurrence_id: None,
                transform: Transform::IDENTITY,
            })
            .collect(),
    }
}
fn ray() -> Ray {
    Ray {
        camera: ViewportCamera {
            position: [0., 0., 100.],
            target: [0., 0., 0.],
            up: [0., 1., 0.],
            vertical_fov_degrees: 45.,
        },
        viewport: [800., 600.],
        point: [400., 300.],
    }
}
#[test]
fn physical_hit_retains_foreground_occluders_and_visible_occurrence_identity() {
    let cancel = AtomicBool::new(false);
    let scene = Arc::new(scene());
    let prepared = Prepared::new(scene.clone(), snapshot(&[1, 2]), &cancel).unwrap();
    let hit = prepared.pick(ray(), &cancel).unwrap().unwrap();
    assert_eq!(
        (hit.body_id, hit.face_id),
        (2, 22),
        "a face outside CAM scope still physically occludes a bore"
    );
    let visible = Prepared::new(scene.clone(), snapshot(&[1]), &cancel).unwrap();
    assert_eq!(
        visible.pick(ray(), &cancel).unwrap().unwrap().body_id,
        1,
        "hidden bodies are absent only from the visibility snapshot"
    );
    let mut instances = snapshot(&[1, 1]);
    instances.instances[0].occurrence_id = Some(41);
    instances.instances[1].occurrence_id = Some(42);
    instances.instances[1].transform.translation.z = 8.;
    let repeated = Prepared::new(scene, instances, &cancel).unwrap();
    let hit = repeated.pick(ray(), &cancel).unwrap().unwrap();
    assert_eq!(
        (hit.body_id, hit.face_id, hit.occurrence_id),
        (1, 11, Some(42))
    );
    assert!((hit.point[2] - 8.).abs() < 1e-4);
    let triangles = repeated.face_triangles(1, 11, &cancel).unwrap();
    assert_eq!(triangles.len(), 18);
    assert_eq!(
        triangles[11], 8.,
        "the overlay uses the occurrence pose while saved identity stays canonical"
    );
}
#[test]
fn physical_preflight_rejects_invalid_ranges_indices_positions_and_effective_work() {
    let cancel = AtomicBool::new(false);
    let rejected = |scene, snapshot| Prepared::new(Arc::new(scene), snapshot, &cancel).is_err();
    let mut bad = scene();
    bad.bodies[0].faces[0].first_index = u32::MAX;
    assert!(rejected(bad, snapshot(&[1])));
    let mut bad = scene();
    bad.bodies[0].faces[0].index_count = 4;
    assert!(rejected(bad, snapshot(&[1])));
    let mut bad = scene();
    bad.bodies[0].mesh.indices.extend([0, 1, 2]);
    bad.bodies[0].faces[0].first_index = 1;
    assert!(
        rejected(bad, snapshot(&[1])),
        "a face range must start at a whole triangle"
    );
    let mut bad = scene();
    bad.bodies[0].mesh.indices[2] = u32::MAX;
    assert!(rejected(bad, snapshot(&[1])));
    let mut bad = scene();
    bad.bodies[0].mesh.positions[2] = f32::NAN;
    assert!(rejected(bad, snapshot(&[1])));
    let mut bad = scene();
    bad.bodies[0]
        .mesh
        .positions
        .resize((MAX_POINTS + 1) * 3, 0.);
    assert!(rejected(bad, snapshot(&[1])));
    let mut repeated = scene();
    repeated.bodies[0].mesh.indices = [0, 1, 2].repeat(256);
    repeated.bodies[0].faces[0].index_count = 768;
    let face = repeated.bodies[0].faces[0].clone();
    repeated.bodies[0].faces = vec![face; 129];
    assert!(rejected(repeated, snapshot(&[1, 1])));
    let mut invalid_pose = snapshot(&[1]);
    invalid_pose.instances[0].transform.translation.x = f32::INFINITY;
    assert!(rejected(scene(), invalid_pose));
}
#[test]
fn cancelled_physical_work_and_invalid_camera_never_return_a_candidate() {
    let cancel = AtomicBool::new(false);
    let prepared = Prepared::new(Arc::new(scene()), snapshot(&[1]), &cancel).unwrap();
    let mut invalid = ray();
    invalid.camera.position[0] = f32::NAN;
    assert!(prepared.pick(invalid, &cancel).is_err());
    cancel.store(true, Ordering::Release);
    assert!(prepared.pick(ray(), &cancel).is_err());
    assert!(prepared.face_triangles(1, 11, &cancel).is_err());
    assert!(Prepared::new(Arc::new(scene()), snapshot(&[1]), &cancel).is_err());
}
