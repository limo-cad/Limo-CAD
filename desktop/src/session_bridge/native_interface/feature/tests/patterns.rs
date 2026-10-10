use super::*;

#[test]
fn native_body_patterns_validate_vectors_counts_and_restore_exact_geometry() {
    let _lock = super::super::super::super::tests::TEST_LOCK.lock().unwrap();
    use SolidField as F;
    for kind in [
        SolidFormKind::RectangularPattern,
        SolidFormKind::CircularPattern,
    ] {
        let fixture = Fixture::new();
        let owner = fixture.owner();
        for (op, args) in [
            ("sketch_begin", json!({"type":"origin_plane","plane":"xy"})),
            (
                "sketch_add_rectangle",
                json!({"mode":"two_point","p1":{"x":20.,"y":0.},"p2":{"x":40.,"y":20.},"ctrl_held":true}),
            ),
            ("sketch_finish", json!({})),
            (
                "solid_extrude",
                json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":10.}}),
            ),
        ] {
            fixture
                .bridge
                .apply_native_mutation(&fixture.engine, &owner, op, &args, || Ok(()))
                .unwrap();
        }
        let source = model_snapshot(&fixture.engine);
        let body = &source.document.scene.bodies[0];
        let mut app = scene(&fixture, &owner);
        let open = |world: &mut World, feature_id| {
            reduce(
                &fixture.engine,
                &fixture.bridge,
                world,
                &owner,
                &FeatureCommand::Open { kind, feature_id },
                &ControlInput::Click,
                || Ok(()),
            )
            .unwrap()["form_id"]
                .as_u64()
                .unwrap()
        };
        let set = |world: &mut World, id, field, value: &str| {
            action(
                &fixture,
                world,
                &owner,
                id,
                FeatureControl::Field(field),
                ControlInput::SetValue(value.into()),
            )
            .unwrap()
        };
        let original = exported(&fixture);
        let id = open(app.world_mut(), None);
        assert!(!panel(app.world()).unwrap().can_apply);
        assert!(accept_pick(
            &fixture.engine,
            &fixture.bridge,
            app.world_mut(),
            &owner,
            id,
            FeaturePick::Bodies(vec![body.id, body.id]),
            || Ok(())
        )
        .is_err());
        accept_pick(
            &fixture.engine,
            &fixture.bridge,
            app.world_mut(),
            &owner,
            id,
            FeaturePick::Bodies(vec![body.id]),
            || Ok(()),
        )
        .unwrap();
        for invalid in ["1", "2.5", "10001", "4 mm", "NaN"] {
            set(app.world_mut(), id, F::Count, invalid);
            assert!(!panel(app.world()).unwrap().can_apply, "{invalid}");
            assert!(action(
                &fixture,
                app.world_mut(),
                &owner,
                id,
                FeatureControl::Apply,
                ControlInput::Click
            )
            .is_err());
            assert_eq!(exported(&fixture), original);
        }
        set(app.world_mut(), id, F::Count, "3");
        let circular = kind == SolidFormKind::CircularPattern;
        let field = if circular {
            F::AxisEdge
        } else {
            F::DirectionEdge
        };
        action(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            FeatureControl::Pick(field),
            ControlInput::Click,
        )
        .unwrap();
        let edge = body
            .edges
            .iter()
            .find(|e| {
                let a = e.points.first().unwrap();
                let b = e.points.last().unwrap();
                if circular {
                    a.x == b.x && a.y == b.y && a.z != b.z
                } else {
                    a.x != b.x && a.y == b.y && a.z == b.z
                }
            })
            .unwrap();
        accept_pick(
            &fixture.engine,
            &fixture.bridge,
            app.world_mut(),
            &owner,
            id,
            FeaturePick::AxisEdge(body.id, edge.id),
            || Ok(()),
        )
        .unwrap();
        action(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            FeatureControl::Clear(field),
            ControlInput::Click,
        )
        .unwrap();
        assert!(!panel(app.world()).unwrap().can_apply);
        set(
            app.world_mut(),
            id,
            if circular {
                F::DirectionZ
            } else {
                F::DirectionX
            },
            "1",
        );
        if circular {
            for field in [F::OriginX, F::OriginY, F::OriginZ] {
                set(app.world_mut(), id, field, "0 mm");
            }
            for invalid in ["0", "361", "NaN"] {
                set(app.world_mut(), id, F::Angle, invalid);
                assert!(!panel(app.world()).unwrap().can_apply);
            }
            set(app.world_mut(), id, F::Angle, "360 deg");
            set(app.world_mut(), id, F::Count, "2+2");
        } else {
            set(app.world_mut(), id, F::Distance, "0");
            assert!(!panel(app.world()).unwrap().can_apply);
            set(app.world_mut(), id, F::Distance, "3 cm");
            set(app.world_mut(), id, F::SecondEnabled, "true");
            set(app.world_mut(), id, F::SecondDistance, "30");
            set(app.world_mut(), id, F::SecondCount, "10000");
            assert!(!panel(app.world()).unwrap().can_apply);
            set(app.world_mut(), id, F::SecondCount, "2");
        }
        assert!(panel(app.world()).unwrap().can_apply);
        assert_eq!(exported(&fixture), original);
        action(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            FeatureControl::Apply,
            ControlInput::Click,
        )
        .unwrap();
        let result = model_snapshot(&fixture.engine);
        assert!(result.document.scene.errors.is_empty());
        assert_eq!(
            result.document.scene.bodies.len(),
            if circular { 4 } else { 6 }
        );
        for body in &result.document.scene.bodies {
            assert!((combine::volume(body) - 4000.).abs() < 1e-4);
        }
        let mut centers: Vec<_> = result
            .document
            .scene
            .bodies
            .iter()
            .map(|b| {
                let bounds = |i| {
                    let values: Vec<_> = b
                        .mesh
                        .positions
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .map(|p| p[i])
                        .collect();
                    (values.iter().copied().fold(f32::INFINITY, f32::min)
                        + values.iter().copied().fold(f32::NEG_INFINITY, f32::max))
                        * 0.5
                };
                (bounds(0).round() as i32, bounds(1).round() as i32)
            })
            .collect();
        centers.sort_unstable();
        assert_eq!(
            centers,
            if circular {
                vec![(-30, -10), (-10, 30), (10, -30), (30, 10)]
            } else {
                vec![(30, 10), (30, 40), (60, 10), (60, 40), (90, 10), (90, 40)]
            }
        );
        let created = exported(&fixture);
        let feature = fixture
            .engine
            .document_snapshot()
            .features
            .last()
            .unwrap()
            .id
            .0;
        let id = open(app.world_mut(), Some(feature));
        assert_eq!(exported(&fixture), created);
        set(app.world_mut(), id, F::Count, "5");
        action(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            FeatureControl::Cancel,
            ControlInput::Click,
        )
        .unwrap();
        assert_eq!(exported(&fixture), created);
        let id = open(app.world_mut(), Some(feature));
        set(
            app.world_mut(),
            id,
            if circular { F::Angle } else { F::Distance },
            if circular { "-180" } else { "-30" },
        );
        action(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            FeatureControl::Apply,
            ControlInput::Click,
        )
        .unwrap();
        let edited = exported(&fixture);
        assert_ne!(edited, created);
        assert!(model_snapshot(&fixture.engine)
            .document
            .scene
            .errors
            .is_empty());
        let restored = fixture
            .bridge
            .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
            .unwrap();
        assert_eq!(exported(&fixture), created);
        fixture
            .bridge
            .apply_native_history(&fixture.engine, &restored.context, true, || Ok(()))
            .unwrap();
        assert_eq!(exported(&fixture), edited);
    }
}
