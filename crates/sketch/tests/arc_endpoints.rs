//! Issue #137: arcs expose usable endpoints and accept tangency at both ends.
use limo_cad_sketch::{
    BreakRequest, Constraint, DragPhase, EntityDto, EntityId, MoveCopyRequest, MovePointRequest,
    OriginPlane, PlaneRef, ScaleRequest, SketchDto, SketchSession, SnapTarget, TrimRequest, Vec2,
};

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn session() -> SketchSession {
    let plane = PlaneRef::OriginPlane {
        plane: OriginPlane::Xy,
    };
    SketchSession::new("Arc", plane, plane.basis().unwrap(), false)
}

fn point(dto: &SketchDto, id: EntityId) -> Vec2 {
    match dto
        .entities
        .iter()
        .find(|entity| entity.id() == id)
        .unwrap()
    {
        EntityDto::Point { position, .. } => *position,
        other => panic!("expected endpoint point, got {other:?}"),
    }
}

fn endpoints(dto: &SketchDto, arc_id: EntityId) -> Vec<EntityId> {
    dto.constraints
        .iter()
        .filter_map(|constraint| match constraint.constraint {
            Constraint::ArcEndpointCoincident { point, arc, .. } if arc == arc_id => Some(point),
            _ => None,
        })
        .collect()
}

fn assert_endpoints_follow_arc(dto: &SketchDto, arc_id: EntityId) {
    let EntityDto::Arc {
        center,
        radius,
        start_angle,
        end_angle,
        ..
    } = dto
        .entities
        .iter()
        .find(|entity| entity.id() == arc_id)
        .unwrap()
    else {
        panic!("expected arc")
    };
    let expected = [*start_angle, *end_angle]
        .map(|angle| *center + v(radius * angle.cos(), radius * angle.sin()));
    let points = endpoints(dto, arc_id);
    assert_eq!(
        points.len(),
        2,
        "each arc must have two persistent endpoint handles"
    );
    for expected in expected {
        assert!(
            points
                .iter()
                .any(|id| point(dto, *id).distance(expected) < 1e-6),
            "arc endpoint {expected:?} has no coincident point: {dto:?}"
        );
    }
}

#[test]
fn standalone_arcs_have_snappable_draggable_endpoints_in_one_undo_step() {
    for center_mode in [false, true] {
        for ctrl_held in [false, true] {
            for clockwise in [false, true] {
                let mut s = session();
                let (start, end) = if clockwise {
                    (v(30.0, 40.0), v(40.0, 30.0))
                } else {
                    (v(40.0, 30.0), v(30.0, 40.0))
                };
                let result = if center_mode {
                    s.add_arc_center_selective(v(30.0, 30.0), start, end, ctrl_held)
                } else {
                    s.add_arc_3pt_selective(start, v(38.0, 38.0), end, ctrl_held)
                }
                .unwrap();
                let arc = result.entities[0];
                assert_endpoints_follow_arc(&result.sketch, arc);
                assert_eq!(result.sketch.dof.value, 5);
                let endpoint = endpoints(&result.sketch, arc)[0];
                let position = point(&result.sketch, endpoint);
                let preview = s.preview_segment(v(60.0, 60.0), position + v(0.1, 0.1), false);
                assert_eq!(preview.snap, SnapTarget::Point { entity: endpoint });
                assert!(s.undo().unwrap().sketch.entities.is_empty());
                assert_endpoints_follow_arc(&s.redo().unwrap().sketch, arc);
                s.move_point(MovePointRequest {
                    point_id: endpoint,
                    to_raw: position + v(2.0, 1.0),
                    ctrl_held: true,
                    phase: DragPhase::Single,
                })
                .unwrap();
                assert_endpoints_follow_arc(&s.dto(), arc);
                assert!(point(&s.dto(), endpoint).distance(position + v(2.0, 1.0)) < 1e-6);
            }
        }
    }
}

