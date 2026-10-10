//! Review regressions: endpoint identity, point ownership and angle edit data.
use limo_cad_sketch::*;
use std::f64::consts::{FRAC_PI_2, TAU};

const XY: PlaneRef = PlaneRef::OriginPlane {
    plane: OriginPlane::Xy,
};
fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}
fn session() -> SketchSession {
    SketchSession::new("Sketch1", XY, XY.basis().unwrap(), false)
}

#[test]
fn clockwise_acquisitions_follow_stored_endpoints() {
    for fixed in [false, true] {
        for connected in [false, true] {
            for locked_radius in [None, Some(5.0)] {
                let mut s = session();
                let a = s.add_point(v(5.0, 0.0)).unwrap().entities[0];
                let b = s.add_point(v(0.0, -5.0)).unwrap().entities[0];
                if connected {
                    s.add_line(v(5.0, 0.0), v(9.0, 3.0), false).unwrap();
                    s.add_line(v(0.0, -5.0), v(-3.0, -9.0), false).unwrap();
                }
                if fixed {
                    s.toggle_fix(a).unwrap();
                    s.toggle_fix(b).unwrap();
                }
                let arc = s
                    .add_arc_center_locked(
                        (v(0.0, 0.0), v(5.0, 0.0), v(0.0, -5.0)),
                        false,
                        locked_radius,
                        None,
                        None,
                        Some(-FRAC_PI_2),
                    )
                    .unwrap()
                    .entities[0];
                let dto = s.dto();
                for (point, expected) in [(a, "End"), (b, "Start")] {
                    assert!(dto.constraints.iter().any(|c| matches!(c.constraint,
                        Constraint::ArcEndpointCoincident { point: p, arc: owner, end }
                        if p == point && owner == arc && format!("{end:?}") == expected)));
                }
                let EntityDto::Arc {
                    radius,
                    start_angle,
                    end_angle,
                    ..
                } = dto.entities.iter().find(|e| e.id() == arc).unwrap()
                else {
                    panic!()
                };
                assert!((radius - 5.0).abs() < 1e-6);
                assert!((end_angle - start_angle - FRAC_PI_2).abs() < 1e-6);
                assert!(s.sketch().point_position(a).unwrap().distance(v(5.0, 0.0)) < 1e-6);
                assert!(s.sketch().point_position(b).unwrap().distance(v(0.0, -5.0)) < 1e-6);
            }
        }
    }
}

#[test]
fn typed_sweep_does_not_acquire_an_off_angle_or_off_radius_cursor_point() {
    for hint in [v(0.0, -5.0), v(-5.2, 0.0)] {
        let mut s = session();
        let point = s.add_point(hint).unwrap().entities[0];
        s.toggle_fix(point).unwrap();
        s.add_arc_center_locked(
            (Vec2::ZERO, v(5.0, 0.0), hint),
            false,
            Some(5.0),
            None,
            Some("90"),
            Some(FRAC_PI_2),
        )
        .unwrap();
        assert!(!s.dto().constraints.iter().any(|c| matches!(c.constraint,
            Constraint::ArcEndpointCoincident { point: p, .. } if p == point)));
        assert_eq!(s.sketch().point_position(point), Some(hint));
    }
}

#[test]
fn driving_arc_angle_retains_formula_and_reference_mode_keeps_formatting() {
    for (text, sweep) in [
        ("=45*2", FRAC_PI_2),
        ("=-45*2", -FRAC_PI_2),
        ("=180*2", TAU),
    ] {
        let mut s = session();
        s.add_arc_center_locked(
            (v(20.0, 20.0), v(25.0, 20.0), v(20.0, 25.0)),
            true,
            None,
            None,
            Some(text),
            Some(sweep),
        )
        .unwrap();
        let d = s.dto().dimensions[0].clone();
        assert_eq!(d.param_expression.as_deref(), Some(&text[1..]));
        assert!(d.param_id.is_some() && d.param_name.is_some());
        assert!((d.value - sweep.to_degrees()).abs() < 1e-6);

        s.edit_dimension(EditDimensionRequest {
            constraint_id: d.constraint_id,
            text: d.param_expression.clone().unwrap(),
        })
        .unwrap();
        assert_eq!(s.dto().dimensions[0].param_expression, d.param_expression);
        s.set_dimension_mode(SetDimensionModeRequest {
            constraint_id: d.constraint_id,
            mode: DimensionMode::Reference,
        })
        .unwrap();
        assert_eq!(
            s.dto().dimensions[0].text,
            format!("({:.2}°)", d.value.abs())
        );
        s.set_dimension_mode(SetDimensionModeRequest {
            constraint_id: d.constraint_id,
            mode: DimensionMode::Driving,
        })
        .unwrap();
        let dto = s.dto();
        assert_eq!(
            s.sketch()
                .params()
                .get(dto.dimensions[0].param_id.unwrap())
                .unwrap()
                .kind,
            ParamKind::Angle
        );
    }
}

