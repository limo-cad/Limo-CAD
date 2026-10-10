//! Integration tests for the sketch-session API: snap priority, H/V +
//! coincident inference, structural coincident merging,
//! interim drag projection, delete cascades, undo/redo, session lifecycle,
//! and the host JSON envelope.

use limo_cad_sketch::host;
use limo_cad_sketch::{
    Constraint, DimensionRequest, DragPhase, EditDimensionRequest, EntityDto, Inference,
    LineTrackingRequest, LockedSegmentRequest, MovePointRequest, OriginPlane, PlaneRef,
    SegmentRequest, SketchManager, SketchSession, SnapTarget, TrackingAxis, Vec2,
};

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

const XY: PlaneRef = PlaneRef::OriginPlane {
    plane: OriginPlane::Xy,
};

/// Session with grid snap OFF (deterministic coordinates).
fn session_off_grid() -> SketchSession {
    SketchSession::new("Sketch1", XY, XY.basis().unwrap(), false)
}

/// Session with grid snap ON (10 mm step).
fn session_on_grid() -> SketchSession {
    SketchSession::new("Sketch1", XY, XY.basis().unwrap(), true)
}

fn seg(from: Vec2, to_raw: Vec2, ctrl_held: bool) -> SegmentRequest {
    SegmentRequest {
        from,
        to_raw,
        ctrl_held,
    }
}

fn line_endpoints(session: &SketchSession, dto_id: limo_cad_sketch::EntityId) -> (Vec2, Vec2) {
    let dto = session.dto();
    match dto.entities.iter().find(|e| e.id() == dto_id) {
        Some(EntityDto::Line { start, end, .. }) => (*start, *end),
        other => panic!("expected line, got {other:?}"),
    }
}

#[test]
fn point_snap_beats_grid_snap() {
    let mut s = session_off_grid();
    let r = s.add_line(v(0.0, 0.0), v(8.0, 0.0), false).unwrap();
    let p = r.end_point_id;
    s.set_grid_snap(true);

    let preview = s.preview_segment(v(0.0, 0.0), v(8.5, 0.4), false);
    assert_eq!(preview.snap, SnapTarget::Point { entity: p });
    assert_eq!(preview.snapped_to, v(8.0, 0.0));
}

#[test]
fn origin_snap_when_no_point_nearby() {
    let s = session_off_grid();
    let preview = s.preview_segment(v(50.0, 50.0), v(0.6, -0.5), false);
    assert_eq!(preview.snap, SnapTarget::Origin);
    assert_eq!(preview.snapped_to, v(0.0, 0.0));
    assert_eq!(preview.inferences, vec![Inference::Coincident]);
}

#[test]
fn grid_snap_rounds_to_intersection_when_on() {
    let s = session_on_grid();
    let preview = s.preview_segment(v(0.0, 0.0), v(12.0, 9.0), true);
    assert_eq!(preview.snap, SnapTarget::Grid);
    assert_eq!(preview.snapped_to, v(10.0, 10.0));
}

#[test]
fn grid_snap_leaves_free_space_unrounded() {
    let s = session_on_grid();
    let raw = v(2.6, 0.0);
    let preview = s.preview_segment(v(50.0, 50.0), raw, true);
    assert_eq!(preview.snap, SnapTarget::None);
    assert_eq!(preview.snapped_to, raw);
}

#[test]
fn adaptive_grid_step_supports_one_micrometer() {
    let mut s = session_on_grid();
    s.set_grid_step(0.001).unwrap();
    let preview = s.preview_segment(v(20.0, 20.0), v(12.34515, 8.76585), true);
    assert_eq!(preview.snap, SnapTarget::Grid);
    assert!((preview.snapped_to.x - 12.345).abs() < 1e-12);
    assert!((preview.snapped_to.y - 8.766).abs() < 1e-12);
    assert!(s.set_grid_step(0.000_1).is_err());
}

#[test]
fn tracking_aligns_the_commit_without_creating_a_hidden_relation() {
    let mut s = session_off_grid();
    let source = s.add_point(v(10.0, 20.0)).unwrap().entities[0];
    let line = s
        .add_line_locked(&LockedSegmentRequest {
            from: v(0.0, 0.0),
            to_hint: v(35.0, 20.4),
            from_crossing: None,
            to_crossing: None,
            length_mm: None,
            angle_deg: None,
            length_text: None,
            angle_text: None,
            ctrl_held: false,
            tracking: Some(LineTrackingRequest {
                point: source,
                axis: TrackingAxis::Horizontal,
            }),
            intersection: None,
        })
        .unwrap();

    let (_, end) = line_endpoints(&s, line.entity_id);
    assert!((end.y - 20.0).abs() < 1e-9);
    assert!(!s.dto().constraints.iter().any(|constraint| matches!(
        constraint.constraint,
        Constraint::HorizontalPoints { .. } | Constraint::VerticalPoints { .. }
    )));
}

#[test]
fn raw_fallback_when_grid_off_and_nothing_near() {
    let s = session_off_grid();
    let preview = s.preview_segment(v(100.0, 100.0), v(12.3, 8.7), true);
    assert_eq!(preview.snap, SnapTarget::None);
    assert_eq!(preview.snapped_to, v(12.3, 8.7));
}

#[test]
fn horizontal_inference_near_axis_projects_endpoint() {
    let s = session_off_grid();

    let preview = s.preview_segment(v(0.0, 0.0), v(50.0, 1.0), false);
    assert_eq!(preview.inferences, vec![Inference::Horizontal]);
    assert_eq!(preview.snapped_to, v(50.0, 0.0));
}

