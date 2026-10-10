//! Typed locks, snap acquisition, preview and mutation share one geometry intent.
use limo_cad_sketch::*;
fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}
fn session(grid: bool, step: f64) -> SketchSession {
    let plane = PlaneRef::OriginPlane {
        plane: OriginPlane::Xy,
    };
    let mut s = SketchSession::new("Intent", plane, plane.basis().unwrap(), grid);
    s.set_grid_step(step).unwrap();
    s
}
fn snapshot(s: &SketchSession) -> String {
    serde_json::to_string(&s.sketch().snapshot()).unwrap()
}
fn assert_preview(curves: &[PreviewCurve], result: &ToolResult) {
    let created: Vec<_> = result
        .sketch
        .entities
        .iter()
        .filter(|e| result.entities.contains(&e.id()) && !matches!(e, EntityDto::Point { .. }))
        .collect();
    assert_eq!(curves.len(), created.len());
    for curve in curves {
        assert!(
            created.iter().any(|entity| match (curve, entity) {
                (PreviewCurve::Line { a, b }, EntityDto::Line { start, end, .. }) =>
                    (a.distance(*start) < 1e-6 && b.distance(*end) < 1e-6)
                        || (b.distance(*start) < 1e-6 && a.distance(*end) < 1e-6),
                (
                    PreviewCurve::Circle {
                        center: a,
                        radius: r,
                    },
                    EntityDto::Circle {
                        center: b,
                        radius: q,
                        ..
                    },
                ) => a.distance(*b) < 1e-6 && (r - q).abs() < 1e-6,
                (
                    PreviewCurve::Arc {
                        center: a,
                        radius: r,
                        start_angle: a0,
                        end_angle: a1,
                    },
                    EntityDto::Arc {
                        center: b,
                        radius: q,
                        start_angle: b0,
                        end_angle: b1,
                        ..
                    },
                ) =>
                    a.distance(*b) < 1e-6
                        && (r - q).abs() < 1e-6
                        && (a0 - b0).abs() < 1e-6
                        && (a1 - b1).abs() < 1e-6,
                _ => false,
            }),
            "preview differs from committed geometry: {curve:?}, {created:?}"
        );
    }
}

#[test]
fn locked_circles_never_move_to_an_off_radius_point_or_override_ctrl() {
    for mode in [CircleMode::CenterDiameter, CircleMode::TwoPoint] {
        for ctrl in [false, true] {
            for on_locus in [false, true] {
                let mut s = session(false, 1.);
                let anchor = v(30., 30.);
                let length = if mode == CircleMode::CenterDiameter {
                    5.
                } else {
                    10.
                };
                let point = s
                    .add_point(anchor + v(length + if on_locus { 0. } else { 0.5 }, 0.))
                    .unwrap()
                    .entities[0];
                s.toggle_fix(point).unwrap();
                let request = LockedCircleRequest {
                    mode,
                    anchor,
                    edge_hint: anchor + v(length, 0.4),
                    diameter_mm: None,
                    diameter_text: Some("=5*2".into()),
                    ctrl_held: ctrl,
                };
                let before = snapshot(&s);
                let preview = s
                    .preview_creation(&CreationPreviewRequest::Circle(request.clone()))
                    .unwrap();
                assert_eq!(
                    snapshot(&s),
                    before,
                    "preview must not consume ids, params or undo"
                );
                assert_eq!(
                    matches!(preview.snap, SnapTarget::Point { .. }),
                    on_locus && !ctrl
                );
                let result = s.add_circle_locked(&request).unwrap();
                assert_preview(&preview.curves, &result);
                assert!((preview.values["diameter"] - 10.).abs() < 1e-9);
                assert!(result.sketch.dimensions.iter().any(|d| d
                    .param_expression
                    .as_deref()
                    .is_some_and(|s| s.contains("5*2"))));
                s.undo().unwrap();
                assert_eq!(snapshot(&s), before);
            }
        }
    }
}

