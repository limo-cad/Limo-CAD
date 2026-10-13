//! Issue #151: circle and center-rectangle centers are selectable,
//! constrainable, and keep their shape symmetric about that center.
use limo_cad_sketch::{
    CircleMode, CircularPatternRequest, Constraint, DragPhase, EntityDto, EntityId, FilletRequest,
    LockedCircleRequest, LockedRectangleRequest, MirrorRequest, MoveCopyRequest, MovePointRequest,
    OffsetRequest, OriginPlane, PlaneRef, RectangleMode, RectangularPatternRequest, ScaleRequest,
    SketchDto, SketchSession, SnapTarget, Vec2,
};

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn session() -> SketchSession {
    let plane = PlaneRef::OriginPlane {
        plane: OriginPlane::Xy,
    };
    SketchSession::new("Centers", plane, plane.basis().unwrap(), false)
}

fn point(dto: &SketchDto, id: EntityId) -> Vec2 {
    match dto
        .entities
        .iter()
        .find(|entity| entity.id() == id)
        .expect("point entity")
    {
        EntityDto::Point { position, .. } => *position,
        other => panic!("expected a point, got {other:?}"),
    }
}

fn circle_center_of(dto: &SketchDto, id: EntityId) -> Vec2 {
    match dto
        .entities
        .iter()
        .find(|entity| entity.id() == id)
        .expect("circle entity")
    {
        EntityDto::Circle { center, .. } => *center,
        other => panic!("expected a circle, got {other:?}"),
    }
}

/// The generated handle a curve owns through `CenterCoincident`.
fn center_handles(dto: &SketchDto, curve: EntityId) -> Vec<EntityId> {
    dto.constraints
        .iter()
        .filter_map(|constraint| match constraint.constraint {
            Constraint::CenterCoincident {
                point,
                curve: owner,
            } if owner == curve => Some(point),
            _ => None,
        })
        .collect()
}

/// The center a center rectangle owns through a span midpoint.
fn rectangle_center(dto: &SketchDto) -> EntityId {
    dto.constraints
        .iter()
        .find_map(|constraint| match constraint.constraint {
            Constraint::SpanMidpoint { point, .. } => Some(point),
            _ => None,
        })
        .expect("a center rectangle must own a midpoint-bound center")
}

fn drag(session: &mut SketchSession, point_id: EntityId, to: Vec2) {
    session
        .move_point(MovePointRequest {
            point_id,
            to_raw: to,
            ctrl_held: true,
            phase: DragPhase::Single,
        })
        .unwrap();
}

#[test]
fn a_circle_exposes_one_selectable_center_without_adding_degrees_of_freedom() {
    for mode in [CircleMode::CenterDiameter, CircleMode::TwoPoint] {
        let mut s = session();
        let circle = s
            .add_circle(mode, v(20.0, 10.0), v(30.0, 10.0))
            .unwrap()
            .entities[0];
        let dto = s.dto();
        let handles = center_handles(&dto, circle);
        assert_eq!(handles.len(), 1, "{mode:?}: exactly one center handle");
        let handle = handles[0];
        assert!(
            point(&dto, handle).distance(circle_center_of(&dto, circle)) < 1e-6,
            "{mode:?}: the handle must sit on the circle center"
        );

        assert_eq!(dto.dof.value, 3, "{mode:?}: circle keeps its 3 DOF");

        let preview = s.preview_segment(v(60.0, 60.0), point(&dto, handle) + v(0.1, 0.1), false);
        assert_eq!(preview.snap, SnapTarget::Point { entity: handle });

        assert!(s.undo().unwrap().sketch.entities.is_empty());
        let redone = s.redo().unwrap().sketch;
        assert_eq!(center_handles(&redone, circle).len(), 1);

        drag(&mut s, handle, v(26.0, 14.0));
        let after = s.dto();
        assert!(circle_center_of(&after, circle).distance(v(26.0, 14.0)) < 1e-6);
        assert_eq!(after.dof.value, 3);
    }
}

#[test]
fn a_circle_center_can_be_constrained_to_rectangle_geometry() {
    let mut s = session();
    let circle = s
        .add_circle(CircleMode::CenterDiameter, v(70.0, 70.0), v(78.0, 70.0))
        .unwrap()
        .entities[0];
    let rectangle = s
        .add_rectangle(RectangleMode::TwoPoint, v(10.0, 10.0), v(40.0, 30.0))
        .unwrap();
    let corner = rectangle.entities[1];
    let handle = center_handles(&s.dto(), circle)[0];

    s.add_constraint(Constraint::Coincident {
        a: handle,
        b: corner,
    })
    .unwrap();

    let dto = s.dto();
    let center = circle_center_of(&dto, circle);
    assert!(
        center.distance(point(&dto, corner)) < 1e-6,
        "the circle center must land on the rectangle corner, got {center:?}"
    );

    match dto.entities.iter().find(|e| e.id() == circle).unwrap() {
        EntityDto::Circle { radius, .. } => assert!((*radius - 8.0).abs() < 1e-6),
        other => panic!("expected a circle, got {other:?}"),
    }

    let corner_target = v(5.0, 25.0);
    drag(&mut s, corner, corner_target);
    let dto = s.dto();
    assert!(circle_center_of(&dto, circle).distance(corner_target) < 1e-6);
}