#[test]
fn deliberate_nine_degree_line_is_not_flattened_by_axis_inference() {
    let s = session_off_grid();
    let nine_degrees = 50.0 * 9.0_f64.to_radians().tan();
    let preview = s.preview_segment(v(0.0, 0.0), v(50.0, nine_degrees), false);
    assert!(preview.inferences.is_empty());
    assert_eq!(preview.snapped_to, v(50.0, nine_degrees));
}

#[test]
fn horizontal_inference_uses_raw_cursor_before_grid_rounding() {
    let s = session_on_grid();

    let preview = s.preview_segment(v(0.0, 15.0), v(30.0, 16.0), false);
    assert_eq!(preview.snap, SnapTarget::None);
    assert_eq!(preview.inferences, vec![Inference::Horizontal]);
    assert_eq!(preview.snapped_to, v(30.0, 15.0));
}

#[test]
fn vertical_inference_near_axis_projects_endpoint() {
    let s = session_off_grid();
    let preview = s.preview_segment(v(0.0, 0.0), v(1.0, 50.0), false);
    assert_eq!(preview.inferences, vec![Inference::Vertical]);
    assert_eq!(preview.snapped_to, v(0.0, 50.0));
}

#[test]
fn no_inference_outside_the_cone() {
    let s = session_off_grid();

    let preview = s.preview_segment(v(0.0, 0.0), v(50.0, 10.0), false);
    assert!(preview.inferences.is_empty());
    assert_eq!(preview.snapped_to, v(50.0, 10.0));
}

#[test]
fn ctrl_disables_inference() {
    let s = session_off_grid();
    let preview = s.preview_segment(v(0.0, 0.0), v(50.0, 1.0), true);
    assert!(preview.inferences.is_empty());
    assert_eq!(preview.snapped_to, v(50.0, 1.0));
}

#[test]
fn ctrl_suppresses_magnetic_point_and_origin_acquisition() {
    let mut s = session_off_grid();
    let point = s.add_point(v(20.0, 20.0)).unwrap().entities[0];

    let near_point = s.preview_segment(v(50.0, 50.0), v(20.4, 19.7), true);
    assert_eq!(near_point.snap, SnapTarget::None);
    assert_eq!(near_point.snapped_to, v(20.4, 19.7));
    assert_ne!(near_point.snap, SnapTarget::Point { entity: point });

    let near_origin = s.preview_segment(v(50.0, 50.0), v(0.4, -0.3), true);
    assert_eq!(near_origin.snap, SnapTarget::None);
    assert_eq!(near_origin.snapped_to, v(0.4, -0.3));
}

#[test]
fn intentional_origin_snap_creates_a_visible_datum_relation_not_fix() {
    let mut s = session_off_grid();
    let line = s.add_line(v(0.4, -0.3), v(24.0, 9.0), false).unwrap();
    assert!(line.created_constraints.iter().any(|created| matches!(
        created.constraint,
        Constraint::OriginCoincident { entity } if entity == line.start_point_id
    )));
    assert!(!line
        .sketch
        .constraints
        .iter()
        .any(|constraint| matches!(constraint.constraint, Constraint::Fix { .. })));
}

#[test]
fn coincident_snap_wins_over_directional_inference() {
    let mut s = session_off_grid();
    let first = s.add_line(v(0.0, 0.0), v(50.0, 0.0), false).unwrap();
    let p = first.end_point_id;

    let preview = s.preview_segment(v(0.0, 0.0), v(50.5, 0.5), false);
    assert_eq!(preview.snap, SnapTarget::Point { entity: p });
    assert_eq!(preview.inferences, vec![Inference::Coincident]);
}

#[test]
fn midpoint_snap_reports_midpoint_target_without_inference() {
    let mut s = session_off_grid();
    s.add_line(v(0.0, 0.0), v(60.0, 0.0), false).unwrap();
    let host = s.dto().entities.iter().find_map(|e| match e {
        EntityDto::Line { id, .. } => Some(*id),
        _ => None,
    });
    let preview = s.preview_segment(v(10.0, 10.0), v(30.5, 0.8), false);
    assert_eq!(
        preview.snap,
        SnapTarget::Midpoint {
            entity: host.unwrap()
        }
    );
    assert_eq!(preview.snapped_to, v(30.0, 0.0));

    assert!(preview.inferences.is_empty());
}

#[test]
fn midpoint_snap_creates_midpoint_constraint_on_commit() {
    let mut s = session_off_grid();
    let l1 = s.add_line(v(0.0, 0.0), v(60.0, 0.0), false).unwrap();
    let r = s.add_line(v(10.0, 10.0), v(30.5, 0.8), false).unwrap();

    let (_, end) = line_endpoints(&s, r.entity_id);
    assert_eq!(end, v(30.0, 0.0));

    let expected = limo_cad_sketch::Constraint::Midpoint {
        a: r.end_point_id,
        b: l1.entity_id,
    };
    assert!(r
        .created_constraints
        .iter()
        .any(|c| c.constraint == expected));
    assert!(s.dto().constraints.iter().any(|c| c.constraint == expected));
}

