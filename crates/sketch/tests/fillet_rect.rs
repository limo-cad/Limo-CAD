//! Reproduction tests for the 2026-07-19 owner bug reports on rectangle
//! corner modify ops: fillet trim re-opened by driving dims (AM round), and
//! "second corner op rejected" (PM round).

use limo_cad_sketch::{
    ChamferRequest, EditDimensionRequest, EntityDto, FilletRequest, LockedRectangleRequest,
    OriginPlane, PlaneRef, RectangleMode, SketchSession, Vec2,
};

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

const XY: PlaneRef = PlaneRef::OriginPlane {
    plane: OriginPlane::Xy,
};

fn session() -> SketchSession {
    SketchSession::new("Sketch1", XY, XY.basis().unwrap(), false)
}

fn lines_of(dto: &limo_cad_sketch::SketchDto) -> Vec<(limo_cad_sketch::EntityId, Vec2, Vec2)> {
    dto.entities
        .iter()
        .filter_map(|e| match e {
            EntityDto::Line { id, start, end, .. } => Some((*id, *start, *end)),
            _ => None,
        })
        .collect()
}

fn line_is_consumed(dto: &limo_cad_sketch::SketchDto, id: limo_cad_sketch::EntityId) -> bool {
    dto.entities
        .iter()
        .find_map(|entity| match entity {
            EntityDto::Line {
                id: line_id,
                consumed,
                ..
            } if *line_id == id => Some(*consumed),
            _ => None,
        })
        .unwrap_or(false)
}

fn close(a: Vec2, b: Vec2) -> bool {
    a.distance(b) < 1e-7
}

#[test]
fn rectangle_corner_fillet_trims_both_edges() {
    let mut s = session();

    s.add_rectangle(RectangleMode::TwoPoint, v(0.0, 0.0), v(40.0, 30.0))
        .unwrap();
    let dto = s.dto();
    let lines = lines_of(&dto);
    assert_eq!(lines.len(), 4);

    let top = lines
        .iter()
        .find(|(_, a, b)| (a.y - 30.0).abs() < 1e-9 && (b.y - 30.0).abs() < 1e-9)
        .unwrap()
        .0;
    let left = lines
        .iter()
        .find(|(_, a, b)| (a.x - 0.0).abs() < 1e-9 && (b.x - 0.0).abs() < 1e-9)
        .unwrap()
        .0;

    s.fillet_lines(&FilletRequest {
        l1: top,
        l2: left,
        radius_text: "4".to_string(),
    })
    .unwrap();
    let dto = s.dto();
    let lines = lines_of(&dto);
    let (ta, tb) = {
        let l = lines.iter().find(|(id, _, _)| *id == top).unwrap();
        (l.1, l.2)
    };
    let (la, lb) = {
        let l = lines.iter().find(|(id, _, _)| *id == left).unwrap();
        (l.1, l.2)
    };

    let top_ok = (close(ta, v(4.0, 30.0)) && close(tb, v(40.0, 30.0)))
        || (close(tb, v(4.0, 30.0)) && close(ta, v(40.0, 30.0)));
    assert!(top_ok, "top edge trimmed at tangent: {ta:?} {tb:?}");

    let left_ok = (close(la, v(0.0, 26.0)) && close(lb, v(0.0, 0.0)))
        || (close(lb, v(0.0, 26.0)) && close(la, v(0.0, 0.0)));
    assert!(left_ok, "left edge trimmed at tangent: {la:?} {lb:?}");
}

