//! Center-point-arc input parity: every pick acquires the same references the
//! line tool has (support-face edge midpoints and the projected face
//! boundary), and a typed radius locks the arc's radius while the cursor keeps
//! aiming the two endpoint picks.
use limo_cad_core::EdgeId;
use limo_cad_sketch::{
    Constraint, EditDimensionRequest, EntityDto, EntityId, OriginPlane, PlaneRef, ProjectedEdgeDto,
    SketchSession, SnapTarget, Vec2,
};

/// Matches `PROJECTED_EDGE_ID_BASE` in the sketch manager.
const RESERVED_ID_FLOOR: u64 = 1 << 40;

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

const XY: PlaneRef = PlaneRef::OriginPlane {
    plane: OriginPlane::Xy,
};

fn close(a: Vec2, b: Vec2) -> bool {
    a.distance(b) < 1e-7
}

/// Session with grid snap off and one support-edge midpoint plus one projected
/// boundary edge, which is what a sketch on a planar face carries.
fn face_session() -> SketchSession {
    let mut session = SketchSession::new(
        "Sketch1",
        PlaneRef::PlanarFace {
            face_id: limo_cad_core::FaceId(1),
        },
        XY.basis().unwrap(),
        false,
    );
    session.set_reference_midpoints(vec![(EdgeId(9), v(10.0, 10.0))]);
    session.set_projected_edges(vec![ProjectedEdgeDto {
        id: RESERVED_ID_FLOOR,
        edge_id: EdgeId(7),
        points: vec![v(0.0, 20.0), v(20.0, 20.0)],
        circle: None,
    }]);
    session
}

fn arc_of(session: &SketchSession, id: limo_cad_sketch::EntityId) -> (Vec2, f64, f64, f64) {
    match session.dto().entities.iter().find(|e| e.id() == id) {
        Some(EntityDto::Arc {
            center,
            radius,
            start_angle,
            end_angle,
            ..
        }) => (*center, *radius, *start_angle, *end_angle),
        other => panic!("expected arc, got {other:?}"),
    }
}

#[test]
fn arc_picks_acquire_support_edge_midpoints_and_the_projected_boundary() {
    let session = face_session();
    let midpoint = session.preview_segment(v(0.0, 0.0), v(10.2, 10.1), false);
    assert_eq!(
        midpoint.snap,
        SnapTarget::ReferenceMidpoint { edge: EdgeId(9) }
    );
    assert_eq!(midpoint.snapped_to, v(10.0, 10.0));

    let boundary = session.preview_segment(v(0.0, 0.0), v(8.0, 20.2), false);
    match boundary.snap {
        SnapTarget::ProjectedEdge { edge, position } => {
            assert_eq!(edge, EdgeId(7));
            assert!(close(position, v(8.0, 20.0)), "{position:?}");
        }
        other => panic!("expected a projected-boundary acquisition, got {other:?}"),
    }

    let mut session = face_session();
    let result = session
        .add_arc_center(v(10.2, 10.1), v(8.0, 20.2), v(19.7, 20.1))
        .unwrap();
    let (center, radius, start_angle, _) = arc_of(&session, result.entities[0]);
    assert!(
        close(center, v(10.0, 10.0)),
        "center {center:?} must acquire the midpoint"
    );
    let expected_radius = v(8.0, 20.0).distance(v(10.0, 10.0));
    assert!(
        (radius - expected_radius).abs() < 1e-9,
        "start acquired the projected boundary: {radius} vs {expected_radius}"
    );

    let start = v(
        center.x + radius * start_angle.cos(),
        center.y + radius * start_angle.sin(),
    );
    assert!(close(start, v(8.0, 20.0)), "start point {start:?}");
}