#[test]
fn lines_drawn_after_an_arc_share_both_endpoints() {
    let mut s = session();
    let arc = s
        .add_arc_center(v(30.0, 40.0), v(30.0, 30.0), v(40.0, 40.0))
        .unwrap()
        .entities[0];
    let dto = s.dto();
    assert_endpoints_follow_arc(&dto, arc);
    let ids = endpoints(&dto, arc);
    let start = ids
        .iter()
        .copied()
        .find(|id| point(&dto, *id).distance(v(30.0, 30.0)) < 1e-6)
        .unwrap();
    let end = ids
        .iter()
        .copied()
        .find(|id| point(&dto, *id).distance(v(40.0, 40.0)) < 1e-6)
        .unwrap();
    let first = s.add_line(v(10.0, 30.0), v(30.1, 30.1), false).unwrap();
    let second = s.add_line(v(40.1, 40.1), v(40.0, 60.0), false).unwrap();
    assert_eq!(first.end_point_id, start);
    assert_eq!(second.start_point_id, end);
    s.move_point(MovePointRequest {
        point_id: start,
        to_raw: v(31.0, 30.0),
        ctrl_held: true,
        phase: DragPhase::Single,
    })
    .unwrap();
    assert_endpoints_follow_arc(&s.dto(), arc);
}

#[test]
fn center_arc_off_radius_sweep_creates_endpoint_on_the_arc() {
    let mut s = session();
    let off_radius = s.add_point(v(30.0, 60.0)).unwrap().entities[0];
    let arc = s
        .add_arc_center(v(30.0, 30.0), v(40.0, 30.0), v(30.0, 60.0))
        .unwrap()
        .entities[0];
    let dto = s.dto();
    assert_endpoints_follow_arc(&dto, arc);
    assert!(!endpoints(&dto, arc).contains(&off_radius));
    assert_eq!(point(&dto, off_radius), v(30.0, 60.0));
}

#[test]
fn connected_arc_accepts_two_tangencies_in_either_order() {
    for reverse in [false, true] {
        let mut s = session();
        let horizontal = s.add_line(v(10.0, 30.0), v(30.0, 30.0), false).unwrap();
        let vertical = s.add_line(v(40.0, 40.0), v(40.0, 60.0), false).unwrap();
        let arc = s
            .add_arc_3pt(v(30.0, 30.0), v(38.0, 32.0), v(40.0, 40.0))
            .unwrap()
            .entities[0];
        assert_eq!(
            s.dto()
                .constraints
                .iter()
                .filter(|c| matches!(c.constraint, Constraint::Tangent { .. }))
                .count(),
            0
        );
        let carriers = if reverse {
            [vertical.entity_id, horizontal.entity_id]
        } else {
            [horizontal.entity_id, vertical.entity_id]
        };
        for (index, line) in carriers.into_iter().enumerate() {
            let before = s.dto().dof.value;
            let relation = if reverse {
                Constraint::Tangent { a: arc, b: line }
            } else {
                Constraint::Tangent { a: line, b: arc }
            };
            s.add_constraint(relation)
                .unwrap_or_else(|error| panic!("tangent {} failed: {error:?}", index + 1));
            assert_eq!(
                s.dto().dof.value,
                before - 1,
                "reverse={reverse} tangent={} dto={:?}",
                index + 1,
                s.dto()
            );
            assert_endpoints_follow_arc(&s.dto(), arc);
        }
        let dto = s.dto();
        let EntityDto::Arc { center, .. } = dto
            .entities
            .iter()
            .find(|entity| entity.id() == arc)
            .unwrap()
        else {
            panic!("expected arc")
        };
        assert!((point(&dto, horizontal.end_point_id).x - center.x).abs() < 1e-6);
        assert!((point(&dto, vertical.start_point_id).y - center.y).abs() < 1e-6);
    }
}

#[test]
fn tangent_inference_can_attach_both_ends_of_a_quarter_arc() {
    let mut s = session();
    s.add_line(v(10.0, 30.0), v(30.0, 30.0), false).unwrap();
    s.add_line(v(40.0, 40.0), v(40.0, 60.0), false).unwrap();
    let arc = s
        .add_arc_center(v(30.0, 40.0), v(30.0, 30.0), v(40.0, 40.0))
        .unwrap();
    assert_eq!(
        arc.sketch
            .constraints
            .iter()
            .filter(|c| matches!(c.constraint, Constraint::Tangent { .. }))
            .count(),
        2
    );
    assert_endpoints_follow_arc(&arc.sketch, arc.entities[0]);
}

