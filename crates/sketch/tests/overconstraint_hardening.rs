//! Redundant/conflicting constraint admission regressions.
//!
//! The pairwise permutation matrix proves order safety for every public
//! operation. These tests cover dependency circuits, which need three or more
//! relations and therefore cannot be exposed by pairwise tests alone.

use limo_cad_sketch::{
    CircleMode, Constraint, DimensionMode, DimensionRequest, DragPhase, EntityDto, EntityId,
    MovePointRequest, OriginPlane, PlaneRef, RectangleMode, SessionError, SetDimensionModeRequest,
    SketchDto, SketchSession, Vec2,
};

const XY: PlaneRef = PlaneRef::OriginPlane {
    plane: OriginPlane::Xy,
};

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn session() -> SketchSession {
    SketchSession::new("overconstraint hardening", XY, XY.basis().unwrap(), false)
}

fn assert_undo_restores(session: &mut SketchSession, before: &SketchDto) {
    let mut expected = before.clone();
    expected.can_redo = true;
    assert_eq!(session.undo().unwrap().sketch, expected);
}

fn line(session: &mut SketchSession, a: Vec2, b: Vec2) -> EntityId {
    session.add_line(a, b, true).unwrap().entity_id
}

fn point(session: &mut SketchSession, at: Vec2) -> EntityId {
    session.add_point(at).unwrap().entities[0]
}

fn circle(session: &mut SketchSession, center: Vec2, radius: f64) -> EntityId {
    session
        .add_circle(CircleMode::CenterDiameter, center, center + v(radius, 0.0))
        .unwrap()
        .entities[0]
}

fn assert_redundant(
    session: &mut SketchSession,
    constraint: Constraint,
    expected_dependencies: &[&str],
) {
    let before = session.dto();
    let error = session.add_constraint(constraint).unwrap_err();
    match error {
        SessionError::RedundantConstraint {
            rejected,
            implied_by,
        } => {
            assert_eq!(rejected.kind, constraint.kind_str());
            for expected in expected_dependencies {
                assert!(
                    implied_by
                        .iter()
                        .any(|dependency| dependency.kind == *expected),
                    "expected {expected} in dependencies {implied_by:?}"
                );
            }
        }
        other => panic!("expected redundant constraint, got {other:?}"),
    }
    assert_eq!(session.dto(), before, "redundant rejection must be atomic");
}

#[test]
fn parallel_and_perpendicular_dependency_rejects_the_implied_right_angle() {
    let mut s = session();
    let bottom = line(&mut s, v(0.0, 0.0), v(40.0, 0.0));
    let right = line(&mut s, v(40.0, 0.0), v(40.0, 25.0));
    let top = line(&mut s, v(0.0, 25.0), v(40.0, 25.0));

    s.add_constraint(Constraint::Parallel { a: bottom, b: top })
        .unwrap();
    s.add_constraint(Constraint::Perpendicular {
        a: bottom,
        b: right,
    })
    .unwrap();

    assert_redundant(
        &mut s,
        Constraint::Perpendicular { a: top, b: right },
        &["parallel", "perpendicular"],
    );
}

#[test]
fn two_right_angles_reject_the_implied_parallel_relation() {
    let mut s = session();
    let a = line(&mut s, v(0.0, 0.0), v(30.0, 0.0));
    let b = line(&mut s, v(45.0, 0.0), v(45.0, 20.0));
    let c = line(&mut s, v(0.0, 35.0), v(30.0, 35.0));
    s.add_constraint(Constraint::Perpendicular { a, b })
        .unwrap();
    s.add_constraint(Constraint::Perpendicular { a: b, b: c })
        .unwrap();

    assert_redundant(&mut s, Constraint::Parallel { a, b: c }, &["perpendicular"]);
}