#[test]
fn midpoint_snap_at_segment_start_creates_constraint() {
    let mut s = session_off_grid();
    let l1 = s.add_line(v(0.0, 0.0), v(60.0, 0.0), false).unwrap();
    let r = s.add_line(v(30.5, 0.8), v(10.0, 10.0), false).unwrap();
    let (start, _) = line_endpoints(&s, r.entity_id);
    assert_eq!(start, v(30.0, 0.0));
    let expected = limo_cad_sketch::Constraint::Midpoint {
        a: r.start_point_id,
        b: l1.entity_id,
    };
    assert!(r
        .created_constraints
        .iter()
        .any(|c| c.constraint == expected));
}

#[test]
fn ctrl_suppresses_midpoint_snap() {
    let mut s = session_off_grid();
    s.add_line(v(0.0, 0.0), v(60.0, 0.0), false).unwrap();
    let preview = s.preview_segment(v(10.0, 10.0), v(30.5, 0.8), true);
    assert_eq!(preview.snap, SnapTarget::None);
    assert_eq!(preview.snapped_to, v(30.5, 0.8));

    let r = s.add_line(v(10.0, 10.0), v(30.5, 0.8), true).unwrap();
    assert!(r.created_constraints.is_empty());
    assert!(!s
        .dto()
        .constraints
        .iter()
        .any(|c| matches!(c.constraint, limo_cad_sketch::Constraint::Midpoint { .. })));
}

#[test]
fn point_snap_beats_midpoint_snap() {
    let mut s = session_off_grid();
    s.add_line(v(0.0, 0.0), v(60.0, 0.0), false).unwrap();
    let p = s.add_point(v(30.0, 0.0)).unwrap().entities[0];
    let preview = s.preview_segment(v(10.0, 10.0), v(30.5, 0.8), false);
    assert_eq!(preview.snap, SnapTarget::Point { entity: p });
    assert_eq!(preview.inferences, vec![Inference::Coincident]);
}

#[test]
fn point_tool_places_an_atomic_coincident_point_on_a_line() {
    let mut s = session_off_grid();
    let carrier = s
        .add_line(v(0.0, 0.0), v(60.0, 0.0), false)
        .unwrap()
        .entity_id;
    let point = s
        .add_point_on(v(31.0, 2.0), Some(carrier))
        .unwrap()
        .entities[0];
    let dto = s.dto();

    let placed = dto
        .entities
        .iter()
        .find_map(|entity| match entity {
            EntityDto::Point { id, position, .. } if *id == point => Some(*position),
            _ => None,
        })
        .unwrap();
    assert!((placed.x - 31.0).abs() < 1e-8);
    assert!(placed.y.abs() < 1e-8);
    assert!(dto.constraints.iter().any(|constraint| matches!(
        constraint.constraint,
        limo_cad_sketch::Constraint::Coincident { a, b }
            if a == point && b == carrier
    )));

    let undone = s.undo().unwrap().sketch;
    assert!(!undone.entities.iter().any(|entity| entity.id() == point));
}

#[test]
fn point_tool_keeps_a_dimensioned_point_on_a_virtual_line_extension() {
    let mut s = session_off_grid();
    let line = s.add_line(v(0.0, 0.0), v(60.0, 0.0), false).unwrap();
    let point = s
        .add_point_on(v(75.0, 2.0), Some(line.entity_id))
        .unwrap()
        .entities[0];

    let placed = s
        .dto()
        .entities
        .iter()
        .find_map(|entity| match entity {
            EntityDto::Point { id, position, .. } if *id == point => Some(*position),
            _ => None,
        })
        .unwrap();
    assert!((placed.x - 75.0).abs() < 1e-8);
    assert!(placed.y.abs() < 1e-8);

    s.toggle_fix(line.end_point_id).unwrap();
    let dimension = s
        .add_dimension(DimensionRequest {
            entities: vec![point, line.end_point_id],
            text_pos: v(75.0, 10.0),
            value_text: None,
        })
        .unwrap();
    let edited = s
        .edit_dimension(EditDimensionRequest {
            constraint_id: dimension.sketch.dimensions[0].constraint_id,
            text: "30".to_string(),
        })
        .unwrap()
        .sketch;
    let moved = edited
        .entities
        .iter()
        .find_map(|entity| match entity {
            EntityDto::Point { id, position, .. } if *id == point => Some(*position),
            _ => None,
        })
        .unwrap();
    assert!(moved.y.abs() < 1e-8);
    assert!((moved.distance(v(60.0, 0.0)) - 30.0).abs() < 1e-8);
    assert!(edited.constraints.iter().any(|constraint| matches!(
        constraint.constraint,
        limo_cad_sketch::Constraint::Coincident { a, b }
            if a == point && b == line.entity_id
    )));
}

#[test]
fn midpoint_snap_respects_point_snap_toggle() {
    let mut s = session_off_grid();
    s.add_line(v(0.0, 0.0), v(60.0, 0.0), false).unwrap();
    s.set_grid_snap(false);
    let preview = s.preview_segment(v(10.0, 10.0), v(30.5, 0.8), false);
    assert_eq!(preview.snap, SnapTarget::None);
}

#[test]
fn chained_lines_share_the_connecting_point() {
    let mut s = session_off_grid();
    let l1 = s.add_line(v(0.0, 0.0), v(50.0, 0.0), false).unwrap();
    let l2 = s.add_line(v(50.0, 0.0), v(50.0, 50.0), false).unwrap();
    assert_eq!(l2.start_point_id, l1.end_point_id);
    assert_ne!(l2.end_point_id, l1.end_point_id);

    assert_eq!(l2.sketch.entities.len(), 5);
}