#[test]
fn an_acquired_circle_center_reuses_the_users_point() {
    let mut s = session();
    let anchor = s.add_point(v(30.0, 30.0)).unwrap().entities[0];
    let circle = s
        .add_circle_selective(
            CircleMode::CenterDiameter,
            v(30.0, 30.0),
            v(38.0, 30.0),
            false,
        )
        .unwrap()
        .entities[0];
    let dto = s.dto();
    let handles = center_handles(&dto, circle);
    assert_eq!(handles, vec![anchor], "the acquired point is the handle");
    assert_eq!(
        dto.entities
            .iter()
            .filter(|e| matches!(e, EntityDto::Point { .. }))
            .count(),
        1,
        "no duplicate handle is created for an acquired center"
    );

    s.scale_entities(&ScaleRequest {
        entity_ids: vec![circle],
        origin: v(0.0, 0.0),
        factor_text: "2".to_string(),
    })
    .unwrap();
    let after = s.dto();
    assert!(
        circle_center_of(&after, circle).distance(point(&after, anchor)) < 1e-6,
        "the center must stay on the point it was snapped to"
    );
    match after.entities.iter().find(|e| e.id() == circle).unwrap() {
        EntityDto::Circle { radius, .. } => assert!((*radius - 16.0).abs() < 1e-6),
        other => panic!("expected a circle, got {other:?}"),
    }
}

#[test]
fn a_center_rectangle_exposes_its_center_and_stays_symmetric() {
    let mut s = session();
    let created = s
        .add_rectangle(RectangleMode::Center, v(50.0, 50.0), v(60.0, 60.0))
        .unwrap();
    let [bl, br, tr, tl] = [
        created.entities[0],
        created.entities[1],
        created.entities[2],
        created.entities[3],
    ];
    let dto = s.dto();
    let center = rectangle_center(&dto);
    assert!(
        point(&dto, center).distance(v(50.0, 50.0)) < 1e-6,
        "the center rectangle owns its center as a real point"
    );

    assert!(matches!(
        dto.constraints
            .iter()
            .find(|c| matches!(c.constraint, Constraint::SpanMidpoint { .. }))
            .map(|c| c.constraint),
        Some(Constraint::SpanMidpoint { point, start, end })
            if point == center && [start, end] == [bl, tr]
    ));

    assert_eq!(dto.dof.value, 4, "center rectangle keeps its 4 DOF");

    drag(&mut s, bl, v(30.0, 30.0));
    let after = s.dto();
    assert!(
        point(&after, center).distance(v(50.0, 50.0)) < 1e-6,
        "the center must not drift, got {:?}",
        point(&after, center)
    );
    for (corner, expected) in [
        (bl, v(30.0, 30.0)),
        (br, v(70.0, 30.0)),
        (tr, v(70.0, 70.0)),
        (tl, v(30.0, 70.0)),
    ] {
        assert!(
            point(&after, corner).distance(expected) < 1e-6,
            "corner {corner:?} should be at {expected:?}, got {:?}",
            point(&after, corner)
        );
    }
    assert_eq!(after.dof.value, 4, "the drag must not add freedom");
}

#[test]
fn a_center_on_the_origin_resizes_symmetrically_about_it() {
    let mut s = session();
    let created = s
        .add_rectangle(RectangleMode::Center, Vec2::ZERO, v(10.0, 10.0))
        .unwrap();
    let [bl, br, tr, tl] = [
        created.entities[0],
        created.entities[1],
        created.entities[2],
        created.entities[3],
    ];
    let dto = s.dto();
    let center = rectangle_center(&dto);
    assert!(
        dto.constraints.iter().any(|c| matches!(
            c.constraint,
            Constraint::OriginCoincident { entity } if entity == center
        )),
        "a center dropped on the origin is pinned there"
    );

    drag(&mut s, tr, v(25.0, 15.0));
    let after = s.dto();
    let center_at = point(&after, center);
    assert!(
        center_at.distance(Vec2::ZERO) < 1e-6,
        "the pinned center must not move, got {center_at:?}"
    );

    for corner in [bl, br, tr, tl] {
        let p = point(&after, corner);
        assert!(p.distance(Vec2::ZERO) > 1e-6);
        let mirrored = [
            point(&after, bl),
            point(&after, br),
            point(&after, tr),
            point(&after, tl),
        ]
        .into_iter()
        .any(|q| q.distance(Vec2::ZERO - p) < 1e-6);
        assert!(
            mirrored,
            "corner {p:?} has no mirror image through the origin"
        );
    }
    assert!((point(&after, tr).distance(v(25.0, 15.0))) < 1e-6);
}

#[test]
fn a_two_point_rectangle_does_not_invent_a_center() {
    let mut s = session();
    let created = s
        .add_rectangle(RectangleMode::TwoPoint, v(10.0, 10.0), v(40.0, 30.0))
        .unwrap();
    let dto = s.dto();
    assert_eq!(created.entities.len(), 8, "corners plus lines only");
    assert!(!dto
        .constraints
        .iter()
        .any(|c| matches!(c.constraint, Constraint::SpanMidpoint { .. })));
}

#[test]
fn deleting_a_center_rectangles_carriers_collects_its_center() {
    let mut s = session();
    let created = s
        .add_rectangle(RectangleMode::Center, v(20.0, 20.0), v(30.0, 24.0))
        .unwrap();
    let center = rectangle_center(&s.dto());
    let lines: Vec<EntityId> = created
        .sketch
        .entities
        .iter()
        .filter(|e| matches!(e, EntityDto::Line { .. }))
        .map(|e| e.id())
        .collect();
    let removed = s.delete_entities(&lines).unwrap();
    assert!(
        removed.removed.contains(&center),
        "the owned center must go with the shape that owns it"
    );
    assert!(s.dto().entities.is_empty(), "{:?}", s.dto().entities);
    assert_eq!(s.undo().unwrap().sketch.entities, created.sketch.entities);
    s.redo().unwrap();
    assert!(s.dto().entities.is_empty());
}

