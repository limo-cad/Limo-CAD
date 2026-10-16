use limo_cad_core::{EdgeId, FaceId, OriginPlane, PlaneRef};
use limo_cad_sketch::{
    Constraint, DragPhase, MovePointRequest, ProjectedCircleDto, ProjectedEdgeDto, SketchSession,
    Vec2,
};
use limo_cad_sketch::{
    DimensionRequest, EntityDto, LockedSegmentRequest, RectangleMode, SnapTarget,
};
fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}
fn session() -> SketchSession {
    let basis = PlaneRef::OriginPlane {
        plane: OriginPlane::Xy,
    }
    .basis()
    .unwrap();
    SketchSession::new(
        "face",
        PlaneRef::PlanarFace { face_id: FaceId(1) },
        basis,
        false,
    )
}
fn edge(y: f64) -> ProjectedEdgeDto {
    ProjectedEdgeDto {
        id: (1 << 40) + 1,
        edge_id: EdgeId(1),
        points: vec![v(0., y), v(20., y)],
        circle: None,
    }
}
#[test]
fn projected_points_slide_follow_upstream_edges_and_keep_current_targets_after_undo() {
    let mut s = session();
    s.set_projected_edges(vec![edge(0.)]);
    let result = s.add_line(v(3., 0.), v(3., 5.), false).unwrap();
    let point = result
        .sketch
        .constraints
        .iter()
        .find_map(|c| match c.constraint {
            Constraint::ReferenceOnEdge {
                point,
                edge: EdgeId(1),
            } => Some(point),
            _ => None,
        })
        .expect("a boundary snap must persist a relation");
    s.set_projected_edges(vec![edge(2.)]);
    assert!((s.sketch().point_position(point).unwrap().y - 2.).abs() < 1e-8);
    s.move_point(MovePointRequest {
        point_id: point,
        to_raw: v(8., 2.),
        ctrl_held: true,
        phase: DragPhase::Single,
    })
    .unwrap();
    assert!(
        s.sketch()
            .point_position(point)
            .unwrap()
            .distance(v(8., 2.))
            < 1e-7
    );
    s.set_projected_edges(vec![edge(4.)]);
    s.undo().unwrap();
    assert!((s.sketch().point_position(point).unwrap().y - 4.).abs() < 1e-8);
    s.redo().unwrap();
    assert!((s.sketch().point_position(point).unwrap().y - 4.).abs() < 1e-8);
    let json = serde_json::to_string(&s.sketch().snapshot()).unwrap();
    let mut reopened = limo_cad_sketch::Sketch::new();
    reopened.restore(serde_json::from_str(&json).unwrap());
    assert!(reopened.solve().is_ok());
    s.set_projected_edges(vec![]);
    assert!(
        s.dto().reference_midpoints.is_empty(),
        "removed edges cannot leave ghost midpoint snaps"
    );
    assert!(
        s.sketch().clone().solve().is_err(),
        "missing edge must not silently free the point"
    );
}

#[test]
fn circular_support_uses_analytic_radius_and_ctrl_creates_no_external_constraint() {
    for ctrl in [false, true] {
        let mut s = session();
        s.set_projected_edges(vec![ProjectedEdgeDto {
            id: (1 << 40) + 1,
            edge_id: EdgeId(1),
            points: (0..=16)
                .map(|i| {
                    let a = std::f64::consts::TAU * i as f64 / 16.;
                    v(30. + 10. * a.cos(), 30. + 10. * a.sin())
                })
                .collect(),
            circle: Some(ProjectedCircleDto {
                center: v(30., 30.),
                radius: 10.,
                closed: true,
            }),
        }]);
        let a: f64 = 0.37;
        let p = v(30. + 10. * a.cos(), 30. + 10. * a.sin());
        let result = s.add_line(p, p + v(4., 5.), ctrl).unwrap();
        let refs: Vec<_> = result
            .sketch
            .constraints
            .iter()
            .filter_map(|c| match c.constraint {
                Constraint::ReferenceOnEdge { point, .. } => Some(point),
                _ => None,
            })
            .collect();
        assert_eq!(refs.len(), usize::from(!ctrl));
        if let Some(id) = refs.first() {
            assert!(
                (s.sketch()
                    .point_position(*id)
                    .unwrap()
                    .distance(v(30., 30.))
                    - 10.)
                    .abs()
                    < 1e-8
            );
        }
    }
}

#[test]
fn point_tool_boundary_attachment_is_authored_and_ctrl_suppresses_it() {
    for ctrl in [false, true] {
        let mut s = session();
        s.set_projected_edges(vec![edge(10.)]);
        let point = s
            .add_point_on_selective(v(7., 10.), None, ctrl)
            .unwrap()
            .entities[0];
        assert_eq!(
            s.dto()
                .constraints
                .iter()
                .filter(|c| matches!(c.constraint, Constraint::ReferenceOnEdge { .. }))
                .count(),
            usize::from(!ctrl)
        );
        s.set_projected_edges(vec![edge(12.)]);
        assert!(
            (s.sketch().point_position(point).unwrap().y - if ctrl { 10. } else { 12. }).abs()
                < 1e-8
        );
    }
}

fn locked_line(from: Vec2, to_hint: Vec2, ctrl: bool) -> LockedSegmentRequest {
    LockedSegmentRequest {
        from,
        to_hint,
        angle_deg: Some(90.),
        length_mm: None,
        angle_text: None,
        length_text: None,
        ctrl_held: ctrl,
        tracking: None,
        intersection: None,
        from_crossing: None,
        to_crossing: None,
    }
}

