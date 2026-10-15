use limo_cad_core::{EdgeId, FaceId, OriginPlane, PlaneRef};
use limo_cad_sketch::{
    Constraint, DimensionPreviewRequest, DimensionRequest, EditDimensionRequest, EntityId,
    ProjectedEdgeDto, RectangleMode, SketchSession, Vec2,
};

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}
fn edge(id: u64, a: Vec2, b: Vec2) -> ProjectedEdgeDto {
    ProjectedEdgeDto {
        id: (1 << 40) + id,
        edge_id: EdgeId(id),
        points: vec![a, b],
        circle: None,
    }
}
fn session() -> SketchSession {
    let basis = PlaneRef::OriginPlane {
        plane: OriginPlane::Xy,
    }
    .basis()
    .unwrap();
    let mut session = SketchSession::new(
        "face",
        PlaneRef::PlanarFace { face_id: FaceId(1) },
        basis,
        false,
    );
    session.set_projected_edges(vec![
        edge(1, v(0., 30.), v(40., 30.)),
        edge(2, v(40., 0.), v(40., 30.)),
    ]);
    session
}
fn request(line: EntityId, text: &str) -> DimensionRequest {
    DimensionRequest {
        entities: vec![line],
        text_pos: v(20., 25.),
        value_text: Some(text.into()),
    }
}

#[test]
fn boundary_offset_translates_sized_rectangle_and_survives_edit_history_and_save() {
    let mut s = session();
    let rect = s
        .add_rectangle(RectangleMode::TwoPoint, v(5., 5.), v(15., 15.))
        .unwrap();
    for line in [rect.entities[4], rect.entities[5]] {
        s.add_dimension(request(line, "10")).unwrap();
    }
    let line = rect.entities[6];
    let before_entities = s.dto().entities.len();
    let result = s
        .add_dimension_with_reference(request(line, "7"), Some(EdgeId(1)))
        .unwrap();
    assert_eq!(
        result.sketch.entities.len(),
        before_entities,
        "a support edge is never copied into the sketch"
    );
    let check = |s: &SketchSession, boundary: f64, gap: f64| {
        let (a, b) = s.sketch().resolved_line(line).unwrap();
        assert!((a.y - (boundary - gap)).abs() < 1e-6);
        assert!((b.y - a.y).abs() < 1e-6);
        assert!((a.distance(b) - 10.).abs() < 1e-6);
        let (bottom, _) = s.sketch().resolved_line(rect.entities[4]).unwrap();
        assert!((a.y - bottom.y - 10.).abs() < 1e-6);
    };
    check(&s, 30., 7.);
    let dimension = result
        .sketch
        .dimensions
        .iter()
        .find(|d| d.reference_edge == Some(EdgeId(1)))
        .unwrap();
    s.edit_dimension(EditDimensionRequest {
        constraint_id: dimension.constraint_id,
        text: "4".into(),
    })
    .unwrap();
    check(&s, 30., 4.);
    s.undo().unwrap();
    check(&s, 30., 7.);
    s.redo().unwrap();
    check(&s, 30., 4.);
    s.set_projected_edges(vec![edge(1, v(0., 35.), v(40., 35.))]);
    check(&s, 35., 4.);
    let json = serde_json::to_string(&s.sketch().snapshot()).unwrap();
    let snapshot: limo_cad_sketch::SketchSnapshot = serde_json::from_str(&json).unwrap();
    let mut reopened = limo_cad_sketch::Sketch::new();
    reopened.restore(snapshot);
    reopened.solve().unwrap();
    assert!((reopened.resolved_line(line).unwrap().0.y - 31.).abs() < 1e-6);
    s.set_projected_edges(vec![]);
    assert!(
        s.sketch().clone().solve().is_err(),
        "a missing support edge must fail, not free the rectangle"
    );
}

#[test]
fn selection_preview_switches_length_to_offset_or_angle_without_mutating() {
    let mut s = session();
    let line = s.add_line(v(5., 12.), v(15., 12.), true).unwrap().entity_id;
    let saved = serde_json::to_string(&s.sketch().snapshot()).unwrap();
    for (edge, kind, value) in [
        (None, "distance", 10.),
        (Some(EdgeId(1)), "distance", 18.),
        (Some(EdgeId(2)), "angle", 90.),
    ] {
        let preview = s
            .preview_dimension(DimensionPreviewRequest {
                entities: vec![line],
                reference_edge: edge,
            })
            .unwrap();
        assert_eq!(preview.kind, kind);
        assert!((preview.value - value).abs() < 1e-8);
        assert_eq!(preview.reference_edge, edge);
        assert!(preview.text_pos.x.is_finite() && preview.text_pos.y.is_finite());
        assert_eq!(
            serde_json::to_string(&s.sketch().snapshot()).unwrap(),
            saved
        );
    }
    let a = s.add_line(v(5., 20.), v(15., 20.), true).unwrap().entity_id;
    let preview = s
        .preview_dimension(DimensionPreviewRequest {
            entities: vec![line, a],
            reference_edge: None,
        })
        .unwrap();
    assert_eq!(preview.kind, "distance");
    assert!((preview.value - 8.).abs() < 1e-8);
}

#[test]
fn boundary_angle_drives_free_line_without_changing_length_or_support_geometry() {
    let mut s = session();
    let line = s.add_line(v(5., 8.), v(15., 8.), true).unwrap().entity_id;
    let result = s
        .add_dimension_with_reference(request(line, "60"), Some(EdgeId(2)))
        .unwrap();
    let (a, b) = s.sketch().resolved_line(line).unwrap();
    assert!((a.distance(b) - 10.).abs() < 1e-6);
    assert!(((b - a).y.atan2((b - a).x).to_degrees() - 30.).abs() < 1e-6);
    let dimension = result
        .sketch
        .dimensions
        .iter()
        .find(|d| d.reference_edge == Some(EdgeId(2)))
        .unwrap();
    assert_eq!(dimension.kind, "angle");
    assert!(result.sketch.constraints.iter().any(|c| matches!(
        c.constraint,
        Constraint::ReferenceAngle {
            edge: EdgeId(2),
            ..
        }
    )));
    s.edit_dimension(EditDimensionRequest {
        constraint_id: dimension.constraint_id,
        text: "45".into(),
    })
    .unwrap();
    let (a, b) = s.sketch().resolved_line(line).unwrap();
    assert!((a.distance(b) - 10.).abs() < 1e-6);
    assert!(((b - a).y.atan2((b - a).x).to_degrees() - 45.).abs() < 1e-6);
    assert_eq!(s.projected_edges()[1].points, vec![v(40., 0.), v(40., 30.)]);
}

#[test]
fn preview_reuses_the_width_driver_on_the_opposite_rectangle_edge() {
    let mut s = session();
    let rect = s
        .add_rectangle(RectangleMode::TwoPoint, v(5., 5.), v(15., 15.))
        .unwrap();
    let result = s.add_dimension(request(rect.entities[4], "10")).unwrap();
    let driver = result.sketch.dimensions[0].constraint_id;
    let preview = s
        .preview_dimension(DimensionPreviewRequest {
            entities: vec![rect.entities[6]],
            reference_edge: None,
        })
        .unwrap();
    assert_eq!(preview.constraint_id, driver);
    let angle = s
        .preview_dimension(DimensionPreviewRequest {
            entities: vec![rect.entities[6]],
            reference_edge: Some(EdgeId(2)),
        })
        .unwrap();
    assert_eq!(angle.kind, "angle");
    assert_eq!(
        angle.mode,
        limo_cad_sketch::DimensionMode::Reference,
        "H/V already determines this angle"
    );
}