#[test]
fn copied_and_transformed_circles_keep_an_attached_center() {
    let mut s = session();
    let circle = s
        .add_circle(CircleMode::CenterDiameter, v(20.0, 10.0), v(30.0, 10.0))
        .unwrap()
        .entities[0];

    s.move_copy_entities(&MoveCopyRequest {
        entity_ids: vec![circle],
        dx: 5.0,
        dy: -3.0,
        copy: false,
    })
    .unwrap();
    let dto = s.dto();
    let handle = center_handles(&dto, circle)[0];
    assert!(point(&dto, handle).distance(v(25.0, 7.0)) < 1e-6);

    s.scale_entities(&ScaleRequest {
        entity_ids: vec![circle],
        origin: v(0.0, 0.0),
        factor_text: "2".to_string(),
    })
    .unwrap();
    let dto = s.dto();
    assert!(circle_center_of(&dto, circle).distance(v(50.0, 14.0)) < 1e-6);
    assert!(point(&dto, handle).distance(v(50.0, 14.0)) < 1e-6);

    let copied = s
        .move_copy_entities(&MoveCopyRequest {
            entity_ids: vec![circle],
            dx: 100.0,
            dy: 0.0,
            copy: true,
        })
        .unwrap();
    let copy_id = copied
        .sketch
        .entities
        .iter()
        .filter(|e| matches!(e, EntityDto::Circle { .. }))
        .map(|e| e.id())
        .find(|id| *id != circle)
        .expect("the copied circle");
    let copy_handles = center_handles(&copied.sketch, copy_id);
    assert_eq!(copy_handles.len(), 1, "the occurrence owns one handle");
    assert_ne!(copy_handles[0], handle, "handles are not shared");
    assert!(
        point(&copied.sketch, copy_handles[0]).distance(v(150.0, 14.0)) < 1e-6,
        "the copied handle follows the copied center"
    );
}

#[test]
fn deleting_unrelated_geometry_keeps_a_rounded_center_rectangle_centered() {
    let mut s = session();
    let created = s
        .add_rectangle(RectangleMode::Center, Vec2::ZERO, v(20.0, 10.0))
        .unwrap();
    let [bottom, right, top, left] = [4, 5, 6, 7].map(|i| created.entities[i]);
    for (l1, l2) in [(bottom, left), (top, right)] {
        s.fillet_lines(&FilletRequest {
            l1,
            l2,
            radius_text: "2".into(),
        })
        .unwrap();
    }
    let center = rectangle_center(&s.dto());
    let rounding_dof = s.dto().dof.value;
    let other = s
        .add_line(v(100.0, 100.0), v(120.0, 100.0), true)
        .unwrap()
        .entity_id;
    s.delete_entities(&[other]).unwrap();
    let after = s.dto();
    assert!(
        after
            .constraints
            .iter()
            .any(|c| matches!(c.constraint, Constraint::SpanMidpoint { .. })),
        "an unrelated delete must not drop the rectangle's diagonal relation"
    );
    assert_eq!(
        after.dof.value, rounding_dof,
        "the rectangle must keep its constraint count"
    );

    let (bl, tr) = (created.entities[0], created.entities[2]);
    let (bl, tr) = (point(&after, bl), point(&after, tr));
    assert!(
        ((bl + tr) * 0.5).distance(point(&after, center)) < 1e-6,
        "the surviving center must still govern its diagonal"
    );
}

#[test]
fn moving_a_center_rectangle_by_its_edges_carries_its_center() {
    let mut s = session();
    let created = s
        .add_rectangle(RectangleMode::Center, v(50.0, 50.0), v(60.0, 56.0))
        .unwrap();
    s.move_copy_entities(&MoveCopyRequest {
        entity_ids: created.entities[4..8].to_vec(),
        dx: 10.0,
        dy: 0.0,
        copy: false,
    })
    .unwrap();
    let dto = s.dto();
    assert!(
        point(&dto, created.entities[0]).distance(v(50.0, 44.0)) < 1e-6,
        "bottom-left corner should land exactly, got {:?}",
        point(&dto, created.entities[0])
    );
    assert!(
        point(&dto, created.entities[8]).distance(v(60.0, 50.0)) < 1e-6,
        "the center should travel with the rectangle, got {:?}",
        point(&dto, created.entities[8])
    );

    s.scale_entities(&ScaleRequest {
        entity_ids: created.entities[4..8].to_vec(),
        origin: v(0.0, 0.0),
        factor_text: "2".to_string(),
    })
    .unwrap();
    let dto = s.dto();
    assert!(point(&dto, created.entities[0]).distance(v(100.0, 88.0)) < 1e-6);
    assert!(point(&dto, created.entities[8]).distance(v(120.0, 100.0)) < 1e-6);
}

