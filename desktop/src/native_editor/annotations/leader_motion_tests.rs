use super::*;
use limo_cad_core::{EdgeId, OriginPlane, PlaneRef};
use limo_cad_sketch::{EntityId, ProjectedEdgeDto, SketchSession};

fn segment(id: u64, a: [f64; 2], b: [f64; 2]) -> EntityDto {
    EntityDto::Line {
        id: EntityId(id),
        start_id: EntityId(id + 100),
        end_id: EntityId(id + 200),
        start: Point::new(a[0], a[1]),
        end: Point::new(b[0], b[1]),
        consumed: false,
        fully_defined: false,
    }
}

fn sketch(entities: Vec<EntityDto>) -> SketchDto {
    let plane = PlaneRef::OriginPlane {
        plane: OriginPlane::Xy,
    };
    let mut sketch = SketchSession::new("leader", plane, plane.basis().unwrap(), false).dto();
    sketch.entities = entities;
    sketch
}

fn dimension(kind: &str, entities: &[u64]) -> DimensionDto {
    serde_json::from_value(json!({
        "constraint_id":0,"mode":"driving","kind":kind,"entities":entities,
        "value":10.,"text":"10.00","text_pos":{"x":5.,"y":5.}
    }))
    .unwrap()
}

fn assert_position(actual: Option<Point>, expected: Point) {
    assert!(
        actual.is_some_and(|p| p.distance(expected) < 1e-8),
        "{actual:?} != {expected:?}"
    );
}

#[test]
fn length_leaders_follow_only_the_normal_of_horizontal_vertical_and_diagonal_lines() {
    for (a, b, cursor, tangent, expected) in [
        ([0., 0.], [10., 0.], [3., 7.], [30., 0.], [5., 7.]),
        ([2., -5.], [2., 5.], [-8., 3.], [0., 30.], [-8., 0.]),
        ([0., 0.], [10., 10.], [20., 2.], [30., 30.], [14., -4.]),
    ] {
        let sketch = sketch(vec![segment(1, a, b)]);
        let dim = dimension("distance", &[1]);
        let cursor = Point::new(cursor[0], cursor[1]);
        let expected = Point::new(expected[0], expected[1]);
        assert_position(preview_position(&dim, &sketch, cursor), expected);
        assert_position(
            preview_position(&dim, &sketch, cursor + Point::new(tangent[0], tangent[1])),
            expected,
        );
        let mut moved = dim;
        moved.text_pos = expected;
        let span = Point::new(b[0] - a[0], b[1] - a[1]);
        let lines = dimension_leaders(&moved, &sketch, 0.5);
        let [from, to] = lines[lines.len() - 1];
        assert!(
            (to - from).distance(span) < 1e-8,
            "cursor movement changed the measured span"
        );
    }
}

#[test]
fn spacing_leaders_slide_along_parallel_sketch_and_support_edges() {
    let mut sketch = sketch(vec![
        segment(1, [0., 0.], [20., 0.]),
        segment(2, [0., 10.], [20., 10.]),
    ]);
    let mut dim = dimension("distance", &[1, 2]);
    for cursor in [Point::new(14., 4.), Point::new(14., 40.)] {
        assert_position(preview_position(&dim, &sketch, cursor), Point::new(14., 5.));
    }
    sketch.projected_edges.push(ProjectedEdgeDto {
        id: 1 << 40,
        edge_id: EdgeId(77),
        points: vec![Point::new(0., 10.), Point::new(20., 10.)],
        circle: None,
    });
    dim.entities.pop();
    dim.reference_edge = Some(EdgeId(77));
    assert_position(
        preview_position(&dim, &sketch, Point::new(-5., 35.)),
        Point::new(-5., 5.),
    );
    assert_eq!(
        sketch.entities.len(),
        2,
        "support edges remain presentation operands"
    );
}

#[test]
fn angle_cursor_changes_arc_radius_and_radial_cursor_follows_freely() {
    let angular = sketch(vec![
        segment(1, [0., 0.], [20., 0.]),
        segment(2, [0., 0.], [0., 20.]),
    ]);
    let dim = dimension("angle", &[1, 2]);
    let r = 50. / 2_f64.sqrt();
    for cursor in [Point::new(30., 40.), Point::new(-40., -30.)] {
        assert_position(preview_position(&dim, &angular, cursor), Point::new(r, r));
    }
    assert!(preview_position(&dim, &angular, Point::new(0., 0.)).is_none());
    let circle = serde_json::from_value(
        json!({"kind":"circle","id":3,"center":{"x":0.,"y":0.},"radius":10.,"fully_defined":false}),
    )
    .unwrap();
    let radial = sketch(vec![circle]);
    for kind in ["radius", "diameter"] {
        assert_position(
            preview_position(&dimension(kind, &[3]), &radial, Point::new(-12., 15.)),
            Point::new(-12., 15.),
        );
    }
    assert!(preview_position(&dim, &angular, Point::new(f64::NAN, 1.)).is_none());
    let degenerate = sketch(vec![segment(1, [0., 0.], [0., 0.])]);
    assert!(preview_position(
        &dimension("distance", &[1]),
        &degenerate,
        Point::new(2., 3.)
    )
    .is_none());
}