#[test]
fn snapping_onto_an_existing_point_merges_structurally() {
    let mut s = session_off_grid();
    let l1 = s.add_line(v(0.0, 0.0), v(50.0, 0.0), false).unwrap();

    let l2 = s.add_line(v(60.0, 10.0), v(50.5, 0.4), false).unwrap();
    assert_eq!(l2.end_point_id, l1.end_point_id);

    assert!(l2.created_constraints.is_empty());

    assert_eq!(l2.sketch.entities.len(), 5);
}

#[test]
fn add_line_creates_hv_constraints_from_inference() {
    let mut s = session_off_grid();
    let h = s.add_line(v(0.0, 0.0), v(50.0, 1.0), false).unwrap();
    assert!(h.created_constraints.iter().any(|created| matches!(
        created.constraint,
        Constraint::Horizontal { entity } if entity == h.entity_id
    )));
    assert!(h.created_constraints.iter().any(|created| matches!(
        created.constraint,
        Constraint::OriginCoincident { entity } if entity == h.start_point_id
    )));

    let (_, end) = line_endpoints(&s, h.entity_id);
    assert_eq!(end, v(50.0, 0.0));

    let vert = s.add_line(v(100.0, 0.0), v(100.5, 50.0), false).unwrap();
    assert_eq!(vert.created_constraints.len(), 1);
}

#[test]
fn a_short_line_does_not_snap_its_endpoint_back_to_its_own_start() {
    let mut s = session_on_grid();
    let line = s.add_line(v(0.0, 0.0), v(0.5, 0.0), false).unwrap();
    let (start, end) = line_endpoints(&s, line.entity_id);
    assert_eq!(start, v(0.0, 0.0));
    assert!((end.distance(start) - 0.5).abs() < 1e-9, "end={end:?}");
}

#[test]
fn a_locked_half_millimeter_line_stays_valid_with_point_snap_enabled() {
    for angle_text in [None, Some("-45")] {
        let mut s = session_on_grid();
        let line = s
            .add_line_locked(&LockedSegmentRequest {
                from: v(0.0, 0.0),
                to_hint: v(1.0, -1.0),
                from_crossing: None,
                to_crossing: None,
                length_mm: None,
                angle_deg: None,
                length_text: Some("0.5".to_string()),
                angle_text: angle_text.map(str::to_string),
                ctrl_held: false,
                tracking: None,
                intersection: None,
            })
            .unwrap();
        let (start, end) = line_endpoints(&s, line.entity_id);
        assert!((end.distance(start) - 0.5).abs() < 1e-8, "end={end:?}");
    }
}

#[test]
fn connected_right_angles_prefer_relational_perpendicular_constraints() {
    let mut s = session_off_grid();
    let left = s.add_line(v(0.0, 0.0), v(0.0, 10.0), false).unwrap();
    let top = s.add_line(v(0.0, 10.0), v(17.0, 10.0), false).unwrap();
    let diagonal = s
        .add_line_locked(&LockedSegmentRequest {
            from: v(17.0, 10.0),
            to_hint: v(27.0, 0.0),
            from_crossing: None,
            to_crossing: None,
            length_mm: None,
            angle_deg: None,
            length_text: None,
            angle_text: Some("-45".to_string()),
            ctrl_held: false,
            tracking: None,
            intersection: None,
        })
        .unwrap();
    let bottom = s.add_line(v(27.0, 0.0), v(0.0, 0.0), false).unwrap();

    let dto = s.dto();
    let has_perpendicular = |other| {
        dto.constraints.iter().any(|constraint| {
            matches!(
                constraint.constraint,
                Constraint::Perpendicular { a, b }
                    if (a == left.entity_id && b == other)
                        || (a == other && b == left.entity_id)
            )
        })
    };
    assert!(has_perpendicular(top.entity_id));
    assert!(has_perpendicular(bottom.entity_id));
    assert!(!dto.constraints.iter().any(|constraint| matches!(
        constraint.constraint,
        Constraint::Horizontal { entity }
            if entity == top.entity_id || entity == bottom.entity_id
    )));
    assert!(dto.constraints.iter().any(|constraint| matches!(
        constraint.constraint,
        Constraint::Angle { a, b, value }
            if a == diagonal.entity_id && b.0 == 0 && (value + 45.0).abs() < 1e-7
    )));
}

#[test]
fn connected_perpendicular_is_previewed_and_the_override_suppresses_it() {
    let mut s = session_off_grid();
    let base = s.add_line(v(10.0, 10.0), v(40.0, 10.0), true).unwrap();

    let preview = s.preview_segment(v(40.0, 10.0), v(40.6, 40.0), false);
    assert!(preview.inferences.contains(&Inference::Perpendicular));
    let committed = s.add_line(v(40.0, 10.0), v(40.6, 40.0), false).unwrap();
    assert!(committed.created_constraints.iter().any(|created| matches!(
        created.constraint,
        Constraint::Perpendicular { a, b }
            if (a == base.entity_id && b == committed.entity_id)
                || (a == committed.entity_id && b == base.entity_id)
    )));
    assert!(
        !committed.created_constraints.iter().any(|created| matches!(
            created.constraint,
            Constraint::Vertical { entity } if entity == committed.entity_id
        ))
    );

    let suppressed = s.preview_segment(v(40.0, 10.0), v(40.6, -20.0), true);
    assert!(!suppressed.inferences.contains(&Inference::Perpendicular));
}