/// The owner's actual scenario (2026-07-19 AM bug report): the rectangle was
/// drawn with TYPED width/height, so the bottom and left edges carry DRIVING
/// distance dims. Filleting the top-left corner must not let the height dim
/// drag the trimmed endpoint back up to the old corner.
#[test]
fn dimensioned_rectangle_corner_fillet_keeps_trim() {
    let mut s = session();
    s.add_rectangle_locked(&LockedRectangleRequest {
        mode: RectangleMode::TwoPoint,
        anchor: v(0.0, 0.0),
        width_mm: None,
        height_mm: None,
        width_text: Some("40".to_string()),
        height_text: Some("15".to_string()),
        corner_hint: v(40.0, 15.0),
        ctrl_held: true,
    })
    .unwrap();
    let dto = s.dto();
    assert_eq!(dto.dimensions.len(), 2, "auto dims on bottom + left");
    let lines = lines_of(&dto);
    let top = lines
        .iter()
        .find(|(_, a, b)| (a.y - 15.0).abs() < 1e-9 && (b.y - 15.0).abs() < 1e-9)
        .unwrap()
        .0;
    let left = lines
        .iter()
        .find(|(_, a, b)| (a.x - 0.0).abs() < 1e-9 && (b.x - 0.0).abs() < 1e-9)
        .unwrap()
        .0;

    s.fillet_lines(&FilletRequest {
        l1: top,
        l2: left,
        radius_text: "4".to_string(),
    })
    .unwrap();
    let dto = s.dto();
    let lines = lines_of(&dto);
    let (la, lb) = {
        let l = lines.iter().find(|(id, _, _)| *id == left).unwrap();
        (l.1, l.2)
    };
    println!("left edge after fillet: {la:?} -> {lb:?}");

    let top_y = {
        let l = lines.iter().find(|(id, _, _)| *id == top).unwrap();
        l.1.y
    };
    let trim_y = la.y.max(lb.y);
    assert!(
        (top_y - trim_y - 4.0).abs() < 1e-3,
        "trim must sit R4 below the top edge (top={top_y}, trim={trim_y})"
    );

    let pts: Vec<Vec2> = dto
        .entities
        .iter()
        .filter_map(|e| match e {
            EntityDto::Point { position, .. } => Some(*position),
            _ => None,
        })
        .collect();
    let corner_dist = |a: Vec2, b: Vec2| {
        let pa = pts.iter().find(|p| p.distance(a) < 1e-3);
        let pb = pts.iter().find(|p| p.distance(b) < 1e-3);
        pa.zip(pb).map(|(a, b)| a.distance(*b))
    };
    let h = corner_dist(v(0.0, 0.0), v(0.0, 15.0)).expect("corner points persist");
    assert!(
        (h - 15.0).abs() < 1e-3,
        "height dim stays 15 across corners, got {h}"
    );
}

fn typed_square(s: &mut SketchSession) -> Vec<(limo_cad_sketch::EntityId, Vec2, Vec2)> {
    s.add_rectangle_locked(&LockedRectangleRequest {
        mode: RectangleMode::TwoPoint,
        anchor: v(0.0, 0.0),
        width_mm: None,
        height_mm: None,
        width_text: Some("20".to_string()),
        height_text: Some("20".to_string()),
        corner_hint: v(20.0, 20.0),
        ctrl_held: true,
    })
    .unwrap();
    lines_of(&s.dto())
}

fn typed_30_square(s: &mut SketchSession) -> Vec<(limo_cad_sketch::EntityId, Vec2, Vec2)> {
    s.add_rectangle_locked(&LockedRectangleRequest {
        mode: RectangleMode::TwoPoint,
        anchor: v(0.0, 0.0),
        width_mm: None,
        height_mm: None,
        width_text: Some("30".to_string()),
        height_text: Some("30".to_string()),
        corner_hint: v(30.0, 30.0),
        ctrl_held: true,
    })
    .unwrap();
    lines_of(&s.dto())
}

fn typed_40_square(s: &mut SketchSession) -> Vec<(limo_cad_sketch::EntityId, Vec2, Vec2)> {
    s.add_rectangle_locked(&LockedRectangleRequest {
        mode: RectangleMode::TwoPoint,
        anchor: v(0.0, 0.0),
        width_mm: None,
        height_mm: None,
        width_text: Some("40".to_string()),
        height_text: Some("40".to_string()),
        corner_hint: v(40.0, 40.0),
        ctrl_held: true,
    })
    .unwrap();
    lines_of(&s.dto())
}

fn edge(
    lines: &[(limo_cad_sketch::EntityId, Vec2, Vec2)],
    axis: char,
    val: f64,
) -> limo_cad_sketch::EntityId {
    lines
        .iter()
        .find(|(_, a, b)| {
            if axis == 'x' {
                (a.x - val).abs() < 1e-6 && (b.x - val).abs() < 1e-6
            } else {
                (a.y - val).abs() < 1e-6 && (b.y - val).abs() < 1e-6
            }
        })
        .unwrap_or_else(|| panic!("edge {axis}={val} exists"))
        .0
}

