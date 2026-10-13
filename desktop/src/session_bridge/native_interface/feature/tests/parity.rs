use super::*;
use limo_cad_interface::KeyChord;
use limo_cad_solid::{ExtrudeExtent, ExtrudeOperation, ExtrudeRequest};

fn area(points: &[f32]) -> f64 {
    points
        .as_chunks::<9>()
        .0
        .iter()
        .map(|p| {
            f64::from(((p[3] - p[0]) * (p[7] - p[1]) - (p[6] - p[0]) * (p[4] - p[1])).abs()) * 0.5
        })
        .sum()
}

#[test]
fn shaded_extrusion_preserves_concavity_holes_and_each_signed_extent() {
    let _lock = super::super::super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    sketch(&fixture);
    let before = exported(&fixture);
    let mut viewport = model_snapshot(&fixture.engine);
    // An L-shaped region with an off-center hole and a duplicate closing point.
    Arc::make_mut(&mut viewport.document).profile_catalog[0].profiles = serde_json::from_value(json!([
        {"index":0,"points":[{"x":0.,"y":0.},{"x":4.,"y":0.},{"x":4.,"y":1.},{"x":1.,"y":1.},{"x":1.,"y":4.},{"x":0.,"y":4.},{"x":0.,"y":0.}],"area":7.,"nesting_depth":0},
        {"index":1,"points":[{"x":0.2,"y":0.2},{"x":0.8,"y":0.2},{"x":0.8,"y":0.8},{"x":0.2,"y":0.8}],"area":0.36,"nesting_depth":1,"parent_index":0}
    ])).unwrap();
    for (extent, flip, range, arrow) in [
        (
            ExtrudeExtent::Distance { distance: 10. },
            false,
            [0., 10.],
            10.,
        ),
        (
            ExtrudeExtent::Distance { distance: 5. },
            true,
            [-5., 0.],
            -5.,
        ),
        (
            ExtrudeExtent::Symmetric { distance: 8. },
            true,
            [-4., 4.],
            -4.,
        ),
        (
            ExtrudeExtent::TwoSides {
                distance: 10.,
                second_distance: 3.,
            },
            false,
            [-3., 10.],
            10.,
        ),
    ] {
        let request = ExtrudeRequest {
            sketch_name: "Sketch1".into(),
            profile_indices: vec![0],
            source_face: None,
            operation: ExtrudeOperation::NewBody,
            extent,
            taper_angle_deg: 0.,
            flip,
            target_body_ids: vec![],
        };
        let preview = preview::build(&request, &viewport).unwrap();
        assert!((area(&preview.triangles[1].positions) - 6.64).abs() < 1e-5);
        let volume = &preview.triangles[0].positions;
        let min = volume
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| p[2])
            .fold(f32::INFINITY, f32::min);
        let max = volume
            .as_chunks::<3>()
            .0
            .iter()
            .map(|p| p[2])
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!([min, max], range);
        for z in range {
            let cap: Vec<_> = volume
                .as_chunks::<9>()
                .0
                .iter()
                .filter(|p| [p[2], p[5], p[8]] == [z; 3])
                .flatten()
                .copied()
                .collect();
            assert!(
                (area(&cap) - 6.64).abs() < 1e-5,
                "The source region, including its hole, is retained on both caps"
            );
        }
        let handle = &preview.arrows[0];
        assert!((f64::from(handle.start[0]) - 9.32 / 6.64).abs() < 1e-5);
        assert!((f64::from(handle.start[1]) - 9.32 / 6.64).abs() < 1e-5);
        assert_eq!(handle.end[2] - handle.start[2], arrow);
        assert!(preview.triangles[0].xray);
    }
    assert_eq!(exported(&fixture), before);
}

#[test]
fn feature_enter_commits_stepped_measurement_once_and_invalid_enter_keeps_the_draft() {
    let _lock = super::super::super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = sketch(&fixture);
    let before = exported(&fixture);
    let mut app = scene(&fixture, &owner);
    let id = open(&fixture, app.world_mut(), &owner, None);
    field(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        SolidField::Distance,
        "6.25 mm",
    );
    action(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        FeatureControl::Step {
            field: SolidField::Distance,
            delta: -1,
        },
        ControlInput::Click,
    )
    .unwrap();
    assert_eq!(exported(&fixture), before);
    action(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        FeatureControl::Field(SolidField::Distance),
        ControlInput::Key(KeyChord::plain("Enter")),
    )
    .unwrap();
    assert!(panel(app.world()).is_none());
    assert!((maximum_z(&fixture) - 5.25).abs() < 1e-6);
    let undone = fixture
        .bridge
        .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
        .unwrap();
    let owner = undone.context;
    let undone = exported(&fixture);
    assert_eq!(
        undone["document"], before["document"],
        "One Undo removes exactly the Enter-confirmed feature"
    );
    assert_eq!(undone["sketches"], before["sketches"]);
    assert_eq!(undone["extrudes"], json!([]));
    assert!(fixture.engine.viewport_snapshot().2.bodies.is_empty());
    native_viewport::apply_interface_model(app.world_mut(), model_snapshot(&fixture.engine))
        .unwrap();
    let id = open(&fixture, app.world_mut(), &owner, None);
    field(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        SolidField::Distance,
        "1 / 0",
    );
    assert!(action(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        FeatureControl::Field(SolidField::Distance),
        ControlInput::Key(KeyChord::plain("Enter"))
    )
    .is_err());
    assert!(panel(app.world()).is_some());
    assert_eq!(exported(&fixture), undone);
    action(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        FeatureControl::Cancel,
        ControlInput::Click,
    )
    .unwrap();
}

#[test]
fn extrusion_arrow_edits_only_the_draft_and_disappearing_volume_cannot_be_dragged() {
    use crate::session_bridge::native_interface::controller::NativeServices;
    let _lock = super::super::super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = sketch(&fixture);
    let before = exported(&fixture);
    let mut app = scene(&fixture, &owner);
    native_viewport::apply_interface_viewport(
        app.world_mut(),
        limo_cad_interface::Rect {
            x: 0.,
            y: 0.,
            width: 1000.,
            height: 700.,
        },
        1.,
    )
    .unwrap();
    let id = open(&fixture, app.world_mut(), &owner, None);
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let tip = manipulator::anchor(app.world()).unwrap();
    let next =
        native_viewport::interface_world_point(app.world(), &owner.document_id, [10., 6., 14.])
            .unwrap()
            .unwrap();
    assert!(manipulator::pointer(
        app.world_mut(),
        &services,
        &owner,
        manipulator::Pointer::Press,
        Some(tip)
    )
    .unwrap());
    assert!(manipulator::pointer(
        app.world_mut(),
        &services,
        &owner,
        manipulator::Pointer::Release,
        Some(next)
    )
    .unwrap());
    let preview = native_viewport::interface_preview_snapshot(app.world());
    assert!(preview.arrows[0].end[2] > 13. && preview.arrows[0].end[2] < 15.);
    assert_eq!(exported(&fixture), before);
    field(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        SolidField::Distance,
        "unfinished+",
    );
    assert!(!manipulator::pointer(
        app.world_mut(),
        &services,
        &owner,
        manipulator::Pointer::Press,
        Some(tip)
    )
    .unwrap());
    assert_eq!(exported(&fixture), before);
    action(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        FeatureControl::Cancel,
        ControlInput::Click,
    )
    .unwrap();
}
