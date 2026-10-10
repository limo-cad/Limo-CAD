//! Solver test suite: Newton convergence per constraint equation, real DOF
//! counts, over-constraint rejection with
//! conflict reports (D4.2), locked dynamic-input endpoint math, and
//! drag-with-constraints cases.

use limo_cad_sketch::{
    CircleMode, Constraint, CurveCrossingRequest, DragPhase, EntityDto, EntityId,
    LineIntersectionRequest, LineTrackingRequest, LockedCircleRequest, LockedSegmentRequest,
    MovePointRequest, OriginPlane, PlaneRef, RectangleMode, SessionError, SketchSession,
    SnapTarget, TrackingAxis, Vec2,
};

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn locked_seg(
    from: Vec2,
    to_hint: Vec2,
    length_mm: Option<f64>,
    angle_deg: Option<f64>,
) -> LockedSegmentRequest {
    LockedSegmentRequest {
        from,
        to_hint,
        from_crossing: None,
        to_crossing: None,
        length_mm,
        angle_deg,
        length_text: None,
        angle_text: None,
        ctrl_held: false,
        tracking: None,
        intersection: None,
    }
}

const XY: PlaneRef = PlaneRef::OriginPlane {
    plane: OriginPlane::Xy,
};

/// Session with grid snap OFF (deterministic coordinates).
fn session() -> SketchSession {
    SketchSession::new("Sketch1", XY, XY.basis().unwrap(), false)
}

fn move_req(point_id: limo_cad_sketch::EntityId, to: Vec2) -> MovePointRequest {
    MovePointRequest {
        point_id,
        to_raw: to,
        ctrl_held: false,
        phase: DragPhase::Single,
    }
}

fn line(dto: &limo_cad_sketch::SketchDto, id: limo_cad_sketch::EntityId) -> (Vec2, Vec2) {
    match dto.entities.iter().find(|e| e.id() == id) {
        Some(EntityDto::Line { start, end, .. }) => (*start, *end),
        other => panic!("expected line, got {other:?}"),
    }
}

fn point(dto: &limo_cad_sketch::SketchDto, id: limo_cad_sketch::EntityId) -> Vec2 {
    match dto.entities.iter().find(|entity| entity.id() == id) {
        Some(EntityDto::Point { position, .. }) => *position,
        other => panic!("expected point, got {other:?}"),
    }
}

fn circle(dto: &limo_cad_sketch::SketchDto, id: limo_cad_sketch::EntityId) -> (Vec2, f64) {
    match dto.entities.iter().find(|entity| entity.id() == id) {
        Some(EntityDto::Circle { center, radius, .. }) => (*center, *radius),
        other => panic!("expected circle, got {other:?}"),
    }
}

fn close(a: Vec2, b: Vec2) -> bool {
    a.distance(b) < 1e-7
}

#[test]
fn dof_counts_free_line_then_h_then_fixed_rectangle() {
    let mut s = session();

    let l = s.add_line(v(3.0, 7.0), v(53.0, 37.0), true).unwrap();
    assert_eq!(l.sketch.dof.value, 4);

    let r = s.add_constraint(Constraint::Horizontal {
        entity: l.entity_id,
    });
    assert!(r.is_ok(), "H on free line is consistent: {:?}", r.err());
    assert_eq!(s.dto().dof.value, 3);

    let mut s = session();
    let rect = s
        .add_rectangle(RectangleMode::TwoPoint, v(5.0, 7.0), v(45.0, 27.0))
        .unwrap();
    let lines: Vec<_> = rect
        .sketch
        .entities
        .iter()
        .filter(|e| matches!(e, EntityDto::Line { .. }))
        .map(|e| e.id())
        .collect();
    assert_eq!(lines.len(), 4);

    assert_eq!(rect.sketch.dof.value, 4);
    let corner = rect.entities[0];
    s.toggle_fix(corner).unwrap();
    assert_eq!(s.dto().dof.value, 2);

    let opposite = rect.entities[2];
    s.toggle_fix(opposite).unwrap();
    let dto = s.dto();
    assert_eq!(dto.dof.value, 0);
    assert!(dto.dof.fully_defined);
    assert!(dto.entities.iter().all(|e| match e {
        EntityDto::Line { fully_defined, .. } => *fully_defined,
        EntityDto::Point { fully_defined, .. } => *fully_defined,
        _ => true,
    }));
}

#[test]
fn parallel_and_perpendicular_hold_and_count_dof() {
    let mut s = session();
    let l1 = s.add_line(v(0.0, 0.0), v(50.0, 0.0), true).unwrap();
    let l2 = s.add_line(v(0.0, 20.0), v(50.0, 25.0), true).unwrap();
    s.add_constraint(Constraint::Parallel {
        a: l1.entity_id,
        b: l2.entity_id,
    })
    .unwrap();
    let dto = s.dto();
    let (a0, a1) = line(&dto, l1.entity_id);
    let (b0, b1) = line(&dto, l2.entity_id);

    let da = a1 - a0;
    let db = b1 - b0;
    assert!((da.x * db.y - da.y * db.x).abs() < 1e-7);
    assert_eq!(dto.dof.value, 8 - 1);
}

#[test]
fn adding_parallel_preserves_authored_line_lengths_without_persisting_a_length_lock() {
    let mut s = session();

    let vertical = s
        .add_line(v(0.0, 0.0), v(0.0, 40.094_115_730_976), true)
        .unwrap();
    let top = s
        .add_line(
            v(0.0, 40.094_115_730_976),
            v(40.094_115_730_976, 40.094_115_730_976),
            true,
        )
        .unwrap();
    let bottom = s.add_line(v(0.0, 0.0), v(40.0, -1.0), true).unwrap();
    assert_eq!(vertical.start_point_id, bottom.start_point_id);

    s.add_constraint(Constraint::Vertical {
        entity: vertical.entity_id,
    })
    .unwrap();
    s.toggle_fix(vertical.start_point_id).unwrap();
    s.add_constraint(Constraint::Perpendicular {
        a: vertical.entity_id,
        b: top.entity_id,
    })
    .unwrap();
    s.add_constraint(Constraint::Equal {
        a: vertical.entity_id,
        b: top.entity_id,
    })
    .unwrap();

    let before = s.dto();
    let (top_a, top_b) = line(&before, top.entity_id);
    let (bottom_a, bottom_b) = line(&before, bottom.entity_id);
    let top_length = top_a.distance(top_b);
    let bottom_length = bottom_a.distance(bottom_b);

    let solved = s
        .add_constraint(Constraint::Parallel {
            a: top.entity_id,
            b: bottom.entity_id,
        })
        .unwrap()
        .sketch;
    let (top_a, top_b) = line(&solved, top.entity_id);
    let (bottom_a, bottom_b) = line(&solved, bottom.entity_id);
    let top_direction = top_b - top_a;
    let bottom_direction = bottom_b - bottom_a;
    assert!(
        (top_direction.x * bottom_direction.y - top_direction.y * bottom_direction.x).abs() < 1e-7,
        "parallelism must hold"
    );
    assert!((top_a.distance(top_b) - top_length).abs() < 1e-7);
    assert!((bottom_a.distance(bottom_b) - bottom_length).abs() < 1e-7);
    assert!(
        bottom_a.distance(bottom_b) < 100.0,
        "line must not run away"
    );
    assert_eq!(solved.dof.value, 2, "temporary stays must not consume DOF");

    let moved = s
        .move_point(move_req(bottom.end_point_id, v(60.0, 0.0)))
        .unwrap()
        .sketch;
    let (bottom_a, bottom_b) = line(&moved, bottom.entity_id);
    assert!((bottom_a.distance(bottom_b) - 60.0).abs() < 1e-7);
}

#[test]
fn parallel_length_preservation_is_stable_with_disconnected_constrained_geometry() {
    let mut s = session();
    let first_point = s.add_point(v(-12.0, 8.0)).unwrap().entities[0];
    let second_point = s.add_point(v(15.0, 11.0)).unwrap().entities[0];
    s.add_constraint(Constraint::HorizontalPoints {
        a: first_point,
        b: second_point,
    })
    .unwrap();
    for index in 0..9 {
        let line = s
            .add_line(
                v(-30.0, -30.0 - index as f64 * 3.0),
                v(-10.0, -29.5 - index as f64 * 3.0),
                true,
            )
            .unwrap();
        s.add_constraint(Constraint::Horizontal {
            entity: line.entity_id,
        })
        .unwrap();
    }

    let vertical = s
        .add_line(v(100.0, 0.0), v(100.0, 40.094_115_730_976), true)
        .unwrap();
    let top = s
        .add_line(
            v(100.0, 40.094_115_730_976),
            v(140.094_115_730_976, 40.094_115_730_976),
            true,
        )
        .unwrap();
    let bottom = s.add_line(v(100.0, 0.0), v(140.0, -1.0), true).unwrap();
    s.add_constraints(vec![
        Constraint::Vertical {
            entity: vertical.entity_id,
        },
        Constraint::Perpendicular {
            a: vertical.entity_id,
            b: top.entity_id,
        },
        Constraint::Equal {
            a: vertical.entity_id,
            b: top.entity_id,
        },
    ])
    .unwrap();
    s.toggle_fix(vertical.start_point_id).unwrap();

    let before = s.dto();
    let top_length = {
        let (a, b) = line(&before, top.entity_id);
        a.distance(b)
    };
    let bottom_length = {
        let (a, b) = line(&before, bottom.entity_id);
        a.distance(b)
    };
    let solved = s
        .add_constraint(Constraint::Parallel {
            a: top.entity_id,
            b: bottom.entity_id,
        })
        .unwrap()
        .sketch;
    let (top_a, top_b) = line(&solved, top.entity_id);
    let (bottom_a, bottom_b) = line(&solved, bottom.entity_id);
    assert!((top_a.distance(top_b) - top_length).abs() < 1e-7);
    assert!((bottom_a.distance(bottom_b) - bottom_length).abs() < 1e-7);
}