#[test]
fn transitive_relation_families_reject_the_closing_edge() {
    let mut s = session();
    let a = line(&mut s, v(0.0, 0.0), v(20.0, 0.0));
    let b = line(&mut s, v(0.0, 10.0), v(20.0, 10.0));
    let c = line(&mut s, v(0.0, 20.0), v(20.0, 20.0));
    s.add_constraint(Constraint::Parallel { a, b }).unwrap();
    s.add_constraint(Constraint::Parallel { a: b, b: c })
        .unwrap();
    assert_redundant(&mut s, Constraint::Parallel { a, b: c }, &["parallel"]);

    let mut s = session();
    let a = line(&mut s, v(0.0, 0.0), v(10.0, 0.0));
    let b = line(&mut s, v(0.0, 10.0), v(10.0, 10.0));
    let c = line(&mut s, v(0.0, 20.0), v(10.0, 20.0));
    s.add_constraint(Constraint::Equal { a, b }).unwrap();
    s.add_constraint(Constraint::Equal { a: b, b: c }).unwrap();
    assert_redundant(&mut s, Constraint::Equal { a, b: c }, &["equal"]);

    let mut s = session();
    let a = line(&mut s, v(0.0, 0.0), v(10.0, 0.0));
    let b = line(&mut s, v(15.0, 0.0), v(25.0, 0.0));
    let c = line(&mut s, v(30.0, 0.0), v(40.0, 0.0));
    s.add_constraint(Constraint::Collinear { a, b }).unwrap();
    s.add_constraint(Constraint::Collinear { a: b, b: c })
        .unwrap();
    assert_redundant(&mut s, Constraint::Collinear { a, b: c }, &["collinear"]);

    let mut s = session();
    let a = point(&mut s, v(0.0, 0.0));
    let b = point(&mut s, v(10.0, 3.0));
    let c = point(&mut s, v(20.0, -2.0));
    s.add_constraint(Constraint::Coincident { a, b }).unwrap();
    s.add_constraint(Constraint::Coincident { a: b, b: c })
        .unwrap();
    assert_redundant(&mut s, Constraint::Coincident { a, b: c }, &["coincident"]);

    let mut s = session();
    let a = circle(&mut s, v(0.0, 0.0), 4.0);
    let b = circle(&mut s, v(15.0, 2.0), 6.0);
    let c = circle(&mut s, v(30.0, -3.0), 8.0);
    s.add_constraint(Constraint::Concentric { a, b }).unwrap();
    s.add_constraint(Constraint::Concentric { a: b, b: c })
        .unwrap();
    assert_redundant(&mut s, Constraint::Concentric { a, b: c }, &["concentric"]);
}

#[test]
fn mixed_relations_reject_equivalent_direction_and_incidence() {
    let mut s = session();
    let horizontal = line(&mut s, v(0.0, 0.0), v(20.0, 0.0));
    let vertical = line(&mut s, v(30.0, 0.0), v(30.0, 20.0));
    s.add_constraint(Constraint::Horizontal { entity: horizontal })
        .unwrap();
    s.add_constraint(Constraint::Vertical { entity: vertical })
        .unwrap();
    assert_redundant(
        &mut s,
        Constraint::Perpendicular {
            a: horizontal,
            b: vertical,
        },
        &["horizontal", "vertical"],
    );

    let mut s = session();
    let reference = line(&mut s, v(0.0, 0.0), v(20.0, 0.0));
    let follower = line(&mut s, v(0.0, 10.0), v(20.0, 10.0));
    s.add_constraint(Constraint::Horizontal { entity: reference })
        .unwrap();
    s.add_constraint(Constraint::Parallel {
        a: reference,
        b: follower,
    })
    .unwrap();
    assert_redundant(
        &mut s,
        Constraint::Horizontal { entity: follower },
        &["horizontal", "parallel"],
    );

    let mut s = session();
    let carrier = line(&mut s, v(0.0, 0.0), v(20.0, 0.0));
    let midpoint = point(&mut s, v(8.0, 5.0));
    s.add_constraint(Constraint::Midpoint {
        a: midpoint,
        b: carrier,
    })
    .unwrap();
    assert_redundant(
        &mut s,
        Constraint::Coincident {
            a: midpoint,
            b: carrier,
        },
        &["midpoint"],
    );
}

#[test]
fn symmetry_rejects_relations_already_carried_by_the_mirror_equations() {
    let mut s = session();
    let axis = line(&mut s, v(0.0, -20.0), v(0.0, 30.0));
    let a = line(&mut s, v(8.0, 2.0), v(15.0, 18.0));
    let b = line(&mut s, v(-6.0, 3.0), v(-16.0, 20.0));
    s.add_constraint(Constraint::Symmetry { a, b, axis })
        .unwrap();
    assert_redundant(&mut s, Constraint::Equal { a, b }, &["symmetry"]);

    let mut s = session();
    let axis = line(&mut s, v(0.0, -20.0), v(0.0, 30.0));
    let carrier = s.add_line(v(-8.0, 5.0), v(11.0, 7.0), true).unwrap();
    s.add_constraint(Constraint::Symmetry {
        a: carrier.start_point_id,
        b: carrier.end_point_id,
        axis,
    })
    .unwrap();
    assert_redundant(
        &mut s,
        Constraint::Perpendicular {
            a: carrier.entity_id,
            b: axis,
        },
        &["symmetry"],
    );
}