#[test]
fn center_acquisition_is_associative_and_never_silently_fixes_a_curve() {
    let mut s = session_off_grid();
    let center = s.add_point(v(20.0, 20.0)).unwrap().entities[0];
    let circle = s
        .add_circle_selective(
            limo_cad_sketch::CircleMode::CenterDiameter,
            v(20.5, 19.6),
            v(32.0, 20.0),
            false,
        )
        .unwrap();
    let circle_id = circle.entities[0];
    assert!(circle.sketch.constraints.iter().any(|constraint| matches!(
        constraint.constraint,
        Constraint::CenterCoincident { point, curve }
            if point == center && curve == circle_id
    )));
    assert!(!circle
        .sketch
        .constraints
        .iter()
        .any(|constraint| matches!(constraint.constraint, Constraint::Fix { entity } if entity == circle_id)));

    let origin_circle = s
        .add_circle_selective(
            limo_cad_sketch::CircleMode::CenterDiameter,
            v(0.3, -0.2),
            v(8.0, 0.0),
            false,
        )
        .unwrap();
    assert!(origin_circle
        .sketch
        .constraints
        .iter()
        .any(|constraint| matches!(
            constraint.constraint,
            Constraint::OriginCoincident { entity } if entity == origin_circle.entities[0]
        )));
}

#[test]
fn explicit_slot_center_datum_keeps_width_editable_and_rejects_invalid_targets() {
    let mut s = session_off_grid();
    let slot = s
        .add_slot(&limo_cad_sketch::SlotRequest {
            ctrl_held: false,
            mode: limo_cad_sketch::SlotMode::CenterToCenter,
            p1: v(0.0, -50.0),
            p2: v(0.0, 50.0),
            cursor: v(9.0, 0.0),
            width_mm: Some(18.0),
            width_text: None,
        })
        .unwrap();
    let line = slot.entities[4];
    let arc = slot.entities[6];
    let width = slot.sketch.dimensions[0].constraint_id;
    s.add_dimension(DimensionRequest {
        entities: vec![line],
        text_pos: v(-30.0, 0.0),
        value_text: Some("100".into()),
    })
    .unwrap();
    s.add_constraint(Constraint::Vertical { entity: line })
        .unwrap();
    let datum = s.add_point(v(0.0, -50.0)).unwrap().entities[0];
    s.add_constraint(Constraint::Fix { entity: datum }).unwrap();
    s.add_constraint(Constraint::CenterCoincident {
        point: datum,
        curve: arc,
    })
    .unwrap();
    assert_eq!(s.dto().dof.value, 0);
    let before = serde_json::to_value(s.dto()).unwrap();
    assert!(s
        .add_constraint(Constraint::CenterCoincident {
            point: datum,
            curve: line
        })
        .is_err());
    assert!(s
        .add_constraint(Constraint::CenterCoincident {
            point: arc,
            curve: arc
        })
        .is_err());
    assert_eq!(serde_json::to_value(s.dto()).unwrap(), before);
    let edited = s
        .edit_dimension(EditDimensionRequest {
            constraint_id: width,
            text: "20".into(),
        })
        .unwrap()
        .sketch;
    assert_eq!(edited.dof.value, 0);
    for entity in &edited.entities {
        if let EntityDto::Arc {
            id, center, radius, ..
        } = entity
        {
            assert!((radius - 10.0).abs() < 1e-6);
            if *id == arc {
                assert!(center.distance(v(0.0, -50.0)) < 1e-6);
            }
        }
    }
}

#[test]
fn arc_endpoint_tangency_is_selective_associative_and_suppressible() {
    let mut inferred = session_off_grid();
    let carrier = inferred.add_line(v(0.0, 0.0), v(10.0, 0.0), true).unwrap();
    let arc = inferred
        .add_arc_3pt_selective(v(10.0, 0.0), v(15.0, 5.0), v(10.0, 10.0), false)
        .unwrap();
    let arc_id = arc.entities[0];
    assert!(arc.sketch.constraints.iter().any(|constraint| matches!(
        constraint.constraint,
        Constraint::ArcEndpointCoincident { point, arc, .. }
            if point == carrier.end_point_id && arc == arc_id
    )));
    assert!(
        arc.sketch.constraints.iter().any(|constraint| matches!(
            constraint.constraint,
            Constraint::Tangent { a, b }
                if (a == carrier.entity_id && b == arc_id)
                    || (a == arc_id && b == carrier.entity_id)
        )),
        "constraints={:?}, entities={:?}",
        arc.sketch.constraints,
        arc.sketch.entities
    );

    let mut suppressed = session_off_grid();
    let carrier = suppressed
        .add_line(v(0.0, 0.0), v(10.0, 0.0), true)
        .unwrap();
    let arc = suppressed
        .add_arc_3pt_selective(v(10.0, 0.0), v(15.0, 5.0), v(10.0, 10.0), true)
        .unwrap();
    assert!(!arc.sketch.constraints.iter().any(|constraint| matches!(
        constraint.constraint,
        Constraint::ArcEndpointCoincident { point, arc: constrained_arc, .. }
            if point == carrier.end_point_id && constrained_arc == arc.entities[0]
    )));
    assert!(!arc.sketch.constraints.iter().any(|constraint| matches!(
        constraint.constraint,
        Constraint::Tangent { a, b }
            if a == arc.entities[0] || b == arc.entities[0]
    )));
}