#[test]
fn second_fillet_on_dimensioned_rect_is_accepted() {
    let mut s = session();
    let lines = typed_square(&mut s);
    let (bottom, left, top, right) = (
        edge(&lines, 'y', 0.0),
        edge(&lines, 'x', 0.0),
        edge(&lines, 'y', 20.0),
        edge(&lines, 'x', 20.0),
    );

    s.fillet_lines(&FilletRequest {
        l1: bottom,
        l2: left,
        radius_text: "4".to_string(),
    })
    .expect("first fillet works");

    let r = s.fillet_lines(&FilletRequest {
        l1: top,
        l2: left,
        radius_text: "4".to_string(),
    });
    assert!(r.is_ok(), "second fillet must be accepted: {r:?}");

    let r = s.fillet_lines(&FilletRequest {
        l1: top,
        l2: right,
        radius_text: "4".to_string(),
    });
    assert!(r.is_ok(), "third fillet: {r:?}");
    let r = s.fillet_lines(&FilletRequest {
        l1: bottom,
        l2: right,
        radius_text: "4".to_string(),
    });
    assert!(r.is_ok(), "fourth fillet: {r:?}");
    let arcs = s
        .dto()
        .entities
        .iter()
        .filter(|e| matches!(e, EntityDto::Arc { .. }))
        .count();
    assert_eq!(arcs, 4, "all four corners filleted");
}

/// Regression from the owner's 2026-08-13 live sketch. A short horizontal
/// setup line overlapped the lower edge of a closed outline. Picking that
/// visible overlap and the right wall produced the correct R10 preview, but
/// commit anchored the setup line's nearest endpoint to the remote wall and
/// rejected the radius as `coincident(Point1, Line8)`.
///
/// The modify tool must resolve the collinear carrier that actually owns the
/// wall corner instead of inventing a persistent corner incidence between
/// geometrically remote entities.
#[test]
fn fillet_resolves_overlapping_line_to_the_adjacent_corner_carrier() {
    let mut s = session();
    let setup = s
        .add_line(v(0.0, 0.0), v(-10.0, 0.0), false)
        .expect("overlapping setup line");

    let bottom = s
        .add_line(v(-10.0, 0.0), v(10.0, 0.0), false)
        .unwrap()
        .entity_id;
    let right = s
        .add_line(v(10.0, 0.0), v(10.0, 60.0), false)
        .unwrap()
        .entity_id;
    let top = s
        .add_line(v(10.0, 60.0), v(-10.0, 60.0), false)
        .unwrap()
        .entity_id;
    let left = s
        .add_line(v(-10.0, 60.0), v(-10.0, 0.0), false)
        .unwrap()
        .entity_id;

    s.fillet_lines(&FilletRequest {
        l1: top,
        l2: left,
        radius_text: "10".to_string(),
    })
    .expect("top-left R10");
    s.fillet_lines(&FilletRequest {
        l1: top,
        l2: right,
        radius_text: "10".to_string(),
    })
    .expect("top-right R10");

    let result = s.fillet_lines(&FilletRequest {
        l1: setup.entity_id,
        l2: right,
        radius_text: "10".to_string(),
    });
    assert!(
        result.is_ok(),
        "overlap pick must resolve to the adjacent bottom carrier: {result:?}"
    );

    let dto = s.dto();
    let (_, bottom_a, bottom_b) = lines_of(&dto)
        .into_iter()
        .find(|(id, _, _)| *id == bottom)
        .expect("bottom carrier remains addressable");
    assert!(
        bottom_a.distance(v(0.0, 0.0)) < 1e-4 || bottom_b.distance(v(0.0, 0.0)) < 1e-4,
        "R10 trims the 20 mm bottom carrier to its midpoint: {bottom_a:?} -> {bottom_b:?}"
    );
    let (_, right_a, right_b) = lines_of(&dto)
        .into_iter()
        .find(|(id, _, _)| *id == right)
        .expect("right carrier remains addressable");
    assert!(
        right_a.distance(v(10.0, 10.0)) < 1e-4 || right_b.distance(v(10.0, 10.0)) < 1e-4,
        "R10 trims the right wall to y=10: {right_a:?} -> {right_b:?}"
    );
}