#[test]
fn tangent_and_center_acquisition_circuits_reject_their_implied_relations() {
    let mut s = session();
    let a = line(&mut s, v(-15.0, 8.0), v(15.0, 8.0));
    let b = line(&mut s, v(-10.0, 14.0), v(12.0, 14.0));
    let curve = circle(&mut s, v(0.0, 0.0), 5.0);
    s.add_constraint(Constraint::Collinear { a, b }).unwrap();
    s.add_constraint(Constraint::Tangent { a, b: curve })
        .unwrap();
    assert_redundant(
        &mut s,
        Constraint::Tangent { a: b, b: curve },
        &["collinear", "tangent"],
    );

    let mut s = session();
    let _center = point(&mut s, v(3.0, 4.0));
    let a = circle(&mut s, v(3.0, 4.0), 5.0);
    let b = circle(&mut s, v(3.0, 4.0), 8.0);
    assert_eq!(
        s.dto()
            .constraints
            .iter()
            .filter(|constraint| constraint.constraint.kind_str() == "center_coincident")
            .count(),
        2,
        "both authored centers should stay associated with the selected point"
    );
    assert_redundant(
        &mut s,
        Constraint::Concentric { a, b },
        &["center_coincident"],
    );

    let mut s = session();
    let at_origin = point(&mut s, Vec2::ZERO);
    assert_redundant(
        &mut s,
        Constraint::Fix { entity: at_origin },
        &["origin_coincident"],
    );
}

#[test]
fn a_satisfied_relation_on_fixed_geometry_is_redundant_but_fixing_partial_geometry_is_valid() {
    let mut s = session();
    let horizontal = line(&mut s, v(0.0, 0.0), v(20.0, 0.0));
    s.add_constraint(Constraint::Fix { entity: horizontal })
        .unwrap();
    assert_redundant(
        &mut s,
        Constraint::Horizontal { entity: horizontal },
        &["fix"],
    );

    let mut s = session();
    let horizontal = line(&mut s, v(0.0, 0.0), v(20.0, 0.0));
    s.add_constraint(Constraint::Horizontal { entity: horizontal })
        .unwrap();
    s.add_constraint(Constraint::Fix { entity: horizontal })
        .expect("Fix still contributes placement and length after Horizontal");
}

#[test]
fn a_batch_keeps_an_independent_subset_of_its_dependency_circuit() {
    let mut s = session();
    let a = line(&mut s, v(0.0, 0.0), v(20.0, 0.0));
    let b = line(&mut s, v(0.0, 10.0), v(20.0, 10.0));
    let c = line(&mut s, v(0.0, 20.0), v(20.0, 20.0));
    let before = s.dto();
    let result = s
        .add_constraints(vec![
            Constraint::Parallel { a, b },
            Constraint::Parallel { a: b, b: c },
            Constraint::Parallel { a, b: c },
        ])
        .unwrap();
    assert_eq!(result.sketch.constraints.len(), 2);
    assert_eq!(
        result.sketch.constraints[0].constraint,
        Constraint::Parallel { a, b }
    );
    assert_eq!(
        result.sketch.constraints[1].constraint,
        Constraint::Parallel { a: b, b: c }
    );
    assert_redundant(&mut s, Constraint::Parallel { a, b: c }, &["parallel"]);
    assert_undo_restores(&mut s, &before);
    assert_eq!(s.redo().unwrap().sketch, result.sketch);
}