#[test]
fn add_line_with_ctrl_creates_no_constraints() {
    let mut s = session_off_grid();
    let r = s.add_line(v(0.0, 0.0), v(50.0, 1.0), true).unwrap();
    assert!(r.created_constraints.is_empty());
    assert_eq!(r.sketch.constraints.len(), 0);
}

#[test]
fn degenerate_segments_are_rejected_without_mutating() {
    let mut s = session_off_grid();
    let first = s.add_line(v(10.0, 10.0), v(30.0, 10.0), false).unwrap();
    let before = first.sketch.entities.len();

    let err = s.add_line(v(10.0, 10.0), v(10.0, 10.0), false).unwrap_err();
    assert!(err.to_string().contains("zero length"));
    assert_eq!(s.dto().entities.len(), before);
}

fn move_req(point_id: limo_cad_sketch::EntityId, to: Vec2, phase: DragPhase) -> MovePointRequest {
    MovePointRequest {
        point_id,
        to_raw: to,
        ctrl_held: false,
        phase,
    }
}

#[test]
fn dragging_a_horizontal_line_endpoint_translates_the_line_keeping_h() {
    let mut s = session_off_grid();
    let l = s.add_line(v(10.0, 10.0), v(60.0, 11.0), false).unwrap();
    let r = s
        .move_point(move_req(l.end_point_id, v(80.0, 30.0), DragPhase::Single))
        .unwrap();
    let (start, end) = line_endpoints_dto(&r.sketch, l.entity_id);
    assert_eq!(end, v(80.0, 30.0));
    assert!((start.y - end.y).abs() < 1e-9, "H must hold after drag");
}

#[test]
fn dragging_a_vertical_line_endpoint_translates_the_line_keeping_v() {
    let mut s = session_off_grid();
    let l = s.add_line(v(10.0, 10.0), v(11.0, 60.0), false).unwrap();
    let r = s
        .move_point(move_req(l.end_point_id, v(30.0, 80.0), DragPhase::Single))
        .unwrap();
    let (start, end) = line_endpoints_dto(&r.sketch, l.entity_id);
    assert_eq!(end, v(30.0, 80.0));
    assert!((start.x - end.x).abs() < 1e-9, "V must hold after drag");
}

#[test]
fn unconstrained_points_drag_freely() {
    let mut s = session_off_grid();
    let l = s.add_line(v(0.0, 0.0), v(50.0, 30.0), true).unwrap();
    let r = s
        .move_point(move_req(l.end_point_id, v(61.0, 42.0), DragPhase::Single))
        .unwrap();
    let (_, end) = line_endpoints_dto(&r.sketch, l.entity_id);
    assert_eq!(end, v(61.0, 42.0));
}

#[test]
fn moving_a_shared_point_moves_both_connected_lines() {
    let mut s = session_off_grid();
    let l1 = s.add_line(v(0.0, 0.0), v(50.0, 30.0), true).unwrap();
    let l2 = s.add_line(v(50.0, 30.0), v(90.0, 10.0), true).unwrap();
    let shared = l1.end_point_id;
    let r = s
        .move_point(move_req(shared, v(55.0, 35.0), DragPhase::Single))
        .unwrap();
    let (_, end1) = line_endpoints_dto(&r.sketch, l1.entity_id);
    let (start2, _) = line_endpoints_dto(&r.sketch, l2.entity_id);
    assert_eq!(end1, v(55.0, 35.0));
    assert_eq!(start2, v(55.0, 35.0));
}

fn line_endpoints_dto(
    dto: &limo_cad_sketch::SketchDto,
    id: limo_cad_sketch::EntityId,
) -> (Vec2, Vec2) {
    match dto.entities.iter().find(|e| e.id() == id) {
        Some(EntityDto::Line { start, end, .. }) => (*start, *end),
        other => panic!("expected line, got {other:?}"),
    }
}

#[test]
fn a_rubber_band_drag_is_one_undoable_command() {
    let mut s = session_off_grid();
    let l = s.add_line(v(0.0, 0.0), v(50.0, 30.0), true).unwrap();
    let p = l.end_point_id;
    s.move_point(move_req(p, v(51.0, 31.0), DragPhase::Begin))
        .unwrap();
    s.move_point(move_req(p, v(55.0, 33.0), DragPhase::Update))
        .unwrap();
    s.move_point(move_req(p, v(60.0, 40.0), DragPhase::End))
        .unwrap();

    let before_undo = s.undo().unwrap();

    let (_, end) = line_endpoints_dto(&before_undo.sketch, l.entity_id);
    assert_eq!(end, v(50.0, 30.0));
    let redone = s.redo().unwrap();
    let (_, end) = line_endpoints_dto(&redone.sketch, l.entity_id);
    assert_eq!(end, v(60.0, 40.0));
}

#[test]
fn deleting_a_point_deletes_connected_lines_and_constraints() {
    let mut s = session_off_grid();
    let l1 = s.add_line(v(0.0, 0.0), v(50.0, 1.0), false).unwrap();
    let l2 = s.add_line(v(50.0, 0.0), v(90.0, 30.0), true).unwrap();
    let shared = l1.end_point_id;

    let r = s.delete_entity(shared).unwrap();
    assert!(r.removed.contains(&shared));
    assert!(r.removed.contains(&l1.entity_id));
    assert!(r.removed.contains(&l2.entity_id));
    assert_eq!(r.sketch.constraints.len(), 1);
    assert!(matches!(
        r.sketch.constraints[0].constraint,
        Constraint::OriginCoincident { entity } if entity == l1.start_point_id
    ));

    assert_eq!(r.sketch.entities.len(), 1);
}