#[test]
fn direction_only_constraints_preserve_authored_line_lengths() {
    let cases = [
        (
            "horizontal",
            Constraint::Horizontal {
                entity: EntityId(0),
            },
        ),
        (
            "vertical",
            Constraint::Vertical {
                entity: EntityId(0),
            },
        ),
    ];

    for (name, template) in cases {
        let mut s = session();
        let line_result = s.add_line(v(5.0, 7.0), v(42.0, 31.0), true).unwrap();
        let before = s.dto();
        let (before_start, before_end) = line(&before, line_result.entity_id);
        let before_length = before_start.distance(before_end);
        let constraint = match template {
            Constraint::Horizontal { .. } => Constraint::Horizontal {
                entity: line_result.entity_id,
            },
            Constraint::Vertical { .. } => Constraint::Vertical {
                entity: line_result.entity_id,
            },
            _ => unreachable!(),
        };
        let solved = s
            .add_constraint(constraint)
            .unwrap_or_else(|error| panic!("{name} failed: {error}"))
            .sketch;
        let (start, end) = line(&solved, line_result.entity_id);
        assert!(
            (start.distance(end) - before_length).abs() < 1e-7,
            "{name} changed length from {before_length} to {}",
            start.distance(end)
        );
    }
}

#[test]
fn perpendicular_and_angle_preserve_both_line_lengths() {
    for use_angle_dimension in [false, true] {
        let mut s = session();
        let first = s.add_line(v(0.0, 0.0), v(37.0, 11.0), true).unwrap();
        let second = s.add_line(v(10.0, 30.0), v(31.0, 68.0), true).unwrap();
        let before = s.dto();
        let first_length = {
            let (a, b) = line(&before, first.entity_id);
            a.distance(b)
        };
        let second_length = {
            let (a, b) = line(&before, second.entity_id);
            a.distance(b)
        };

        let constraint = if use_angle_dimension {
            Constraint::Angle {
                a: first.entity_id,
                b: second.entity_id,
                value: 90.0,
            }
        } else {
            Constraint::Perpendicular {
                a: first.entity_id,
                b: second.entity_id,
            }
        };
        let solved = s.add_constraint(constraint).unwrap().sketch;
        for (id, expected) in [
            (first.entity_id, first_length),
            (second.entity_id, second_length),
        ] {
            let (a, b) = line(&solved, id);
            assert!(
                (a.distance(b) - expected).abs() < 1e-7,
                "{} changed line {id:?} from {expected} to {}",
                if use_angle_dimension {
                    "angle"
                } else {
                    "perpendicular"
                },
                a.distance(b)
            );
        }
    }
}

#[test]
fn parallel_keeps_the_reference_chain_and_rotates_only_the_free_follower_end() {
    let mut s = session();
    let bottom = s.add_line(v(0.0, 0.0), v(30.0, 0.0), true).unwrap();
    let right = s.add_line(v(30.0, 0.0), v(36.0, 24.0), true).unwrap();
    let top = s.add_line(v(36.0, 24.0), v(2.0, 30.0), true).unwrap();

    s.toggle_fix(bottom.start_point_id).unwrap();
    s.add_constraint(Constraint::Horizontal {
        entity: bottom.entity_id,
    })
    .unwrap();
    s.add_constraint(Constraint::Perpendicular {
        a: bottom.entity_id,
        b: right.entity_id,
    })
    .unwrap();

    let before = s.dto();
    let before_bottom = line(&before, bottom.entity_id);
    let before_right = line(&before, right.entity_id);
    let before_top = line(&before, top.entity_id);
    let top_length = before_top.0.distance(before_top.1);

    let solved = s
        .add_constraint(Constraint::Parallel {
            a: bottom.entity_id,
            b: top.entity_id,
        })
        .unwrap()
        .sketch;
    let after_bottom = line(&solved, bottom.entity_id);
    let after_right = line(&solved, right.entity_id);
    let after_top = line(&solved, top.entity_id);

    assert!(close(after_bottom.0, before_bottom.0));
    assert!(close(after_bottom.1, before_bottom.1));
    assert!(close(after_right.0, before_right.0));
    assert!(close(after_right.1, before_right.1));
    assert!(close(after_top.0, before_top.0));
    assert!((after_top.0.distance(after_top.1) - top_length).abs() < 1e-7);
    let bottom_direction = after_bottom.1 - after_bottom.0;
    let top_direction = after_top.1 - after_top.0;
    let cross = bottom_direction.x * top_direction.y - bottom_direction.y * top_direction.x;
    assert!(cross.abs() < 1e-7);
}

#[test]
fn point_axis_alignment_preserves_the_pair_distance() {
    for horizontal in [false, true] {
        let mut s = session();
        let first = s.add_point(v(3.0, 7.0)).unwrap().entities[0];
        let second = s.add_point(v(31.0, 29.0)).unwrap().entities[0];
        let before_distance = point(&s.dto(), first).distance(point(&s.dto(), second));
        let constraint = if horizontal {
            Constraint::HorizontalPoints {
                a: first,
                b: second,
            }
        } else {
            Constraint::VerticalPoints {
                a: first,
                b: second,
            }
        };
        let solved = s.add_constraint(constraint).unwrap().sketch;
        let after_distance = point(&solved, first).distance(point(&solved, second));
        assert!(
            (after_distance - before_distance).abs() < 1e-7,
            "point alignment changed distance from {before_distance} to {after_distance}"
        );
    }
}

fn assert_same_bearing(before: Vec2, after: Vec2, label: &str) {
    let scale = before.length() * after.length();
    assert!(scale > 1e-12, "{label}: degenerate direction");
    let cross = before.x * after.y - before.y * after.x;
    assert!(
        cross.abs() / scale < 1e-7 && before.dot(after) > 0.0,
        "{label}: bearing changed from {before:?} to {after:?}"
    );
}

#[test]
fn equal_changes_size_only_and_uses_the_first_selection_as_reference() {
    let mut s = session();
    let reference = s.add_line(v(0.0, 0.0), v(48.0, 14.0), true).unwrap();
    let target = s.add_line(v(10.0, 30.0), v(26.0, 54.0), true).unwrap();
    let before = s.dto();
    let (reference_a, reference_b) = line(&before, reference.entity_id);
    let (target_a, target_b) = line(&before, target.entity_id);
    let reference_direction = reference_b - reference_a;
    let target_direction = target_b - target_a;

    let solved = s
        .add_constraint(Constraint::Equal {
            a: reference.entity_id,
            b: target.entity_id,
        })
        .unwrap()
        .sketch;
    let (reference_a2, reference_b2) = line(&solved, reference.entity_id);
    let (target_a2, target_b2) = line(&solved, target.entity_id);
    assert!(
        (reference_a2.distance(reference_b2) - reference_direction.length()).abs() < 1e-7,
        "reference length changed from {} to {}",
        reference_direction.length(),
        reference_a2.distance(reference_b2)
    );
    assert!((target_a2.distance(target_b2) - reference_direction.length()).abs() < 1e-7);
    assert_same_bearing(
        reference_direction,
        reference_b2 - reference_a2,
        "equal reference",
    );
    assert_same_bearing(target_direction, target_b2 - target_a2, "equal target");

    let mut s = session();
    let reference = s
        .add_circle(CircleMode::CenterDiameter, v(5.0, 7.0), v(15.0, 7.0))
        .unwrap()
        .entities[0];
    let target = s
        .add_circle(CircleMode::CenterDiameter, v(40.0, 30.0), v(46.0, 30.0))
        .unwrap()
        .entities[0];
    let before = s.dto();
    let (reference_center, reference_radius) = circle(&before, reference);
    let (target_center, _) = circle(&before, target);
    let solved = s
        .add_constraint(Constraint::Equal {
            a: reference,
            b: target,
        })
        .unwrap()
        .sketch;
    let (reference_center2, reference_radius2) = circle(&solved, reference);
    let (target_center2, target_radius2) = circle(&solved, target);
    assert!(close(reference_center2, reference_center));
    assert!(close(target_center2, target_center));
    assert!((reference_radius2 - reference_radius).abs() < 1e-7);
    assert!((target_radius2 - reference_radius).abs() < 1e-7);
}