#[test]
fn rectangle_and_slot_previews_match_all_modes_locks_modifiers_and_grid_scales() {
    for step in [0.1, 1., 10.] {
        for ctrl in [false, true] {
            for mode in [RectangleMode::TwoPoint, RectangleMode::Center] {
                for locks in [0, 1, 2, 3] {
                    let mut s = session(true, step);
                    s.add_point(v(42.2, 38.2)).unwrap();
                    let request = LockedRectangleRequest {
                        mode,
                        anchor: v(30., 30.),
                        corner_hint: v(42.3, 38.3),
                        width_mm: None,
                        height_mm: None,
                        width_text: (locks & 1 != 0).then(|| "=6*2".into()),
                        height_text: (locks & 2 != 0).then(|| "=4*2".into()),
                        ctrl_held: ctrl,
                    };
                    let preview = s
                        .preview_creation(&CreationPreviewRequest::Rectangle(request.clone()))
                        .unwrap();
                    let result = s.add_rectangle_locked(&request).unwrap();
                    assert_preview(&preview.curves, &result);
                    if locks & 1 != 0 {
                        assert!((preview.values["width"] - 12.).abs() < 1e-8);
                    }
                    if locks & 2 != 0 {
                        assert!((preview.values["height"] - 8.).abs() < 1e-8);
                    }
                }
            }
            for mode in [
                SlotMode::CenterToCenter,
                SlotMode::Overall,
                SlotMode::CenterPoint,
            ] {
                for locked in [false, true] {
                    let mut s = session(true, step);
                    let request = SlotRequest {
                        mode,
                        p1: v(30., 30.),
                        p2: v(50., 30.),
                        cursor: v(42., 33.3),
                        width_mm: None,
                        width_text: locked.then(|| "=3*2".into()),
                        ctrl_held: ctrl,
                    };
                    let preview = s
                        .preview_creation(&CreationPreviewRequest::Slot(request.clone()))
                        .unwrap();
                    let result = s.add_slot(&request).unwrap();
                    assert_preview(&preview.curves, &result);
                    if locked {
                        assert!((preview.values["width"] - 6.).abs() < 1e-8);
                    }
                }
            }
        }
    }
}

#[test]
fn invalid_locked_input_is_rejected_atomically_in_preview_and_commit() {
    for text in ["0", "-2", "=unknown", "=1/0", "=1+"] {
        let mut s = session(false, 1.);
        let before = snapshot(&s);
        let circle = LockedCircleRequest {
            mode: CircleMode::CenterDiameter,
            anchor: v(30., 30.),
            edge_hint: v(40., 30.),
            diameter_mm: None,
            diameter_text: Some(text.into()),
            ctrl_held: false,
        };
        assert!(
            s.preview_creation(&CreationPreviewRequest::Circle(circle.clone()))
                .is_err(),
            "{text}"
        );
        assert!(s.add_circle_locked(&circle).is_err(), "{text}");
        let rectangle = LockedRectangleRequest {
            mode: RectangleMode::Center,
            anchor: v(30., 30.),
            corner_hint: v(40., 40.),
            width_mm: None,
            height_mm: None,
            width_text: Some(text.into()),
            height_text: None,
            ctrl_held: false,
        };
        assert!(s
            .preview_creation(&CreationPreviewRequest::Rectangle(rectangle.clone()))
            .is_err());
        assert!(s.add_rectangle_locked(&rectangle).is_err());
        assert_eq!(
            snapshot(&s),
            before,
            "{text} left partial geometry or consumed an id"
        );
    }
}

#[test]
fn ctrl_rectangle_does_not_merge_exact_corners_and_virtual_center_does_not_constrain_other_points()
{
    for ctrl in [false, true] {
        let mut s = session(false, 1.);
        let point = s.add_point(v(30., 30.)).unwrap().entities[0];
        let result = s
            .add_rectangle_selective(RectangleMode::TwoPoint, v(30., 30.), v(40., 40.), ctrl)
            .unwrap();
        assert_eq!(result.entities.contains(&point), !ctrl);
    }
    let mut s = session(false, 1.);
    s.add_point_on_selective(Vec2::ZERO, None, true).unwrap();
    let before = s.dto().constraints;
    s.add_rectangle_selective(RectangleMode::Center, Vec2::ZERO, v(10., 10.), false)
        .unwrap();
    assert!(!s
        .dto()
        .constraints
        .iter()
        .any(|c| matches!(c.constraint, Constraint::OriginCoincident { .. })));
    assert!(before.is_empty());
}