#[test]
fn deleting_a_line_keeps_only_its_constrained_points() {
    let mut s = session_off_grid();
    let l = s.add_line(v(0.0, 0.0), v(50.0, 0.0), false).unwrap();
    let r = s.delete_entity(l.entity_id).unwrap();
    assert_eq!(r.removed, vec![l.end_point_id, l.entity_id]);
    assert_eq!(r.sketch.entities.len(), 1);
}

#[test]
fn undo_redo_add_line_roundtrip_with_stable_ids() {
    let mut s = session_off_grid();
    let l = s.add_line(v(0.0, 0.0), v(50.0, 0.0), false).unwrap();
    assert!(l.sketch.can_undo);
    assert!(!l.sketch.can_redo);

    let undone = s.undo().unwrap();
    assert_eq!(undone.sketch.entities.len(), 0);
    assert!(!undone.sketch.can_undo);
    assert!(undone.sketch.can_redo);

    let redone = s.redo().unwrap();
    assert_eq!(redone.sketch.entities.len(), 3);
    assert!(redone.sketch.entities.iter().any(|e| e.id() == l.entity_id));
}

#[test]
fn delete_is_undoable_with_full_cascade_restore() {
    let mut s = session_off_grid();
    let l1 = s.add_line(v(0.0, 0.0), v(50.0, 1.0), false).unwrap();
    s.add_line(v(50.0, 0.0), v(90.0, 30.0), true).unwrap();
    s.delete_entity(l1.end_point_id).unwrap();
    assert_eq!(s.dto().entities.len(), 1);

    let restored = s.undo().unwrap();
    assert_eq!(restored.sketch.entities.len(), 5);
    assert_eq!(restored.sketch.constraints.len(), 2);
    assert!(restored
        .sketch
        .constraints
        .iter()
        .any(|constraint| matches!(
            constraint.constraint,
            Constraint::OriginCoincident { entity } if entity == l1.start_point_id
        )));
}

#[test]
fn a_new_mutation_clears_the_redo_stack() {
    let mut s = session_off_grid();
    s.add_line(v(0.0, 0.0), v(50.0, 0.0), false).unwrap();
    s.undo().unwrap();
    assert!(s.dto().can_redo);
    let r = s.add_line(v(0.0, 0.0), v(0.0, 50.0), false).unwrap();
    assert!(!r.sketch.can_redo);
    assert!(s.redo().is_err());
}

#[test]
fn begin_sketch_names_and_registers_in_browser_tree() {
    let mut m = SketchManager::new();
    let dto = m.begin_sketch(XY).unwrap();
    assert_eq!(dto.name, "Sketch1");
    let doc = m.document_dto();
    let sketches = doc
        .browser
        .iter()
        .find(|n| n.kind == limo_cad_core::BrowserNodeKind::SketchesFolder)
        .unwrap();
    assert_eq!(sketches.children.len(), 1);
    assert_eq!(sketches.children[0].name.as_deref(), Some("Sketch1"));

    m.end_sketch().unwrap();
    let dto2 = m.begin_sketch(XY).unwrap();
    assert_eq!(dto2.name, "Sketch2");
    let doc = m.document_dto();
    let sketches = doc
        .browser
        .iter()
        .find(|n| n.kind == limo_cad_core::BrowserNodeKind::SketchesFolder)
        .unwrap();
    assert_eq!(sketches.children.len(), 2);
}

#[test]
fn lifecycle_errors_are_explicit() {
    let mut m = SketchManager::new();
    assert!(m
        .preview_segment(seg(v(0.0, 0.0), v(1.0, 1.0), false))
        .is_err());
    assert!(m.end_sketch().is_err());
    m.begin_sketch(XY).unwrap();
    assert!(m.begin_sketch(XY).is_err());
    assert!(m.active_snapshot().is_some());
    m.end_sketch().unwrap();
    assert!(m.active_snapshot().is_none());
}

#[test]
fn edit_sketch_round_trip_preserves_session_and_undo() {
    let mut m = SketchManager::new();
    m.begin_sketch(XY).unwrap();
    m.add_line(seg(v(0.0, 0.0), v(50.0, 0.0), false)).unwrap();
    m.add_line(seg(v(50.0, 0.0), v(50.0, 50.0), false)).unwrap();
    m.end_sketch().unwrap();

    let finished = m.finished_sketches();
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0].name, "Sketch1");
    assert_eq!(finished[0].entities.len(), 5);

    let dto = m.edit_sketch("Sketch1").unwrap();
    assert_eq!(dto.entities.len(), 5);
    assert!(dto.can_undo);
    assert!(m.finished_sketches().is_empty());

    m.undo().unwrap();
    assert_eq!(m.active_snapshot().unwrap().entities.len(), 3);
    m.end_sketch().unwrap();
    assert_eq!(m.finished_sketches().len(), 1);

    assert!(m.edit_sketch("Nope").is_err());
    m.begin_sketch(XY).unwrap();
    assert!(m.edit_sketch("Sketch1").is_err());
}

#[test]
fn unsupported_plane_kinds_are_rejected() {
    let mut m = SketchManager::new();
    let face = PlaneRef::PlanarFace {
        face_id: limo_cad_sketch::FaceId(1),
    };
    assert!(m.begin_sketch(face).is_err());
}