#[test]
fn position_constraints_preserve_carrier_shape_and_curve_size() {
    for midpoint in [false, true] {
        let mut s = session();
        let carrier = s.add_line(v(2.0, 5.0), v(43.0, 18.0), true).unwrap();
        let marker = s.add_point(v(25.0, 37.0)).unwrap().entities[0];
        let before = s.dto();
        let (before_a, before_b) = line(&before, carrier.entity_id);
        let before_direction = before_b - before_a;
        let constraint = if midpoint {
            Constraint::Midpoint {
                a: marker,
                b: carrier.entity_id,
            }
        } else {
            Constraint::Coincident {
                a: marker,
                b: carrier.entity_id,
            }
        };
        let solved = s.add_constraint(constraint).unwrap().sketch;
        let (after_a, after_b) = line(&solved, carrier.entity_id);
        assert!((after_a.distance(after_b) - before_direction.length()).abs() < 1e-7);
        assert_same_bearing(
            before_direction,
            after_b - after_a,
            if midpoint { "midpoint" } else { "coincident" },
        );
    }

    let mut s = session();
    let curve = s
        .add_circle(CircleMode::CenterDiameter, v(10.0, 10.0), v(18.0, 10.0))
        .unwrap()
        .entities[0];
    let marker = s.add_point(v(32.0, 27.0)).unwrap().entities[0];
    let radius = circle(&s.dto(), curve).1;
    let solved = s
        .add_constraint(Constraint::Coincident {
            a: marker,
            b: curve,
        })
        .unwrap()
        .sketch;
    assert!(
        (circle(&solved, curve).1 - radius).abs() < 1e-7,
        "coincident radius changed from {radius} to {}",
        circle(&solved, curve).1
    );
}

#[test]
fn tangent_preserves_line_lengths_and_curve_radii() {
    let mut s = session();
    let line_result = s.add_line(v(0.0, 0.0), v(47.0, 13.0), true).unwrap();
    let curve = s
        .add_circle(CircleMode::CenterDiameter, v(25.0, 35.0), v(34.0, 35.0))
        .unwrap()
        .entities[0];
    let before = s.dto();
    let (line_a, line_b) = line(&before, line_result.entity_id);
    let radius = circle(&before, curve).1;
    let solved = s
        .add_constraint(Constraint::Tangent {
            a: line_result.entity_id,
            b: curve,
        })
        .unwrap()
        .sketch;
    let (line_a2, line_b2) = line(&solved, line_result.entity_id);
    assert!((line_a2.distance(line_b2) - line_a.distance(line_b)).abs() < 1e-7);
    assert!((circle(&solved, curve).1 - radius).abs() < 1e-7);

    let mut s = session();
    let first = s
        .add_circle(CircleMode::CenterDiameter, v(0.0, 0.0), v(6.0, 0.0))
        .unwrap()
        .entities[0];
    let second = s
        .add_circle(CircleMode::CenterDiameter, v(30.0, 8.0), v(39.0, 8.0))
        .unwrap()
        .entities[0];
    let first_radius = circle(&s.dto(), first).1;
    let second_radius = circle(&s.dto(), second).1;
    let solved = s
        .add_constraint(Constraint::Tangent {
            a: first,
            b: second,
        })
        .unwrap()
        .sketch;
    assert!(
        (circle(&solved, first).1 - first_radius).abs() < 1e-7,
        "first tangent radius changed from {first_radius} to {}",
        circle(&solved, first).1
    );
    assert!(
        (circle(&solved, second).1 - second_radius).abs() < 1e-7,
        "second tangent radius changed from {second_radius} to {}",
        circle(&solved, second).1
    );
}

#[test]
fn concentric_and_collinear_change_position_or_angle_without_resizing() {
    let mut s = session();
    let first = s
        .add_circle(CircleMode::CenterDiameter, v(0.0, 0.0), v(7.0, 0.0))
        .unwrap()
        .entities[0];
    let second = s
        .add_circle(CircleMode::CenterDiameter, v(30.0, 20.0), v(41.0, 20.0))
        .unwrap()
        .entities[0];
    let first_radius = circle(&s.dto(), first).1;
    let second_radius = circle(&s.dto(), second).1;
    let solved = s
        .add_constraint(Constraint::Concentric {
            a: first,
            b: second,
        })
        .unwrap()
        .sketch;
    assert!((circle(&solved, first).1 - first_radius).abs() < 1e-7);
    assert!((circle(&solved, second).1 - second_radius).abs() < 1e-7);

    let mut s = session();
    let first = s.add_line(v(0.0, 0.0), v(36.0, 8.0), true).unwrap();
    let second = s.add_line(v(10.0, 30.0), v(27.0, 67.0), true).unwrap();
    let before = s.dto();
    let (first_before_a, first_before_b) = line(&before, first.entity_id);
    let (second_before_a, second_before_b) = line(&before, second.entity_id);
    let first_length = { first_before_a.distance(first_before_b) };
    let second_length = { second_before_a.distance(second_before_b) };
    let solved = s
        .add_constraint(Constraint::Collinear {
            a: first.entity_id,
            b: second.entity_id,
        })
        .unwrap()
        .sketch;
    let (first_a, first_b) = line(&solved, first.entity_id);
    let (second_a, second_b) = line(&solved, second.entity_id);
    assert!((first_a.distance(first_b) - first_length).abs() < 1e-7);
    assert!((second_a.distance(second_b) - second_length).abs() < 1e-7);
    assert!(close(first_a, first_before_a));
    assert!(close(first_b, first_before_b));

    let reference_direction = (first_before_b - first_before_a) * (1.0 / first_length);
    let reference_midpoint = (first_before_a + first_before_b) * 0.5;
    let before_along =
        ((second_before_a + second_before_b) * 0.5 - reference_midpoint).dot(reference_direction);
    let after_along = ((second_a + second_b) * 0.5 - reference_midpoint).dot(reference_direction);
    assert!(
        (after_along - before_along).abs() < 1e-7,
        "collinear slid the target along its reference: {before_along} -> {after_along}"
    );
}

#[test]
fn size_dimensions_preserve_bearings_and_unmeasured_shape() {
    let mut s = session();
    let line_result = s.add_line(v(5.0, 8.0), v(38.0, 29.0), true).unwrap();
    let before = s.dto();
    let (before_a, before_b) = line(&before, line_result.entity_id);
    let before_direction = before_b - before_a;
    let solved = s
        .add_constraint(Constraint::Distance {
            from: line_result.entity_id,
            to: None,
            value: 70.0,
        })
        .unwrap()
        .sketch;
    let (after_a, after_b) = line(&solved, line_result.entity_id);
    assert!((after_a.distance(after_b) - 70.0).abs() < 1e-7);
    assert_same_bearing(before_direction, after_b - after_a, "line length dimension");

    let mut s = session();
    let first = s.add_point(v(3.0, 4.0)).unwrap().entities[0];
    let second = s.add_point(v(24.0, 31.0)).unwrap().entities[0];
    let before_direction = point(&s.dto(), second) - point(&s.dto(), first);
    let solved = s
        .add_constraint(Constraint::Distance {
            from: first,
            to: Some(second),
            value: 55.0,
        })
        .unwrap()
        .sketch;
    let after_direction = point(&solved, second) - point(&solved, first);
    assert!((after_direction.length() - 55.0).abs() < 1e-7);
    assert_same_bearing(
        before_direction,
        after_direction,
        "point distance dimension",
    );

    let mut s = session();
    let carrier = s.add_line(v(0.0, 0.0), v(42.0, 11.0), true).unwrap();
    let marker = s.add_point(v(20.0, 31.0)).unwrap().entities[0];
    let before = s.dto();
    let (before_a, before_b) = line(&before, carrier.entity_id);
    let before_direction = before_b - before_a;
    let solved = s
        .add_constraint(Constraint::Distance {
            from: marker,
            to: Some(carrier.entity_id),
            value: 18.0,
        })
        .unwrap()
        .sketch;
    let (after_a, after_b) = line(&solved, carrier.entity_id);
    assert!((after_a.distance(after_b) - before_direction.length()).abs() < 1e-7);
    assert_same_bearing(before_direction, after_b - after_a, "point-line distance");

    let mut s = session();
    let curve = s
        .add_circle(CircleMode::CenterDiameter, v(12.0, 14.0), v(19.0, 14.0))
        .unwrap()
        .entities[0];
    let before_center = circle(&s.dto(), curve).0;
    let solved = s
        .add_constraint(Constraint::Diameter {
            entity: curve,
            value: 24.0,
        })
        .unwrap()
        .sketch;
    let (after_center, after_radius) = circle(&solved, curve);
    assert!(close(before_center, after_center));
    assert!((after_radius - 12.0).abs() < 1e-7);
}

