use super::*;
use limo_cad_core::PlaneRef;

#[test]
fn mirror_and_split_validate_sources_and_preserve_exact_geometry_through_history() {
    let _lock = super::super::super::super::tests::TEST_LOCK.lock().unwrap();
    for kind in [SolidFormKind::Mirror, SolidFormKind::SplitBody] {
        let fixture = Fixture::new();
        let owner = fixture.owner();
        for (i, x) in [20., 60.].into_iter().enumerate() {
            for (op, args) in [
                ("sketch_begin", json!({"type":"origin_plane","plane":"xy"})),
                (
                    "sketch_add_rectangle",
                    json!({"mode":"two_point","p1":{"x":x,"y":0.},"p2":{"x":x+20.,"y":20.},"ctrl_held":true}),
                ),
                ("sketch_finish", json!({})),
                (
                    "solid_extrude",
                    json!({"sketch_name":format!("Sketch{}",i+1),"profile_indices":[0],"extent":{"type":"distance","distance":10.}}),
                ),
            ] {
                fixture
                    .bridge
                    .apply_native_mutation(&fixture.engine, &owner, op, &args, || Ok(()))
                    .unwrap();
            }
        }
        for distance in [30., 35.] {
            fixture
                .bridge
                .apply_native_mutation(
                    &fixture.engine,
                    &owner,
                    "construction_plane_offset",
                    &json!({"reference":PlaneRef::ORIGIN_PLANES[2],"distance":distance}),
                    || Ok(()),
                )
                .unwrap();
        }
        let source = model_snapshot(&fixture.engine);
        let ids: Vec<_> = source.document.scene.bodies.iter().map(|b| b.id).collect();
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
        let pick = |world: &mut World, id, pick| {
            accept_pick(
                &fixture.engine,
                &fixture.bridge,
                world,
                &owner,
                id,
                pick,
                || Ok(()),
            )
        };
        let plane = |world: &mut World, id, plane| {
            action(
                &fixture,
                world,
                &owner,
                id,
                FeatureControl::Pick(SolidField::FirstPlane),
                ControlInput::Click,
            )
            .unwrap();
            pick(world, id, FeaturePick::Plane(plane))
        };
        let original = exported(&fixture);
        let id = open(app.world_mut(), None);
        assert!(!panel(app.world()).unwrap().can_apply);
        assert!(pick(
            app.world_mut(),
            id,
            FeaturePick::Bodies(vec![BodyId(u64::MAX)])
        )
        .is_err());
        if kind == SolidFormKind::SplitBody {
            assert!(pick(app.world_mut(), id, FeaturePick::Bodies(ids.clone())).is_err());
            pick(app.world_mut(), id, FeaturePick::Bodies(vec![ids[0]])).unwrap();
        } else {
            pick(app.world_mut(), id, FeaturePick::Bodies(ids.clone())).unwrap();
        }
        plane(
            app.world_mut(),
            id,
            if kind == SolidFormKind::Mirror {
                PlaneRef::ORIGIN_PLANES[2]
            } else {
                PlaneRef::DatumPlane {
                    datum_id: source.document.datum_planes[0].datum_id,
                }
            },
        )
        .unwrap();
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
            if kind == SolidFormKind::Mirror { 4 } else { 3 }
        );
        let total: f64 = result
            .document
            .scene
            .bodies
            .iter()
            .map(combine::volume)
            .sum();
        assert!(
            (total
                - if kind == SolidFormKind::Mirror {
                    16000.
                } else {
                    8000.
                })
            .abs()
                < 1e-4
        );
        if kind == SolidFormKind::Mirror {
            let xs: Vec<_> = result
                .document
                .scene
                .bodies
                .iter()
                .flat_map(|b| b.mesh.positions.as_chunks::<3>().0.iter().map(|p| p[0]))
                .collect();
            assert_eq!(xs.iter().copied().fold(f32::INFINITY, f32::min), -80.);
            assert_eq!(xs.iter().copied().fold(f32::NEG_INFINITY, f32::max), 80.);
        } else {
            let volumes: Vec<_> = result
                .document
                .scene
                .bodies
                .iter()
                .map(combine::volume)
                .collect();
            assert_eq!(
                volumes
                    .iter()
                    .filter(|v| (**v - 2000.).abs() < 1e-4)
                    .count(),
                2
            );
        }
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
        assert!(panel(app.world()).unwrap().can_apply);
        assert_eq!(exported(&fixture), created);
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
        if kind == SolidFormKind::SplitBody {
            plane(app.world_mut(), id, PlaneRef::ORIGIN_PLANES[2]).unwrap();
            assert!(
                action(
                    &fixture,
                    app.world_mut(),
                    &owner,
                    id,
                    FeatureControl::Apply,
                    ControlInput::Click
                )
                .is_err(),
                "A nonintersecting plane must not commit a split"
            );
            assert_eq!(exported(&fixture), created);
        }
        plane(
            app.world_mut(),
            id,
            if kind == SolidFormKind::Mirror {
                PlaneRef::ORIGIN_PLANES[1]
            } else {
                PlaneRef::DatumPlane {
                    datum_id: source.document.datum_planes[1].datum_id,
                }
            },
        )
        .unwrap();
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
        let result = model_snapshot(&fixture.engine);
        assert!(result.document.scene.errors.is_empty());
        if kind == SolidFormKind::SplitBody {
            let volumes: Vec<_> = result
                .document
                .scene
                .bodies
                .iter()
                .map(combine::volume)
                .collect();
            for expected in [1000., 3000., 4000.] {
                assert!(volumes.iter().any(|v| (v - expected).abs() < 1e-4));
            }
        }
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