#[test]
fn grid_snap_preference_applies_to_sessions() {
    let mut m = SketchManager::new();
    m.begin_sketch(XY).unwrap();

    let p = m
        .preview_segment(seg(v(0.0, 0.0), v(12.0, 9.0), true))
        .unwrap();
    assert_eq!(p.snapped_to, v(10.0, 10.0));
    m.set_grid_snap(limo_cad_sketch::SetGridSnapRequest { enabled: false })
        .unwrap();
    let p = m
        .preview_segment(seg(v(0.0, 0.0), v(12.0, 9.0), true))
        .unwrap();
    assert_eq!(p.snapped_to, v(12.0, 9.0));
}

#[test]
fn adaptive_grid_preference_applies_to_new_sessions() {
    let mut m = SketchManager::new();
    m.set_grid_step(limo_cad_sketch::SetGridStepRequest { step_mm: 0.001 })
        .unwrap();
    m.begin_sketch(XY).unwrap();
    let p = m
        .preview_segment(seg(v(20.0, 20.0), v(12.34515, 8.76585), true))
        .unwrap();
    assert!((p.snapped_to.x - 12.345).abs() < 1e-12);
    assert!((p.snapped_to.y - 8.766).abs() < 1e-12);
}

#[test]
fn host_envelope_ok_and_error_shapes() {
    let mut m = SketchManager::new();
    let doc = host::handle(&mut m, "document", "");
    let v_doc: serde_json::Value = serde_json::from_str(&doc).unwrap();
    assert_eq!(v_doc["ok"], true);
    assert_eq!(v_doc["value"]["settings"]["units"], "mm");

    let bad = host::handle(&mut m, "begin_sketch", "{not json");
    let v_bad: serde_json::Value = serde_json::from_str(&bad).unwrap();
    assert_eq!(v_bad["ok"], false);
    assert!(v_bad["error"].as_str().unwrap().contains("bad request"));

    let unknown = host::handle(&mut m, "explode", "");
    let v_unknown: serde_json::Value = serde_json::from_str(&unknown).unwrap();
    assert_eq!(v_unknown["ok"], false);
    assert!(v_unknown["error"]
        .as_str()
        .unwrap()
        .contains("unknown engine method"));
}

#[test]
fn host_redundant_constraint_envelope_distinguishes_dependency_from_conflict() {
    let mut manager = SketchManager::new();
    let begun = host::handle(
        &mut manager,
        "begin_sketch",
        r#"{"type":"origin_plane","plane":"xy"}"#,
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&begun).unwrap()["ok"],
        true
    );

    let add_line = |manager: &mut SketchManager, from: Vec2, to: Vec2| {
        let payload = serde_json::json!({
            "from": from,
            "to_raw": to,
            "ctrl_held": true,
        })
        .to_string();
        let result = host::handle(manager, "add_line", &payload);
        let envelope: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(envelope["ok"], true, "{envelope}");
        envelope["value"]["entity_id"].as_u64().unwrap()
    };

    let bottom = add_line(&mut manager, v(0.0, 0.0), v(40.0, 0.0));
    let right = add_line(&mut manager, v(40.0, 0.0), v(40.0, 25.0));
    let top = add_line(&mut manager, v(0.0, 25.0), v(40.0, 25.0));
    for payload in [
        serde_json::json!({ "type": "parallel", "a": bottom, "b": top }),
        serde_json::json!({ "type": "perpendicular", "a": bottom, "b": right }),
    ] {
        let result = host::handle(&mut manager, "add_constraint", &payload.to_string());
        let envelope: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(envelope["ok"], true, "{envelope}");
    }

    let redundant = host::handle(
        &mut manager,
        "add_constraint",
        &serde_json::json!({ "type": "perpendicular", "a": top, "b": right }).to_string(),
    );
    let envelope: serde_json::Value = serde_json::from_str(&redundant).unwrap();
    assert_eq!(envelope["ok"], false);
    assert_eq!(envelope["data"]["reason"], "redundant");
    let dependencies = envelope["data"]["conflicts_with"].as_array().unwrap();
    assert!(dependencies.iter().any(|item| item["kind"] == "parallel"));
    assert!(dependencies
        .iter()
        .any(|item| item["kind"] == "perpendicular"));
}

#[test]
fn host_roundtrip_begin_add_undo() {
    let mut m = SketchManager::new();
    let r = host::handle(
        &mut m,
        "begin_sketch",
        r#"{"type":"origin_plane","plane":"xy"}"#,
    );
    let v: serde_json::Value = serde_json::from_str(&r).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["value"]["name"], "Sketch1");
    assert_eq!(
        v["value"]["basis"]["normal"],
        serde_json::json!([0.0, 0.0, 1.0])
    );

    let r = host::handle(
        &mut m,
        "add_line",
        r#"{"from":{"x":0.0,"y":0.0},"to_raw":{"x":50.0,"y":1.0},"ctrl_held":false}"#,
    );
    let v: serde_json::Value = serde_json::from_str(&r).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["value"]["created_constraints"][0]["type"], "horizontal");
    assert_eq!(v["value"]["sketch"]["can_undo"], true);

    let r = host::handle(&mut m, "undo", "");
    let v: serde_json::Value = serde_json::from_str(&r).unwrap();
    assert_eq!(
        v["value"]["sketch"]["entities"].as_array().unwrap().len(),
        0
    );
}