#[test]
fn line_and_radial_offset_dimensions_do_not_distort_their_references() {
    let mut s = session();
    let first = s.add_line(v(0.0, 0.0), v(39.0, 9.0), true).unwrap();
    let second = s.add_line(v(12.0, 26.0), v(49.0, 41.0), true).unwrap();
    let before = s.dto();
    let (first_a, first_b) = line(&before, first.entity_id);
    let (second_a, second_b) = line(&before, second.entity_id);
    let first_direction = first_b - first_a;
    let second_direction = second_b - second_a;
    let solved = s
        .add_constraint(Constraint::Distance {
            from: first.entity_id,
            to: Some(second.entity_id),
            value: 20.0,
        })
        .unwrap()
        .sketch;
    let (first_a2, first_b2) = line(&solved, first.entity_id);
    let (second_a2, second_b2) = line(&solved, second.entity_id);
    assert!((first_a2.distance(first_b2) - first_direction.length()).abs() < 1e-7);
    assert!((second_a2.distance(second_b2) - second_direction.length()).abs() < 1e-7);
    assert_same_bearing(first_direction, first_b2 - first_a2, "first offset line");
    assert_same_bearing(
        second_direction,
        second_b2 - second_a2,
        "second offset line",
    );

    let mut s = session();
    let reference = s
        .add_circle(CircleMode::CenterDiameter, v(0.0, 0.0), v(5.0, 0.0))
        .unwrap()
        .entities[0];
    let target = s
        .add_circle(CircleMode::CenterDiameter, v(0.0, 0.0), v(8.0, 0.0))
        .unwrap()
        .entities[0];
    let before = s.dto();
    let (reference_center, reference_radius) = circle(&before, reference);
    let (target_center, _) = circle(&before, target);
    let solved = s
        .add_constraint(Constraint::Distance {
            from: reference,
            to: Some(target),
            value: 6.0,
        })
        .unwrap()
        .sketch;
    let (reference_center2, reference_radius2) = circle(&solved, reference);
    let (target_center2, target_radius2) = circle(&solved, target);
    assert!(close(reference_center, reference_center2));
    assert!(close(target_center, target_center2));
    assert!((reference_radius2 - reference_radius).abs() < 1e-7);
    assert!((target_radius2 - (reference_radius + 6.0)).abs() < 1e-7);
}

#[test]
fn axis_angle_dimension_changes_angle_without_changing_length() {
    let mut s = session();
    let line_result = s.add_line(v(3.0, 4.0), v(31.0, 29.0), true).unwrap();
    let before_length = {
        let (a, b) = line(&s.dto(), line_result.entity_id);
        a.distance(b)
    };
    let solved = s
        .add_constraint(Constraint::Angle {
            a: line_result.entity_id,
            b: EntityId(0),
            value: 30.0,
        })
        .unwrap()
        .sketch;
    let (a, b) = line(&solved, line_result.entity_id);
    let direction = b - a;
    assert!((direction.length() - before_length).abs() < 1e-7);
    assert!((direction.y.atan2(direction.x).to_degrees() - 30.0).abs() < 1e-7);
}

#[test]
fn symmetry_preserves_the_datum_shape_and_first_line_size() {
    let mut s = session();
    let reference = s.add_line(v(8.0, 9.0), v(36.0, 18.0), true).unwrap();
    let target = s.add_line(v(-13.0, 11.0), v(-29.0, 28.0), true).unwrap();
    let axis = s.add_line(v(1.0, -20.0), v(4.0, 50.0), true).unwrap();
    let before = s.dto();
    let (reference_a, reference_b) = line(&before, reference.entity_id);
    let (axis_a, axis_b) = line(&before, axis.entity_id);
    let reference_length = reference_a.distance(reference_b);
    let axis_direction = axis_b - axis_a;

    let solved = s
        .add_constraint(Constraint::Symmetry {
            a: reference.entity_id,
            b: target.entity_id,
            axis: axis.entity_id,
        })
        .unwrap()
        .sketch;
    let (reference_a2, reference_b2) = line(&solved, reference.entity_id);
    let (target_a2, target_b2) = line(&solved, target.entity_id);
    let (axis_a2, axis_b2) = line(&solved, axis.entity_id);
    assert!((reference_a2.distance(reference_b2) - reference_length).abs() < 1e-7);
    assert!((target_a2.distance(target_b2) - reference_length).abs() < 1e-7);
    assert!((axis_a2.distance(axis_b2) - axis_direction.length()).abs() < 1e-7);
    assert_same_bearing(axis_direction, axis_b2 - axis_a2, "symmetry axis");
}

#[test]
fn applying_fix_does_not_move_the_entity_it_records() {
    let mut s = session();
    let line_result = s.add_line(v(4.0, 7.0), v(39.0, 21.0), true).unwrap();
    let before = s.dto();
    let (before_a, before_b) = line(&before, line_result.entity_id);
    let solved = s.toggle_fix(line_result.entity_id).unwrap().sketch;
    let (after_a, after_b) = line(&solved, line_result.entity_id);
    assert!(close(before_a, after_a));
    assert!(close(before_b, after_b));
}

#[test]
fn coincident_point_on_line_solves() {
    let mut s = session();
    let l = s.add_line(v(0.0, 0.0), v(50.0, 0.0), true).unwrap();
    let p = s.add_point(v(25.0, 10.0)).unwrap();
    s.add_constraint(Constraint::Coincident {
        a: p.entities[0],
        b: l.entity_id,
    })
    .unwrap();
    let dto = s.dto();
    let point = dto
        .entities
        .iter()
        .find(|e| e.id() == p.entities[0])
        .unwrap();
    match point {
        EntityDto::Point { position, .. } => {
            let (a, b) = line(&dto, l.entity_id);
            let d = (b.x - a.x) * (position.y - a.y) - (b.y - a.y) * (position.x - a.x);
            assert!(d.abs() < 1e-7, "point must lie on the line");
        }
        _ => panic!("expected point"),
    }
}