#[test]
fn short_lines_drawn_from_an_arc_accept_both_tangencies() {
    let mut s = session();
    let arc = s
        .add_arc_3pt(v(30.0, 30.0), v(38.0, 32.0), v(40.0, 40.0))
        .unwrap()
        .entities[0];
    let horizontal = s
        .add_line(v(30.0, 30.0), v(24.448841102869146, 30.0), false)
        .unwrap();
    let vertical = s
        .add_line(v(40.0, 40.0), v(40.0, 42.12499577841624), false)
        .unwrap();
    for line in [horizontal.entity_id, vertical.entity_id] {
        s.add_constraint(Constraint::Tangent { a: line, b: arc })
            .unwrap();
        assert_endpoints_follow_arc(&s.dto(), arc);
    }
}

#[test]
fn moving_and_scaling_an_arc_transforms_shared_handles_once() {
    let mut s = session();
    let arc = s
        .add_arc_center(v(30.0, 40.0), v(40.0, 40.0), v(30.0, 50.0))
        .unwrap()
        .entities[0];
    let mut selected = endpoints(&s.dto(), arc);
    selected.push(arc);
    s.move_copy_entities(&MoveCopyRequest {
        entity_ids: selected.clone(),
        dx: 20.0,
        dy: 10.0,
        copy: false,
    })
    .unwrap();
    s.scale_entities(&ScaleRequest {
        entity_ids: selected,
        origin: v(0.0, 0.0),
        factor_text: "-2".into(),
    })
    .unwrap();
    let dto = s.dto();
    assert_endpoints_follow_arc(&dto, arc);
    let EntityDto::Arc { center, radius, .. } =
        dto.entities.iter().find(|e| e.id() == arc).unwrap()
    else {
        panic!("expected arc")
    };
    assert!(center.distance(v(-100.0, -100.0)) < 1e-6);
    assert!((radius - 20.0).abs() < 1e-6);
}

#[test]
fn breaking_an_arc_keeps_original_connections_and_shares_the_cut() {
    let mut s = session();
    let arc = s
        .add_arc_center(v(30.0, 30.0), v(40.0, 30.0), v(20.0, 30.0))
        .unwrap()
        .entities[0];
    let line = s.add_line(v(20.0, 30.0), v(10.0, 20.0), false).unwrap();
    let before = s.dto();
    s.break_curve(&BreakRequest {
        entity: arc,
        at: v(30.0, 40.0),
    })
    .unwrap();
    let dto = s.dto();
    let arcs: Vec<_> = dto
        .entities
        .iter()
        .filter(|e| matches!(e, EntityDto::Arc { .. }))
        .map(EntityDto::id)
        .collect();
    assert_eq!(arcs.len(), 2);
    for id in &arcs {
        assert_endpoints_follow_arc(&dto, *id);
    }
    let first = endpoints(&dto, arcs[0]);
    let second = endpoints(&dto, arcs[1]);
    assert_eq!(first.iter().filter(|id| second.contains(id)).count(), 1);
    assert!(second.contains(&line.start_point_id));
    assert_eq!(
        point(&dto, line.start_point_id),
        point(&before, line.start_point_id)
    );
    let undone = s.undo().unwrap().sketch;
    assert_eq!(undone.entities, before.entities);
    assert_eq!(undone.constraints, before.constraints);
}

#[test]
fn trimming_an_arc_rebinds_the_changed_endpoint_without_distorting_the_curve() {
    let mut s = session();
    let arc = s
        .add_arc_center(v(30.0, 30.0), v(40.0, 30.0), v(20.0, 30.0))
        .unwrap()
        .entities[0];
    s.add_line(v(30.0, 20.0), v(30.0, 50.0), true).unwrap();
    s.trim_entity(&TrimRequest {
        entity: arc,
        click: v(37.0, 37.0),
    })
    .unwrap();
    let dto = s.dto();
    assert_endpoints_follow_arc(&dto, arc);
    let EntityDto::Arc {
        center,
        radius,
        start_angle,
        end_angle,
        ..
    } = dto.entities.iter().find(|e| e.id() == arc).unwrap()
    else {
        panic!("expected arc")
    };
    assert!(center.distance(v(30.0, 30.0)) < 1e-6);
    assert!((radius - 10.0).abs() < 1e-6);
    assert!((start_angle - std::f64::consts::FRAC_PI_2).abs() < 1e-6);
    assert!((end_angle - std::f64::consts::PI).abs() < 1e-6);
}
