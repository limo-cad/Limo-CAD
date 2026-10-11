use limo_cad_sketch::*;

fn session() -> SketchSession {
    let plane = PlaneRef::OriginPlane {
        plane: OriginPlane::Xy,
    };
    SketchSession::new("Angled chamfer", plane, plane.basis().unwrap(), false)
}

fn snapshot(session: &SketchSession) -> String {
    serde_json::to_string(&session.sketch().snapshot()).unwrap()
}

fn line(dto: &SketchDto, id: EntityId) -> (Vec2, Vec2) {
    match dto
        .entities
        .iter()
        .find(|entity| entity.id() == id)
        .unwrap()
    {
        EntityDto::Line { start, end, .. } => (*start, *end),
        _ => panic!("expected a line"),
    }
}

fn request(l1: EntityId, l2: EntityId, angle: &str) -> ChamferRequest {
    ChamferRequest {
        l1,
        l2,
        distance_text: "2".into(),
        angle_text: Some(angle.into()),
    }
}

fn cut(dto: &SketchDto, id: EntityId, far: Vec2) -> Vec2 {
    let (a, b) = line(dto, id);
    if a.distance(far) < 1e-6 {
        b
    } else {
        a
    }
}

fn check_right_corner(dto: &SketchDto, l1: EntityId, l2: EntityId, distance: f64, angle: f64) {
    let first = cut(dto, l1, Vec2::new(50., 0.));
    let second = cut(dto, l2, Vec2::new(0., 50.));
    assert!(first.distance(Vec2::new(distance, 0.)) < 1e-6, "{first:?}");
    assert!(
        second.distance(Vec2::new(0., distance * angle.to_radians().tan())) < 1e-6,
        "{second:?}"
    );
    assert!(
        line(dto, l1).0.distance(Vec2::new(50., 0.)) < 1e-6
            || line(dto, l1).1.distance(Vec2::new(50., 0.)) < 1e-6
    );
    assert!(
        line(dto, l2).0.distance(Vec2::new(0., 50.)) < 1e-6
            || line(dto, l2).1.distance(Vec2::new(0., 50.)) < 1e-6
    );
}

#[test]
fn arbitrary_angles_match_preview_in_every_corner_orientation() {
    for angle in [0.1_f64, 15., 30., 45., 60., 75., 89.] {
        for reverse in [false, true] {
            for sign in [-1., 1.] {
                let mut s = session();
                let origin = Vec2::ZERO;
                let first_far = Vec2::new(1000., 0.);
                let second_far = Vec2::new(0., sign * 1000.);
                let (a, b) = if reverse {
                    (first_far, origin)
                } else {
                    (origin, first_far)
                };
                let first = s.add_line(a, b, true).unwrap().entity_id;
                let second = s.add_line(origin, second_far, true).unwrap().entity_id;
                let before = snapshot(&s);
                let r = request(first, second, &angle.to_string());
                let PreviewCurve::Line { a, b } = s.chamfer_preview(&r).unwrap() else {
                    panic!("line preview")
                };
                assert_eq!(
                    snapshot(&s),
                    before,
                    "preview changes neither geometry nor history"
                );
                assert!(a.distance(Vec2::new(2., 0.)) < 1e-6);
                assert!(b.distance(Vec2::new(0., sign * 2. * angle.to_radians().tan())) < 1e-6);
                let result = s.chamfer_lines(&r).unwrap();
                assert!(cut(&result.sketch, first, first_far).distance(a) < 1e-6);
                assert!(cut(&result.sketch, second, second_far).distance(b) < 1e-6);
                assert!(result
                    .sketch
                    .dimensions
                    .iter()
                    .any(|d| d.kind == "angle" && (d.value - angle).abs() < 1e-8));
                assert_eq!(result.sketch.dimensions.len(), 2);
                assert!(!result
                    .sketch
                    .constraints
                    .iter()
                    .any(|c| matches!(c.constraint, Constraint::EqualDistance { .. })));
                let after = snapshot(&s);
                s.undo().unwrap();
                assert_eq!(snapshot(&s), before);
                s.redo().unwrap();
                assert_eq!(snapshot(&s), after);
            }
        }
    }
}