#[test]
fn bulk_fix_of_a_connected_rectangle_is_reversible_in_one_step() {
    for at_origin in [false, true] {
        for include_points in [false, true] {
            let mut s = session();
            let first = if at_origin { Vec2::ZERO } else { v(10.0, 10.0) };
            s.add_rectangle(RectangleMode::TwoPoint, first, first + v(40.0, 30.0))
                .unwrap();
            let before = s.dto();
            let ids = before
                .entities
                .iter()
                .filter(|e| include_points || matches!(e, EntityDto::Line { .. }))
                .map(EntityDto::id)
                .collect::<Vec<_>>();
            assert!(!before.dof.fully_defined);
            let fixed = s.toggle_fix_entities(ids.clone()).unwrap().sketch;
            assert!(fixed.dof.fully_defined);
            assert_eq!(
                fixed
                    .constraints
                    .iter()
                    .filter(|c| matches!(c.constraint, Constraint::Fix { .. }))
                    .count(),
                ids.len()
            );
            assert_eq!(
                fixed.entities.iter().map(EntityDto::id).collect::<Vec<_>>(),
                before
                    .entities
                    .iter()
                    .map(EntityDto::id)
                    .collect::<Vec<_>>()
            );
            assert_undo_restores(&mut s, &before);
            assert_eq!(s.redo().unwrap().sketch, fixed);

            let unfixed = s.toggle_fix_entities(ids).unwrap().sketch;
            assert_eq!(unfixed.constraints, before.constraints);
            assert_eq!(unfixed.entities, before.entities);
            assert_eq!(unfixed.dof, before.dof);
            assert_undo_restores(&mut s, &fixed);
            assert_eq!(s.redo().unwrap().sketch, unfixed);
        }
    }
}

#[test]
fn mixed_fix_unfix_can_release_an_anchor_even_if_the_new_anchor_is_implied() {
    let mut s = session();
    let datum = point(&mut s, Vec2::ZERO);
    let free_line = line(&mut s, v(10.0, 10.0), v(30.0, 15.0));
    s.toggle_fix(free_line).unwrap();
    let before = s.dto();
    let result = s
        .toggle_fix_entities(vec![datum, free_line])
        .unwrap()
        .sketch;
    assert!(!result.dof.fully_defined);
    assert!(s.sketch().fix_constraint_on(free_line).is_none());
    assert!(s.sketch().fix_constraint_on(datum).is_some());
    assert_undo_restores(&mut s, &before);
    assert_eq!(s.redo().unwrap().sketch, result);
}

#[test]
fn bulk_hv_on_parallel_lines_keeps_one_new_driver_in_either_order() {
    for horizontal in [false, true] {
        for reversed in [false, true] {
            let mut s = session();
            let direction = if horizontal {
                v(20.0, 5.0)
            } else {
                v(5.0, 20.0)
            };
            let a = line(&mut s, v(10.0, 10.0), v(10.0, 10.0) + direction);
            let b = line(&mut s, v(40.0, 40.0), v(40.0, 40.0) + direction);
            s.add_constraint(Constraint::Parallel { a, b }).unwrap();
            let before = s.dto();
            let ids = if reversed { [b, a] } else { [a, b] };
            let constraints = ids.map(|entity| {
                if horizontal {
                    Constraint::Horizontal { entity }
                } else {
                    Constraint::Vertical { entity }
                }
            });
            let after = s.add_constraints(constraints.to_vec()).unwrap().sketch;
            assert_eq!(after.constraints.len(), before.constraints.len() + 1);
            assert_eq!(after.constraints.last().unwrap().constraint, constraints[0]);
            for id in ids {
                let (start, end) = s.sketch().resolved_line(id).unwrap();
                let delta = end - start;
                assert!((if horizontal { delta.y } else { delta.x }).abs() < 1e-7);
                assert!((delta.length() - direction.length()).abs() < 1e-7);
            }
            assert_undo_restores(&mut s, &before);
            assert_eq!(s.redo().unwrap().sketch, after);
        }
    }
}

#[test]
fn wholly_implied_and_contradictory_batches_still_reject_atomically() {
    let mut s = session();
    let a = line(&mut s, v(0.0, 0.0), v(20.0, 0.0));
    let b = line(&mut s, v(0.0, 10.0), v(20.0, 10.0));
    let c = line(&mut s, v(0.0, 20.0), v(20.0, 20.0));
    s.add_constraint(Constraint::Parallel { a, b }).unwrap();
    s.add_constraint(Constraint::Parallel { a: b, b: c })
        .unwrap();
    s.add_constraint(Constraint::Horizontal { entity: a })
        .unwrap();
    let before = s.dto();
    let error = s
        .add_constraints(vec![
            Constraint::Horizontal { entity: b },
            Constraint::Horizontal { entity: c },
        ])
        .unwrap_err();
    assert!(matches!(error, SessionError::RedundantConstraint { .. }));
    assert_eq!(s.dto(), before);

    let mut s = session();
    let free = line(&mut s, v(0.0, 0.0), v(20.0, 5.0));
    let fixed = line(&mut s, v(40.0, 0.0), v(40.0, 20.0));
    s.toggle_fix(fixed).unwrap();
    let before = s.dto();
    s.add_constraints(vec![
        Constraint::Horizontal { entity: free },
        Constraint::Horizontal { entity: fixed },
    ])
    .unwrap_err();
    assert_eq!(
        s.dto(),
        before,
        "a conflict must not leave the first member applied"
    );
}