/// Same live sketch as the overlap regression above, but selecting the
/// corner actually owned by the short overlapping segment.  At R10 the
/// fillet consumes that 10 mm segment exactly.  This is a valid topology
/// boundary (the trimmed carrier remains as a zero-span parametric entity),
/// not an accidental solver collapse.
#[test]
fn one_fillet_may_exactly_consume_a_short_overlapping_carrier() {
    let mut s = session();
    let setup = s
        .add_line(v(0.0, 0.0), v(-10.0, 0.0), false)
        .expect("overlapping setup line");

    s.add_line(v(-10.0, 0.0), v(10.0, 0.0), false)
        .expect("bottom outline");
    let right = s
        .add_line(v(10.0, 0.0), v(10.0, 60.0), false)
        .expect("right outline")
        .entity_id;
    let top = s
        .add_line(v(10.0, 60.0), v(-10.0, 60.0), false)
        .unwrap()
        .entity_id;
    let left = s
        .add_line(v(-10.0, 60.0), v(-10.0, 0.0), false)
        .unwrap()
        .entity_id;

    s.fillet_lines(&FilletRequest {
        l1: top,
        l2: left,
        radius_text: "10".to_string(),
    })
    .expect("top-left R10");
    s.fillet_lines(&FilletRequest {
        l1: top,
        l2: right,
        radius_text: "10".to_string(),
    })
    .expect("top-right R10");

    let result = s.fillet_lines(&FilletRequest {
        l1: setup.entity_id,
        l2: left,
        radius_text: "10".to_string(),
    });
    assert!(
        result.is_ok(),
        "an exact one-ended trim may consume its carrier: {result:?}"
    );
    let result = result.unwrap();
    assert!(
        line_is_consumed(&s.dto(), setup.entity_id),
        "the zero-span setup carrier remains editable but is not presented"
    );

    let radius_dimension = result
        .sketch
        .dimensions
        .iter()
        .max_by_key(|dimension| dimension.entities[0].0)
        .expect("new fillet radius dimension");
    let over_limit = s.edit_dimension(EditDimensionRequest {
        constraint_id: radius_dimension.constraint_id,
        text: "10.1".to_string(),
    });
    assert!(
        over_limit.is_err(),
        "a one-ended trim must not cross beyond its opposite endpoint"
    );
    let message = over_limit.unwrap_err().to_string();
    assert!(
        message.len() < 300,
        "user-facing conflicts must stay concise: {message}"
    );

    let reopened = s
        .edit_dimension(EditDimensionRequest {
            constraint_id: radius_dimension.constraint_id,
            text: "9".to_string(),
        })
        .expect("reducing the radius reopens the one-ended carrier");
    let (_, a, b) = lines_of(&reopened.sketch)
        .into_iter()
        .find(|(id, _, _)| *id == setup.entity_id)
        .expect("setup carrier remains addressable");
    assert!((a.distance(b) - 1.0).abs() < 1e-4, "reopened={a:?}->{b:?}");
    assert!(!line_is_consumed(&reopened.sketch, setup.entity_id));
}

#[test]
fn chamfer_resolves_overlapping_line_to_the_adjacent_corner_carrier() {
    let mut s = session();
    let setup = s
        .add_line(v(0.0, 0.0), v(-10.0, 0.0), false)
        .expect("overlapping setup line");

    let bottom = s
        .add_line(v(-10.0, 0.0), v(10.0, 0.0), false)
        .unwrap()
        .entity_id;
    let right = s
        .add_line(v(10.0, 0.0), v(10.0, 30.0), false)
        .unwrap()
        .entity_id;

    s.chamfer_lines(&ChamferRequest {
        l1: setup.entity_id,
        l2: right,
        distance_text: "4".to_string(),
    })
    .expect("overlap pick resolves to the adjacent bottom carrier");

    let dto = s.dto();
    let (_, bottom_a, bottom_b) = lines_of(&dto)
        .into_iter()
        .find(|(id, _, _)| *id == bottom)
        .expect("bottom carrier remains addressable");
    assert!(
        bottom_a.distance(v(6.0, 0.0)) < 1e-4 || bottom_b.distance(v(6.0, 0.0)) < 1e-4,
        "4 mm chamfer trims the bottom carrier to x=6: {bottom_a:?} -> {bottom_b:?}"
    );
    let (_, right_a, right_b) = lines_of(&dto)
        .into_iter()
        .find(|(id, _, _)| *id == right)
        .expect("right carrier remains addressable");
    assert!(
        right_a.distance(v(10.0, 4.0)) < 1e-4 || right_b.distance(v(10.0, 4.0)) < 1e-4,
        "4 mm chamfer trims the right carrier to y=4: {right_a:?} -> {right_b:?}"
    );
}

