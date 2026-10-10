use super::*;
use serde_json::json;

fn setup() -> CamSetupDto {
    serde_json::from_value(json!({"id":1,"name":"Test","body_ids":[1],
        "stock":{"min":{"x":-10.,"y":-10.,"z":-10.},"max":{"x":30.,"y":20.,"z":2.}}}))
    .unwrap()
}
fn point(p: [f64; 3]) -> serde_json::Value {
    json!({"x":p[0],"y":p[1],"z":p[2]})
}
fn plane(normal: [f64; 3]) -> serde_json::Value {
    json!({"origin":[0.,0.,0.],"u":[1.,0.,0.],"v":[0.,1.,0.],"normal":normal})
}
fn face(key: &str, normal: [f64; 3], edges: Vec<String>) -> serde_json::Value {
    json!({"id":1,"key":key,"first_index":0,"index_count":0,"plane":plane(normal),"edge_keys":edges})
}
fn edge(key: String, a: [f64; 3], b: [f64; 3]) -> serde_json::Value {
    json!({"id":1,"key":key,"points":[point(a),point(b)]})
}
fn bevel() -> SolidSceneDto {
    let upper = [[1., 1., 0.], [19., 1., 0.], [19., 9., 0.], [1., 9., 0.]];
    let lower = [
        [0., 0., -1.],
        [20., 0., -1.],
        [20., 10., -1.],
        [0., 10., -1.],
    ];
    let q = std::f64::consts::FRAC_1_SQRT_2;

    let normals = [[0., q, -q], [-q, 0., -q], [0., -q, -q], [q, 0., -q]];
    let mut edges = Vec::new();
    let mut faces = vec![face(
        "top",
        [0., 0., 1.],
        (0..4).map(|i| format!("u{i}")).collect(),
    )];
    for i in 0..4 {
        let j = (i + 1) % 4;
        edges.extend([
            edge(format!("u{i}"), upper[i], upper[j]),
            edge(format!("l{i}"), lower[i], lower[j]),
            edge(format!("b{i}"), lower[i], upper[i]),
        ]);
        faces.push(face(
            &format!("bevel{i}"),
            normals[i],
            vec![
                format!("u{i}"),
                format!("l{i}"),
                format!("b{i}"),
                format!("b{j}"),
            ],
        ));
    }
    serde_json::from_value(json!({"bodies":[{"id":1,"name":"Block","feature_id":1,
        "mesh":{"positions":[],"normals":[],"indices":[]},"edges":edges,"faces":faces}],"errors":[]})).unwrap()
}
fn reference(keys: &[&str], reversed: bool) -> CamChainRefDto {
    CamChainRefDto {
        source: CamChainSource::Model,
        keys: keys.iter().map(|k| format!("edge:1:{k}")).collect(),
        reversed,
    }
}

#[test]
fn upper_lower_and_open_modeled_bevels_use_the_same_upper_rim() {
    let scene = bevel();
    let setup = setup();
    let upper = resolve(&scene, &setup, &reference(&["u0", "u1", "u2", "u3"], false)).unwrap();
    let lower = resolve(&scene, &setup, &reference(&["l0", "l1", "l2", "l3"], false)).unwrap();
    assert_eq!(upper.path, lower.path);
    assert_eq!(upper.width, 1.);
    assert_eq!(upper.top_z, 0.);
    assert_eq!(upper.wall_side, ContourCompensation::Inside);
    assert!(upper.closed);
    assert!(!upper.corner_transitions);
    for key in ["u0", "u1", "u2", "u3"] {
        let open = resolve(&scene, &setup, &reference(&[key], false)).unwrap();
        let reversed = resolve(&scene, &setup, &reference(&[key], true)).unwrap();
        assert!(!open.closed);
        assert_eq!(open.path.len(), 2);
        assert_eq!(
            open.path,
            reversed.path.into_iter().rev().collect::<Vec<_>>()
        );
        assert_ne!(open.wall_side, reversed.wall_side);
    }
}