#[test]
fn typed_angle_boundary_snap_survives_length_edits_undo_and_support_refresh() {
    let mut s = session();
    s.set_projected_edges(vec![edge(20.)]);
    let rectangle = s
        .add_rectangle(RectangleMode::TwoPoint, v(3., 5.), v(13., 15.))
        .unwrap();
    for line in [rectangle.entities[4], rectangle.entities[5]] {
        s.add_dimension(DimensionRequest {
            entities: vec![line],
            text_pos: v(25., 10.),
            value_text: Some("10".into()),
        })
        .unwrap();
    }
    let from = v(8., 15.);
    let cursor = v(8.3, 19.7);
    let preview = s.preview_segment_locked(
        from,
        (None, Some(90.)),
        cursor,
        false,
        (None, None, None, None),
    );
    assert_eq!(
        preview.snap,
        SnapTarget::ProjectedEdge {
            edge: EdgeId(1),
            position: v(8., 20.)
        }
    );
    assert!(preview.snapped_to.distance(v(8., 20.)) < 1e-8);
    let result = s
        .add_line_locked(&locked_line(from, cursor, false))
        .unwrap();
    assert!(result
        .sketch
        .constraints
        .iter()
        .any(|c| matches!(c.constraint,
        Constraint::ReferenceOnEdge { point, edge: EdgeId(1) } if point == result.end_point_id)));
    s.add_dimension(DimensionRequest {
        entities: vec![result.entity_id],
        text_pos: v(6., 17.),
        value_text: Some("8".into()),
    })
    .unwrap();
    let check = |s: &SketchSession, length: f64, boundary: f64| {
        let a = s.sketch().point_position(result.start_point_id).unwrap();
        let b = s.sketch().point_position(result.end_point_id).unwrap();
        assert!(
            (b.y - boundary).abs() < 1e-6,
            "endpoint left boundary: {b:?}"
        );
        assert!((a.distance(b) - length).abs() < 1e-6);
        assert!((a.x - b.x).abs() < 1e-6);
    };
    check(&s, 8., 20.);
    s.undo().unwrap();
    check(&s, 5., 20.);
    s.redo().unwrap();
    check(&s, 8., 20.);
    s.set_projected_edges(vec![edge(22.)]);
    check(&s, 8., 22.);
    let saved = serde_json::to_string(&s.sketch().snapshot()).unwrap();
    let mut reopened = limo_cad_sketch::Sketch::new();
    reopened.restore(serde_json::from_str(&saved).unwrap());
    reopened.solve().unwrap();
    assert!((reopened.point_position(result.end_point_id).unwrap().y - 22.).abs() < 1e-6);
}

#[test]
fn line_start_acquires_rectangle_edge_interior_and_keeps_sliding_coincidence() {
    let mut s = session();
    s.set_projected_edges(vec![edge(25.)]);
    let rectangle = s
        .add_rectangle(RectangleMode::TwoPoint, v(3., 5.), v(33., 15.))
        .unwrap();
    let carrier = rectangle.entities[6];
    let raw = v(10.2, 15.3);
    let preview = s.preview_creation_point(limo_cad_sketch::CreationPointPreviewRequest {
        raw,
        ctrl_held: false,
        allow_midpoint: true,
        allow_curve: true,
        exclude_position: None,
    });
    assert_eq!(preview.snap, SnapTarget::Curve { entity: carrier });
    let result = s
        .add_line_locked(&locked_line(raw, v(10.2, 25.), false))
        .unwrap();
    assert!(s.dto().constraints.iter().any(|c| matches!(c.constraint,
        Constraint::Coincident { a, b } if a == result.start_point_id && b == carrier)));
    s.add_dimension(DimensionRequest {
        entities: vec![result.entity_id],
        text_pos: v(8., 20.),
        value_text: Some("7".into()),
    })
    .unwrap();
    let EntityDto::Line { start, end, .. } = s
        .dto()
        .entities
        .into_iter()
        .find(|e| e.id() == carrier)
        .unwrap()
    else {
        panic!("rectangle edge missing");
    };
    let point = s.sketch().point_position(result.start_point_id).unwrap();
    assert!((point.y - start.y).abs() < 1e-6 && (start.y - end.y).abs() < 1e-6);
    assert!((s.sketch().point_position(result.end_point_id).unwrap().y - 25.).abs() < 1e-6);
}

#[test]
fn angle_lock_obeys_finite_boundary_and_inference_override() {
    let mut s = session();
    s.set_projected_edges(vec![edge(20.)]);
    for (from, cursor, ctrl) in [
        (v(8., 10.), v(8., 20.), true),
        (v(8., 10.), v(8., 0.), false),
        (v(22.1, 10.), v(22.1, 20.), false),
    ] {
        let preview = s.preview_segment_locked(
            from,
            (None, Some(90.)),
            cursor,
            ctrl,
            (None, None, None, None),
        );
        assert!(!matches!(
            preview.snap,
            SnapTarget::ProjectedEdge { .. } | SnapTarget::ReferenceMidpoint { .. }
        ));
    }
    let preview = s.preview_segment_locked(
        v(10., 10.),
        (None, Some(90.)),
        v(10.2, 20.),
        false,
        (None, None, None, None),
    );
    assert_eq!(
        preview.snap,
        SnapTarget::ReferenceMidpoint { edge: EdgeId(1) }
    );
}