#[test]
fn deleting_part_of_a_center_rectangle_drops_the_unused_corner() {
    let mut s = session();
    let created = s
        .add_rectangle(RectangleMode::Center, v(50.0, 50.0), v(60.0, 56.0))
        .unwrap();
    let [bl, br, tr, tl] = [
        created.entities[0],
        created.entities[1],
        created.entities[2],
        created.entities[3],
    ];
    let (right, top) = (created.entities[5], created.entities[6]);
    s.delete_entities(&[right, top]).unwrap();
    let dto = s.dto();
    assert!(
        dto.entities.iter().all(|e| e.id() != tr),
        "the orphaned top-right corner must be collected, got {:?}",
        dto.entities
    );
    for survivor in [bl, br, tl] {
        assert!(dto.entities.iter().any(|e| e.id() == survivor));
    }
}

#[test]
fn transforms_of_a_circle_with_a_shared_anchor_reject_without_distortion() {
    let mut s = session();
    let created = s
        .add_rectangle(RectangleMode::TwoPoint, v(10.0, 10.0), v(50.0, 40.0))
        .unwrap();
    let corner = created.entities[0];
    let circle = s
        .add_circle_selective(
            CircleMode::CenterDiameter,
            v(10.0, 10.0),
            v(18.0, 10.0),
            false,
        )
        .unwrap()
        .entities[0];
    assert_eq!(center_handles(&s.dto(), circle), vec![corner]);
    let original = serde_json::to_value(s.dto().entities).unwrap();
    s.move_copy_entities(&MoveCopyRequest {
        entity_ids: vec![circle],
        dx: 100.0,
        dy: 0.0,
        copy: true,
    })
    .unwrap();
    let copied = serde_json::to_value(s.dto()).unwrap();
    s.undo().unwrap();
    assert_eq!(serde_json::to_value(s.dto().entities).unwrap(), original);
    let before = serde_json::to_value(s.dto()).unwrap();

    let moved = s
        .move_copy_entities(&MoveCopyRequest {
            entity_ids: vec![circle],
            dx: 100.0,
            dy: 0.0,
            copy: false,
        })
        .unwrap_err();
    assert!(moved.to_string().contains("conflicts"));
    assert_eq!(serde_json::to_value(s.dto()).unwrap(), before);
    let scaled = s
        .scale_entities(&ScaleRequest {
            entity_ids: vec![circle],
            origin: v(0.0, 0.0),
            factor_text: "2".into(),
        })
        .unwrap_err();
    assert!(scaled.to_string().contains("conflicts"));
    assert_eq!(serde_json::to_value(s.dto()).unwrap(), before);
    s.redo().unwrap();
    assert_eq!(serde_json::to_value(s.dto()).unwrap(), copied);
}

#[test]
fn scaling_about_an_acquired_circle_center_preserves_unselected_geometry() {
    let mut s = session();
    let rectangle = s
        .add_rectangle(RectangleMode::TwoPoint, v(10.0, 10.0), v(50.0, 40.0))
        .unwrap();
    let circle = s
        .add_circle_selective(
            CircleMode::CenterDiameter,
            v(10.0, 10.0),
            v(18.0, 10.0),
            false,
        )
        .unwrap()
        .entities[0];
    let before = s.dto();
    s.scale_entities(&ScaleRequest {
        entity_ids: vec![circle],
        origin: v(10.0, 10.0),
        factor_text: "2".into(),
    })
    .unwrap();
    let after = s.dto();
    for &point_id in &rectangle.entities[..4] {
        assert!(point(&after, point_id).distance(point(&before, point_id)) < 1e-6);
    }
    assert!(
        circle_center_of(&after, circle).distance(point(&before, rectangle.entities[0])) < 1e-6
    );
    let radius = |dto: &SketchDto| match dto
        .entities
        .iter()
        .find(|entity| entity.id() == circle)
        .unwrap()
    {
        EntityDto::Circle { radius, .. } => *radius,
        _ => panic!("Expected a circle"),
    };
    assert!((radius(&after) - 2.0 * radius(&before)).abs() < 1e-6);
    s.undo().unwrap();
    let mut restored = s.dto();
    assert!(restored.can_redo);
    restored.can_redo = before.can_redo;
    assert_eq!(
        serde_json::to_value(restored).unwrap(),
        serde_json::to_value(before).unwrap()
    );
}

#[test]
fn copying_a_center_rectangle_copies_its_center() {
    let mut s = session();
    let created = s
        .add_rectangle(RectangleMode::Center, v(20.0, 20.0), v(30.0, 26.0))
        .unwrap();
    let copied = s
        .move_copy_entities(&MoveCopyRequest {
            entity_ids: created.entities.clone(),
            dx: 100.0,
            dy: 0.0,
            copy: true,
        })
        .unwrap();
    let centers: Vec<EntityId> = copied
        .sketch
        .constraints
        .iter()
        .filter_map(|c| match c.constraint {
            Constraint::SpanMidpoint { point, .. } => Some(point),
            _ => None,
        })
        .collect();
    assert_eq!(centers.len(), 2, "source and occurrence each own a center");
    let copy_center = centers
        .into_iter()
        .find(|point| *point != created.entities[8])
        .expect("a distinct copied center");
    let copied_circle_center = match copied
        .sketch
        .entities
        .iter()
        .find(|e| e.id() == copy_center)
    {
        Some(EntityDto::Point { position, .. }) => *position,
        other => panic!("expected the copied center point, got {other:?}"),
    };
    assert!(
        copied_circle_center.distance(v(120.0, 20.0)) < 1e-6,
        "the copied center should sit at the occurrence's middle, got {copied_circle_center:?}"
    );
}

