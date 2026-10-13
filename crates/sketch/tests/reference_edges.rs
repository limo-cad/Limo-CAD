use limo_cad_core::{EdgeId, FaceId, OriginPlane, PlaneRef};
use limo_cad_sketch::{
    Constraint, DragPhase, MovePointRequest, ProjectedCircleDto, ProjectedEdgeDto, SketchSession,
    Vec2,
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
