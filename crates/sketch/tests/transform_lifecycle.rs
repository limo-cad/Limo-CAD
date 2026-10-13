//! Derived geometry must have the same topology and ownership as drawn geometry.
use limo_cad_sketch::ArcEndpoint;
use limo_cad_sketch::{
    CircularPatternRequest, Constraint, DragPhase, Entity, EntityDto, EntityId, MirrorRequest,
    MoveCopyRequest, MovePointRequest, OriginPlane, PlaneRef, RectangularPatternRequest, Sketch,
    SketchDto, SketchSession, Vec2,
};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}
fn session() -> SketchSession {
    let plane = PlaneRef::OriginPlane {
        plane: OriginPlane::Xy,
    };
    SketchSession::new("Transforms", plane, plane.basis().unwrap(), false)
}
fn handles(dto: &SketchDto, owner: EntityId) -> Vec<(EntityId, ArcEndpoint)> {
    dto.constraints
        .iter()
        .filter_map(|c| match c.constraint {
            Constraint::ArcEndpointCoincident { point, arc, end } if arc == owner => {
                Some((point, end))
            }
            _ => None,
        })
        .collect()
}
fn assert_handles(sketch: &Sketch, owner: EntityId) {
    let Some(Entity::Arc {
        center,
        radius,
        start_angle,
        end_angle,
    }) = sketch.entity(owner)
    else {
        panic!("arc missing")
    };
    let bindings: Vec<_> = sketch
        .constraints()
        .filter_map(|(_, c)| match *c {
            Constraint::ArcEndpointCoincident { point, arc, end } if arc == owner => {
                Some((point, end))
            }
            _ => None,
        })
        .collect();
    assert_eq!(bindings.len(), 2);
    for (point, end) in bindings {
        let angle = if end == ArcEndpoint::Start {
            *start_angle
        } else {
            *end_angle
        };
        let expected = *center + v(radius * angle.cos(), radius * angle.sin());
        assert!(sketch.point_position(point).unwrap().distance(expected) < 1e-6);
    }
}

#[test]
fn connected_arc_occurrences_keep_handles_through_edit_delete_undo_and_serialization() {
    for tool in ["copy", "mirror", "rectangular", "circular"] {
        let mut s = session();
        let axis = s.add_line(v(0., -50.), v(0., 50.), true).unwrap().entity_id;
        let arc = s
            .add_arc_center_selective(v(30., 30.), v(40., 30.), v(30., 40.), true)
            .unwrap()
            .entities[0];
        let first = s
            .add_line(v(40., 30.), v(40., 20.), false)
            .unwrap()
            .entity_id;
        let second = s
            .add_line(v(30., 40.), v(20., 40.), false)
            .unwrap()
            .entity_id;
        let ids = vec![arc, first, second];
        let before = s.dto();
        match tool {
            "copy" => s.move_copy_entities(&MoveCopyRequest {
                entity_ids: ids.clone(),
                dx: 100.,
                dy: 0.,
                copy: true,
            }),
            "mirror" => s.mirror_entities(&MirrorRequest {
                entity_ids: ids.clone(),
                axis_line: axis,
            }),
            "rectangular" => s.rectangular_pattern(&RectangularPatternRequest {
                entity_ids: ids.clone(),
                direction: v(1., 0.),
                spacing: 100.,
                count: 2,
                second_direction: None,
                second_spacing: 0.,
                second_count: 1,
            }),
            _ => s.circular_pattern(&CircularPatternRequest {
                entity_ids: ids.clone(),
                center: Vec2::ZERO,
                count: 2,
                total_angle_deg: 90.,
            }),
        }
        .unwrap();
        let after = s.dto();
        let copied = after
            .entities
            .iter()
            .find_map(|e| match e {
                EntityDto::Arc { id, .. } if *id != arc => Some(*id),
                _ => None,
            })
            .unwrap();
        assert_handles(s.sketch(), copied);
        let points = handles(&after, copied);
        for (point, _) in &points {
            assert!(s.sketch().entities().any(|(id, entity)| matches!(entity,
                Entity::Line { start, end } if id != first && id != second && (*start == *point || *end == *point))), "{tool}: lost arc/line connection");
        }
        assert_eq!(s.undo().unwrap().sketch.entities, before.entities);
        assert_eq!(s.redo().unwrap().sketch.entities, after.entities);
        let target = s.sketch().point_position(points[0].0).unwrap() + v(1., 2.);
        s.move_point(MovePointRequest {
            point_id: points[0].0,
            to_raw: target,
            ctrl_held: true,
            phase: DragPhase::Single,
        })
        .unwrap();
        assert_handles(s.sketch(), copied);
        assert!(
            s.sketch()
                .point_position(points[0].0)
                .unwrap()
                .distance(target)
                < 1e-6,
            "{tool}: copied handle cannot be dragged"
        );
        s.delete_entities(&ids).unwrap();
        assert_handles(s.sketch(), copied);
        let encoded = serde_json::to_string(&s.sketch().snapshot()).unwrap();
        let mut reopened = Sketch::new();
        reopened.restore(serde_json::from_str(&encoded).unwrap());
        assert_handles(&reopened, copied);
        reopened.remove_entity(copied);

        for (point, _) in &points {
            assert!(reopened.entity(*point).is_some());
        }
        let copied_lines: Vec<_> = reopened
            .entities()
            .filter_map(|(id, e)| (id != axis && matches!(e, Entity::Line { .. })).then_some(id))
            .collect();
        for line in copied_lines {
            reopened.remove_entity(line);
        }
        for (point, _) in &points {
            assert!(reopened.entity(*point).is_none());
        }
        assert_eq!(reopened.entity_count(), 3, "{tool}: orphaned handles");
    }
}

#[test]
fn mirror_flips_arc_winding_and_preserves_major_and_full_sweeps() {
    for sweep in [FRAC_PI_2, PI, PI * 1.5, TAU, -FRAC_PI_2, -PI * 1.5] {
        let mut s = session();
        let axis = s.add_line(v(0., -50.), v(0., 50.), true).unwrap().entity_id;
        let center = v(30., 30.);
        let arc = s
            .add_arc_center_locked(
                (
                    center,
                    center + v(10., 0.),
                    center + v(10. * sweep.cos(), 10. * sweep.sin()),
                ),
                true,
                None,
                None,
                None,
                Some(sweep),
            )
            .unwrap()
            .entities[0];
        s.mirror_entities(&MirrorRequest {
            entity_ids: vec![arc],
            axis_line: axis,
        })
        .unwrap();
        let (id, center, radius, a0, a1) = s
            .sketch()
            .entities()
            .find_map(|(id, e)| match e {
                Entity::Arc {
                    center,
                    radius,
                    start_angle,
                    end_angle,
                } if id != arc => Some((id, *center, *radius, *start_angle, *end_angle)),
                _ => None,
            })
            .unwrap();
        assert!((a1 - a0 - sweep.abs()).abs() < 1e-8);
        let original_mid = v(30., 30.) + v(10. * (sweep / 2.).cos(), 10. * (sweep / 2.).sin());
        let reflected_mid = center
            + v(
                radius * ((a0 + a1) / 2.).cos(),
                radius * ((a0 + a1) / 2.).sin(),
            );
        assert!(reflected_mid.distance(v(-original_mid.x, original_mid.y)) < 1e-6);
        assert_handles(s.sketch(), id);
    }
}