#[test]
fn an_offset_circle_owns_a_center_handle() {
    let mut s = session();
    let source = s
        .add_circle(CircleMode::CenterDiameter, v(20.0, 20.0), v(26.0, 20.0))
        .unwrap()
        .entities[0];
    let offset = s
        .offset_curve_op(&OffsetRequest {
            entity: source,
            distance_text: "4".into(),
            cursor: v(20.0, 40.0),
        })
        .unwrap();
    let derived = offset
        .sketch
        .entities
        .iter()
        .filter(|e| matches!(e, EntityDto::Circle { .. }))
        .map(|e| e.id())
        .find(|id| *id != source)
        .expect("the offset circle");
    assert_eq!(
        center_handles(&offset.sketch, derived).len(),
        1,
        "an offset circle needs a selectable center"
    );
}

#[test]
fn deleting_a_center_relation_reaps_its_handle() {
    let mut s = session();
    let circle = s
        .add_circle(CircleMode::CenterDiameter, v(20.0, 20.0), v(28.0, 20.0))
        .unwrap()
        .entities[0];
    let handle = center_handles(&s.dto(), circle)[0];
    let relation = s
        .dto()
        .constraints
        .iter()
        .find(|c| {
            matches!(c.constraint,
                Constraint::CenterCoincident { point, curve } if point == handle && curve == circle)
        })
        .expect("the center relation")
        .id;
    s.delete_constraint(relation).unwrap();
    let dto = s.dto();
    assert!(
        dto.entities.iter().all(|e| e.id() != handle),
        "the orphaned handle must be reaped, got {:?}",
        dto.entities
    );
    assert!(dto.entities.iter().any(|e| e.id() == circle));
}

#[test]
fn a_ctrl_placed_center_reuses_an_exact_vertex() {
    let mut s = session();
    let line = s.add_line(v(10.0, 10.0), v(30.0, 10.0), true).unwrap();
    let existing = line.start_point_id;
    let circle = s
        .add_circle_selective(
            CircleMode::CenterDiameter,
            v(10.0, 10.0),
            v(18.0, 10.0),
            true,
        )
        .unwrap()
        .entities[0];
    let dto = s.dto();
    assert_eq!(
        center_handles(&dto, circle),
        vec![existing],
        "a Ctrl pick exactly on a vertex reuses it"
    );
    assert_eq!(
        dto.entities
            .iter()
            .filter(|e| matches!(e, EntityDto::Point { .. }))
            .count(),
        2,
        "the line's two endpoints are still the only points"
    );
}

#[test]
fn deleting_a_filleted_center_rectangles_curves_collects_its_center() {
    let mut s = session();
    let created = s
        .add_rectangle(RectangleMode::Center, v(20.0, 20.0), v(30.0, 26.0))
        .unwrap();
    let [bottom, right, top, left] = [4, 5, 6, 7].map(|i| created.entities[i]);
    for (l1, l2) in [(bottom, left), (bottom, right), (top, right), (top, left)] {
        s.fillet_lines(&FilletRequest {
            l1,
            l2,
            radius_text: "1".into(),
        })
        .unwrap();
    }
    let curves: Vec<EntityId> = s
        .dto()
        .entities
        .iter()
        .filter(|entity| !matches!(entity, EntityDto::Point { .. }))
        .map(|entity| entity.id())
        .collect();
    assert_eq!(
        curves.len(),
        8,
        "four trimmed carriers plus four fillet arcs"
    );
    s.delete_entities(&curves).unwrap();
    assert!(
        s.dto().entities.is_empty(),
        "a fully erased rounded rectangle leaves nothing: {:?}",
        s.dto().entities
    );
}

#[test]
fn dragging_any_corner_resizes_a_center_rectangle_about_its_center() {
    let moves = [
        (v(30.0, 30.0), v(70.0, 70.0)),
        (v(70.0, 30.0), v(30.0, 70.0)),
        (v(70.0, 70.0), v(30.0, 30.0)),
        (v(30.0, 70.0), v(70.0, 30.0)),
    ];
    for (index, (target, opposite)) in moves.into_iter().enumerate() {
        let mut s = session();
        let created = s
            .add_rectangle(RectangleMode::Center, v(50.0, 50.0), v(60.0, 60.0))
            .unwrap();
        let center = rectangle_center(&s.dto());
        let dragged = created.entities[index];
        let opposite_id = created.entities[(index + 2) % 4];
        drag(&mut s, dragged, target);
        let dto = s.dto();
        assert!(
            point(&dto, dragged).distance(target) < 1e-6,
            "corner {index} did not follow the cursor"
        );
        assert!(
            point(&dto, center).distance(v(50.0, 50.0)) < 1e-6,
            "corner {index} moved the center to {:?}",
            point(&dto, center)
        );
        assert!(
            point(&dto, opposite_id).distance(opposite) < 1e-6,
            "corner {index} left the opposite corner at {:?}",
            point(&dto, opposite_id)
        );
    }
}

#[test]
fn a_dimensioned_center_rectangle_still_moves_when_a_corner_is_dragged() {
    let mut s = session();
    let created = s
        .add_rectangle_locked(&LockedRectangleRequest {
            mode: RectangleMode::Center,
            anchor: v(50.0, 50.0),
            width_mm: Some(20.0),
            height_mm: Some(10.0),
            width_text: None,
            height_text: None,
            corner_hint: v(60.0, 55.0),
            ctrl_held: false,
        })
        .unwrap();
    let bl = created.entities[0];
    assert!(point(&s.dto(), bl).distance(v(40.0, 45.0)) < 1e-6);
    drag(&mut s, bl, v(35.0, 40.0));
    let dto = s.dto();
    assert!(
        point(&dto, bl).distance(v(35.0, 40.0)) < 1e-6,
        "a dimensioned center rectangle must still translate, got {:?}",
        point(&dto, bl)
    );
}

