//! Integration tests for the fit-point Spline tool (M1 follow-up): creation,
//! tessellation in the DTO, duplicate cleanup, degenerate rejection,
//! single-record undo, delete, and self-contained move/scale.

use limo_cad_sketch::{
    EntityDto, MoveCopyRequest, OriginPlane, PlaneRef, ScaleRequest, SketchSession, SplineRequest,
    Vec2,
};

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

const XY: PlaneRef = PlaneRef::OriginPlane {
    plane: OriginPlane::Xy,
};

/// Session with grid snap OFF (deterministic coordinates).
fn session() -> SketchSession {
    SketchSession::new("Sketch1", XY, XY.basis().unwrap(), false)
}

fn req(points: &[(f64, f64)]) -> SplineRequest {
    SplineRequest {
        points: points.iter().map(|&(x, y)| v(x, y)).collect(),
    }
}

fn spline(dto: &limo_cad_sketch::SketchDto) -> &EntityDto {
    dto.entities
        .iter()
        .find(|e| matches!(e, EntityDto::Spline { .. }))
        .expect("spline exists")
}

#[test]
fn create_interpolating_spline_with_tessellation() {
    let mut s = session();
    let r = s
        .add_spline(&req(&[(0.0, 0.0), (20.0, 20.0), (40.0, 0.0), (60.0, 10.0)]))
        .unwrap();
    let dto = r.sketch;
    match spline(&dto) {
        EntityDto::Spline {
            points,
            tessellation,
            ..
        } => {
            assert_eq!(points.len(), 4);

            assert_eq!(tessellation.len(), 3 * 16 + 1);

            for p in points {
                assert!(
                    tessellation.iter().any(|q| q.distance(*p) < 1e-7),
                    "fit point {p:?} on the curve"
                );
            }
            assert!(tessellation[0].distance(v(0.0, 0.0)) < 1e-9);
            assert!(tessellation.last().unwrap().distance(v(60.0, 10.0)) < 1e-9);
        }
        _ => unreachable!(),
    }
}

#[test]
fn two_points_make_a_straight_segment() {
    let mut s = session();
    let r = s.add_spline(&req(&[(0.0, 0.0), (30.0, 0.0)])).unwrap();
    match spline(&r.sketch) {
        EntityDto::Spline { tessellation, .. } => {
            assert_eq!(tessellation.len(), 2);
        }
        _ => unreachable!(),
    }
}

#[test]
fn consecutive_duplicates_are_dropped() {
    let mut s = session();
    let r = s
        .add_spline(&req(&[
            (0.0, 0.0),
            (0.0, 0.0),
            (20.0, 10.0),
            (20.0, 10.0),
            (40.0, 0.0),
        ]))
        .unwrap();
    match spline(&r.sketch) {
        EntityDto::Spline { points, .. } => assert_eq!(points.len(), 3),
        _ => unreachable!(),
    }
}

#[test]
fn fewer_than_two_points_rejected() {
    let mut s = session();
    assert!(s.add_spline(&req(&[(5.0, 5.0)])).is_err());
    assert!(s.add_spline(&req(&[])).is_err());

    assert!(s.add_spline(&req(&[(5.0, 5.0), (5.0, 5.0)])).is_err());
    assert!(s.dto().entities.is_empty());
}

#[test]
fn spline_is_one_undo_record_and_deletable() {
    let mut s = session();
    s.add_spline(&req(&[(0.0, 0.0), (20.0, 20.0), (40.0, 0.0)]))
        .unwrap();
    let id = s.dto().entities[0].id();
    let r = s.undo().unwrap();
    assert!(r.sketch.entities.is_empty());
    s.redo().unwrap();
    s.delete_entity(id).unwrap();
    assert!(s.dto().entities.is_empty());
}

#[test]
fn move_and_scale_act_on_fit_points() {
    let mut s = session();
    s.add_spline(&req(&[(0.0, 0.0), (20.0, 20.0), (40.0, 0.0)]))
        .unwrap();
    let id = s.dto().entities[0].id();
    s.move_copy_entities(&MoveCopyRequest {
        entity_ids: vec![id],
        dx: 5.0,
        dy: -5.0,
        copy: false,
    })
    .unwrap();
    match spline(&s.dto()) {
        EntityDto::Spline { points, .. } => {
            assert!(points[0].distance(v(5.0, -5.0)) < 1e-9);
            assert!(points[2].distance(v(45.0, -5.0)) < 1e-9);
        }
        _ => unreachable!(),
    }
    s.scale_entities(&ScaleRequest {
        entity_ids: vec![id],
        origin: v(5.0, -5.0),
        factor_text: "2".to_string(),
    })
    .unwrap();
    match spline(&s.dto()) {
        EntityDto::Spline {
            points,
            tessellation,
            ..
        } => {
            assert!(points[0].distance(v(5.0, -5.0)) < 1e-9, "origin pinned");
            assert!(
                points[2].distance(v(85.0, -5.0)) < 1e-9,
                "far point doubled"
            );
            assert_eq!(tessellation.len(), 2 * 16 + 1);
        }
        _ => unreachable!(),
    }
}

#[test]
fn fix_unfix_controls_spline_fit_points_and_definition_state() {
    let mut s = session();
    s.add_spline(&req(&[(0.0, 0.0), (20.0, 20.0), (40.0, 0.0)]))
        .unwrap();
    let id = s.dto().entities[0].id();
    assert_eq!(s.dto().dof.value, 6);

    let fixed = s.toggle_fix(id).unwrap().sketch;
    assert_eq!(fixed.dof.value, 0);
    match spline(&fixed) {
        EntityDto::Spline {
            points,
            fully_defined,
            ..
        } => {
            assert!(*fully_defined);
            assert_eq!(points[1], v(20.0, 20.0));
        }
        _ => unreachable!(),
    }

    let rejected = s
        .move_copy_entities(&MoveCopyRequest {
            entity_ids: vec![id],
            dx: 10.0,
            dy: 5.0,
            copy: false,
        })
        .unwrap_err();
    assert!(rejected.to_string().contains("conflicts"), "{rejected}");
    assert_eq!(s.dto(), fixed);

    let rejected = s
        .scale_entities(&ScaleRequest {
            entity_ids: vec![id],
            origin: v(0.0, 0.0),
            factor_text: "2".into(),
        })
        .unwrap_err();
    assert!(rejected.to_string().contains("conflicts"), "{rejected}");
    assert_eq!(s.dto(), fixed);

    let unfixed = s.toggle_fix(id).unwrap().sketch;
    assert_eq!(unfixed.dof.value, 6);
    match spline(&unfixed) {
        EntityDto::Spline { fully_defined, .. } => assert!(!fully_defined),
        _ => unreachable!(),
    }

    let moved = s
        .move_copy_entities(&MoveCopyRequest {
            entity_ids: vec![id],
            dx: 10.0,
            dy: 5.0,
            copy: false,
        })
        .unwrap();
    match spline(&moved.sketch) {
        EntityDto::Spline { points, .. } => {
            assert_eq!(points[0], v(10.0, 5.0));
            assert_eq!(points[1], v(30.0, 25.0));
            assert_eq!(points[2], v(50.0, 5.0));
        }
        _ => unreachable!(),
    }
}