#[test]
fn midpoint_constraint_moves_point_to_midpoint() {
    let mut s = session();
    let l = s.add_line(v(0.0, 0.0), v(50.0, 20.0), true).unwrap();
    let p = s.add_point(v(30.0, 30.0)).unwrap();
    s.add_constraint(Constraint::Midpoint {
        a: p.entities[0],
        b: l.entity_id,
    })
    .unwrap();

    let dto = s.dto();
    let (a, b) = line(&dto, l.entity_id);
    match dto
        .entities
        .iter()
        .find(|e| e.id() == p.entities[0])
        .unwrap()
    {
        EntityDto::Point { position, .. } => {
            let mid = v((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
            assert!(
                close(*position, mid),
                "point {position:?} != midpoint {mid:?}"
            );
        }
        _ => panic!("expected point"),
    }
}

#[test]
fn equal_lines_equalize_lengths() {
    let mut s = session();
    let l1 = s.add_line(v(0.0, 0.0), v(50.0, 0.0), true).unwrap();
    let l2 = s.add_line(v(0.0, 20.0), v(30.0, 20.0), true).unwrap();
    s.add_constraint(Constraint::Equal {
        a: l1.entity_id,
        b: l2.entity_id,
    })
    .unwrap();
    let dto = s.dto();
    let (a0, a1) = line(&dto, l1.entity_id);
    let (b0, b1) = line(&dto, l2.entity_id);
    assert!((a0.distance(a1) - b0.distance(b1)).abs() < 1e-7);
}

#[test]
fn tangent_line_circle_solves() {
    let mut s = session();
    let c = s
        .add_circle(CircleMode::CenterDiameter, v(50.0, 50.0), v(60.0, 50.0))
        .unwrap();
    let l = s.add_line(v(0.0, 30.0), v(80.0, 30.0), true).unwrap();
    s.add_constraint(Constraint::Tangent {
        a: l.entity_id,
        b: c.entities[0],
    })
    .unwrap();
    let dto = s.dto();
    let circle = dto
        .entities
        .iter()
        .find(|e| e.id() == c.entities[0])
        .unwrap();
    let (a, b) = line(&dto, l.entity_id);
    match circle {
        EntityDto::Circle { center, radius, .. } => {
            let d = ((b.x - a.x) * (center.y - a.y) - (b.y - a.y) * (center.x - a.x)).abs()
                / a.distance(b);
            assert!((d - radius).abs() < 1e-6, "d={d} r={radius}");
        }
        _ => panic!("expected circle"),
    }
}

#[test]
fn concentric_circles_share_a_center() {
    let mut s = session();
    let c1 = s
        .add_circle(CircleMode::CenterDiameter, v(10.0, 10.0), v(20.0, 10.0))
        .unwrap();
    let c2 = s
        .add_circle(CircleMode::CenterDiameter, v(40.0, 30.0), v(50.0, 30.0))
        .unwrap();
    s.add_constraint(Constraint::Concentric {
        a: c1.entities[0],
        b: c2.entities[0],
    })
    .unwrap();
    let dto = s.dto();
    let centers: Vec<Vec2> = [c1.entities[0], c2.entities[0]]
        .iter()
        .map(
            |id| match dto.entities.iter().find(|e| e.id() == *id).unwrap() {
                EntityDto::Circle { center, .. } => *center,
                _ => panic!("expected circle"),
            },
        )
        .collect();
    assert!(close(centers[0], centers[1]));
}

#[test]
fn symmetry_of_two_points_about_a_line() {
    let mut s = session();
    let axis = s.add_line(v(0.0, 0.0), v(0.0, 50.0), true).unwrap();

    s.toggle_fix(axis.start_point_id).unwrap();
    s.toggle_fix(axis.end_point_id).unwrap();
    let p1 = s.add_point(v(10.0, 20.0)).unwrap();
    let p2 = s.add_point(v(-8.0, 20.0)).unwrap();
    s.add_constraint(Constraint::Symmetry {
        a: p1.entities[0],
        b: p2.entities[0],
        axis: axis.entity_id,
    })
    .unwrap();
    let dto = s.dto();
    let pos = |id| match dto.entities.iter().find(|e| e.id() == id).unwrap() {
        EntityDto::Point { position, .. } => *position,
        _ => panic!("expected point"),
    };
    let (a, b) = (pos(p1.entities[0]), pos(p2.entities[0]));

    assert!(
        ((a.x + b.x) / 2.0).abs() < 1e-7,
        "midpoint x = {}",
        (a.x + b.x) / 2.0
    );
    assert!((a.y - b.y).abs() < 1e-7, "segment perpendicular to axis");
}

#[test]
fn symmetry_converges_from_ordinary_offsets_with_a_free_axis() {
    for offset in [5.0, 12.0] {
        let mut s = session();
        let axis = s.add_line(v(40.0, -60.0), v(40.0, -20.0), true).unwrap();
        s.add_constraint(Constraint::Vertical {
            entity: axis.entity_id,
        })
        .unwrap();
        let a = s.add_point(v(30.0, -40.0)).unwrap().entities[0];
        let b = s
            .add_point(v(50.0 + offset, -40.0 + offset))
            .unwrap()
            .entities[0];

        s.add_constraint(Constraint::Symmetry {
            a,
            b,
            axis: axis.entity_id,
        })
        .unwrap_or_else(|error| panic!("offset {offset} failed: {error}"));

        let dto = s.dto();
        let (axis_start, axis_end) = line(&dto, axis.entity_id);
        let axis_direction = axis_end - axis_start;
        let pa = point(&dto, a);
        let pb = point(&dto, b);
        let midpoint = (pa + pb) * 0.5;
        let midpoint_cross = axis_direction.x * (midpoint.y - axis_start.y)
            - axis_direction.y * (midpoint.x - axis_start.x);
        assert!(
            midpoint_cross.abs() / axis_direction.length() < 1e-7,
            "offset {offset}: midpoint is not on the axis"
        );
        assert!(
            (pb - pa).dot(axis_direction).abs() / ((pb - pa).length() * axis_direction.length())
                < 1e-7,
            "offset {offset}: mirrored segment is not perpendicular"
        );
    }
}

#[test]
fn circle_tangent_converges_without_inflating_unconstrained_radii() {
    for center_distance in [15.0, 30.0] {
        let mut s = session();
        let first = s
            .add_circle(CircleMode::CenterDiameter, v(0.0, 0.0), v(5.0, 0.0))
            .unwrap()
            .entities[0];
        let second = s
            .add_circle(
                CircleMode::CenterDiameter,
                v(center_distance, 0.0),
                v(center_distance + 5.0, 0.0),
            )
            .unwrap()
            .entities[0];

        s.add_constraint(Constraint::Tangent {
            a: first,
            b: second,
        })
        .unwrap_or_else(|error| panic!("distance {center_distance} failed: {error}"));

        let dto = s.dto();
        let (first_center, first_radius) = circle(&dto, first);
        let (second_center, second_radius) = circle(&dto, second);
        assert!((first_center.distance(second_center) - first_radius - second_radius).abs() < 1e-6);
        assert!(
            (first_radius - 5.0).abs() < 0.05 && (second_radius - 5.0).abs() < 0.05,
            "distance {center_distance}: radii changed to {first_radius}, {second_radius}"
        );
    }
}

#[test]
fn off_axis_unequal_circles_keep_external_tangency_after_projection_and_history() {
    let mut s = session();
    let support = s.add_line(v(-26., 12.), v(-2., 12.), true).unwrap();
    s.add_constraint(Constraint::Horizontal {
        entity: support.entity_id,
    })
    .unwrap();
    let first = s
        .add_circle(CircleMode::CenterDiameter, v(-10., 6.), v(-4., 6.))
        .unwrap()
        .entities[0];
    s.add_constraint(Constraint::Tangent {
        a: support.entity_id,
        b: first,
    })
    .unwrap();
    let center = v(15.809719827477455, -10.961402263097117);
    let second = s
        .add_circle(
            CircleMode::CenterDiameter,
            center,
            center + v(4.215921294620028, 0.),
        )
        .unwrap()
        .entities[0];
    let before = s.dto();
    let (first_center, first_radius) = circle(&before, first);
    let (_, second_radius) = circle(&before, second);
    s.add_constraint(Constraint::Tangent {
        a: first,
        b: second,
    })
    .unwrap();
    let applied = s.dto();
    let (a, ra) = circle(&applied, first);
    let (b, rb) = circle(&applied, second);
    assert!(close(a, first_center));
    assert!((ra - first_radius).abs() < 1e-9 && (rb - second_radius).abs() < 1e-9);
    assert!(
        (a.distance(b) - ra - rb).abs() < 1e-9,
        "disjoint circles must retain the external branch: {a:?}, {b:?}, {ra}, {rb}"
    );
    assert!(b.distance(center) < first_center.distance(center) - (ra - rb).abs());
    assert_eq!(s.undo().unwrap().sketch.entities, before.entities);
    assert_eq!(s.redo().unwrap().sketch.entities, applied.entities);
}

#[test]
fn fix_pins_geometry_and_blocks_conflicting_moves() {
    let mut s = session();
    let l = s.add_line(v(0.0, 0.0), v(50.0, 0.0), true).unwrap();
    s.toggle_fix(l.start_point_id).unwrap();

    let before = s.dto();
    let error = s
        .move_point(move_req(l.start_point_id, v(10.0, 10.0)))
        .unwrap_err();
    assert!(matches!(
        error,
        SessionError::InvalidConstraint(ref message)
            if message.contains("could not move this point")
                && message.contains("review the point's constraints")
    ));
    assert_eq!(s.dto(), before);
    let (start, _) = line(&s.dto(), l.entity_id);
    assert!(close(start, v(0.0, 0.0)), "fixed point must not move");

    s.toggle_fix(l.start_point_id).unwrap();
    let r = s
        .move_point(move_req(l.start_point_id, v(10.0, 10.0)))
        .unwrap();
    let (start, _) = line(&r.sketch, l.entity_id);
    assert!(close(start, v(10.0, 10.0)));
}

#[test]
fn perpendicular_conflicting_with_parallel_is_rejected_and_named() {
    let mut s = session();
    let l1 = s.add_line(v(0.0, 0.0), v(50.0, 0.0), true).unwrap();
    let l2 = s.add_line(v(0.0, 20.0), v(50.0, 20.0), true).unwrap();
    s.add_constraint(Constraint::Parallel {
        a: l1.entity_id,
        b: l2.entity_id,
    })
    .unwrap();

    let err = s
        .add_constraint(Constraint::Perpendicular {
            a: l1.entity_id,
            b: l2.entity_id,
        })
        .unwrap_err();
    let msg = err.to_string();
    match err {
        limo_cad_sketch::SessionError::OverConstrained {
            rejected,
            conflicts_with,
        } => {
            assert_eq!(rejected.kind, "perpendicular");
            assert!(
                conflicts_with.iter().any(|c| c.kind == "parallel"),
                "conflicts: {conflicts_with:?}"
            );
            assert!(msg.contains("conflicts with"), "{msg}");
        }
        other => panic!("expected OverConstrained, got {other:?}"),
    }

    let dto = s.dto();
    assert_eq!(dto.constraints.len(), 1);
    assert_eq!(dto.constraints[0].constraint.kind_str(), "parallel");
}

#[test]
fn duplicate_horizontal_is_rejected_without_polluting_the_graph() {
    let mut s = session();
    let l = s.add_line(v(0.0, 0.0), v(50.0, 0.0), true).unwrap();
    s.add_constraint(Constraint::Horizontal {
        entity: l.entity_id,
    })
    .unwrap();
    let r = s
        .add_constraint(Constraint::Horizontal {
            entity: l.entity_id,
        })
        .unwrap_err();
    assert!(
        r.to_string().contains("already exists"),
        "unexpected duplicate message: {r}"
    );
    assert_eq!(s.dto().constraints.len(), 1);
}

#[test]
fn narrow_axis_inference_does_not_flatten_a_deliberate_shallow_diagonal() {
    let mut s = session();
    let eight_degrees = 8.0_f64.to_radians();
    let diagonal = s
        .add_line(v(0.0, 0.0), v(100.0, eight_degrees.tan() * 100.0), false)
        .unwrap();
    let (_, diagonal_end) = line(&diagonal.sketch, diagonal.entity_id);
    assert!(
        diagonal_end.y > 10.0,
        "8° line was flattened: {diagonal_end:?}"
    );
    assert!(!diagonal
        .sketch
        .constraints
        .iter()
        .any(|constraint| matches!(
            constraint.constraint,
            Constraint::Horizontal { entity } if entity == diagonal.entity_id
        )));

    let two_degrees = 2.0_f64.to_radians();
    let inferred = s
        .add_line(
            v(0.0, 30.0),
            v(100.0, 30.0 + two_degrees.tan() * 100.0),
            false,
        )
        .unwrap();
    let (_, inferred_end) = line(&inferred.sketch, inferred.entity_id);
    assert!((inferred_end.y - 30.0).abs() < 1e-9);
    assert!(inferred
        .sketch
        .constraints
        .iter()
        .any(|constraint| matches!(
            constraint.constraint,
            Constraint::Horizontal { entity } if entity == inferred.entity_id
        )));
}

#[test]
fn conflicting_fix_is_rejected() {
    let mut s = session();
    let l1 = s.add_line(v(0.0, 0.0), v(50.0, 0.0), true).unwrap();
    let l2 = s.add_line(v(0.0, 20.0), v(30.0, 20.0), true).unwrap();
    for pid in [
        l1.start_point_id,
        l1.end_point_id,
        l2.start_point_id,
        l2.end_point_id,
    ] {
        s.toggle_fix(pid).unwrap();
    }
    assert_eq!(s.dto().dof.value, 0);
    let err = s
        .add_constraint(Constraint::Equal {
            a: l1.entity_id,
            b: l2.entity_id,
        })
        .unwrap_err();
    assert!(
        matches!(err, limo_cad_sketch::SessionError::OverConstrained { .. }),
        "got {err:?}"
    );
}

#[test]
fn locked_length_and_angle_produce_an_exact_point() {
    let mut s = session();
    let r = s
        .add_line_locked(&locked_seg(
            v(0.0, 0.0),
            v(99.0, 99.0),
            Some(50.0),
            Some(30.0),
        ))
        .unwrap();
    let (_, end) = line(&r.sketch, r.entity_id);
    let expect = v(50.0 * 30f64.cos().to_radians().cos(), 0.0);
    let _ = expect;
    let want = v(
        50.0 * (30.0_f64.to_radians()).cos(),
        50.0 * (30.0_f64.to_radians()).sin(),
    );
    assert!(close(end, want), "end={end:?} want={want:?}");
}

#[test]
fn locked_length_only_projects_onto_the_circle() {
    let mut s = session();

    let r = s
        .add_line_locked(&locked_seg(v(0.0, 0.0), v(10.0, 40.0), Some(50.0), None))
        .unwrap();
    let (_, end) = line(&r.sketch, r.entity_id);
    assert!((end.length() - 50.0).abs() < 1e-7);

    let d = v(10.0, 40.0);
    assert!((end.x * d.y - end.y * d.x).abs() < 1e-7);
}

#[test]
fn locked_angle_only_projects_onto_the_ray() {
    let mut s = session();
    let r = s
        .add_line_locked(&locked_seg(v(0.0, 0.0), v(40.0, 33.0), None, Some(90.0)))
        .unwrap();
    let (_, end) = line(&r.sketch, r.entity_id);
    assert!(close(end, v(0.0, 33.0)));
}

#[test]
fn locked_endpoint_overrides_grid_snap_and_hv_inference() {
    let mut s = SketchSession::new("Sketch1", XY, XY.basis().unwrap(), true);
    let r = s
        .add_line_locked(&locked_seg(
            v(0.0, 0.0),
            v(40.0, 5.0),
            Some(37.5),
            Some(10.0),
        ))
        .unwrap();
    let (_, end) = line(&r.sketch, r.entity_id);
    let want = v(
        37.5 * (10.0_f64.to_radians()).cos(),
        37.5 * (10.0_f64.to_radians()).sin(),
    );
    assert!(close(end, want), "locks must beat grid snap");

    let kinds: Vec<_> = r
        .sketch
        .constraints
        .iter()
        .map(|c| c.constraint.kind_str())
        .collect();
    assert_eq!(kinds.iter().filter(|k| **k == "distance").count(), 1);
    assert_eq!(kinds.iter().filter(|k| **k == "angle").count(), 1);
    assert!(!kinds.iter().any(|k| *k == "horizontal" || *k == "vertical"));
}

/// Rectangle with H/V reshapes when a corner is dragged; adjacent corners
/// follow so all four constraints keep holding.
#[test]
fn dragging_a_rectangle_corner_keeps_it_rectangle_shaped() {
    let mut s = session();
    let rect = s
        .add_rectangle(RectangleMode::TwoPoint, v(0.0, 0.0), v(40.0, 20.0))
        .unwrap();
    let lines: Vec<_> = rect
        .sketch
        .entities
        .iter()
        .filter_map(|e| match e {
            EntityDto::Line { id, .. } => Some(*id),
            _ => None,
        })
        .collect();

    let corner = rect.entities[2];
    let r = s.move_point(move_req(corner, v(55.0, 35.0))).unwrap();
    let dto = r.sketch;
    for id in lines {
        let (a, b) = line(&dto, id);
        let axis_aligned = (a.x - b.x).abs() < 1e-7 || (a.y - b.y).abs() < 1e-7;
        assert!(
            axis_aligned,
            "line {id:?} must stay axis-aligned: {a:?}-{b:?}"
        );
    }

    let c = dto.entities.iter().find(|e| e.id() == corner).unwrap();
    match c {
        EntityDto::Point { position, .. } => assert!(close(*position, v(55.0, 35.0))),
        _ => panic!("expected point"),
    }
}

/// Coincident chains stay connected through a solver drag.
#[test]
fn dragging_a_chain_joint_keeps_both_lines_connected() {
    let mut s = session();
    let l1 = s.add_line(v(0.0, 0.0), v(30.0, 0.0), true).unwrap();
    let l2 = s.add_line(v(30.0, 0.0), v(30.0, 30.0), true).unwrap();
    s.add_constraint(Constraint::Horizontal {
        entity: l1.entity_id,
    })
    .unwrap();
    let joint = l1.end_point_id;
    let r = s.move_point(move_req(joint, v(45.0, 25.0))).unwrap();
    let (a, b) = line(&r.sketch, l1.entity_id);
    let (c, d) = line(&r.sketch, l2.entity_id);
    assert!(close(b, c), "shared joint must stay shared");
    assert!(close(b, v(45.0, 25.0)));
    assert!((a.y - b.y).abs() < 1e-7, "H must hold on l1");
    let _ = d;
}

/// WASM smoke sequence: chained H then inferred right-angle. #47 prefers a
/// relational Perpendicular over world-axis Vertical; the shared corner must
/// still follow the remaining free axis. A raw mm2 Perp residual used to
/// stall the drag so move_point reverted.
#[test]
fn dragging_a_chained_right_angle_follows_the_free_axis() {
    let mut s = SketchSession::new("Sketch1", XY, XY.basis().unwrap(), true);
    let l1 = s.add_line(v(0.0, 0.0), v(50.0, 1.0), false).unwrap();
    let l2 = s.add_line(v(50.0, 0.0), v(51.0, 50.0), false).unwrap();
    assert!(
        l2.created_constraints.iter().any(|c| matches!(
            c.constraint,
            Constraint::Perpendicular { a, b }
                if (a == l1.entity_id && b == l2.entity_id)
                    || (a == l2.entity_id && b == l1.entity_id)
        )),
        "chained right angle should persist as Perpendicular: {:?}",
        l2.created_constraints
    );
    let _l3 = s.add_line(v(50.0, 50.0), v(0.5, 0.4), false).unwrap();
    let dragged = s
        .move_point(move_req(l2.start_point_id, v(80.0, 0.0)))
        .unwrap();
    let (s1, e1) = line(&dragged.sketch, l1.entity_id);
    let (s2, e2) = line(&dragged.sketch, l2.entity_id);
    assert!((s1.y - e1.y).abs() < 1e-9, "H must hold");
    assert!((s2.x - e2.x).abs() < 1e-6, "right angle must stay vertical");
    assert!(
        s1.x.abs() < 1e-6 && s1.y.abs() < 1e-6,
        "origin must stay fixed"
    );
    assert!(
        close(e1, v(80.0, 0.0)),
        "corner should follow the free axis, got {e1:?}"
    );
}

#[test]
fn rectangle_creates_four_hv_constrained_lines_in_one_undo_step() {
    let mut s = session();
    let r = s
        .add_rectangle(RectangleMode::TwoPoint, v(10.0, 10.0), v(50.0, 30.0))
        .unwrap();
    let kinds: Vec<_> = r
        .sketch
        .constraints
        .iter()
        .map(|c| c.constraint.kind_str())
        .collect();
    assert_eq!(kinds.iter().filter(|k| **k == "horizontal").count(), 2);
    assert_eq!(kinds.iter().filter(|k| **k == "vertical").count(), 2);
    assert_eq!(r.sketch.entities.len(), 8);
    let undone = s.undo().unwrap();
    assert_eq!(undone.sketch.entities.len(), 0);
}

#[test]
fn center_rectangle_uses_half_extents() {
    let mut s = session();
    let r = s
        .add_rectangle(RectangleMode::Center, v(50.0, 50.0), v(60.0, 60.0))
        .unwrap();
    let xs: Vec<f64> = r
        .sketch
        .entities
        .iter()
        .filter_map(|e| match e {
            EntityDto::Point { position, .. } => Some(position.x),
            _ => None,
        })
        .collect();
    assert!(xs.contains(&40.0) && xs.contains(&60.0));
}

#[test]
fn circle_modes_and_locked_diameter() {
    let mut s = session();
    let r = s
        .add_circle_locked(&LockedCircleRequest {
            mode: CircleMode::CenterDiameter,
            anchor: v(20.0, 20.0),
            diameter_mm: Some(30.0),
            diameter_text: None,
            edge_hint: v(99.0, 99.0),
            ctrl_held: false,
        })
        .unwrap();
    match r
        .sketch
        .entities
        .iter()
        .find(|e| e.id() == r.entities[0])
        .unwrap()
    {
        EntityDto::Circle { center, radius, .. } => {
            assert!(close(*center, v(20.0, 20.0)));
            assert!((radius - 15.0).abs() < 1e-9);
        }
        _ => panic!("expected circle"),
    }

    let r2 = s
        .add_circle(CircleMode::TwoPoint, v(0.0, 0.0), v(40.0, 0.0))
        .unwrap();
    match r2
        .sketch
        .entities
        .iter()
        .find(|e| e.id() == r2.entities[0])
        .unwrap()
    {
        EntityDto::Circle { center, radius, .. } => {
            assert!(close(*center, v(20.0, 0.0)));
            assert!((radius - 20.0).abs() < 1e-9);
        }
        _ => panic!("expected circle"),
    }
}

#[test]
fn three_point_arc_passes_through_all_three_points() {
    let mut s = session();
    let r = s
        .add_arc_3pt(v(20.0, 0.0), v(0.0, 20.0), v(-20.0, 0.0))
        .unwrap();
    match r
        .sketch
        .entities
        .iter()
        .find(|e| e.id() == r.entities[0])
        .unwrap()
    {
        EntityDto::Arc { center, radius, .. } => {
            assert!(close(*center, v(0.0, 0.0)), "center={center:?}");
            assert!((radius - 20.0).abs() < 1e-7);
        }
        _ => panic!("expected arc"),
    }
}

#[test]
fn midpoint_line_mirrors_endpoints_and_adds_midpoint_constraint() {
    let mut s = session();
    let r = s
        .add_line_midpoint(v(50.0, 50.0), v(80.0, 70.0), false)
        .unwrap();
    let line_id = r.entities[3];
    let (a, b) = line(&r.sketch, line_id);
    let mid = v((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    assert!(close(mid, v(50.0, 50.0)));
    assert!(r
        .sketch
        .constraints
        .iter()
        .any(|c| c.constraint.kind_str() == "midpoint"));
}

#[test]
fn midpoint_line_reuses_snapped_midpoint_and_keeps_axis_inference() {
    let mut s = session();
    s.add_line(v(0.0, 0.0), v(60.0, 0.0), true).unwrap();
    let midpoint = s.add_point(v(30.0, 0.0)).unwrap().entities[0];
    let before_points = s
        .dto()
        .entities
        .iter()
        .filter(|entity| matches!(entity, EntityDto::Point { .. }))
        .count();
    let result = s
        .add_line_midpoint(v(30.7, 0.5), v(50.0, 0.6), false)
        .unwrap();
    assert_eq!(
        result.entities[0], midpoint,
        "midpoint snap must reuse the point"
    );
    assert_eq!(
        result
            .sketch
            .entities
            .iter()
            .filter(|entity| matches!(entity, EntityDto::Point { .. }))
            .count(),
        before_points + 2,
        "only the two mirrored endpoints should be new"
    );
    let line_id = result.entities[3];
    let (a, b) = line(&result.sketch, line_id);
    assert!((a.y - b.y).abs() < 1e-9);
    assert!(result.sketch.constraints.iter().any(|constraint| {
        matches!(
            constraint.constraint,
            Constraint::Horizontal { entity } if entity == line_id
        )
    }));
}

#[test]
fn unlocked_circle_edge_and_center_arc_sweep_snap_to_existing_points() {
    let mut s = session();
    s.add_point(v(12.0, 0.0)).unwrap();
    let circle = s
        .add_circle_locked(&LockedCircleRequest {
            mode: CircleMode::CenterDiameter,
            anchor: v(0.0, 0.0),
            diameter_mm: None,
            diameter_text: None,
            edge_hint: v(11.2, 0.5),
            ctrl_held: false,
        })
        .unwrap();
    match circle
        .sketch
        .entities
        .iter()
        .find(|entity| entity.id() == circle.entities[0])
        .unwrap()
    {
        EntityDto::Circle { radius, .. } => assert!((*radius - 12.0).abs() < 1e-9),
        other => panic!("expected circle, got {other:?}"),
    }

    s.add_point(v(0.0, 10.0)).unwrap();
    let arc = s
        .add_arc_center(v(0.0, 0.0), v(10.0, 0.0), v(0.7, 9.2))
        .unwrap();
    match arc
        .sketch
        .entities
        .iter()
        .find(|entity| entity.id() == arc.entities[0])
        .unwrap()
    {
        EntityDto::Arc { end_angle, .. } => {
            assert!((*end_angle - std::f64::consts::FRAC_PI_2).abs() < 1e-9)
        }
        other => panic!("expected arc, got {other:?}"),
    }
}

#[test]
fn bulk_constraints_and_fix_toggle_are_atomic_single_undo_actions() {
    let mut s = session();
    let first = s.add_line(v(0.0, 0.0), v(20.0, 0.5), true).unwrap();
    let second = s.add_line(v(30.0, 0.0), v(30.5, 20.0), true).unwrap();
    let before_batch = s.dto();
    let applied = s
        .add_constraints(vec![
            Constraint::Horizontal {
                entity: first.entity_id,
            },
            Constraint::Vertical {
                entity: second.entity_id,
            },
        ])
        .unwrap();
    assert_eq!(
        applied
            .sketch
            .constraints
            .iter()
            .filter(|constraint| matches!(
                constraint.constraint,
                Constraint::Horizontal { .. } | Constraint::Vertical { .. }
            ))
            .count(),
        2
    );
    let undone = s.undo().unwrap().sketch;
    assert_eq!(undone.entities, before_batch.entities);
    assert_eq!(undone.constraints, before_batch.constraints);

    let before_error = s.dto();
    s.add_constraints(vec![
        Constraint::Horizontal {
            entity: first.entity_id,
        },
        Constraint::Horizontal {
            entity: EntityId(9_999),
        },
    ])
    .unwrap_err();
    assert_eq!(
        s.dto(),
        before_error,
        "invalid late item must roll back the batch"
    );

    let fixed = s
        .toggle_fix_entities(vec![first.start_point_id, first.end_point_id])
        .unwrap();
    assert_eq!(
        fixed
            .sketch
            .constraints
            .iter()
            .filter(|constraint| matches!(constraint.constraint, Constraint::Fix { .. }))
            .count(),
        2
    );
    let undone = s.undo().unwrap().sketch;
    assert_eq!(undone.entities, before_error.entities);
    assert_eq!(undone.constraints, before_error.constraints);
}

#[test]
fn equivalent_carrier_relations_are_rejected_inside_one_batch() {
    let mut s = session();
    let line = s.add_line(v(0.0, 0.0), v(20.0, 1.0), true).unwrap();
    let error = s
        .add_constraints(vec![
            Constraint::Horizontal {
                entity: line.entity_id,
            },
            Constraint::HorizontalPoints {
                a: line.start_point_id,
                b: line.end_point_id,
            },
        ])
        .unwrap_err();
    assert!(error.to_string().contains("already exists"));
    assert!(s.dto().constraints.is_empty());
}

#[test]
fn carrier_endpoint_contradiction_names_the_actual_constraint() {
    let mut s = session();
    let line = s.add_line(v(0.0, 0.0), v(20.0, 1.0), true).unwrap();
    s.add_constraint(Constraint::Horizontal {
        entity: line.entity_id,
    })
    .unwrap();
    let error = s
        .add_constraint(Constraint::VerticalPoints {
            a: line.start_point_id,
            b: line.end_point_id,
        })
        .unwrap_err();
    let message = error.to_string();
    assert!(message.contains("conflicts with horizontal"), "{message}");
    assert_eq!(s.dto().constraints.len(), 1);
}

#[test]
fn undo_restores_pre_constraint_state_including_solver_motion() {
    let mut s = session();
    let l1 = s.add_line(v(0.0, 0.0), v(50.0, 0.0), true).unwrap();
    let l2 = s.add_line(v(0.0, 20.0), v(50.0, 25.0), true).unwrap();
    s.add_constraint(Constraint::Parallel {
        a: l1.entity_id,
        b: l2.entity_id,
    })
    .unwrap();
    let solved = s.dto();

    let (a0, a1) = line(&solved, l1.entity_id);
    let (b0, b1) = line(&solved, l2.entity_id);
    let da = a1 - a0;
    let db = b1 - b0;
    assert!((da.x * db.y - da.y * db.x).abs() < 1e-7, "must be parallel");

    let undone = s.undo().unwrap();
    let (_, b1u) = line(&undone.sketch, l2.entity_id);
    assert!(
        close(b1u, v(50.0, 25.0)),
        "undo restores pre-solve geometry"
    );
    assert_eq!(undone.sketch.constraints.len(), 0);
}

#[test]
fn typed_angle_snaps_its_free_distance_to_the_active_grid() {
    let mut s = SketchSession::new("Sketch1", XY, XY.basis().unwrap(), true);
    s.set_grid_step(5.0).unwrap();
    let preview = s.preview_segment_locked(
        v(5.0, 10.0),
        (None, Some(-45.0)),
        v(15.16, -0.16),
        false,
        (None, None, None, None),
    );
    assert_eq!(preview.snap, SnapTarget::Grid);
    assert!(close(preview.snapped_to, v(15.0, 0.0)));
    let delta = preview.snapped_to - v(5.0, 10.0);
    assert!((delta.y / delta.x + 1.0).abs() < 1e-9);
}

#[test]
fn line_tracking_is_exact_but_remains_a_temporary_placement_aid() {
    let mut s = session();
    let reference = s.add_point(v(0.0, 5.0)).unwrap().entities[0];
    let result = s
        .add_line_locked(&LockedSegmentRequest {
            from: v(5.0, 15.0),
            to_hint: v(15.1, 4.9),
            from_crossing: None,
            to_crossing: None,
            length_mm: None,
            angle_deg: Some(-45.0),
            length_text: None,
            angle_text: None,
            ctrl_held: false,
            tracking: Some(LineTrackingRequest {
                point: reference,
                axis: TrackingAxis::Horizontal,
            }),
            intersection: None,
        })
        .unwrap();
    let (_, endpoint) = line(&result.sketch, result.entity_id);
    assert!(close(endpoint, v(15.0, 5.0)));
    assert!(!result.sketch.constraints.iter().any(|constraint| matches!(
        constraint.constraint,
        Constraint::HorizontalPoints { a, b }
            if a == reference && b == result.end_point_id
    )));

    let moved = s
        .move_point(move_req(reference, v(0.0, 7.0)))
        .unwrap()
        .sketch;
    let (_, moved_endpoint) = line(&moved, result.entity_id);
    assert!((moved_endpoint.y - 5.0).abs() < 1e-7);
}

#[test]
fn vertical_curve_intersection_beats_grid_and_persists_on_the_carrier() {
    let mut s = session();
    let diagonal = s.add_line(v(17.0, 8.0), v(25.0, 0.0), true).unwrap();
    s.set_grid_step(2.5).unwrap();
    s.set_grid_snap(true);
    let short_bottom = s
        .add_line_locked(&LockedSegmentRequest {
            from: v(25.0, 0.0),
            to_hint: v(24.5, 0.0),
            from_crossing: None,
            to_crossing: None,
            length_mm: None,
            angle_deg: None,
            length_text: Some("0.5".to_string()),
            angle_text: Some("180".to_string()),
            ctrl_held: false,
            tracking: None,
            intersection: None,
        })
        .unwrap();
    let (_, anchor) = line(&short_bottom.sketch, short_bottom.entity_id);
    assert!(close(anchor, v(24.5, 0.0)));

    let request = LineIntersectionRequest {
        curve: diagonal.entity_id,
        axis: TrackingAxis::Vertical,
    };
    let preview = s.preview_segment_locked(
        v(24.5, 0.0),
        (None, None),
        v(24.48, 0.54),
        false,
        (None, Some(request), None, None),
    );
    assert_eq!(
        preview.snap,
        SnapTarget::Curve {
            entity: diagonal.entity_id
        }
    );
    assert!(close(preview.snapped_to, v(24.5, 0.5)));
    assert!(preview
        .inferences
        .contains(&limo_cad_sketch::Inference::Vertical));
    assert!(preview
        .inferences
        .contains(&limo_cad_sketch::Inference::Coincident));

    let result = s
        .add_line_locked(&LockedSegmentRequest {
            from: v(24.5, 0.0),
            to_hint: v(24.48, 0.54),
            from_crossing: None,
            to_crossing: None,
            length_mm: None,
            angle_deg: None,
            length_text: None,
            angle_text: None,
            ctrl_held: false,
            tracking: None,
            intersection: Some(request),
        })
        .unwrap();
    let (start, end) = line(&result.sketch, result.entity_id);
    assert!(close(start, v(24.5, 0.0)), "start={start:?}, end={end:?}");
    assert!(close(end, v(24.5, 0.5)), "start={start:?}, end={end:?}");
    assert!((end.x - start.x).abs() < 1e-7);
    assert!(result.sketch.constraints.iter().any(|constraint| matches!(
        constraint.constraint,
        Constraint::Vertical { entity } if entity == result.entity_id
    ) || matches!(
        constraint.constraint,
        Constraint::Perpendicular { a, b }
            if (a == result.entity_id && b == short_bottom.entity_id)
                || (b == result.entity_id && a == short_bottom.entity_id)
    )));
    assert!(result.sketch.constraints.iter().any(|constraint| matches!(
        constraint.constraint,
        Constraint::Coincident { a, b }
            if a == result.end_point_id && b == diagonal.entity_id
    )));
}

#[test]
fn exact_crossing_start_survives_a_half_mm_chain_and_vertical_turn() {
    let mut s = session();
    let horizontal = s.add_line(v(0.0, 0.0), v(30.0, 0.0), true).unwrap();
    let diagonal = s.add_line(v(15.0, -5.0), v(25.0, 5.0), true).unwrap();
    let crossing = CurveCrossingRequest {
        first: horizontal.entity_id,
        second: diagonal.entity_id,
    };
    s.set_grid_step(10.0).unwrap();
    s.set_grid_snap(true);

    let short = s
        .add_line_locked(&LockedSegmentRequest {
            from: v(20.17, -0.13),
            to_hint: v(19.4, 0.2),
            from_crossing: Some(crossing),
            to_crossing: None,
            length_mm: None,
            angle_deg: None,
            length_text: Some("0.5".to_string()),
            angle_text: Some("180".to_string()),
            ctrl_held: false,
            tracking: None,
            intersection: None,
        })
        .unwrap();
    let (start, end) = line(&short.sketch, short.entity_id);
    assert!(close(start, v(20.0, 0.0)), "start={start:?}");
    assert!(close(end, v(19.5, 0.0)), "end={end:?}");
    assert!((start.distance(end) - 0.5).abs() < 1e-9);

    for carrier in [horizontal.entity_id, diagonal.entity_id] {
        assert!(
            short.sketch.constraints.iter().any(|constraint| matches!(
                constraint.constraint,
                Constraint::Coincident { a, b }
                    if a == short.start_point_id && b == carrier
            )),
            "crossing point should remain attached to {carrier:?}"
        );
    }

    let vertical_preview = s.preview_segment_locked(
        end,
        (None, None),
        v(19.53, 4.7),
        false,
        (None, None, None, None),
    );
    assert!(vertical_preview
        .inferences
        .contains(&limo_cad_sketch::Inference::Vertical));
    assert!((vertical_preview.snapped_to.x - 19.5).abs() < 1e-9);

    let upright = s
        .add_line_locked(&LockedSegmentRequest {
            from: end,
            to_hint: v(19.53, 4.7),
            from_crossing: None,
            to_crossing: None,
            length_mm: None,
            angle_deg: None,
            length_text: None,
            angle_text: None,
            ctrl_held: false,
            tracking: None,
            intersection: None,
        })
        .unwrap();
    let (turn_start, turn_end) = line(&upright.sketch, upright.entity_id);
    assert!(close(turn_start, end));
    assert!((turn_end.x - turn_start.x).abs() < 1e-9);
    assert!(upright.sketch.constraints.iter().any(|constraint| matches!(
        constraint.constraint,
        Constraint::Perpendicular { a, b }
            if (a == short.entity_id && b == upright.entity_id)
                || (a == upright.entity_id && b == short.entity_id)
    )));
}