#[test]
fn upper_rim_survives_separate_three_sided_corner_transitions() {
    let mut scene = bevel();
    let lower = [
        [[1., 0., -1.], [19., 0., -1.]],
        [[20., 1., -1.], [20., 9., -1.]],
        [[19., 10., -1.], [1., 10., -1.]],
        [[0., 9., -1.], [0., 1., -1.]],
    ];
    for (i, ends) in lower.iter().enumerate() {
        let edge = scene.bodies[0]
            .edges
            .iter_mut()
            .find(|e| e.key == format!("l{i}"))
            .unwrap();
        edge.points = ends
            .iter()
            .map(|p| Point3Dto {
                x: p[0],
                y: p[1],
                z: p[2],
            })
            .collect();
    }
    let result = resolve(
        &scene,
        &setup(),
        &reference(&["u0", "u1", "u2", "u3"], false),
    )
    .unwrap();
    assert!(result.closed);
    assert!(result.corner_transitions);
    assert_eq!(result.width, 1.);
    assert_eq!(
        result.path,
        vec![
            Point2Dto::new(1., 1.),
            Point2Dto::new(19., 1.),
            Point2Dto::new(19., 9.),
            Point2Dto::new(1., 9.)
        ]
    );
}

#[test]
fn unavailable_underside_or_ambiguous_bevel_geometry_is_rejected() {
    let original = bevel();
    let setup = setup();
    let reference = reference(&["u0"], false);
    let mut scene = original.clone();
    scene.bodies[0].faces[0].plane.as_mut().unwrap().normal = [0., 0., -1.];
    assert!(resolve(&scene, &setup, &reference).is_err());
    let mut scene = original.clone();
    scene.bodies[0].faces[1].plane.as_mut().unwrap().normal = [0., 0.8, 0.6];
    assert!(resolve(&scene, &setup, &reference).is_err());
    let mut scene = original.clone();
    let duplicate = scene.bodies[0].faces[1].clone();
    scene.bodies[0].faces.push(duplicate);
    assert!(resolve(&scene, &setup, &reference).is_err());
    let mut scene = original;
    scene.bodies[0].edges.retain(|e| e.key != "u0");
    assert!(resolve(&scene, &setup, &reference).is_err());
}

#[test]
fn conical_hole_uses_exact_rim_radii_and_rejects_wrong_angle() {
    let mut scene = bevel();
    let body = &mut scene.bodies[0];
    body.edges.clear();
    body.faces.clear();
    for (key, r, z) in [("u", 6., 0.), ("l", 5., -1.)] {
        let points = (0..=72)
            .map(|i| {
                let a = std::f64::consts::TAU * i as f64 / 72.;
                point([r * a.cos(), r * a.sin(), z])
            })
            .collect::<Vec<_>>();
        body.edges.push(serde_json::from_value(json!({"id":1,"key":key,"points":points,
            "circle":{"center":point([0.,0.,z]),"normal":point([0.,0.,1.]),"reference":point([1.,0.,0.]),"radius":r,"closed":true}})).unwrap());
    }
    body.faces
        .push(serde_json::from_value(face("top", [0., 0., 1.], vec!["u".into()])).unwrap());
    body.faces.push(serde_json::from_value(json!({"id":2,"key":"cone","plane":null,"first_index":0,"index_count":0,
        "edge_keys":["u","l"],"cone":{"axis":point([0.,0.,-1.]),"semi_angle":std::f64::consts::FRAC_PI_4}})).unwrap());
    let setup = setup();
    let upper = resolve(&scene, &setup, &reference(&["u"], false)).unwrap();
    let lower = resolve(&scene, &setup, &reference(&["l"], false)).unwrap();
    assert_eq!(upper.width, 1.);
    assert_eq!(upper.path, lower.path);
    assert!(upper.closed);
    assert_eq!(upper.wall_side, ContourCompensation::Outside);
    scene.bodies[0].faces[1].cone.as_mut().unwrap().semi_angle = std::f64::consts::PI / 6.;
    assert!(resolve(&scene, &setup, &reference(&["u"], false)).is_err());
}