#[test]
fn a_circle_center_snapped_onto_an_orphaned_handle_is_bound() {
    let mut s = session();
    let arc = s
        .add_arc_center(v(30.0, 30.0), v(40.0, 30.0), v(30.0, 40.0))
        .unwrap()
        .entities[0];
    let endpoints: Vec<EntityId> = s
        .dto()
        .constraints
        .iter()
        .filter_map(|constraint| match constraint.constraint {
            Constraint::ArcEndpointCoincident {
                point, arc: owner, ..
            } if owner == arc => Some(point),
            _ => None,
        })
        .collect();
    assert_eq!(endpoints.len(), 2);

    let relation = s
        .dto()
        .constraints
        .iter()
        .find(|constraint| {
            matches!(constraint.constraint,
                Constraint::ArcEndpointCoincident { point, .. } if point == endpoints[0])
        })
        .expect("the endpoint relation")
        .id;
    s.delete_constraint(relation).unwrap();
    let orphan = endpoints[0];
    let position = point(&s.dto(), orphan);

    let circle = s
        .add_circle_selective(
            CircleMode::CenterDiameter,
            position,
            position + v(6.0, 0.0),
            false,
        )
        .unwrap()
        .entities[0];
    let dto = s.dto();
    assert_eq!(
        center_handles(&dto, circle),
        vec![orphan],
        "the acquired centre must be bound, not silently dropped"
    );
    assert!(circle_center_of(&dto, circle).distance(position) < 1e-6);
}

/// Two circles drawn on one exact center, so they share a generated handle.
fn concentric_pair(s: &mut SketchSession) -> Vec<EntityId> {
    [10.0, 15.0]
        .into_iter()
        .map(|radius| {
            s.add_circle_selective(
                CircleMode::CenterDiameter,
                v(20.0, 10.0),
                v(20.0 + radius, 10.0),
                true,
            )
            .unwrap()
            .entities[0]
        })
        .collect()
}

#[test]
fn moving_all_circles_that_share_a_center_reaches_the_requested_position() {
    let mut s = session();
    let ids = concentric_pair(&mut s);
    let dto = s.dto();
    assert_eq!(
        center_handles(&dto, ids[0]),
        center_handles(&dto, ids[1]),
        "the pair shares one handle"
    );
    s.move_copy_entities(&MoveCopyRequest {
        entity_ids: ids.clone(),
        dx: 12.0,
        dy: 0.0,
        copy: false,
    })
    .unwrap();
    for id in ids {
        let actual = circle_center_of(&s.dto(), id);
        assert!(
            actual.distance(v(32.0, 10.0)) < 1e-6,
            "requested (32,10), got {actual:?}"
        );
    }
}

#[test]
fn scaling_all_circles_that_share_a_center_reaches_the_requested_position() {
    let mut s = session();
    let ids = concentric_pair(&mut s);
    s.scale_entities(&ScaleRequest {
        entity_ids: ids.clone(),
        origin: v(0.0, 0.0),
        factor_text: "2".into(),
    })
    .unwrap();
    for id in ids {
        let actual = circle_center_of(&s.dto(), id);
        assert!(
            actual.distance(v(40.0, 20.0)) < 1e-6,
            "requested (40,20), got {actual:?}"
        );
    }
}