#[test]
fn deleting_arc_preserves_authored_shared_and_constrained_points() {
    for protected in ["authored", "shared", "fixed", "adopted"] {
        let mut s = session();
        let authored =
            (protected == "authored").then(|| s.add_point(v(25.0, 20.0)).unwrap().entities[0]);
        let arc = s
            .add_arc_center(v(20.0, 20.0), v(25.0, 20.0), v(20.0, 25.0))
            .unwrap()
            .entities[0];
        let point = s.sketch().nearest_point(v(25.0, 20.0), 1e-6).unwrap().0;
        let shared = if protected == "shared" {
            Some(
                s.add_line(v(25.0, 20.0), v(32.0, 24.0), false)
                    .unwrap()
                    .entity_id,
            )
        } else {
            None
        };
        if protected == "fixed" {
            s.toggle_fix(point).unwrap();
        }
        if protected == "adopted" {
            assert_eq!(s.add_point(v(25.0, 20.0)).unwrap().entities[0], point);
        }
        if let Some(authored) = authored {
            assert_eq!(authored, point);
        }
        let before = s.dto();
        let result = s.delete_entity(arc).unwrap();
        assert!(!result.removed.contains(&point), "{protected}");
        assert!(s.sketch().point_position(point).is_some());
        assert_eq!(s.undo().unwrap().sketch.entities, before.entities);
        s.redo().unwrap();
        if let Some(line) = shared {
            s.delete_entity(line).unwrap();
            assert!(
                s.dto().entities.is_empty(),
                "last owner removes shared generated handles"
            );
        }
    }
}

#[test]
fn negative_literal_angle_keeps_its_signed_editor_value() {
    let mut s = session();
    s.add_arc_center_locked(
        (v(20.0, 20.0), v(25.0, 20.0), v(20.0, 15.0)),
        true,
        None,
        None,
        Some("-90"),
        Some(-FRAC_PI_2),
    )
    .unwrap();
    let d = s.dto().dimensions[0].clone();
    assert_eq!(d.value, -90.0);
    assert_eq!(d.text, "90.00°");
    assert!(d.param_expression.is_none());
    s.edit_dimension(EditDimensionRequest {
        constraint_id: d.constraint_id,
        text: d.value.to_string(),
    })
    .unwrap();
    assert_eq!(
        s.sketch().params().get(d.param_id.unwrap()).unwrap().value,
        -90.0
    );
}

#[test]
fn all_creation_tools_clean_up_only_their_generated_handles() {
    for tool in [
        "line",
        "midpoint_line",
        "rectangle",
        "center_rectangle",
        "slot",
        "polygon",
        "arc",
        "arc3",
        "circle",
        "spline",
    ] {
        let mut s = session();
        s.set_grid_snap(false);
        match tool {
            "line" => {
                s.add_line(v(20.0, 20.0), v(30.0, 24.0), true).unwrap();
            }
            "midpoint_line" => {
                s.add_line_midpoint(v(20.0, 20.0), v(30.0, 24.0), true)
                    .unwrap();
            }
            "rectangle" | "center_rectangle" => {
                s.add_rectangle(
                    if tool == "rectangle" {
                        RectangleMode::TwoPoint
                    } else {
                        RectangleMode::Center
                    },
                    v(20.0, 20.0),
                    v(30.0, 24.0),
                )
                .unwrap();
            }
            "slot" => {
                s.add_slot(&SlotRequest {
                    ctrl_held: false,
                    mode: SlotMode::CenterToCenter,
                    p1: v(20.0, 20.0),
                    p2: v(40.0, 20.0),
                    cursor: v(30.0, 25.0),
                    width_mm: None,
                    width_text: None,
                })
                .unwrap();
            }
            "polygon" => {
                s.polygon_create(&PolygonRequest {
                    center: v(20.0, 20.0),
                    edge_count: 5,
                    radius_text: "5".into(),
                    rotation_deg: 0.0,
                    mode: "inscribed".into(),
                })
                .unwrap();
            }
            "arc" => {
                s.add_arc_center(v(20.0, 20.0), v(25.0, 20.0), v(20.0, 25.0))
                    .unwrap();
            }
            "arc3" => {
                s.add_arc_3pt(v(25.0, 20.0), v(23.0, 24.0), v(20.0, 25.0))
                    .unwrap();
            }
            "circle" => {
                s.add_circle(CircleMode::CenterDiameter, v(20.0, 20.0), v(25.0, 20.0))
                    .unwrap();
            }
            "spline" => {
                s.add_spline(&SplineRequest {
                    points: vec![v(20.0, 20.0), v(23.0, 27.0), v(30.0, 24.0)],
                })
                .unwrap();
            }
            _ => unreachable!(),
        }
        let before = s.dto();
        let curves: Vec<_> = before
            .entities
            .iter()
            .filter(|e| !matches!(e, EntityDto::Point { .. }))
            .map(|e| e.id())
            .collect();
        s.delete_entities(&curves).unwrap();
        assert!(
            s.dto().entities.is_empty(),
            "{tool}: {:?}",
            s.dto().entities
        );
        assert_eq!(s.undo().unwrap().sketch.entities, before.entities);
        s.redo().unwrap();
        assert!(s.dto().entities.is_empty(), "{tool} redo");
    }
}