#[test]
fn distance_and_angle_edits_keep_the_corner_and_far_ends() {
    let mut s = session();
    let l1 = s
        .add_line(Vec2::ZERO, Vec2::new(50., 0.), true)
        .unwrap()
        .entity_id;
    let l2 = s
        .add_line(Vec2::ZERO, Vec2::new(0., 50.), true)
        .unwrap()
        .entity_id;
    s.chamfer_lines(&request(l1, l2, "45")).unwrap();
    let dto = s.dto();
    let angle_id = dto
        .dimensions
        .iter()
        .find(|d| d.kind == "angle")
        .unwrap()
        .constraint_id;
    let distance_id = dto
        .dimensions
        .iter()
        .find(|d| d.kind != "angle")
        .unwrap()
        .constraint_id;
    s.edit_dimension(EditDimensionRequest {
        constraint_id: angle_id,
        text: "60/2".into(),
    })
    .unwrap();
    check_right_corner(&s.dto(), l1, l2, 2., 30.);
    s.edit_dimension(EditDimensionRequest {
        constraint_id: distance_id,
        text: "4".into(),
    })
    .unwrap();
    check_right_corner(&s.dto(), l1, l2, 4., 30.);
    let before = snapshot(&s);
    for invalid in ["0", "90", "-1", "91", "1/0"] {
        assert!(s
            .edit_dimension(EditDimensionRequest {
                constraint_id: angle_id,
                text: invalid.into()
            })
            .is_err());
        assert_eq!(
            snapshot(&s),
            before,
            "invalid edit {invalid} must roll back parameters and geometry"
        );
    }
    assert!(s
        .edit_dimension(EditDimensionRequest {
            constraint_id: distance_id,
            text: "100".into()
        })
        .is_err());
    assert_eq!(snapshot(&s), before);
    s.undo().unwrap();
    check_right_corner(&s.dto(), l1, l2, 2., 30.);
    s.undo().unwrap();
    check_right_corner(&s.dto(), l1, l2, 2., 45.);
    s.redo().unwrap();
    s.redo().unwrap();
    assert_eq!(snapshot(&s), before);
}

#[test]
fn non_right_corners_use_the_actual_angle_from_the_first_line() {
    for corner in [60_f64, 120.] {
        let mut s = session();
        let first = s
            .add_line(Vec2::ZERO, Vec2::new(50., 0.), true)
            .unwrap()
            .entity_id;
        let ray = Vec2::new(corner.to_radians().cos(), corner.to_radians().sin());
        let second = s.add_line(Vec2::ZERO, ray * 50., true).unwrap().entity_id;
        let PreviewCurve::Line { a, b } = s.chamfer_preview(&request(first, second, "30")).unwrap()
        else {
            panic!("line preview")
        };
        let bevel = a - b;
        assert!((bevel.y.atan2(bevel.x).to_degrees().abs() - 30.).abs() < 1e-8);
        let expected = 2. * 30_f64.to_radians().sin() / (180. - corner - 30.).to_radians().sin();
        assert!(b.distance(ray * expected) < 1e-8);
        s.chamfer_lines(&request(first, second, "30")).unwrap();
    }
}