#[test]
fn copying_circles_that_share_a_center_keeps_their_incidence() {
    let mut s = session();
    let ids = concentric_pair(&mut s);
    s.move_copy_entities(&MoveCopyRequest {
        entity_ids: ids.clone(),
        dx: 50.0,
        dy: 0.0,
        copy: true,
    })
    .unwrap();
    let dto = s.dto();
    let copied: Vec<EntityId> = dto
        .entities
        .iter()
        .filter_map(|entity| match entity {
            EntityDto::Circle { id, .. } if !ids.contains(id) => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(copied.len(), 2);
    assert_eq!(
        center_handles(&dto, copied[0]),
        center_handles(&dto, copied[1]),
        "the occurrence keeps the pair concentric"
    );
}

fn copied_rectangle_center_after_corner_drag(select_all: bool) -> Vec2 {
    let mut s = session();
    let created = s
        .add_rectangle(RectangleMode::Center, v(50.0, 50.0), v(60.0, 60.0))
        .unwrap();
    let source_ids = created.entities.clone();
    let selection = if select_all {
        source_ids.clone()
    } else {
        source_ids[4..8].to_vec()
    };
    s.move_copy_entities(&MoveCopyRequest {
        entity_ids: selection,
        dx: 50.0,
        dy: 0.0,
        copy: true,
    })
    .unwrap();
    let (center, corner) = s
        .dto()
        .constraints
        .iter()
        .find_map(|constraint| match constraint.constraint {
            Constraint::SpanMidpoint { point, start, .. } if !source_ids.contains(&point) => {
                Some((point, start))
            }
            _ => None,
        })
        .expect("the copied rectangle's diagonal");
    assert!(point(&s.dto(), center).distance(v(100.0, 50.0)) < 1e-6);
    drag(&mut s, corner, v(85.0, 35.0));
    point(&s.dto(), center)
}

#[test]
fn copying_center_rectangle_edges_keeps_centered_resize() {
    let actual = copied_rectangle_center_after_corner_drag(false);
    assert!(
        actual.distance(v(100.0, 50.0)) < 1e-6,
        "edge selection: got {actual:?}"
    );
}

#[test]
fn copying_a_whole_center_rectangle_keeps_centered_resize() {
    let actual = copied_rectangle_center_after_corner_drag(true);
    assert!(
        actual.distance(v(100.0, 50.0)) < 1e-6,
        "whole selection: got {actual:?}"
    );
}

#[test]
fn manually_binding_a_detached_arc_endpoint_to_a_circle_is_not_redundant() {
    let mut s = session();
    let arc = s
        .add_arc_center(v(30.0, 30.0), v(40.0, 30.0), v(30.0, 40.0))
        .unwrap()
        .entities[0];
    let (relation, orphan) = s
        .dto()
        .constraints
        .iter()
        .find_map(|constraint| match constraint.constraint {
            Constraint::ArcEndpointCoincident {
                point, arc: owner, ..
            } if owner == arc => Some((constraint.id, point)),
            _ => None,
        })
        .unwrap();
    s.delete_constraint(relation).unwrap();
    let circle = s
        .add_circle_selective(
            CircleMode::CenterDiameter,
            v(70.0, 70.0),
            v(76.0, 70.0),
            true,
        )
        .unwrap()
        .entities[0];
    assert!(point(&s.dto(), orphan).distance(circle_center_of(&s.dto(), circle)) > 1.0);

    let added = s.add_constraint(Constraint::CenterCoincident {
        point: orphan,
        curve: circle,
    });
    assert!(
        added.is_ok(),
        "the independent binding was rejected: {added:?}"
    );
    assert!(
        point(&s.dto(), orphan).distance(circle_center_of(&s.dto(), circle)) < 1e-6,
        "the detached handle must land on the circle center"
    );
}

#[test]
fn the_vise_recipe_circle_is_located_on_its_point_without_an_explicit_step() {
    let mut s = session();
    s.set_grid_snap(false);
    let point = s.add_point(v(20.0, 10.0)).unwrap().entities[0];
    s.add_constraint(Constraint::Fix { entity: point }).unwrap();
    let circle = s
        .add_circle_locked(&LockedCircleRequest {
            mode: CircleMode::CenterDiameter,
            anchor: v(20.0, 10.0),
            edge_hint: v(25.0, 10.0),
            diameter_mm: Some(10.0),
            diameter_text: None,
            ctrl_held: true,
        })
        .unwrap()
        .entities[0];
    let dto = s.dto();
    assert_eq!(
        center_handles(&dto, circle),
        vec![point],
        "the circle owns the point it was placed on"
    );
    assert!(circle_center_of(&dto, circle).distance(v(20.0, 10.0)) < 1e-6);

    assert!(s
        .add_constraint(Constraint::CenterCoincident {
            point,
            curve: circle
        })
        .is_err());
}

fn rectangle_and_circle(s: &mut SketchSession, circle_first: bool) -> (Vec<EntityId>, EntityId) {
    let circle = |s: &mut SketchSession| {
        s.add_circle_selective(
            CircleMode::CenterDiameter,
            v(50.0, 50.0),
            v(56.0, 50.0),
            true,
        )
        .unwrap()
        .entities[0]
    };
    let first = circle_first.then(|| circle(s));
    let rectangle = s
        .add_rectangle(RectangleMode::Center, v(50.0, 50.0), v(60.0, 60.0))
        .unwrap()
        .entities;
    let curve = first.unwrap_or_else(|| circle(s));
    assert_eq!(center_handles(&s.dto(), curve), vec![rectangle[8]]);
    (rectangle, curve)
}

#[test]
fn mixed_center_owners_move_and_scale_exactly_in_either_creation_order() {
    for circle_first in [false, true] {
        for scale in [false, true] {
            let mut s = session();
            let (rectangle, circle) = rectangle_and_circle(&mut s, circle_first);
            let before = s.dto();
            let ids = rectangle[4..8].iter().copied().chain([circle]).collect();
            let transform = |p: Vec2| if scale { p * 2.0 } else { p + v(12.0, 7.0) };
            if scale {
                s.scale_entities(&ScaleRequest {
                    entity_ids: ids,
                    origin: Vec2::ZERO,
                    factor_text: "2".into(),
                })
                .unwrap();
            } else {
                s.move_copy_entities(&MoveCopyRequest {
                    entity_ids: ids,
                    dx: 12.0,
                    dy: 7.0,
                    copy: false,
                })
                .unwrap();
            }
            let after = s.dto();
            for id in rectangle[..4].iter().chain([&rectangle[8]]) {
                let actual = point(&after, *id);
                let expected = transform(point(&before, *id));
                assert!(
                    actual.distance(expected) < 1e-6,
                    "circle_first={circle_first}, scale={scale}: {actual:?} != {expected:?}"
                );
            }
            assert!(circle_center_of(&after, circle).distance(transform(v(50.0, 50.0))) < 1e-6);
            assert_eq!(s.undo().unwrap().sketch.entities, before.entities);
            assert_eq!(s.redo().unwrap().sketch.entities, after.entities);
        }
    }
}

#[test]
fn mixed_center_occurrences_preserve_internal_bindings_and_cleanup() {
    for circle_first in [false, true] {
        for tool in ["copy", "mirror", "rectangular", "circular"] {
            for include_circle in [false, true] {
                let mut s = session();
                let axis = s
                    .add_line(v(0.0, -50.0), v(0.0, 100.0), true)
                    .unwrap()
                    .entity_id;
                let (rectangle, circle) = rectangle_and_circle(&mut s, circle_first);
                let before = s.dto();
                let ids: Vec<_> = rectangle[4..8]
                    .iter()
                    .copied()
                    .chain(include_circle.then_some(circle))
                    .collect();
                match tool {
                    "copy" => s.move_copy_entities(&MoveCopyRequest {
                        entity_ids: ids,
                        dx: 100.0,
                        dy: 0.0,
                        copy: true,
                    }),
                    "mirror" => s.mirror_entities(&MirrorRequest {
                        entity_ids: ids,
                        axis_line: axis,
                    }),
                    "rectangular" => s.rectangular_pattern(&RectangularPatternRequest {
                        entity_ids: ids,
                        direction: v(1.0, 0.0),
                        spacing: 100.0,
                        count: 2,
                        second_direction: None,
                        second_spacing: 0.0,
                        second_count: 1,
                    }),
                    _ => s.circular_pattern(&CircularPatternRequest {
                        entity_ids: ids,
                        center: Vec2::ZERO,
                        count: 2,
                        total_angle_deg: 90.0,
                    }),
                }
                .unwrap();
                let after = s.dto();
                let center = after.constraints.iter().find_map(|c| match c.constraint {
                    Constraint::SpanMidpoint { point, .. } if point != rectangle[8] => Some(point),
                    _ => None,
                }).unwrap_or_else(|| panic!("{tool}, include_circle={include_circle}: copied rectangle lost its center"));
                let copied_curves: Vec<_> = after
                    .entities
                    .iter()
                    .filter(|entity| !before.entities.iter().any(|old| old.id() == entity.id()))
                    .filter(|entity| {
                        matches!(entity, EntityDto::Line { .. } | EntityDto::Circle { .. })
                    })
                    .map(EntityDto::id)
                    .collect();
                assert_eq!(copied_curves.len(), if include_circle { 5 } else { 4 });
                if include_circle {
                    let copied_circle = after
                        .entities
                        .iter()
                        .find_map(|e| match e {
                            EntityDto::Circle { id, .. } if *id != circle => Some(*id),
                            _ => None,
                        })
                        .unwrap();
                    assert_eq!(
                        center_handles(&after, copied_circle),
                        vec![center],
                        "{tool}: split center"
                    );
                }
                assert_eq!(
                    center_handles(&after, circle),
                    vec![rectangle[8]],
                    "source changed"
                );
                assert!(point(&after, rectangle[8]).distance(v(50.0, 50.0)) < 1e-6);
                let expected = match tool {
                    "mirror" => v(-50.0, 50.0),
                    "circular" => v(-50.0, 50.0),
                    _ => v(150.0, 50.0),
                };
                assert!(point(&after, center).distance(expected) < 1e-6);
                assert_eq!(s.undo().unwrap().sketch.entities, before.entities);
                assert_eq!(s.redo().unwrap().sketch.entities, after.entities);
                s.delete_entities(&copied_curves).unwrap();
                assert_eq!(
                    s.dto().entities,
                    before.entities,
                    "{tool}: orphaned generated center/corner"
                );
            }
        }
    }
}

#[test]
fn an_unselected_rectangle_does_not_make_its_shared_center_transform_owned() {
    let mut centers = Vec::new();
    for authored_anchor in [true, false] {
        let mut s = session();
        if authored_anchor {
            s.add_point(v(50.0, 50.0)).unwrap();
        }
        let first = s
            .add_rectangle(RectangleMode::Center, v(50.0, 50.0), v(60.0, 60.0))
            .unwrap();
        let second = s
            .add_rectangle(RectangleMode::Center, v(50.0, 50.0), v(70.0, 70.0))
            .unwrap();
        assert_eq!(first.entities[8], second.entities[8]);
        s.move_copy_entities(&MoveCopyRequest {
            entity_ids: first.entities[4..8].to_vec(),
            dx: 12.0,
            dy: 0.0,
            copy: false,
        })
        .unwrap();
        centers.push(point(&s.dto(), first.entities[8]));
    }
    assert!(
        centers[0].distance(centers[1]) < 1e-6,
        "external rectangle's center was rigidly carried: {centers:?}"
    );
}

#[test]
fn a_circle_attached_to_a_rectangle_center_does_not_disable_centered_resize() {
    for circle_first in [false, true] {
        for corner in 0..4 {
            let mut s = session();
            let (rectangle, circle) = rectangle_and_circle(&mut s, circle_first);
            let before = s.dto();
            let target = point(&before, rectangle[corner]) + v(-5.0, -5.0);
            drag(&mut s, rectangle[corner], target);
            let after = s.dto();
            let actual = point(&after, rectangle[8]);
            assert!(
                actual.distance(v(50.0, 50.0)) < 1e-6,
                "circle_first={circle_first}, corner={corner}: center moved to {actual:?}"
            );
            assert!(circle_center_of(&after, circle).distance(v(50.0, 50.0)) < 1e-6);
            assert!(point(&after, rectangle[corner]).distance(target) < 1e-6);
            assert!(
                point(&after, rectangle[(corner + 2) % 4]).distance(v(100.0, 100.0) - target)
                    < 1e-6
            );
        }
    }
}