#[test]
fn trim_keeps_explicit_endpoint_but_removes_unused_generated_endpoint() {
    for authored in [false, true] {
        let mut s = session();
        if authored {
            s.add_point(v(40.0, 30.0)).unwrap();
        }
        let arc = s
            .add_arc_center(v(30.0, 30.0), v(40.0, 30.0), v(20.0, 30.0))
            .unwrap()
            .entities[0];
        let point = s.sketch().nearest_point(v(40.0, 30.0), 1e-6).unwrap().0;
        s.add_line(v(30.0, 20.0), v(30.0, 50.0), true).unwrap();
        s.trim_entity(&TrimRequest {
            entity: arc,
            click: v(37.0, 37.0),
        })
        .unwrap();
        assert_eq!(s.sketch().entity(point).is_some(), authored);
        s.undo().unwrap();
        assert!(s.sketch().entity(point).is_some());
    }
}

#[test]
fn ownership_roundtrips_and_missing_legacy_ownership_preserves_points() {
    let mut s = session();
    let authored = s.add_point(v(25.0, 20.0)).unwrap().entities[0];
    let arc = s
        .add_arc_center(v(20.0, 20.0), v(25.0, 20.0), v(20.0, 25.0))
        .unwrap()
        .entities[0];
    for legacy in [false, true] {
        let mut value = serde_json::to_value(s.sketch().snapshot()).unwrap();
        if legacy {
            value.as_object_mut().unwrap().remove("generated_points");
        }
        let mut restored = Sketch::new();
        restored.restore(serde_json::from_value(value).unwrap());
        restored.remove_entity(arc);
        assert!(restored.entity(authored).is_some());
        assert_eq!(restored.entity_count(), if legacy { 2 } else { 1 });
    }
}

#[test]
fn rectangle_does_not_round_acquired_boundary_back_to_the_grid() {
    for mode in [RectangleMode::TwoPoint, RectangleMode::Center] {
        for width in [None, Some(6.0)] {
            let mut s = SketchSession::new(
                "Sketch1",
                PlaneRef::PlanarFace { face_id: FaceId(1) },
                XY.basis().unwrap(),
                true,
            );
            s.set_projected_edges(vec![ProjectedEdgeDto {
                id: 1 << 40,
                edge_id: limo_cad_core::EdgeId(1),
                points: vec![v(0.0, 10.1), v(20.0, 10.1)],
                circle: None,
            }]);
            s.add_rectangle_locked(&LockedRectangleRequest {
                mode,
                anchor: v(2.0, 2.0),
                corner_hint: v(8.04, 10.09),
                width_mm: width,
                height_mm: None,
                width_text: None,
                height_text: None,
                ctrl_held: false,
            })
            .unwrap();
            let top = s
                .dto()
                .entities
                .iter()
                .filter_map(|e| match e {
                    EntityDto::Point { position, .. } => Some(position.y),
                    _ => None,
                })
                .fold(f64::NEG_INFINITY, f64::max);
            assert!(
                (top - 10.1).abs() < 1e-6,
                "{mode:?} width={width:?}: top={top}"
            );
        }
    }
}

#[test]
fn modify_does_not_collect_a_previously_detached_point() {
    let mut s = session();
    let arc = s
        .add_arc_center(v(20.0, 20.0), v(25.0, 20.0), v(20.0, 25.0))
        .unwrap()
        .entities[0];
    let point = s.sketch().nearest_point(v(25.0, 20.0), 1e-6).unwrap().0;
    let relation = s
        .dto()
        .constraints
        .iter()
        .find(|c| {
            matches!(c.constraint,
        Constraint::ArcEndpointCoincident { point: p, .. } if p == point)
        })
        .unwrap()
        .id;
    s.delete_constraint(relation).unwrap();
    s.move_copy_entities(&MoveCopyRequest {
        entity_ids: vec![arc],
        dx: 10.0,
        dy: 0.0,
        copy: false,
    })
    .unwrap();
    assert_eq!(s.sketch().point_position(point), Some(v(25.0, 20.0)));
}