/// Regression from the owner's 2026-08-12 report. A construction line tied
/// between the midpoints of opposite rectangle edges is an overall-part
/// datum. Filleting one end of an edge must not reinterpret that datum as the
/// midpoint of only the shortened carrier and reject the radius dimension.
#[test]
fn fillet_preserves_midpoint_datum_across_original_corner_span() {
    let mut s = session();
    let lines = typed_40_square(&mut s);
    let (bottom, top, right) = (
        edge(&lines, 'y', 0.0),
        edge(&lines, 'y', 40.0),
        edge(&lines, 'x', 40.0),
    );

    let centerline = s
        .add_line(v(20.0, 40.0), v(20.0, 0.0), false)
        .expect("midpoint-to-midpoint construction line");
    assert!(centerline.sketch.constraints.iter().any(|constraint| {
        matches!(
            constraint.constraint,
            limo_cad_sketch::Constraint::Midpoint { a, b }
                if a == centerline.start_point_id && b == top
        )
    }));
    assert!(centerline.sketch.constraints.iter().any(|constraint| {
        matches!(
            constraint.constraint,
            limo_cad_sketch::Constraint::Midpoint { a, b }
                if a == centerline.end_point_id && b == bottom
        )
    }));
    let before_redundant = s.dto();
    let redundant = s
        .add_constraint(limo_cad_sketch::Constraint::Vertical {
            entity: centerline.entity_id,
        })
        .expect_err("the two midpoint relations already make the centerline vertical");
    assert!(matches!(
        redundant,
        limo_cad_sketch::SessionError::RedundantConstraint { .. }
    ));
    assert_eq!(s.dto(), before_redundant);

    let result = s.fillet_lines(&FilletRequest {
        l1: top,
        l2: right,
        radius_text: "10".to_string(),
    });
    assert!(
        result.is_ok(),
        "corner fillet must preserve the overall-edge midpoint datum: {result:?}"
    );

    let dto = s.dto();
    let top_midpoint = dto
        .entities
        .iter()
        .find_map(|entity| match entity {
            EntityDto::Point { id, position, .. } if *id == centerline.start_point_id => {
                Some(*position)
            }
            _ => None,
        })
        .expect("construction-line top datum remains");
    assert!(
        top_midpoint.distance(v(20.0, 40.0)) < 1e-3,
        "overall top midpoint stays at the original span center: {top_midpoint:?}"
    );
}

/// Boundary regression from the owner's 2026-07-20 report: two fillets on
/// the ends of one 30 mm edge may each be R15. The remaining straight edge
/// is a valid zero-length carrier at exactly R1 + R2 == L; only values above
/// that boundary are invalid.
#[test]
fn adjacent_r15_fillets_fit_exactly_on_a_30_mm_edge() {
    let mut s = session();
    let lines = typed_30_square(&mut s);
    let (bottom, left, right) = (
        edge(&lines, 'y', 0.0),
        edge(&lines, 'x', 0.0),
        edge(&lines, 'x', 30.0),
    );

    s.fillet_lines(&FilletRequest {
        l1: bottom,
        l2: left,
        radius_text: "15".to_string(),
    })
    .expect("first R15 fillet works");
    let result = s.fillet_lines(&FilletRequest {
        l1: bottom,
        l2: right,
        radius_text: "15".to_string(),
    });
    assert!(
        result.is_ok(),
        "R15 + R15 must fit a 30 mm edge: {result:?}"
    );

    let dto = s.dto();
    let (_, a, b) = lines_of(&dto)
        .into_iter()
        .find(|(id, _, _)| *id == bottom)
        .expect("zero-length carrier line remains addressable");
    assert!(
        a.distance(b) < 1e-6,
        "shared edge must close exactly: {a:?} -> {b:?}"
    );
    assert!(
        line_is_consumed(&dto, bottom),
        "the parametric carrier stays addressable but is marked non-presentational"
    );
}