#[test]
fn a_typed_radius_locks_the_arc_and_adds_a_driving_dimension() {
    let mut session = face_session();
    let result = session
        .add_arc_center_locked(
            (v(0.0, 0.0), v(7.0, 0.0), v(0.0, 9.0)),
            false,
            None,
            Some("12"),
            None,
            None,
        )
        .unwrap();
    let (center, radius, start_angle, end_angle) = arc_of(&session, result.entities[0]);
    assert!(close(center, Vec2::ZERO));
    assert!((radius - 12.0).abs() < 1e-9, "locked radius {radius}");
    assert!(start_angle.abs() < 1e-9, "start keeps the hint direction");
    assert!(
        (end_angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9,
        "sweep keeps the third pick's direction: {end_angle}"
    );
    let arc_id = result.entities[0];
    let dto = session.dto();
    let radius_constraint = dto
        .constraints
        .iter()
        .find(|c| matches!(c.constraint, Constraint::Radius { entity, .. } if entity == arc_id));
    assert!(radius_constraint.is_some(), "driving Radius relation");
    let dimensions = dto.dimensions.clone();
    assert_eq!(dimensions.len(), 1);
    assert_eq!(dimensions[0].kind, "radius");
    assert_eq!(dimensions[0].text, "R12.00");
    assert_eq!(dimensions[0].entities, vec![arc_id]);

    let gap = (radius * 0.5).clamp(2.0, 8.0);
    let mid_angle = (start_angle + end_angle) / 2.0;
    let expected = center + Vec2::new(mid_angle.cos(), mid_angle.sin()) * (radius + gap);
    assert!(
        close(dimensions[0].text_pos, expected),
        "radius text at {:?}, expected {:?}",
        dimensions[0].text_pos,
        expected
    );

    session
        .edit_dimension(EditDimensionRequest {
            constraint_id: dimensions[0].constraint_id,
            text: "20".to_string(),
        })
        .unwrap();
    let (_, radius, _, _) = arc_of(&session, arc_id);
    assert!((radius - 20.0).abs() < 1e-6, "edited radius {radius}");
}

#[test]
fn a_typed_radius_expression_is_evaluated() {
    let mut session = face_session();
    let result = session
        .add_arc_center_locked(
            (v(0.0, 0.0), v(7.0, 0.0), v(0.0, 9.0)),
            false,
            None,
            Some("=6*2"),
            None,
            None,
        )
        .unwrap();
    let (_, radius, _, _) = arc_of(&session, result.entities[0]);
    assert!((radius - 12.0).abs() < 1e-9, "formula radius {radius}");
    let dimension = &session.dto().dimensions[0];
    assert_eq!(dimension.text, "R12.00");
    assert_eq!(dimension.param_expression.as_deref(), Some("6*2"));
    assert!((dimension.value - 12.0).abs() < 1e-9);
}

#[test]
fn one_undo_removes_a_locked_arc_and_its_dimension() {
    let mut session = face_session();
    session
        .add_arc_center_locked(
            (v(0.0, 0.0), v(7.0, 0.0), v(0.0, 9.0)),
            false,
            None,
            Some("12"),
            None,
            None,
        )
        .unwrap();
    assert_eq!(session.dto().dimensions.len(), 1);
    session.undo().unwrap();
    let dto = session.dto();
    assert!(dto.entities.is_empty(), "arc removed: {:?}", dto.entities);
    assert!(dto.dimensions.is_empty(), "dimension removed with it");
}

#[test]
fn the_drag_direction_decides_which_half_the_arc_covers() {
    let mut cw = face_session();
    cw.add_arc_center_locked(
        (v(0.0, 0.0), v(5.0, 0.0), v(-5.0, 0.0)),
        false,
        None,
        None,
        None,
        Some(-std::f64::consts::PI),
    )
    .unwrap();
    let cw_id = limo_cad_sketch::EntityId(1);
    let (_, radius, start_angle, end_angle) = arc_of(&cw, cw_id);
    assert!((radius - 5.0).abs() < 1e-9, "start pick radius {radius}");

    assert!(end_angle > start_angle, "{start_angle} .. {end_angle}");
    assert!(
        (end_angle - start_angle - std::f64::consts::PI).abs() < 1e-9,
        "half turn"
    );
    let mid = (start_angle + end_angle) / 2.0;
    assert!(
        mid < 0.0 && (mid + std::f64::consts::FRAC_PI_2).abs() < 1e-9,
        "the arc must pass below the centre, got mid ray {mid}"
    );

    let mut ccw = face_session();
    ccw.add_arc_center_locked(
        (v(0.0, 0.0), v(5.0, 0.0), v(-5.0, 0.0)),
        false,
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let (_, _, start_angle, end_angle) = arc_of(&ccw, cw_id);
    let mid = (start_angle + end_angle) / 2.0;
    assert!(
        (mid - std::f64::consts::FRAC_PI_2).abs() < 1e-9,
        "the counter-clockwise half passes above the centre, got {mid}"
    );

    let mut quarter = face_session();
    quarter
        .add_arc_center_locked(
            (v(0.0, 0.0), v(5.0, 0.0), v(0.0, -5.0)),
            false,
            None,
            None,
            None,
            Some(-std::f64::consts::FRAC_PI_2),
        )
        .unwrap();
    let (_, _, start_angle, end_angle) = arc_of(&quarter, cw_id);
    assert!(
        (end_angle - start_angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9,
        "a clockwise quarter turn stays a quarter turn: {start_angle} .. {end_angle}"
    );
    let mid = (start_angle + end_angle) / 2.0;
    assert!(
        (mid + std::f64::consts::FRAC_PI_4).abs() < 1e-9,
        "the quarter turn covers the lower-right quadrant, got {mid}"
    );
}

#[test]
fn deleting_an_arc_takes_its_own_endpoints_with_it() {
    let mut session = face_session();
    let result = session
        .add_arc_center_locked(
            (v(0.0, 0.0), v(5.0, 0.0), v(0.0, 5.0)),
            false,
            None,
            None,
            None,
            None,
        )
        .unwrap();
    let arc_id = result.entities[0];
    let endpoints: Vec<EntityId> = session
        .dto()
        .entities
        .iter()
        .filter(|entity| matches!(entity, EntityDto::Point { .. }))
        .map(|entity| entity.id())
        .collect();
    assert_eq!(endpoints.len(), 2, "the arc owns two endpoint points");

    let removed = session.delete_entities(&[arc_id]).unwrap();
    for endpoint in endpoints {
        assert!(
            removed.removed.contains(&endpoint),
            "the arc's endpoint {endpoint:?} must go with it, removed {removed:?}"
        );
        assert!(
            session
                .dto()
                .entities
                .iter()
                .all(|entity| entity.id() != endpoint),
            "deleting the arc leaves no loose point behind"
        );
    }

    let mut shared = face_session();
    let arc = shared
        .add_arc_center_locked(
            (v(0.0, 0.0), v(5.0, 0.0), v(0.0, 5.0)),
            false,
            None,
            None,
            None,
            None,
        )
        .unwrap();
    let arc_id = arc.entities[0];
    let endpoint = arc
        .entities
        .iter()
        .copied()
        .find(|id| {
            shared
                .dto()
                .entities
                .iter()
                .any(|entity| entity.id() == *id && matches!(entity, EntityDto::Point { .. }))
        })
        .or_else(|| {
            shared
                .dto()
                .entities
                .iter()
                .find(|entity| matches!(entity, EntityDto::Point { .. }))
                .map(|entity| entity.id())
        })
        .expect("an arc endpoint point");
    shared
        .add_constraint(Constraint::Fix { entity: endpoint })
        .unwrap();
    let removed = shared.delete_entities(&[arc_id]).unwrap();
    assert!(
        !removed.removed.contains(&endpoint),
        "a pinned endpoint belongs to the user's relation, not the arc"
    );
}

#[test]
fn a_typed_sweep_angle_becomes_a_driving_dimension() {
    let mut session = face_session();
    let result = session
        .add_arc_center_locked(
            (v(0.0, 0.0), v(9.0, 0.0), v(0.0, 9.0)),
            false,
            None,
            None,
            Some("90"),
            Some(std::f64::consts::FRAC_PI_2),
        )
        .unwrap();
    let arc_id = result.entities[0];
    let (center, radius, start_angle, end_angle) = arc_of(&session, arc_id);
    assert!((radius - 9.0).abs() < 1e-9, "radius {radius}");
    assert!(
        (end_angle - start_angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9,
        "the typed angle sizes the sweep: {start_angle} .. {end_angle}"
    );

    let dto = session.dto();
    let dimensions = dto.dimensions.clone();
    assert_eq!(
        dimensions.len(),
        1,
        "one sweep dimension, no radius dimension"
    );
    assert_eq!(dimensions[0].kind, "angle");
    assert_eq!(dimensions[0].entities, vec![arc_id]);
    assert_eq!(dimensions[0].text, "90.00°");

    let mut cw = face_session();
    let cw_result = cw
        .add_arc_center_locked(
            (v(0.0, 0.0), v(9.0, 0.0), v(0.0, 9.0)),
            false,
            None,
            None,
            Some("-90"),
            Some(-std::f64::consts::FRAC_PI_2),
        )
        .unwrap();
    let cw_id = cw_result.entities[0];
    let (_, _, cw_start, cw_end) = arc_of(&cw, cw_id);
    assert!(
        (cw_end - cw_start - std::f64::consts::FRAC_PI_2).abs() < 1e-9,
        "a typed -90 keeps a clockwise quarter: {cw_start} .. {cw_end}"
    );
    let mid = (cw_start + cw_end) / 2.0;
    assert!(
        mid < 0.0,
        "the arc covers the clockwise side, got mid ray {mid}"
    );
    assert_eq!(cw.dto().dimensions[0].text, "90.00°");

    let reach = dimensions[0].text_pos.distance(center);
    assert!(
        reach > 0.0 && reach < radius,
        "the angle value sits inside the arc, got {reach} for radius {radius}"
    );

    session
        .edit_dimension(EditDimensionRequest {
            constraint_id: dimensions[0].constraint_id,
            text: "30".to_string(),
        })
        .unwrap();
    let (_, _, start_angle, end_angle) = arc_of(&session, arc_id);
    assert!(
        (end_angle - start_angle - 30.0_f64.to_radians()).abs() < 1e-6,
        "edited sweep {start_angle} .. {end_angle}"
    );
}

#[test]
fn a_click_that_never_moved_is_not_a_full_circle() {
    let mut session = face_session();
    let refused = session.add_arc_center_locked(
        (v(0.0, 0.0), v(5.0, 0.0), v(5.0, 0.0)),
        false,
        None,
        None,
        None,
        Some(0.0),
    );
    assert!(
        refused.is_err(),
        "a zero-travel sweep must not become an arc"
    );
    assert!(
        session.dto().entities.is_empty(),
        "the refused arc must not be left behind"
    );

    let mut full = face_session();
    full.add_arc_center_locked(
        (v(0.0, 0.0), v(5.0, 0.0), v(5.0, 0.0)),
        false,
        None,
        None,
        None,
        Some(std::f64::consts::TAU),
    )
    .unwrap();
    let (_, radius, start_angle, end_angle) = arc_of(&full, limo_cad_sketch::EntityId(1));
    assert!((radius - 5.0).abs() < 1e-9, "radius {radius}");
    assert!(
        (end_angle - start_angle - std::f64::consts::TAU).abs() < 1e-9,
        "a full turn stays a full circle: {start_angle} .. {end_angle}"
    );
}