fn tangent_fixture(
    radius: f64,
    angle: f64,
    center: Vec2,
) -> (SketchSession, EntityId, EntityId, EntityId, Vec2) {
    let mut s = session();
    let normal = v(angle.cos(), angle.sin());
    let tangent = v(-normal.y, normal.x);
    let contact = center + normal * radius;
    let curve = s
        .add_circle_selective(CircleMode::CenterDiameter, center, contact, true)
        .unwrap()
        .entities[0];
    let carrier = line(
        &mut s,
        contact - tangent * (2.0 * radius),
        contact + tangent * (2.0 * radius),
    );
    let p = s
        .add_point_on_selective(contact, None, true)
        .unwrap()
        .entities[0];
    s.toggle_fix(curve).unwrap();
    s.toggle_fix(carrier).unwrap();
    (s, curve, carrier, p, contact)
}

#[test]
fn tangent_point_incidence_is_independent_in_both_orders_and_rotated_poses() {
    for (radius, angle, center) in [
        (5.0, 0.0, v(10.0, 10.0)),
        (1.0, 0.63, v(-25.0, 33.0)),
        (500.0, std::f64::consts::FRAC_PI_2, v(1000.0, -2000.0)),
    ] {
        for circle_first in [false, true] {
            let (mut s, curve, carrier, p, contact) = tangent_fixture(radius, angle, center);
            let [first, second] = if circle_first {
                [curve, carrier]
            } else {
                [carrier, curve]
            };
            s.add_constraint(Constraint::Coincident { a: p, b: first })
                .unwrap();
            let before = s.dto();
            let after = s
                .add_constraint(Constraint::Coincident { a: p, b: second })
                .unwrap()
                .sketch;
            assert!(s.sketch().point_position(p).unwrap().distance(contact) < 1e-7);
            assert_eq!(after.constraints.len(), before.constraints.len() + 1);
            assert_undo_restores(&mut s, &before);
            assert_eq!(s.redo().unwrap().sketch, after);
        }
    }
}

#[test]
fn tangent_point_cannot_leave_the_line_until_its_new_incidence_is_removed() {
    let (mut s, curve, carrier, p, contact) = tangent_fixture(5.0, 0.0, v(10.0, 10.0));
    s.add_constraint(Constraint::Coincident { a: p, b: curve })
        .unwrap();
    let attached = s
        .add_constraint(Constraint::Coincident { a: p, b: carrier })
        .unwrap()
        .constraint_id;
    let target = v(10.0, 15.0);
    let before = s.dto();
    let error = s
        .move_point(MovePointRequest {
            point_id: p,
            to_raw: target,
            phase: DragPhase::Single,
            ctrl_held: true,
        })
        .unwrap_err();
    assert!(matches!(
        error,
        SessionError::InvalidConstraint(ref message)
            if message.contains("could not move this point")
                && message.contains("review the point's constraints")
    ));
    assert_eq!(s.dto(), before);
    assert!(s.sketch().point_position(p).unwrap().distance(contact) < 1e-7);
    s.delete_constraint(attached).unwrap();
    s.move_point(MovePointRequest {
        point_id: p,
        to_raw: target,
        phase: DragPhase::Single,
        ctrl_held: true,
    })
    .unwrap();
    assert!(s.sketch().point_position(p).unwrap().distance(target) < 1e-7);
}