/// Owner's 2026-07-20 Chamfer boundary report: two bottom R20 fillets on a
/// 40 × 40 square leave exactly 20 mm of each vertical side. A distance-20
/// chamfer at the top corner may consume that remaining carrier completely;
/// only a distance beyond 20 is invalid.
#[test]
fn r20_bottom_fillets_allow_exact_distance_20_top_chamfer() {
    let mut s = session();
    let lines = typed_40_square(&mut s);
    let (bottom, left, top, right) = (
        edge(&lines, 'y', 0.0),
        edge(&lines, 'x', 0.0),
        edge(&lines, 'y', 40.0),
        edge(&lines, 'x', 40.0),
    );

    s.fillet_lines(&FilletRequest {
        l1: bottom,
        l2: left,
        radius_text: "20".to_string(),
    })
    .expect("first R20 fillet works");
    s.fillet_lines(&FilletRequest {
        l1: bottom,
        l2: right,
        radius_text: "20".to_string(),
    })
    .expect("second R20 fillet works");

    let before = s.dto();
    let (_, left_a, left_b) = lines_of(&before)
        .into_iter()
        .find(|(id, _, _)| *id == left)
        .expect("left carrier exists before chamfer");
    let remaining = left_a.distance(left_b);
    let too_large = s.chamfer_lines(&ChamferRequest {
        l1: top,
        l2: left,
        distance_text: "20.001".to_string(),
    });
    assert!(too_large.is_err(), "distance beyond the carrier must fail");
    assert_eq!(
        s.dto().entities.len(),
        before.entities.len(),
        "failure rolls back"
    );

    let result = s.chamfer_lines(&ChamferRequest {
        l1: top,
        l2: left,
        distance_text: "20".to_string(),
    });
    assert!(
        result.is_ok(),
        "distance equal to the remaining carrier must succeed (remaining={remaining:.17}): {result:?}"
    );

    let (_, a, b) = lines_of(&s.dto())
        .into_iter()
        .find(|(id, _, _)| *id == left)
        .expect("consumed left carrier remains addressable");
    assert!(
        a.distance(b) < 5e-4,
        "left carrier closes at equality: {}",
        a.distance(b)
    );
}

/// Matches the screenshot path: create the second fillet below the limit,
/// edit its radius to the exact boundary, then reduce it again. Keeping the
/// carrier line in the model makes the topology transition reversible.
#[test]
fn editing_adjacent_fillet_to_exact_sum_is_accepted_and_reversible() {
    let mut s = session();
    let lines = typed_30_square(&mut s);
    let (bottom, left, right) = (
        edge(&lines, 'y', 0.0),
        edge(&lines, 'x', 0.0),
        edge(&lines, 'x', 30.0),
    );

    s.fillet_lines(&FilletRequest {
        l1: bottom,
        l2: left,
        radius_text: "15".to_string(),
    })
    .unwrap();
    let before_second = s.dto();
    let first_arc = before_second
        .entities
        .iter()
        .find_map(|entity| match entity {
            EntityDto::Arc { id, .. } => Some(*id),
            _ => None,
        })
        .unwrap();
    let second = s
        .fillet_lines(&FilletRequest {
            l1: bottom,
            l2: right,
            radius_text: "10".to_string(),
        })
        .unwrap();
    let second_arc = second
        .sketch
        .entities
        .iter()
        .find_map(|entity| match entity {
            EntityDto::Arc { id, .. } if *id != first_arc => Some(*id),
            _ => None,
        })
        .unwrap();
    let radius_dimension = second
        .sketch
        .dimensions
        .iter()
        .find(|dimension| dimension.entities == vec![second_arc])
        .expect("second fillet radius dimension");

    let equal = s
        .edit_dimension(EditDimensionRequest {
            constraint_id: radius_dimension.constraint_id,
            text: "15".to_string(),
        })
        .expect("editing to R1 + R2 == L is valid");
    let (_, a, b) = lines_of(&equal.sketch)
        .into_iter()
        .find(|(id, _, _)| *id == bottom)
        .unwrap();
    assert!(
        a.distance(b) < 5e-4,
        "edge closes at equality: {a:?} -> {b:?}"
    );
    assert!(line_is_consumed(&equal.sketch, bottom));

    let over_limit = s.edit_dimension(EditDimensionRequest {
        constraint_id: radius_dimension.constraint_id,
        text: "15.1".to_string(),
    });
    assert!(
        over_limit.is_err(),
        "R1 + R2 > L must remain invalid: {over_limit:?}"
    );

    let reopened = s
        .edit_dimension(EditDimensionRequest {
            constraint_id: radius_dimension.constraint_id,
            text: "10".to_string(),
        })
        .expect("reducing the radius reopens the carrier edge");
    let (_, a, b) = lines_of(&reopened.sketch)
        .into_iter()
        .find(|(id, _, _)| *id == bottom)
        .unwrap();
    assert!(
        (a.distance(b) - 5.0).abs() < 5e-4,
        "reopened edge length: {}",
        a.distance(b)
    );
    assert!(
        !line_is_consumed(&reopened.sketch, bottom),
        "reducing either fillet restores the carrier to rendering and picking"
    );
}

