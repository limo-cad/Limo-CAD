use super::*;

// Read-only capture of the reported sketch: two connected semicircles,
// fifteen driving dimensions, and one free endpoint on the vertical line.
fn connected_arc_session() -> SketchSession {
    let plane = crate::PlaneRef::OriginPlane {
        plane: crate::OriginPlane::Xz,
    };
    let mut session = SketchSession::new("Connected arcs", plane, plane.basis().unwrap(), false);
    session.sketch.restore(
        serde_json::from_str(include_str!("fixtures/connected-arc-dimension-edit.json")).unwrap(),
    );
    session.recompute();
    session
}

#[test]
fn editing_branch_length_preserves_connected_arcs_and_other_dimensions() {
    for text in ["20", "18", "d12*3"] {
        let mut session = connected_arc_session();
        let before = session.dto();
        let snapshot = serde_json::to_string(&session.sketch.snapshot()).unwrap();
        let expected = session.eval_text(text).unwrap();
        let result = session.edit_dimension(EditDimensionRequest {
            constraint_id: ConstraintId(37),
            text: text.into(),
        });
        assert!(
            result.is_ok(),
            "19 to {text} mm must be feasible: {result:?}"
        );
        let after = session.dto();
        assert_eq!(after.dof, before.dof);
        for (id, constraint) in session.sketch.constraints() {
            assert!(
                crate::solver::constraint_residual(&session.sketch, id) < 1e-7,
                "{id:?}: {constraint:?}"
            );
        }
        assert!((session.line_length(EntityId(67)).unwrap() - expected).abs() < 1e-7);
        // Upstream geometry and both arcs retain their authored solution. The
        // lower endpoint and its attached horizontal stub move together.
        let original: crate::SketchSnapshot = serde_json::from_str(&snapshot).unwrap();
        let mut saved = crate::Sketch::new();
        saved.restore(original);
        for (id, entity) in saved.entities() {
            if [66, 68].contains(&id.0) {
                continue;
            }
            match (entity, session.sketch.entity(id).unwrap()) {
                (Entity::Point { position: a }, Entity::Point { position: b }) => {
                    // The free end can take submicron minimum-motion steps;
                    // keep its support at solver precision and guard against
                    // visible sliding along the unconstrained direction.
                    if id == EntityId(63) {
                        assert!(
                            (a.x - b.x).abs() < 1e-7 && (a.y - b.y).abs() < 1e-3,
                            "Free endpoint slid: {a:?} -> {b:?}"
                        );
                    } else {
                        assert!(
                            a.distance(*b) < 1e-7,
                            "Point {} moved: {a:?} -> {b:?}",
                            id.0
                        );
                    }
                }
                (
                    Entity::Arc {
                        center: a,
                        radius: ra,
                        start_angle: sa,
                        end_angle: ea,
                    },
                    Entity::Arc {
                        center: b,
                        radius: rb,
                        start_angle: sb,
                        end_angle: eb,
                    },
                ) => {
                    assert!(a.distance(*b) < 1e-7);
                    assert!(
                        (ra - rb).abs() < 1e-7 && (sa - sb).abs() < 1e-7 && (ea - eb).abs() < 1e-7
                    );
                }
                (a, b) => assert_eq!(a, b),
            }
        }
        let after_snapshot = serde_json::to_string(&session.sketch.snapshot()).unwrap();
        session.undo().unwrap();
        assert_eq!(
            serde_json::to_string(&session.sketch.snapshot()).unwrap(),
            snapshot
        );
        session.redo().unwrap();
        assert_eq!(
            serde_json::to_string(&session.sketch.snapshot()).unwrap(),
            after_snapshot
        );
    }
}

#[test]
fn conflicting_arc_endpoint_distances_still_reject_the_edit_without_changing_geometry_or_history() {
    let mut session = connected_arc_session();
    // Both halves of the lower arc lie on opposite sides of its constrained
    // center. Their lengths must agree; changing just one is impossible.
    let before = serde_json::to_string(&session.sketch.snapshot()).unwrap();
    let history = (session.undo.len(), session.redo.len());
    assert!(session
        .edit_dimension(EditDimensionRequest {
            constraint_id: ConstraintId(31),
            text: "7".into()
        })
        .is_err());
    assert_eq!(
        serde_json::to_string(&session.sketch.snapshot()).unwrap(),
        before
    );
    assert_eq!((session.undo.len(), session.redo.len()), history);
}
