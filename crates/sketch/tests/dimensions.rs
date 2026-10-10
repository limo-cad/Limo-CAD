//! Dimension tests (D9): dimensional solver equations + DOF, parametric
//! re-solve on edit, conflict rejection, auto-dimension on typed input,
//! formula-driven dimensions, lock/snap composition.

use limo_cad_core::EdgeId;
use limo_cad_sketch::{
    Constraint, DimensionMode, DimensionRequest, EditDimensionRequest, LockedRectangleRequest,
    LockedSegmentRequest, MoveDimensionRequest, OriginPlane, PlaneRef, RectangleMode,
    SetDimensionModeRequest, SketchSession, Vec2,
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

fn locked_seg_text(
    from: Vec2,
    to_hint: Vec2,
    length_text: Option<&str>,
    angle_text: Option<&str>,
) -> LockedSegmentRequest {
    LockedSegmentRequest {
        from,
        to_hint,
        from_crossing: None,
        to_crossing: None,
        length_mm: None,
        angle_deg: None,
        length_text: length_text.map(|t| t.to_string()),
        angle_text: angle_text.map(|t| t.to_string()),
        ctrl_held: false,
        tracking: None,
        intersection: None,
    }
}

fn line(dto: &limo_cad_sketch::SketchDto, id: limo_cad_sketch::EntityId) -> (Vec2, Vec2) {
    match dto.entities.iter().find(|e| e.id() == id) {
        Some(limo_cad_sketch::EntityDto::Line { start, end, .. }) => (*start, *end),
        other => panic!("expected line, got {other:?}"),
    }
}

fn close(a: Vec2, b: Vec2) -> bool {
    a.distance(b) < 1e-7
}

fn assert_same_bearing(before: Vec2, after: Vec2, context: &str) {
    let scale = before.length() * after.length();
    let cross = before.x * after.y - before.y * after.x;
    let dot = before.dot(after);
    assert!(
        cross.abs() < 1e-7 * scale.max(1.0) && dot > 0.0,
        "{context} changed bearing from {before:?} to {after:?}"
    );
}

fn point(dto: &limo_cad_sketch::SketchDto, id: limo_cad_sketch::EntityId) -> Vec2 {
    match dto.entities.iter().find(|entity| entity.id() == id) {
        Some(limo_cad_sketch::EntityDto::Point { position, .. }) => *position,
        other => panic!("expected point, got {other:?}"),
    }
}

#[test]
fn support_edge_midpoint_remains_exact_through_dimension_edits_and_history() {
    let mut s = session();
    let edge = EdgeId(77);
    s.set_reference_midpoints(vec![(edge, v(10.0, 0.0))]);

    let line = s.add_line(v(10.2, 0.2), v(20.0, 0.0), false).unwrap();
    assert!(close(
        point(&line.sketch, line.start_point_id),
        v(10.0, 0.0)
    ));
    assert!(s.sketch().constraints().any(|(_, constraint)| matches!(
        constraint,
        Constraint::ReferenceMidpoint {
            point,
            edge: constrained_edge,
            position,
        } if *point == line.start_point_id
            && *constrained_edge == edge
            && close(*position, v(10.0, 0.0))
    )));

    let dimension = s
        .add_dimension(DimensionRequest {
            entities: vec![line.entity_id],
            text_pos: v(15.0, 5.0),
            value_text: None,
        })
        .unwrap();
    let edited = s
        .edit_dimension(EditDimensionRequest {
            constraint_id: dimension.sketch.dimensions[0].constraint_id,
            text: "15".to_string(),
        })
        .unwrap()
        .sketch;
    assert!(close(point(&edited, line.start_point_id), v(10.0, 0.0)));
    assert!(close(point(&edited, line.end_point_id), v(25.0, 0.0)));

    s.set_reference_midpoints(vec![(edge, v(12.0, 3.0))]);
    assert!(close(point(&s.dto(), line.start_point_id), v(12.0, 3.0)));
    let undone = s.undo().unwrap().sketch;
    assert!(close(point(&undone, line.start_point_id), v(12.0, 3.0)));
    let redone = s.redo().unwrap().sketch;
    assert!(close(point(&redone, line.start_point_id), v(12.0, 3.0)));
}

#[test]
fn distance_dim_drives_line_length_and_counts_dof() {
    let mut s = session();
    let l = s.add_line(v(0.0, 0.0), v(40.0, 0.0), true).unwrap();
    assert_eq!(l.sketch.dof.value, 4);
    let d = s
        .add_dimension(DimensionRequest {
            entities: vec![l.entity_id],
            text_pos: v(20.0, 15.0),
            value_text: None,
        })
        .unwrap();
    let dto = d.sketch;
    assert_eq!(dto.dof.value, 3, "one length dim removes one DOF");
    assert_eq!(dto.dimensions.len(), 1);
    assert_eq!(dto.dimensions[0].param_name.as_deref(), Some("d1"));
    assert_eq!(dto.dimensions[0].text, "40.00");

    s.toggle_fix(l.start_point_id).unwrap();
    let cid = dto.dimensions[0].constraint_id;
    let r = s
        .edit_dimension(EditDimensionRequest {
            constraint_id: cid,
            text: "65".to_string(),
        })
        .unwrap();
    let (_, end) = line(&r.sketch, l.entity_id);
    assert!(close(end, v(65.0, 0.0)), "end={end:?}");
    let stored = r
        .sketch
        .constraints
        .iter()
        .find(|constraint| constraint.id == cid)
        .expect("dimension constraint");
    assert!(matches!(
        stored.constraint,
        Constraint::Distance { value, .. } if (value - 65.0).abs() < 1e-9
    ));
}

#[test]
fn editing_length_changes_only_length_not_line_bearing() {
    let mut s = session();
    let line_result = s.add_line(v(4.0, 7.0), v(39.0, 28.0), true).unwrap();
    let before = s.dto();
    let (before_start, before_end) = line(&before, line_result.entity_id);
    let before_direction = before_end - before_start;
    let dimension = s
        .add_dimension(DimensionRequest {
            entities: vec![line_result.entity_id],
            text_pos: v(20.0, 30.0),
            value_text: None,
        })
        .unwrap();
    let edited = s
        .edit_dimension(EditDimensionRequest {
            constraint_id: dimension.sketch.dimensions[0].constraint_id,
            text: "70".to_string(),
        })
        .unwrap()
        .sketch;
    let (after_start, after_end) = line(&edited, line_result.entity_id);
    let after_direction = after_end - after_start;
    assert!((after_direction.length() - 70.0).abs() < 1e-7);
    assert_same_bearing(before_direction, after_direction, "length edit");
}

#[test]
fn editing_point_distance_changes_only_spacing_not_bearing() {
    let mut s = session();
    let first = s.add_point(v(3.0, 5.0)).unwrap().entities[0];
    let second = s.add_point(v(24.0, 32.0)).unwrap().entities[0];
    let before_direction = point(&s.dto(), second) - point(&s.dto(), first);
    let dimension = s
        .add_dimension(DimensionRequest {
            entities: vec![first, second],
            text_pos: v(15.0, 25.0),
            value_text: None,
        })
        .unwrap();
    let edited = s
        .edit_dimension(EditDimensionRequest {
            constraint_id: dimension.sketch.dimensions[0].constraint_id,
            text: "55".to_string(),
        })
        .unwrap()
        .sketch;
    let after_direction = point(&edited, second) - point(&edited, first);
    assert!((after_direction.length() - 55.0).abs() < 1e-7);
    assert_same_bearing(before_direction, after_direction, "point-distance edit");
}

#[test]
fn editing_angle_changes_only_angle_not_either_line_length() {
    let mut s = session();
    let first = s.add_line(v(2.0, 5.0), v(43.0, 18.0), true).unwrap();
    let second = s.add_line(v(13.0, 31.0), v(28.0, 70.0), true).unwrap();
    let before = s.dto();
    let (first_start, first_end) = line(&before, first.entity_id);
    let (second_start, second_end) = line(&before, second.entity_id);
    let first_length = first_start.distance(first_end);
    let second_length = second_start.distance(second_end);
    let dimension = s
        .add_dimension(DimensionRequest {
            entities: vec![first.entity_id, second.entity_id],
            text_pos: v(25.0, 25.0),
            value_text: None,
        })
        .unwrap();
    let edited = s
        .edit_dimension(EditDimensionRequest {
            constraint_id: dimension.sketch.dimensions[0].constraint_id,
            text: "60".to_string(),
        })
        .unwrap()
        .sketch;
    let (first_start, first_end) = line(&edited, first.entity_id);
    let (second_start, second_end) = line(&edited, second.entity_id);
    assert!((first_start.distance(first_end) - first_length).abs() < 1e-7);
    assert!((second_start.distance(second_end) - second_length).abs() < 1e-7);
}

#[test]
fn editing_line_offset_changes_only_separation_not_carrier_shapes() {
    let mut s = session();
    let first = s.add_line(v(0.0, 0.0), v(36.0, 12.0), true).unwrap();
    let second = s.add_line(v(8.0, 24.0), v(44.0, 36.0), true).unwrap();
    let before = s.dto();
    let (first_start, first_end) = line(&before, first.entity_id);
    let (second_start, second_end) = line(&before, second.entity_id);
    let first_direction = first_end - first_start;
    let second_direction = second_end - second_start;
    let dimension = s
        .add_dimension(DimensionRequest {
            entities: vec![first.entity_id, second.entity_id],
            text_pos: v(20.0, 20.0),
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
    let (first_start2, first_end2) = line(&edited, first.entity_id);
    let (second_start2, second_end2) = line(&edited, second.entity_id);
    assert!((first_start2.distance(first_end2) - first_direction.length()).abs() < 1e-7);
    assert!((second_start2.distance(second_end2) - second_direction.length()).abs() < 1e-7);
    assert_same_bearing(
        first_direction,
        first_end2 - first_start2,
        "first offset carrier",
    );
    assert_same_bearing(
        second_direction,
        second_end2 - second_start2,
        "second offset carrier",
    );
}

#[test]
fn editing_point_line_distance_does_not_rotate_or_resize_the_carrier() {
    let mut s = session();
    let carrier = s.add_line(v(2.0, 4.0), v(45.0, 19.0), true).unwrap();
    let marker = s.add_point(v(17.0, 38.0)).unwrap().entities[0];
    let before = s.dto();
    let (before_start, before_end) = line(&before, carrier.entity_id);
    let before_direction = before_end - before_start;
    let dimension = s
        .add_dimension(DimensionRequest {
            entities: vec![marker, carrier.entity_id],
            text_pos: v(25.0, 30.0),
            value_text: None,
        })
        .unwrap();
    let edited = s
        .edit_dimension(EditDimensionRequest {
            constraint_id: dimension.sketch.dimensions[0].constraint_id,
            text: "24".to_string(),
        })
        .unwrap()
        .sketch;
    let (after_start, after_end) = line(&edited, carrier.entity_id);
    let after_direction = after_end - after_start;
    assert!((after_direction.length() - before_direction.length()).abs() < 1e-7);
    assert_same_bearing(before_direction, after_direction, "point-line carrier");
}

#[test]
fn distance_from_a_free_point_to_the_origin_datum_can_be_edited() {
    let mut s = SketchSession::new("Sketch1", XY, XY.basis().unwrap(), true);
    let origin = s.add_point(v(0.0, 0.0)).unwrap().entities[0];
    let movable = s.add_point(v(10.0, 0.0)).unwrap().entities[0];
    let dimension = s
        .add_dimension(DimensionRequest {
            entities: vec![movable, origin],
            text_pos: v(5.0, 5.0),
            value_text: None,
        })
        .unwrap();
    let edited = s
        .edit_dimension(EditDimensionRequest {
            constraint_id: dimension.sketch.dimensions[0].constraint_id,
            text: "8".to_string(),
        })
        .unwrap()
        .sketch;

    assert!(close(point(&edited, origin), Vec2::ZERO));
    assert!((point(&edited, movable).distance(Vec2::ZERO) - 8.0).abs() < 1e-7);
}

#[test]
fn point_on_line_distance_to_the_origin_datum_can_be_edited() {
    let mut s = SketchSession::new("Sketch1", XY, XY.basis().unwrap(), true);
    let carrier = s.add_line(v(0.0, 0.0), v(30.0, 0.0), false).unwrap();
    let movable = s.add_point(v(10.0, 0.0)).unwrap().entities[0];
    s.add_constraint(Constraint::Coincident {
        a: movable,
        b: carrier.entity_id,
    })
    .unwrap();
    let dimension = s
        .add_dimension(DimensionRequest {
            entities: vec![movable, carrier.start_point_id],
            text_pos: v(5.0, 5.0),
            value_text: None,
        })
        .unwrap();
    let edited = s
        .edit_dimension(EditDimensionRequest {
            constraint_id: dimension.sketch.dimensions[0].constraint_id,
            text: "8".to_string(),
        })
        .unwrap()
        .sketch;

    assert!(close(point(&edited, carrier.start_point_id), Vec2::ZERO));
    assert!(close(point(&edited, movable), v(8.0, 0.0)));
}

#[test]
fn origin_anchored_chain_moves_attached_rectangle_outward_on_dimension_edit() {
    let mut s = SketchSession::new("Sketch1", XY, XY.basis().unwrap(), true);
    let first = s
        .add_line_locked(&locked_seg_text(
            v(0.0, 0.0),
            v(15.0, 0.0),
            Some("15"),
            None,
        ))
        .unwrap();
    let second = s
        .add_line_locked(&locked_seg_text(
            v(15.0, 0.0),
            v(15.0, 15.0),
            Some("15"),
            None,
        ))
        .unwrap();
    let vertical_dimension = second
        .sketch
        .dimensions
        .iter()
        .find(|dimension| dimension.entities == vec![second.entity_id])
        .unwrap()
        .constraint_id;

    let rectangle = s
        .add_rectangle_locked(&LockedRectangleRequest {
            mode: RectangleMode::TwoPoint,
            anchor: v(15.0, 15.0),
            width_mm: None,
            height_mm: None,
            width_text: Some("30".to_string()),
            height_text: Some("20".to_string()),
            corner_hint: v(45.0, 35.0),
            ctrl_held: false,
        })
        .unwrap();

    assert_eq!(
        rectangle.entities[0], second.end_point_id,
        "a rectangle started from a line endpoint must share that corner"
    );
    assert!(rectangle.sketch.constraints.iter().any(|constraint| {
        matches!(
            constraint.constraint,
            Constraint::OriginCoincident { entity } if entity == first.start_point_id
        )
    }));

    let edited = s
        .edit_dimension(EditDimensionRequest {
            constraint_id: vertical_dimension,
            text: "7.5".to_string(),
        })
        .unwrap()
        .sketch;

    assert!(close(point(&edited, first.start_point_id), v(0.0, 0.0)));
    assert!(close(point(&edited, first.end_point_id), v(15.0, 0.0)));
    assert!(close(point(&edited, second.end_point_id), v(15.0, 7.5)));
    assert!(close(point(&edited, rectangle.entities[1]), v(45.0, 7.5)));
    assert!(close(point(&edited, rectangle.entities[3]), v(15.0, 27.5)));
}

#[test]
fn diameter_dim_drives_circle_radius() {
    let mut s = session();
    let c = s
        .add_circle(
            limo_cad_sketch::CircleMode::CenterDiameter,
            v(50.0, 50.0),
            v(60.0, 50.0),
        )
        .unwrap();
    let d = s
        .add_dimension(DimensionRequest {
            entities: vec![c.entities[0]],
            text_pos: v(80.0, 80.0),
            value_text: None,
        })
        .unwrap();
    assert_eq!(d.sketch.dimensions[0].text, "Ø20.00");
    let r = s
        .edit_dimension(EditDimensionRequest {
            constraint_id: d.sketch.dimensions[0].constraint_id,
            text: "35".to_string(),
        })
        .unwrap();
    match r
        .sketch
        .entities
        .iter()
        .find(|e| e.id() == c.entities[0])
        .unwrap()
    {
        limo_cad_sketch::EntityDto::Circle { radius, .. } => {
            assert!((radius - 17.5).abs() < 1e-9)
        }
        _ => panic!("expected circle"),
    }
}

#[test]
fn angle_dim_drives_line_direction() {
    let mut s = session();

    let l1 = s.add_line(v(0.0, 0.0), v(50.0, 0.0), true).unwrap();
    let l2 = s.add_line(v(0.0, 0.0), v(50.0, 5.0), true).unwrap();
    let d = s
        .add_dimension(DimensionRequest {
            entities: vec![l1.entity_id, l2.entity_id],
            text_pos: v(20.0, 20.0),
            value_text: None,
        })
        .unwrap();
    assert_eq!(d.sketch.dimensions[0].kind, "angle");
    assert!(d.sketch.dimensions[0].text.ends_with('°'));

    s.toggle_fix(l1.start_point_id).unwrap();
    s.toggle_fix(l1.end_point_id).unwrap();
    let r = s
        .edit_dimension(EditDimensionRequest {
            constraint_id: d.sketch.dimensions[0].constraint_id,
            text: "30".to_string(),
        })
        .unwrap();
    let (a, b) = line(&r.sketch, l2.entity_id);
    let ang = ((b.y - a.y).atan2(b.x - a.x)).to_degrees();
    assert!((ang - 30.0).abs() < 1e-6, "angle={ang}");
}

#[test]
fn radial_and_angular_reference_dimensions_follow_solved_geometry() {
    let mut s = session();

    let arc = s
        .add_arc_center(v(0.0, 0.0), v(10.0, 0.0), v(0.0, 10.0))
        .unwrap();
    let arc_id = arc.entities[0];
    let radius_driver = s
        .add_dimension(DimensionRequest {
            entities: vec![arc_id],
            text_pos: v(14.0, 14.0),
            value_text: None,
        })
        .unwrap()
        .sketch
        .dimensions
        .into_iter()
        .find(|dimension| dimension.entities == vec![arc_id])
        .unwrap();
    let radius_reference = s
        .add_dimension(DimensionRequest {
            entities: vec![arc_id],
            text_pos: v(18.0, 18.0),
            value_text: None,
        })
        .unwrap()
        .sketch
        .dimensions
        .into_iter()
        .find(|dimension| {
            dimension.entities == vec![arc_id] && dimension.mode == DimensionMode::Reference
        })
        .unwrap();
    assert_eq!(radius_reference.text, "(R10.00)");
    let edited_radius = s
        .edit_dimension(EditDimensionRequest {
            constraint_id: radius_driver.constraint_id,
            text: "15".to_string(),
        })
        .unwrap()
        .sketch;
    assert_eq!(
        edited_radius
            .dimensions
            .iter()
            .find(|dimension| dimension.constraint_id == radius_reference.constraint_id)
            .unwrap()
            .text,
        "(R15.00)"
    );

    let circle = s
        .add_circle(
            limo_cad_sketch::CircleMode::CenterDiameter,
            v(40.0, 0.0),
            v(50.0, 0.0),
        )
        .unwrap();
    let circle_id = circle.entities[0];
    let diameter_driver = s
        .add_dimension(DimensionRequest {
            entities: vec![circle_id],
            text_pos: v(55.0, 10.0),
            value_text: None,
        })
        .unwrap()
        .sketch
        .dimensions
        .into_iter()
        .find(|dimension| dimension.entities == vec![circle_id])
        .unwrap();
    let diameter_reference = s
        .add_dimension(DimensionRequest {
            entities: vec![circle_id],
            text_pos: v(60.0, 15.0),
            value_text: None,
        })
        .unwrap()
        .sketch
        .dimensions
        .into_iter()
        .find(|dimension| {
            dimension.entities == vec![circle_id] && dimension.mode == DimensionMode::Reference
        })
        .unwrap();
    assert_eq!(diameter_reference.text, "(Ø20.00)");
    let edited_diameter = s
        .edit_dimension(EditDimensionRequest {
            constraint_id: diameter_driver.constraint_id,
            text: "24".to_string(),
        })
        .unwrap()
        .sketch;
    assert_eq!(
        edited_diameter
            .dimensions
            .iter()
            .find(|dimension| dimension.constraint_id == diameter_reference.constraint_id)
            .unwrap()
            .text,
        "(Ø24.00)"
    );

    let base = s.add_line(v(0.0, 40.0), v(20.0, 40.0), true).unwrap();
    let angled = s.add_line(v(0.0, 40.0), v(20.0, 45.0), true).unwrap();
    let angle_driver = s
        .add_dimension(DimensionRequest {
            entities: vec![base.entity_id, angled.entity_id],
            text_pos: v(12.0, 52.0),
            value_text: None,
        })
        .unwrap()
        .sketch
        .dimensions
        .into_iter()
        .find(|dimension| dimension.kind == "angle" && dimension.mode == DimensionMode::Driving)
        .unwrap();
    let angle_reference = s
        .add_dimension(DimensionRequest {
            entities: vec![base.entity_id, angled.entity_id],
            text_pos: v(16.0, 56.0),
            value_text: None,
        })
        .unwrap()
        .sketch
        .dimensions
        .into_iter()
        .find(|dimension| dimension.kind == "angle" && dimension.mode == DimensionMode::Reference)
        .unwrap();
    let edited_angle = s
        .edit_dimension(EditDimensionRequest {
            constraint_id: angle_driver.constraint_id,
            text: "45".to_string(),
        })
        .unwrap()
        .sketch;
    assert_eq!(
        edited_angle
            .dimensions
            .iter()
            .find(|dimension| dimension.constraint_id == angle_reference.constraint_id)
            .unwrap()
            .text,
        "(45.00°)"
    );
}

#[test]
fn fully_dimensioned_rectangle_is_fully_defined() {
    let mut s = session();
    let rect = s
        .add_rectangle(
            limo_cad_sketch::RectangleMode::TwoPoint,
            v(5.0, 5.0),
            v(45.0, 25.0),
        )
        .unwrap();
    let lines = &rect.entities[4..8];

    s.add_dimension(DimensionRequest {
        entities: vec![lines[0]],
        text_pos: v(20.0, -15.0),
        value_text: None,
    })
    .unwrap();
    s.add_dimension(DimensionRequest {
        entities: vec![lines[3]],
        text_pos: v(-15.0, 10.0),
        value_text: None,
    })
    .unwrap();
    assert_eq!(s.dto().dof.value, 2, "w+h dims leave only position free");

    s.toggle_fix(rect.entities[0]).unwrap();
    let dto = s.dto();
    assert_eq!(dto.dof.value, 0);
    assert!(dto.dof.fully_defined);
}

#[test]
fn explicit_duplicate_driver_is_rejected_without_an_orphan_parameter() {
    let mut s = session();
    let l = s.add_line(v(0.0, 0.0), v(40.0, 0.0), true).unwrap();
    s.add_dimension(DimensionRequest {
        entities: vec![l.entity_id],
        text_pos: v(20.0, 15.0),
        value_text: None,
    })
    .unwrap();

    let err = s
        .add_dimension(DimensionRequest {
            entities: vec![l.entity_id],
            text_pos: v(20.0, 25.0),
            value_text: Some("55".to_string()),
        })
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("already driven"), "{msg}");

    assert_eq!(s.dto().dimensions.len(), 1);
    assert_eq!(s.sketch().params().all().len(), 1);
}

#[test]
fn duplicate_measurement_becomes_a_live_reference_dimension() {
    let mut s = session();
    let line_result = s.add_line(v(0.0, 0.0), v(40.0, 0.0), true).unwrap();
    let driving = s
        .add_dimension(DimensionRequest {
            entities: vec![line_result.entity_id],
            text_pos: v(20.0, 10.0),
            value_text: None,
        })
        .unwrap();
    let driving_id = driving.sketch.dimensions[0].constraint_id;
    let constrained_dof = driving.sketch.dof.value;

    let referenced = s
        .add_dimension(DimensionRequest {
            entities: vec![line_result.entity_id],
            text_pos: v(20.0, 20.0),
            value_text: None,
        })
        .unwrap()
        .sketch;
    assert_eq!(referenced.dimensions.len(), 2);
    assert_eq!(referenced.dof.value, constrained_dof);
    assert_eq!(s.sketch().params().all().len(), 1);
    let reference = referenced
        .dimensions
        .iter()
        .find(|dimension| dimension.mode == DimensionMode::Reference)
        .expect("reference dimension");
    assert_eq!(reference.text, "(40.00)");
    assert_eq!(reference.value, 40.0);
    assert_eq!(reference.param_id, None);
    assert_eq!(reference.param_name, None);
    let reference_id = reference.constraint_id;

    s.toggle_fix(line_result.start_point_id).unwrap();
    let edited = s
        .edit_dimension(EditDimensionRequest {
            constraint_id: driving_id,
            text: "55".to_string(),
        })
        .unwrap()
        .sketch;
    let reference = edited
        .dimensions
        .iter()
        .find(|dimension| dimension.constraint_id == reference_id)
        .unwrap();
    assert_eq!(reference.text, "(55.00)");
    assert!((reference.value - 55.0).abs() < 1e-8);
    let flattened = edited
        .constraints
        .iter()
        .find(|constraint| constraint.id == reference_id)
        .unwrap();
    assert!(matches!(
        flattened.constraint,
        Constraint::Distance { value, .. } if (value - 55.0).abs() < 1e-8
    ));

    let error = s
        .edit_dimension(EditDimensionRequest {
            constraint_id: reference_id,
            text: "60".to_string(),
        })
        .unwrap_err();
    assert!(error.to_string().contains("Reference dimensions"));
}

#[test]
fn reference_creation_is_undoable_and_duplicate_references_are_rejected() {
    let mut s = session();
    let line_result = s.add_line(v(0.0, 0.0), v(25.0, 0.0), true).unwrap();
    s.add_dimension(DimensionRequest {
        entities: vec![line_result.entity_id],
        text_pos: v(12.5, 8.0),
        value_text: None,
    })
    .unwrap();
    s.add_dimension(DimensionRequest {
        entities: vec![line_result.entity_id],
        text_pos: v(12.5, 16.0),
        value_text: None,
    })
    .unwrap();
    assert_eq!(s.dto().dimensions.len(), 2);

    let error = s
        .add_dimension(DimensionRequest {
            entities: vec![line_result.entity_id],
            text_pos: v(12.5, 24.0),
            value_text: None,
        })
        .unwrap_err();
    assert!(error.to_string().contains("reference dimension already"));
    assert_eq!(s.dto().dimensions.len(), 2);

    assert_eq!(s.undo().unwrap().sketch.dimensions.len(), 1);
    let redone = s.redo().unwrap().sketch;
    assert_eq!(redone.dimensions.len(), 2);
    assert!(redone
        .dimensions
        .iter()
        .any(|dimension| dimension.mode == DimensionMode::Reference));
}

#[test]
fn dimension_mode_conversion_changes_dof_without_moving_geometry() {
    let mut s = session();
    let line_result = s.add_line(v(0.0, 0.0), v(32.0, 0.0), true).unwrap();
    let created = s
        .add_dimension(DimensionRequest {
            entities: vec![line_result.entity_id],
            text_pos: v(16.0, 10.0),
            value_text: None,
        })
        .unwrap()
        .sketch;
    let cid = created.dimensions[0].constraint_id;
    let driven_dof = created.dof.value;
    let geometry = line(&created, line_result.entity_id);

    let referenced = s
        .set_dimension_mode(SetDimensionModeRequest {
            constraint_id: cid,
            mode: DimensionMode::Reference,
        })
        .unwrap()
        .sketch;
    assert_eq!(referenced.dimensions[0].mode, DimensionMode::Reference);
    assert_eq!(referenced.dimensions[0].text, "(32.00)");
    assert_eq!(referenced.dof.value, driven_dof + 1);
    assert_eq!(line(&referenced, line_result.entity_id), geometry);
    assert!(s.sketch().params().all().is_empty());

    let driving = s
        .set_dimension_mode(SetDimensionModeRequest {
            constraint_id: cid,
            mode: DimensionMode::Driving,
        })
        .unwrap()
        .sketch;
    assert_eq!(driving.dimensions[0].mode, DimensionMode::Driving);
    assert_eq!(driving.dimensions[0].text, "32.00");
    assert_eq!(driving.dof.value, driven_dof);
    assert_eq!(line(&driving, line_result.entity_id), geometry);
    assert_eq!(s.sketch().params().all().len(), 1);

    let undone = s.undo().unwrap().sketch;
    assert_eq!(undone.dimensions[0].mode, DimensionMode::Reference);
    assert_eq!(
        s.redo().unwrap().sketch.dimensions[0].mode,
        DimensionMode::Driving
    );
}

#[test]
fn conversion_protects_existing_drivers_and_parameter_dependencies() {
    let mut s = session();
    let first = s
        .add_line_locked(&locked_seg_text(
            v(0.0, 0.0),
            v(40.0, 0.0),
            Some("40"),
            None,
        ))
        .unwrap();
    s.add_line_locked(&locked_seg_text(
        v(0.0, 20.0),
        v(20.0, 20.0),
        Some("=d1/2"),
        None,
    ))
    .unwrap();
    let first_dimension = s
        .dto()
        .dimensions
        .iter()
        .find(|dimension| dimension.entities.contains(&first.entity_id))
        .unwrap()
        .constraint_id;
    let error = s
        .set_dimension_mode(SetDimensionModeRequest {
            constraint_id: first_dimension,
            mode: DimensionMode::Reference,
        })
        .unwrap_err();
    assert!(error.to_string().contains("used by d2"));
    assert_eq!(s.dto().dimensions[0].mode, DimensionMode::Driving);

    let reference = s
        .add_dimension(DimensionRequest {
            entities: vec![first.entity_id],
            text_pos: v(20.0, 12.0),
            value_text: None,
        })
        .unwrap()
        .sketch
        .dimensions
        .into_iter()
        .find(|dimension| dimension.mode == DimensionMode::Reference)
        .unwrap();
    let error = s
        .set_dimension_mode(SetDimensionModeRequest {
            constraint_id: reference.constraint_id,
            mode: DimensionMode::Driving,
        })
        .unwrap_err();
    assert!(error.to_string().contains("Another driving dimension"));
}

#[test]
fn typed_length_and_angle_create_dimensions_with_annotations() {
    let mut s = session();
    let r = s
        .add_line_locked(&locked_seg_text(
            v(0.0, 0.0),
            v(99.0, 99.0),
            Some("=25*2"),
            Some("30"),
        ))
        .unwrap();
    let dto = r.sketch;
    assert_eq!(dto.dimensions.len(), 2);
    let dist = dto
        .dimensions
        .iter()
        .find(|d| d.kind == "distance")
        .unwrap();
    assert_eq!(dist.text, "50.00");
    assert_eq!(dist.param_expression.as_deref(), Some("25*2"));
    let ang = dto.dimensions.iter().find(|d| d.kind == "angle").unwrap();
    assert_eq!(ang.text, "30.00°");

    assert_eq!(dist.param_name.as_deref(), Some("d1"));
    assert_eq!(ang.param_name.as_deref(), Some("d2"));

    let undone = s.undo().unwrap();
    assert_eq!(undone.sketch.entities.len(), 0);
    assert_eq!(undone.sketch.dimensions.len(), 0);
    assert!(undone.sketch.constraints.is_empty());
}

#[test]
fn formula_dimensions_chain_and_edit_reevaluates_dependents() {
    let mut s = session();
    let l1 = s
        .add_line_locked(&locked_seg_text(
            v(0.0, 0.0),
            v(99.0, 0.0),
            Some("50"),
            None,
        ))
        .unwrap();
    let l2 = s
        .add_line_locked(&locked_seg_text(
            v(0.0, 30.0),
            v(99.0, 30.0),
            Some("=d1/2"),
            None,
        ))
        .unwrap();
    let dto = s.dto();
    assert_eq!(dto.dimensions.len(), 2);
    assert_eq!(dto.dimensions[1].text, "25.00");
    assert_eq!(dto.dimensions[1].param_expression.as_deref(), Some("d1/2"));

    s.toggle_fix(l2.start_point_id).unwrap();
    let cid = dto.dimensions[0].constraint_id;
    let r = s
        .edit_dimension(EditDimensionRequest {
            constraint_id: cid,
            text: "60".to_string(),
        })
        .unwrap();
    let (_, e1) = line(&r.sketch, l1.entity_id);
    let (_, e2) = line(&r.sketch, l2.entity_id);
    assert!(close(e1, v(60.0, 0.0)), "e1={e1:?}");
    assert!(close(e2, v(30.0, 30.0)), "e2={e2:?}");
    assert_eq!(r.sketch.dimensions[1].text, "30.00");
    let dependent = r
        .sketch
        .constraints
        .iter()
        .find(|constraint| constraint.id == r.sketch.dimensions[1].constraint_id)
        .expect("dependent dimension constraint");
    assert!(matches!(
        dependent.constraint,
        Constraint::Distance { value, .. } if (value - 30.0).abs() < 1e-9
    ));
}

#[test]
fn cycle_through_dimension_edit_surfaces_a_clear_error() {
    let mut s = session();
    s.add_line_locked(&locked_seg_text(
        v(0.0, 0.0),
        v(50.0, 0.0),
        Some("50"),
        None,
    ))
    .unwrap();
    s.add_line_locked(&locked_seg_text(
        v(0.0, 30.0),
        v(50.0, 30.0),
        Some("=d1"),
        None,
    ))
    .unwrap();
    let dto = s.dto();

    let err = s
        .edit_dimension(EditDimensionRequest {
            constraint_id: dto.dimensions[0].constraint_id,
            text: "=d2".to_string(),
        })
        .unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("circular reference"), "{msg}");
    assert!(msg.contains("d1") && msg.contains("d2"), "{msg}");

    assert_eq!(s.dto().dimensions[0].value, 50.0);
}

#[test]
fn dimension_on_fully_constrained_geometry_becomes_reference() {
    let mut s = session();
    let line_result = s.add_line(v(0.0, 0.0), v(40.0, 0.0), true).unwrap();
    s.toggle_fix(line_result.start_point_id).unwrap();
    s.toggle_fix(line_result.end_point_id).unwrap();
    let before = s.dto();
    let params_before = s.sketch().params().all().len();
    let dof_before = before.dof;
    let dimension = s
        .add_dimension(DimensionRequest {
            entities: vec![line_result.entity_id],
            text_pos: v(20.0, 10.0),
            value_text: None,
        })
        .unwrap();
    let cid = dimension.sketch.dimensions[0].constraint_id;
    let reference = &dimension.sketch.dimensions[0];
    assert_eq!(reference.mode, DimensionMode::Reference);
    assert_eq!(reference.text, "(40.00)");
    assert_eq!(reference.param_id, None);
    assert_eq!(reference.param_name, None);
    assert_eq!(s.sketch().params().all().len(), params_before);
    assert_eq!(dimension.sketch.dof, dof_before);
    assert_eq!(
        line(&dimension.sketch, line_result.entity_id),
        line(&before, line_result.entity_id)
    );

    let error = s
        .edit_dimension(EditDimensionRequest {
            constraint_id: cid,
            text: "55".to_string(),
        })
        .unwrap_err();
    assert!(
        error.to_string().contains("Reference dimensions"),
        "{error}"
    );

    let error = s
        .set_dimension_mode(SetDimensionModeRequest {
            constraint_id: cid,
            mode: DimensionMode::Driving,
        })
        .unwrap_err();
    assert!(
        error.to_string().contains("must remain a reference"),
        "{error}"
    );

    let after = s.dto();
    assert_eq!(after.dimensions[0].value, 40.0);
    assert_eq!(after.dimensions[0].mode, DimensionMode::Reference);
    assert_eq!(s.sketch().params().all().len(), params_before);
    assert_eq!(after.dof, dof_before);
    assert_eq!(
        line(&after, line_result.entity_id),
        line(&before, line_result.entity_id)
    );
}

#[test]
fn locked_length_still_snaps_to_points_on_the_circle() {
    let mut s = session();

    let p = s.add_point(v(50.0, 0.0)).unwrap();

    let r = s
        .add_line_locked(&locked_seg_text(
            v(0.0, 0.0),
            v(49.0, 2.0),
            Some("50"),
            None,
        ))
        .unwrap();
    assert_eq!(
        r.end_point_id, p.entities[0],
        "must merge onto the snapped point"
    );
    let (_, end) = line(&r.sketch, r.entity_id);
    assert!(close(end, v(50.0, 0.0)));
}

#[test]
fn locked_length_axis_inference_still_works() {
    let mut s = session();

    let r = s
        .add_line_locked(&locked_seg_text(
            v(10.0, 10.0),
            v(50.0, 10.4),
            Some("35"),
            None,
        ))
        .unwrap();
    let (_, end) = line(&r.sketch, r.entity_id);
    assert!(close(end, v(45.0, 10.0)), "end={end:?}");
    assert!(r
        .created_constraints
        .iter()
        .any(|c| c.constraint.kind_str() == "horizontal"));
}

#[test]
fn locked_angle_still_snaps_to_points_on_the_ray() {
    let mut s = session();
    let p = s.add_point(v(30.0, 30.0)).unwrap();
    let r = s
        .add_line_locked(&locked_seg_text(
            v(0.0, 0.0),
            v(31.0, 29.0),
            None,
            Some("45"),
        ))
        .unwrap();
    assert_eq!(r.end_point_id, p.entities[0]);
}

#[test]
fn dimension_move_and_delete() {
    let mut s = session();
    let l = s.add_line(v(0.0, 0.0), v(40.0, 0.0), true).unwrap();
    let d = s
        .add_dimension(DimensionRequest {
            entities: vec![l.entity_id],
            text_pos: v(20.0, 15.0),
            value_text: None,
        })
        .unwrap();
    let cid = d.sketch.dimensions[0].constraint_id;
    let pid = d.sketch.dimensions[0]
        .param_id
        .expect("driving dimension parameter");
    s.move_dimension(MoveDimensionRequest {
        constraint_id: cid,
        text_pos: v(5.0, 40.0),
    })
    .unwrap();
    assert_eq!(s.dto().dimensions[0].text_pos, v(5.0, 40.0));
    s.delete_dimension(cid).unwrap();
    let dto = s.dto();
    assert!(dto.dimensions.is_empty());
    assert!(
        s.sketch().params().get(pid).is_none(),
        "orphan param removed"
    );

    let undone = s.undo().unwrap();
    assert_eq!(undone.sketch.dimensions.len(), 1);
    assert_eq!(undone.sketch.dimensions[0].text_pos, v(5.0, 40.0));
}

#[test]
fn typed_dimension_default_offset_scales_with_the_measured_feature() {
    let mut small = session();
    let small_line = small
        .add_line_locked(&locked_seg_text(
            v(0.0, 0.0),
            v(0.5, 0.0),
            Some("0.5"),
            None,
        ))
        .unwrap();
    let small_dim = small_line
        .sketch
        .dimensions
        .first()
        .expect("typed length should create a dimension");
    assert!(
        (small_dim.text_pos.y - 1.5).abs() < 1e-9,
        "sub-millimetre geometry should receive a compact readable offset"
    );

    let mut large = session();
    let large_line = large
        .add_line_locked(&locked_seg_text(
            v(0.0, 0.0),
            v(100.0, 0.0),
            Some("100"),
            None,
        ))
        .unwrap();
    let large_dim = large_line
        .sketch
        .dimensions
        .first()
        .expect("typed length should create a dimension");
    assert!(
        (large_dim.text_pos.y - 10.0).abs() < 1e-9,
        "large geometry should cap the initial extension-line offset"
    );
}