/// A zero-span carrier must remember its directed support, not merely that it
/// was horizontal/vertical. Rectangle edges have all four possible endpoint
/// orderings; each one must cross the equality boundary and reopen correctly.
#[test]
fn exact_sum_reopens_on_every_rectangle_edge_orientation() {
    for (carrier_axis, carrier_value, end_axis, end1_value, end2_value) in [
        ('y', 0.0, 'x', 0.0, 30.0),
        ('y', 30.0, 'x', 0.0, 30.0),
        ('x', 0.0, 'y', 0.0, 30.0),
        ('x', 30.0, 'y', 0.0, 30.0),
    ] {
        let mut s = session();
        let lines = typed_30_square(&mut s);
        let carrier = edge(&lines, carrier_axis, carrier_value);
        let end1 = edge(&lines, end_axis, end1_value);
        let end2 = edge(&lines, end_axis, end2_value);

        s.fillet_lines(&FilletRequest {
            l1: carrier,
            l2: end1,
            radius_text: "15".to_string(),
        })
        .unwrap();
        let first_arc = s
            .dto()
            .entities
            .iter()
            .find_map(|entity| match entity {
                EntityDto::Arc { id, .. } => Some(*id),
                _ => None,
            })
            .unwrap();
        let second = s
            .fillet_lines(&FilletRequest {
                l1: carrier,
                l2: end2,
                radius_text: "10".to_string(),
            })
            .unwrap();
        let second_arc = second
            .sketch
            .entities
            .iter()
            .find_map(|entity| match entity {
                EntityDto::Arc { id, .. } if *id != first_arc => Some(*id),
                _ => None,
            })
            .unwrap();
        let radius_dimension = second
            .sketch
            .dimensions
            .iter()
            .find(|dimension| dimension.entities == vec![second_arc])
            .unwrap();

        let equal = s
            .edit_dimension(EditDimensionRequest {
                constraint_id: radius_dimension.constraint_id,
                text: "15".to_string(),
            })
            .unwrap_or_else(|error| {
                panic!("{carrier_axis}={carrier_value} must accept equality: {error:?}")
            });
        let (_, a, b) = lines_of(&equal.sketch)
            .into_iter()
            .find(|(id, _, _)| *id == carrier)
            .unwrap();
        assert!(
            a.distance(b) < 5e-4,
            "{carrier_axis}={carrier_value} closes: {}",
            a.distance(b)
        );

        let reopened = s
            .edit_dimension(EditDimensionRequest {
                constraint_id: radius_dimension.constraint_id,
                text: "10".to_string(),
            })
            .unwrap_or_else(|error| {
                panic!("{carrier_axis}={carrier_value} must reopen after equality: {error:?}")
            });
        let (_, a, b) = lines_of(&reopened.sketch)
            .into_iter()
            .find(|(id, _, _)| *id == carrier)
            .unwrap();
        assert!(
            (a.distance(b) - 5.0).abs() < 5e-4,
            "{carrier_axis}={carrier_value} reopened length: {}",
            a.distance(b)
        );
    }
}

#[test]
fn chamfer_then_fillet_on_dimensioned_rect() {
    let mut s = session();
    let lines = typed_square(&mut s);
    let (bottom, left, top) = (
        edge(&lines, 'y', 0.0),
        edge(&lines, 'x', 0.0),
        edge(&lines, 'y', 20.0),
    );
    s.chamfer_lines(&ChamferRequest {
        l1: bottom,
        l2: left,
        distance_text: "4".to_string(),
    })
    .expect("chamfer works");
    let r = s.fillet_lines(&FilletRequest {
        l1: top,
        l2: left,
        radius_text: "4".to_string(),
    });
    assert!(r.is_ok(), "fillet after chamfer: {r:?}");
}