#[test]
fn retained_angle_and_distance_edits_preserve_the_rest_of_a_rectangle() {
    for locked in [false, true] {
        let mut s = session();
        if locked {
            s.add_rectangle_locked(&LockedRectangleRequest {
                mode: RectangleMode::TwoPoint,
                anchor: Vec2::ZERO,
                width_mm: None,
                height_mm: None,
                width_text: Some("40".into()),
                height_text: Some("30".into()),
                corner_hint: Vec2::new(40., 30.),
                ctrl_held: true,
            })
            .unwrap();
        } else {
            s.add_rectangle(RectangleMode::TwoPoint, Vec2::ZERO, Vec2::new(40., 30.))
                .unwrap();
        }
        let before = s.dto();
        let first = before
            .entities
            .iter()
            .find_map(|entity| match entity {
                EntityDto::Line { id, start, end, .. } if start.y == 30. && end.y == 30. => {
                    Some(*id)
                }
                _ => None,
            })
            .unwrap();
        let second = before
            .entities
            .iter()
            .find_map(|entity| match entity {
                EntityDto::Line { id, start, end, .. } if start.x == 0. && end.x == 0. => Some(*id),
                _ => None,
            })
            .unwrap();
        s.chamfer_lines(&request(first, second, "30")).unwrap();
        let dimensions = s.dto().dimensions;
        let angle = dimensions
            .iter()
            .find(|dimension| dimension.kind == "angle")
            .unwrap()
            .constraint_id;
        let distance = dimensions
            .iter()
            .find(|dimension| dimension.kind != "angle" && (dimension.value - 2.).abs() < 1e-9)
            .unwrap()
            .constraint_id;
        s.edit_dimension(EditDimensionRequest {
            constraint_id: angle,
            text: "60".into(),
        })
        .unwrap();
        s.edit_dimension(EditDimensionRequest {
            constraint_id: distance,
            text: "5".into(),
        })
        .unwrap();
        let after = s.dto();
        assert!(cut(&after, first, Vec2::new(40., 30.)).distance(Vec2::new(5., 30.)) < 1e-6);
        assert!(
            cut(&after, second, Vec2::ZERO)
                .distance(Vec2::new(0., 30. - 5. * 60_f64.to_radians().tan()))
                < 1e-6
        );
        for entity in before.entities.iter() {
            if let EntityDto::Line { id, start, end, .. } = entity {
                if *id != first && *id != second {
                    let (actual_start, actual_end) = line(&after, *id);
                    assert!(
                        actual_start.distance(*start) < 1e-6 && actual_end.distance(*end) < 1e-6
                    );
                }
            }
        }
        let saved = snapshot(&s);
        assert!(s
            .edit_dimension(EditDimensionRequest {
                constraint_id: angle,
                text: "89".into()
            })
            .is_err());
        assert_eq!(snapshot(&s), saved);
    }
}

#[test]
fn invalid_or_oversized_chamfers_leave_no_geometry_parameters_or_undo_entry() {
    let mut s = session();
    let first = s
        .add_line(Vec2::ZERO, Vec2::new(10., 0.), true)
        .unwrap()
        .entity_id;
    let second = s
        .add_line(Vec2::ZERO, Vec2::new(0., 10.), true)
        .unwrap()
        .entity_id;
    let before = snapshot(&s);
    for invalid in ["0", "90", "-10", "100", "89", "1/0"] {
        assert!(s.chamfer_preview(&request(first, second, invalid)).is_err());
        assert!(s.chamfer_lines(&request(first, second, invalid)).is_err());
        assert_eq!(snapshot(&s), before);
    }
    s.undo().unwrap();
    assert_eq!(
        s.dto()
            .entities
            .iter()
            .filter(|e| matches!(e, EntityDto::Line { .. }))
            .count(),
        1
    );
}

#[test]
fn old_requests_keep_their_equal_setback_construction() {
    let mut s = session();
    let first = s
        .add_line(Vec2::ZERO, Vec2::new(50., 0.), true)
        .unwrap()
        .entity_id;
    let second = s
        .add_line(Vec2::ZERO, Vec2::new(25., 43.30127018922193), true)
        .unwrap()
        .entity_id;
    let request: ChamferRequest =
        serde_json::from_value(serde_json::json!({"l1":first,"l2":second,"distance_text":"2"}))
            .unwrap();
    assert_eq!(request.angle_text, None);
    let result = s.chamfer_lines(&request).unwrap();
    assert_eq!(result.sketch.dimensions.len(), 1);
    assert!(result
        .sketch
        .constraints
        .iter()
        .any(|c| matches!(c.constraint, Constraint::EqualDistance { .. })));
}