#[test]
fn invalid_line_locks_are_rejected_before_mutation() {
    for text in ["0", "-2", "=missing", "=1/0", "=1+"] {
        let mut s = session(false, 1.);
        let before = snapshot(&s);
        let request: LockedSegmentRequest = serde_json::from_value(serde_json::json!({
            "from": {"x": 20., "y": 20.}, "to_hint": {"x": 30., "y": 20.},
            "length_text": text, "ctrl_held": true
        }))
        .unwrap();
        assert!(s.add_line_locked(&request).is_err(), "{text}");
        assert_eq!(snapshot(&s), before);
    }
}

#[test]
fn locked_line_only_acquires_points_that_satisfy_the_locks() {
    for ctrl in [false, true] {
        for locks in [1, 2, 3] {
            let mut s = session(false, 1.);
            let point = s.add_point(v(40.4, 30.5)).unwrap().entities[0];
            s.toggle_fix(point).unwrap();
            let request = LockedSegmentRequest {
                from: v(30., 30.),
                to_hint: v(40., 30.),
                length_mm: (locks & 1 != 0).then_some(10.),
                angle_deg: (locks & 2 != 0).then_some(0.),
                length_text: None,
                angle_text: None,
                ctrl_held: ctrl,
                tracking: None,
                intersection: None,
                from_crossing: None,
                to_crossing: None,
            };
            let preview = s.preview_segment_locked(
                request.from,
                (request.length_mm, request.angle_deg),
                request.to_hint,
                ctrl,
                (None, None, None, None),
            );
            assert!(!matches!(preview.snap, SnapTarget::Point { .. }));
            let result = s.add_line_locked(&request).unwrap();
            let EntityDto::Line { start, end, .. } = result
                .sketch
                .entities
                .iter()
                .find(|e| e.id() == result.entity_id)
                .unwrap()
            else {
                panic!()
            };
            assert!(end.distance(preview.snapped_to) < 1e-6);
            if locks & 1 != 0 {
                assert!((end.distance(*start) - 10.).abs() < 1e-6);
            }
            if locks & 2 != 0 {
                assert!((end.y - start.y).abs() < 1e-6);
            }
        }
    }
}

#[test]
fn arcs_and_chamfers_share_formula_validation_and_resolved_geometry() {
    use limo_cad_sketch::{ArcCenterRequest, ChamferRequest};
    for sweep in [-360., -270., -180., -90., 90., 180., 270., 360.] {
        let mut s = session(false, 1.);
        let request = ArcCenterRequest {
            center: v(30., 30.),
            start: v(40., 30.),
            sweep: v(30., 50.),
            radius_mm: None,
            radius_text: Some("=5*2".into()),
            angle_text: Some(format!("={sweep:+}/2*2")),
            sweep_rad: Some(0.1),
            ctrl_held: true,
        };
        let preview = s
            .preview_creation(&CreationPreviewRequest::ArcCenter(request.clone()))
            .unwrap();
        let result = s
            .add_arc_center_locked(
                (request.center, request.start, request.sweep),
                true,
                None,
                request.radius_text.as_deref(),
                request.angle_text.as_deref(),
                request.sweep_rad,
            )
            .unwrap();
        assert_preview(&preview.curves, &result);
        assert!((preview.values["angle"] - sweep).abs() < 1e-8);
    }
    for angle in ["0", "361", "-361", "=1/0", "=1+"] {
        let mut s = session(false, 1.);
        let before = snapshot(&s);
        assert!(s
            .add_arc_center_locked(
                (v(30., 30.), v(40., 30.), v(30., 40.)),
                true,
                None,
                None,
                Some(angle),
                Some(1.)
            )
            .is_err());
        assert_eq!(snapshot(&s), before);
    }
    let mut s = session(false, 1.);
    let l1 = s
        .add_line(v(20., 20.), v(40., 20.), true)
        .unwrap()
        .entity_id;
    let l2 = s
        .add_line(v(40., 20.), v(40., 40.), true)
        .unwrap()
        .entity_id;
    let request = ChamferRequest {
        l1,
        l2,
        distance_text: "=3*2".into(),
    };
    let preview = s
        .preview_creation(&CreationPreviewRequest::Chamfer(request.clone()))
        .unwrap();
    let result = s.chamfer_lines(&request).unwrap();

    assert!(result
        .sketch
        .entities
        .iter()
        .any(|e| match (&preview.curves[0], e) {
            (PreviewCurve::Line { a, b }, EntityDto::Line { start, end, .. }) =>
                a.distance(*start) < 1e-6 && b.distance(*end) < 1e-6,
            _ => false,
        }));
}