#[test]
fn corner_point_persists_as_constraint_reference() {
    let mut s = session();
    s.add_rectangle_locked(&LockedRectangleRequest {
        mode: RectangleMode::TwoPoint,
        anchor: v(0.0, 0.0),
        width_mm: None,
        height_mm: None,
        width_text: Some("40".to_string()),
        height_text: Some("15".to_string()),
        corner_hint: v(40.0, 15.0),
        ctrl_held: true,
    })
    .unwrap();
    let lines = lines_of(&s.dto());
    let top = lines
        .iter()
        .find(|(_, a, b)| (a.y - 15.0).abs() < 1e-9 && (b.y - 15.0).abs() < 1e-9)
        .unwrap()
        .0;
    let left = lines
        .iter()
        .find(|(_, a, b)| (a.x - 0.0).abs() < 1e-9 && (b.x - 0.0).abs() < 1e-9)
        .unwrap()
        .0;
    s.fillet_lines(&FilletRequest {
        l1: top,
        l2: left,
        radius_text: "4".to_string(),
    })
    .unwrap();
    let dto = s.dto();

    let corner_alive = dto.entities.iter().any(|e| {
        matches!(e, EntityDto::Point { position, .. } if position.distance(v(0.0, 15.0)) < 1e-3)
    });
    assert!(corner_alive, "original corner point must persist at (0,15)");

    let height_dim = dto
        .dimensions
        .iter()
        .find(|d| d.text == "15.00")
        .expect("height dim");
    assert_eq!(height_dim.text, "15.00");
    let bl_alive = dto.entities.iter().any(
        |e| matches!(e, EntityDto::Point { position, .. } if position.distance(v(0.0, 0.0)) < 1e-3),
    );
    assert!(bl_alive, "no shift: bottom-left corner stays at (0,0)");

    let (la, lb) = {
        let l = lines_of(&dto)
            .into_iter()
            .find(|(id, _, _)| *id == left)
            .unwrap();
        (l.1, l.2)
    };
    let trim_y = la.y.max(lb.y);
    assert!(
        (15.0 - trim_y - 4.0).abs() < 1e-3,
        "trim 4 below top: {trim_y}"
    );
}

/// Every ordered pair of DISTINCT corners × every op pair — the full
/// cross-product the owner asked us to verify in reverse. All must be accepted.
#[test]
fn corner_op_matrix_on_dimensioned_rect() {
    for (c1x, c1y) in [(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)] {
        for (c2x, c2y) in [(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)] {
            if (c1x, c1y) == (c2x, c2y) {
                continue;
            }
            for ops in ["ff", "cf", "fc", "cc"] {
                let mut s = session();
                let lines = typed_square(&mut s);
                let l1a = edge(&lines, 'y', c1y);
                let l1b = edge(&lines, 'x', c1x);
                let do_c1 = |s: &mut SketchSession, la, lb| {
                    if ops.starts_with('f') {
                        s.fillet_lines(&FilletRequest {
                            l1: la,
                            l2: lb,
                            radius_text: "4".to_string(),
                        })
                        .map(|_| ())
                    } else {
                        s.chamfer_lines(&ChamferRequest {
                            l1: la,
                            l2: lb,
                            distance_text: "4".to_string(),
                        })
                        .map(|_| ())
                    }
                };
                do_c1(&mut s, l1a, l1b)
                    .unwrap_or_else(|e| panic!("op1 {ops} @({c1x},{c1y}): {e:?}"));
                let lines = lines_of(&s.dto());
                let l2a = edge(&lines, 'y', c2y);
                let l2b = edge(&lines, 'x', c2x);
                let r = if ops.ends_with('f') {
                    s.fillet_lines(&FilletRequest {
                        l1: l2a,
                        l2: l2b,
                        radius_text: "4".to_string(),
                    })
                    .map(|_| ())
                } else {
                    s.chamfer_lines(&ChamferRequest {
                        l1: l2a,
                        l2: l2b,
                        distance_text: "4".to_string(),
                    })
                    .map(|_| ())
                };
                r.unwrap_or_else(|e| {
                    panic!("op2 {ops} @({c2x},{c2y}) after @({c1x},{c1y}): {e:?}")
                });
            }
        }
    }
}
