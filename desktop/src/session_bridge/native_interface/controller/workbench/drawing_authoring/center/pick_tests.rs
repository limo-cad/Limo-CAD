//! Regressions for the real Linux center pick that chose a cylinder's rear edge
//! because its numeric topology ID sorted before the coincident front edge.
use super::super::tests as fixture;
use super::*;
use limo_cad_core::EdgeId;

#[test]
fn equal_radius_centers_choose_front_depth_before_ids_and_preserve_exact_occurrences() {
    for direction in [
        [0., 0., 1.],
        [0., 0., -1.],
        [1., 0., 0.],
        [0., -1., 0.],
        [0.6, 0.8, 0.],
    ] {
        for ids in [
            [1_944_501_535_378_220, 1_947_800_070_262_853],
            [1_947_800_070_262_853, 1_944_501_535_378_220],
            [5, 5],
        ] {
            for reverse in [false, true] {
                let mut document = fixture::document();
                let view = &mut document.sheets[0].views[0];
                view.direction = direction.map(|n| -n);
                view.up = if direction[2] != 0. {
                    [0., 1., 0.]
                } else {
                    [0., 0., 1.]
                };
                view.scale = 2.;
                let mut projection = fixture::projection();
                projection.circles[1] = projection.circles[0].clone();
                let rear = &mut projection.circles[0];
                rear.edge_id = EdgeId(ids[0]);
                rear.edge_key = "rear".into();
                rear.occurrence_id = Some(OccurrenceId(31));
                rear.center_model = [110., 15., 6.];
                rear.normal_model = direction;
                let rear_position = rear.center_model;
                let front = &mut projection.circles[1];
                front.edge_id = EdgeId(ids[1]);
                front.edge_key = "front".into();
                front.occurrence_id = Some(OccurrenceId(32));
                front.center_model = std::array::from_fn(|i| rear_position[i] + 10. * direction[i]);
                front.normal_model = direction;
                let expected = DrawingCircularRefDto {
                    topology_signature: Some("exact-body-topology".into()),
                    occurrence_id: front.occurrence_id,
                    body_id: front.body_id,
                    edge_id: front.edge_id,
                    edge_key: front.edge_key.clone(),
                    fallback_center: front.center_model,
                    fallback_normal: front.normal_model,
                    fallback_radius: front.radius,
                    closed: true,
                };
                if reverse {
                    projection.circles.reverse();
                }
                let before = serde_json::to_value((&document, &projection)).unwrap();
                let chosen = targets(&document.sheets[0].views[0], &projection, direction).unwrap();
                assert_eq!(chosen.len(), 1);
                assert_eq!(chosen[0].reference, expected);
                let linear =
                    anchors::circles(&document.sheets[0].views[0], &projection, direction, false)
                        .unwrap();
                assert_eq!(linear.len(), 1);
                let linear = anchors::circle_ref(linear[0], &projection);
                assert_eq!(linear.edge_id, expected.edge_id);
                assert_eq!(linear.edge_key, expected.edge_key);
                assert_eq!(linear.occurrence_id, expected.occurrence_id);
                assert_eq!(linear.topology_signature, expected.topology_signature);
                assert_eq!(linear.fallback_point, expected.fallback_center);
                let created = Placement::default()
                    .click(&super::tests::stamp(), &chosen[0], false, &document)
                    .unwrap()
                    .unwrap();
                let DrawingAnnotationDto::CenterMark { feature, .. } =
                    created.sheets[0].annotations.last().unwrap()
                else {
                    panic!("Expected center mark");
                };
                assert_eq!(feature, &expected);
                assert_eq!(
                    serde_json::to_value((&document, &projection)).unwrap(),
                    before
                );
            }
        }
    }
}

#[test]
fn center_depth_ties_keep_visibility_largest_radius_and_stable_ids() {
    let mut document = fixture::document();
    let view = &mut document.sheets[0].views[0];
    let mut projection = fixture::projection();
    projection.circles[0].center_model[2] = 10.;
    projection.circles[1].center_model[2] = 0.;
    let chosen = |view: &DrawingViewDto, projection: &DrawingProjectionDto| {
        let selected = targets(view, projection, [0., 0., 1.]).unwrap()[0]
            .reference
            .edge_key
            .clone();
        assert_eq!(
            anchors::circles(view, projection, [0., 0., 1.], false).unwrap()[0].edge_key,
            selected
        );
        selected
    };
    assert_eq!(
        chosen(view, &projection),
        "outer",
        "Concentric radius policy is unchanged"
    );
    projection.circles[1].radius = projection.circles[0].radius;
    assert_eq!(chosen(view, &projection), "inner");
    projection.circles[0].hidden = true;
    for show_hidden in [false, true] {
        view.show_hidden_lines = show_hidden;
        assert_eq!(
            chosen(view, &projection),
            "outer",
            "Visible edges outrank hidden front edges"
        );
    }
    projection.circles[1].hidden = true;
    assert_eq!(chosen(view, &projection), "inner");
    view.show_hidden_lines = false;
    assert!(targets(view, &projection, [0., 0., 1.]).unwrap().is_empty());
    assert!(anchors::circles(view, &projection, [0., 0., 1.], false)
        .unwrap()
        .is_empty());
    view.show_hidden_lines = true;
    projection.circles[1].center_model = projection.circles[0].center_model;
    projection.circles[0].edge_id = EdgeId(9);
    projection.circles[1].edge_id = EdgeId(8);
    for _ in 0..2 {
        assert_eq!(
            chosen(view, &projection),
            "outer",
            "IDs only break equal-depth ties"
        );
        projection.circles.reverse();
    }
}

#[test]
fn linear_circle_centers_keep_arc_eligibility_but_reject_non_finite_depth() {
    let document = fixture::document();
    let view = &document.sheets[0].views[0];
    let mut projection = fixture::projection();
    projection.circles[1].closed = false;
    let linear = anchors::circles(view, &projection, view.direction, false).unwrap();
    assert_eq!(
        linear[0].edge_key, "outer",
        "Linear dimensions still accept arc centers"
    );
    let marks = targets(view, &projection, view.direction).unwrap();
    assert_eq!(
        marks[0].reference.edge_key, "inner",
        "Marks filter arcs before deduplication"
    );
    projection.circles[0].center_model[2] = f64::NAN;
    assert!(anchors::circles(view, &projection, view.direction, false).is_err());
    assert!(targets(view, &projection, view.direction).is_err());
}