#[test]
fn batch_admission_keeps_a_singular_incidence_and_rejects_redundant_typed_drivers() {
    let (mut s, curve, carrier, p, contact) = tangent_fixture(5.0, 0.0, v(10.0, 10.0));
    s.add_constraint(Constraint::Coincident { a: p, b: curve })
        .unwrap();
    let free = line(&mut s, v(40.0, 40.0), v(60.0, 45.0));
    let before = s.dto();
    let after = s
        .add_constraints(vec![
            Constraint::Coincident { a: p, b: carrier },
            Constraint::Horizontal { entity: free },
        ])
        .unwrap()
        .sketch;
    assert_eq!(after.constraints.len(), before.constraints.len() + 2);
    assert!(s.sketch().point_position(p).unwrap().distance(contact) < 1e-7);
    assert_undo_restores(&mut s, &before);
    assert_eq!(s.redo().unwrap().sketch, after);

    let mut s = session();
    let fixed = line(&mut s, v(0.0, 0.0), v(20.0, 0.0));
    let free = line(&mut s, v(40.0, 40.0), v(60.0, 45.0));
    s.toggle_fix(fixed).unwrap();
    let before = s.dto();
    s.add_constraints(vec![
        Constraint::Horizontal { entity: free },
        Constraint::Distance {
            from: fixed,
            to: None,
            value: 20.0,
        },
    ])
    .unwrap_err();
    assert_eq!(
        s.dto(),
        before,
        "do not discard an explicit driver while keeping the other batch members"
    );
}

#[test]
fn a_variable_measurement_at_a_stationary_pose_stays_driving() {
    let (mut s, curve, _, p, _) = tangent_fixture(5.0, 0.0, v(10.0, 10.0));
    let carrier = line(&mut s, v(20.0, 0.0), v(20.0, 20.0));
    s.toggle_fix(carrier).unwrap();
    s.add_constraint(Constraint::Coincident { a: p, b: curve })
        .unwrap();
    let result = s
        .add_dimension(DimensionRequest {
            entities: vec![p, carrier],
            text_pos: v(17.0, 8.0),
            value_text: None,
        })
        .unwrap()
        .sketch;
    let dimension = &result.dimensions[0];
    assert_eq!(dimension.mode, DimensionMode::Driving);
    let cid = dimension.constraint_id;
    s.set_dimension_mode(SetDimensionModeRequest {
        constraint_id: cid,
        mode: DimensionMode::Reference,
    })
    .unwrap();
    let converted = s
        .set_dimension_mode(SetDimensionModeRequest {
            constraint_id: cid,
            mode: DimensionMode::Driving,
        })
        .unwrap();
    assert_eq!(converted.sketch.dimensions[0].mode, DimensionMode::Driving);
}

#[test]
fn implied_dimensions_become_reference_instead_of_adding_solver_rows() {
    let mut s = session();
    let a = line(&mut s, v(0.0, 0.0), v(20.0, 0.0));
    let b = line(&mut s, v(0.0, 10.0), v(20.0, 10.0));
    s.add_constraint(Constraint::Equal { a, b }).unwrap();
    s.add_dimension(DimensionRequest {
        entities: vec![a],
        text_pos: v(10.0, -5.0),
        value_text: None,
    })
    .unwrap();
    let result = s
        .add_dimension(DimensionRequest {
            entities: vec![b],
            text_pos: v(10.0, 15.0),
            value_text: None,
        })
        .unwrap()
        .sketch;
    assert_eq!(result.dimensions.len(), 2);
    assert_eq!(result.dimensions[0].mode, DimensionMode::Driving);
    assert_eq!(result.dimensions[1].mode, DimensionMode::Reference);

    let mut s = session();
    let a = line(&mut s, v(0.0, 0.0), v(20.0, 0.0));
    let b = line(&mut s, v(30.0, 0.0), v(30.0, 20.0));
    s.add_constraint(Constraint::Perpendicular { a, b })
        .unwrap();
    let result = s
        .add_dimension(DimensionRequest {
            entities: vec![a, b],
            text_pos: v(25.0, 5.0),
            value_text: None,
        })
        .unwrap()
        .sketch;
    assert_eq!(result.dimensions.len(), 1);
    assert_eq!(result.dimensions[0].mode, DimensionMode::Reference);

    let mut s = session();
    let a = circle(&mut s, v(0.0, 0.0), 6.0);
    let b = circle(&mut s, v(20.0, 0.0), 9.0);
    s.add_constraint(Constraint::Equal { a, b }).unwrap();
    s.add_dimension(DimensionRequest {
        entities: vec![a],
        text_pos: v(0.0, -10.0),
        value_text: None,
    })
    .unwrap();
    let result = s
        .add_dimension(DimensionRequest {
            entities: vec![b],
            text_pos: v(20.0, -10.0),
            value_text: None,
        })
        .unwrap()
        .sketch;
    assert_eq!(result.dimensions.len(), 2);
    assert_eq!(result.dimensions[0].mode, DimensionMode::Driving);
    assert_eq!(result.dimensions[1].mode, DimensionMode::Reference);
}
