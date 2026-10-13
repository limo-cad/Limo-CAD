//! Integration tests for the Slot tool (M1 follow-up): capsule creation in
//! all three modes, structural constraints, typed-width Ø dimension (D9),
//! cursor-derived width, degenerate rejection, and single-record undo.

use limo_cad_sketch::{
    Constraint, EntityDto, OriginPlane, PlaneRef, SketchSession, SlotMode, SlotRequest, Vec2,
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

fn req(mode: SlotMode, p1: Vec2, p2: Vec2, cursor: Vec2, width_text: Option<&str>) -> SlotRequest {
    SlotRequest {
        ctrl_held: false,
        mode,
        p1,
        p2,
        cursor,
        width_mm: None,
        width_text: width_text.map(str::to_string),
    }
}

fn lines(dto: &limo_cad_sketch::SketchDto) -> Vec<&EntityDto> {
    dto.entities
        .iter()
        .filter(|e| matches!(e, EntityDto::Line { .. }))
        .collect()
}

fn arcs(dto: &limo_cad_sketch::SketchDto) -> Vec<&EntityDto> {
    dto.entities
        .iter()
        .filter(|e| matches!(e, EntityDto::Arc { .. }))
        .collect()
}

fn close(a: Vec2, b: Vec2) -> bool {
    a.distance(b) < 1e-7
}

#[test]
fn center_to_center_slot_geometry_constraints_and_dim() {
    let mut s = session();
    let r = s
        .add_slot(&req(
            SlotMode::CenterToCenter,
            v(0.0, 0.0),
            v(40.0, 0.0),
            v(40.0, 6.0),
            Some("10"),
        ))
        .unwrap();
    let dto = r.sketch;

    let ls = lines(&dto);
    let as_ = arcs(&dto);
    assert_eq!(ls.len(), 2, "two side lines");
    assert_eq!(as_.len(), 2, "two end-cap arcs");

    let (l1, l2) = match (ls[0], ls[1]) {
        (
            EntityDto::Line {
                start: a, end: b, ..
            },
            EntityDto::Line {
                start: c, end: d, ..
            },
        ) => ((a, b), (c, d)),
        _ => unreachable!(),
    };
    assert!(
        close(*l1.0, v(0.0, 5.0)) && close(*l1.1, v(40.0, 5.0)),
        "line1 on +y side"
    );
    assert!(
        close(*l2.0, v(0.0, -5.0)) && close(*l2.1, v(40.0, -5.0)),
        "line2 on -y side"
    );
    for arc in &as_ {
        match arc {
            EntityDto::Arc {
                center,
                radius,
                start_angle,
                end_angle,
                ..
            } => {
                assert!((radius - 5.0).abs() < 1e-9, "radius = width/2");
                assert!(close(*center, v(0.0, 0.0)) || close(*center, v(40.0, 0.0)));
                let sweep = (end_angle - start_angle).abs();
                assert!((sweep - std::f64::consts::PI).abs() < 1e-9, "semicircle");
            }
            _ => unreachable!(),
        }
    }

    let count = |pred: fn(&Constraint) -> bool| {
        dto.constraints
            .iter()
            .filter(|c| pred(&c.constraint))
            .count()
    };
    assert_eq!(count(|c| matches!(c, Constraint::Tangent { .. })), 4);
    assert_eq!(count(|c| matches!(c, Constraint::Parallel { .. })), 1);
    assert_eq!(count(|c| matches!(c, Constraint::Equal { .. })), 1);

    assert_eq!(dto.dimensions.len(), 1);
    assert_eq!(dto.dimensions[0].text, "Ø10.00");
    assert_eq!(dto.dimensions[0].kind, "diameter");
}

#[test]
fn overall_mode_insets_centers_by_radius() {
    let mut s = session();
    let r = s
        .add_slot(&req(
            SlotMode::Overall,
            v(0.0, 0.0),
            v(50.0, 0.0),
            v(50.0, 8.0),
            Some("10"),
        ))
        .unwrap();
    let dto = r.sketch;
    for arc in arcs(&dto) {
        match arc {
            EntityDto::Arc { center, .. } => {
                assert!(
                    close(*center, v(5.0, 0.0)) || close(*center, v(45.0, 0.0)),
                    "centers inset by r=5"
                );
            }
            _ => unreachable!(),
        }
    }

    match lines(&dto)[0] {
        EntityDto::Line { start, end, .. } => {
            assert!((start.distance(*end) - 40.0).abs() < 1e-9);
        }
        _ => unreachable!(),
    }
}

#[test]
fn center_point_mode_mirrors_the_far_center() {
    let mut s = session();
    let r = s
        .add_slot(&req(
            SlotMode::CenterPoint,
            v(20.0, 0.0),
            v(40.0, 0.0),
            v(40.0, 6.0),
            Some("10"),
        ))
        .unwrap();
    let dto = r.sketch;
    for arc in arcs(&dto) {
        match arc {
            EntityDto::Arc { center, .. } => {
                assert!(
                    close(*center, v(40.0, 0.0)) || close(*center, v(0.0, 0.0)),
                    "mirrored about (20,0)"
                );
            }
            _ => unreachable!(),
        }
    }
}

#[test]
fn cursor_driven_width_without_typing() {
    let mut s = session();

    let r = s
        .add_slot(&req(
            SlotMode::CenterToCenter,
            v(0.0, 0.0),
            v(40.0, 0.0),
            v(40.0, 6.0),
            None,
        ))
        .unwrap();
    let dto = r.sketch;
    match arcs(&dto)[0] {
        EntityDto::Arc { radius, .. } => assert!((radius - 6.0).abs() < 1e-9),
        _ => unreachable!(),
    }
    assert!(dto.dimensions.is_empty());
}

#[test]
fn formula_width_keeps_expression() {
    let mut s = session();
    let r = s
        .add_slot(&req(
            SlotMode::CenterToCenter,
            v(0.0, 0.0),
            v(40.0, 0.0),
            v(40.0, 0.0),
            Some("=5*2"),
        ))
        .unwrap();
    let dto = r.sketch;
    match arcs(&dto)[0] {
        EntityDto::Arc { radius, .. } => assert!((radius - 5.0).abs() < 1e-9),
        _ => unreachable!(),
    }
    assert_eq!(dto.dimensions[0].text, "Ø10.00");
    assert_eq!(dto.dimensions[0].param_expression.as_deref(), Some("5*2"));
}

#[test]
fn degenerate_cases_rejected() {
    let mut s = session();

    assert!(s
        .add_slot(&req(
            SlotMode::CenterToCenter,
            v(0.0, 0.0),
            v(0.0, 0.0),
            v(0.0, 5.0),
            Some("10")
        ))
        .is_err());

    assert!(s
        .add_slot(&req(
            SlotMode::Overall,
            v(0.0, 0.0),
            v(8.0, 0.0),
            v(8.0, 6.0),
            Some("10")
        ))
        .is_err());

    assert!(s
        .add_slot(&req(
            SlotMode::CenterToCenter,
            v(0.0, 0.0),
            v(40.0, 0.0),
            v(40.0, 0.0),
            None
        ))
        .is_err());

    assert!(s.dto().entities.is_empty());
}

#[test]
fn slot_is_one_undo_record() {
    let mut s = session();
    s.add_slot(&req(
        SlotMode::CenterToCenter,
        v(0.0, 0.0),
        v(40.0, 0.0),
        v(40.0, 6.0),
        Some("10"),
    ))
    .unwrap();
    assert!(!s.dto().entities.is_empty());
    let r = s.undo().unwrap();
    assert!(
        r.sketch.entities.is_empty(),
        "single undo removes points+lines+arcs+dim"
    );
    assert!(r.sketch.dimensions.is_empty());
}
