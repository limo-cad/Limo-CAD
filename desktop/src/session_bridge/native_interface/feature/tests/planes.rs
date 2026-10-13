use super::*;
use limo_cad_core::PlaneRef;

#[test]
fn planes_validate_references_preview_exact_geometry_and_preserve_dependent_history() {
    let _lock = super::super::super::super::tests::TEST_LOCK.lock().unwrap();
    for kind in [
        SolidFormKind::OffsetPlane,
        SolidFormKind::Midplane,
        SolidFormKind::AnglePlane,
    ] {
        let fixture = Fixture::new();
        let owner = sketch(&fixture);
        for (op, args) in [
            (
                "solid_extrude",
                json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":10.}}),
            ),
            (
                "construction_plane_offset",
                json!({"reference":PlaneRef::ORIGIN_PLANES[0],"distance":30.}),
            ),
        ] {
            fixture
                .bridge
                .apply_native_mutation(&fixture.engine, &owner, op, &args, || Ok(()))
                .unwrap();
        }
        let snapshot = model_snapshot(&fixture.engine);
        let datum = snapshot.document.datum_planes[0].datum_id;
        let body = &snapshot.document.scene.bodies[0];
        let axis = body
            .edges
            .iter()
            .find(|e| {
                e.points.len() >= 2
                    && e.points
                        .iter()
                        .all(|p| p.y.abs() < 1e-7 && p.z.abs() < 1e-7)
            })
            .unwrap()
            .id;
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
        let before = exported(&fixture);
        let id = open(app.world_mut(), None);
        assert!(!panel(app.world()).unwrap().can_apply);
        pick(
            app.world_mut(),
            id,
            FeaturePick::Plane(PlaneRef::ORIGIN_PLANES[0]),
        )
        .unwrap();
        match kind {
            SolidFormKind::OffsetPlane => field(
                &fixture,
                app.world_mut(),
                &owner,
                id,
                SolidField::Distance,
                "1 in",
            ),
            SolidFormKind::Midplane => {
                pick(
                    app.world_mut(),
                    id,
                    FeaturePick::Plane(PlaneRef::ORIGIN_PLANES[1]),
                )
                .unwrap();
                assert!(
                    !panel(app.world()).unwrap().can_apply,
                    "Perpendicular planes cannot form a midplane"
                );
                action(
                    &fixture,
                    app.world_mut(),
                    &owner,
                    id,
                    FeatureControl::Pick(SolidField::SecondPlane),
                    ControlInput::Click,
                )
                .unwrap();
                pick(
                    app.world_mut(),
                    id,
                    FeaturePick::Plane(PlaneRef::DatumPlane { datum_id: datum }),
                )
                .unwrap();
            }
            SolidFormKind::AnglePlane => {
                pick(app.world_mut(), id, FeaturePick::AxisEdge(body.id, axis)).unwrap();
                field(
                    &fixture,
                    app.world_mut(),
                    &owner,
                    id,
                    SolidField::Angle,
                    "361",
                );
                assert!(!panel(app.world()).unwrap().can_apply);
                field(
                    &fixture,
                    app.world_mut(),
                    &owner,
                    id,
                    SolidField::Angle,
                    "30 deg",
                );
            }
            _ => unreachable!(),
        }
        assert_eq!(
            exported(&fixture),
            before,
            "Preview must not mutate history"
        );
        let state = app.world().resource::<NativeFeature>();
        let editor = state.editor.as_ref().unwrap();
        let preview = editor
            .form
            .plane_preview(&editor.snapshot.model(None))
            .unwrap()
            .unwrap();
        if kind == SolidFormKind::OffsetPlane {
            assert!((preview.origin[2] - 25.4).abs() < 1e-8);
        }
        if kind == SolidFormKind::Midplane {
            assert!((preview.origin[2] - 15.).abs() < 1e-8);
        }
        if kind == SolidFormKind::AnglePlane {
            assert!((preview.normal[2] - 30f64.to_radians().cos()).abs() < 1e-8);
        }
        action(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            FeatureControl::Apply,
            ControlInput::Click,
        )
        .unwrap();
        let definition = model_snapshot(&fixture.engine)
            .document
            .datum_planes
            .last()
            .unwrap()
            .clone();
        assert_eq!(
            definition.basis, preview,
            "Rendered plane and saved plane must use the same calculation"
        );
        for (op, args) in [
            (
                "sketch_begin",
                json!({"type":"datum_plane","datum_id":definition.datum_id}),
            ),
            (
                "sketch_add_rectangle",
                json!({"mode":"two_point","p1":{"x":40.,"y":0.},"p2":{"x":60.,"y":20.},"ctrl_held":true}),
            ),
            ("sketch_finish", json!({})),
            (
                "solid_extrude",
                json!({"sketch_name":"Sketch2","profile_indices":[0],"extent":{"type":"distance","distance":5.}}),
            ),
        ] {
            fixture
                .bridge
                .apply_native_mutation(&fixture.engine, &owner, op, &args, || Ok(()))
                .unwrap();
        }
        native_viewport::apply_interface_model(app.world_mut(), model_snapshot(&fixture.engine))
            .unwrap();
        let created = exported(&fixture);
        let id = open(app.world_mut(), Some(definition.feature_id.0));
        assert!(panel(app.world()).unwrap().can_apply);
        assert_eq!(panel(app.world()).unwrap().kind, kind);
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
        let id = open(app.world_mut(), Some(definition.feature_id.0));
        match kind {
            SolidFormKind::OffsetPlane => field(
                &fixture,
                app.world_mut(),
                &owner,
                id,
                SolidField::Distance,
                "40",
            ),
            SolidFormKind::AnglePlane => field(
                &fixture,
                app.world_mut(),
                &owner,
                id,
                SolidField::Angle,
                "60",
            ),
            SolidFormKind::Midplane => {
                action(
                    &fixture,
                    app.world_mut(),
                    &owner,
                    id,
                    FeatureControl::Pick(SolidField::FirstPlane),
                    ControlInput::Click,
                )
                .unwrap();
                let top = body
                    .faces
                    .iter()
                    .find(|f| {
                        f.plane.is_some_and(|b| {
                            (b.origin[2] - 10.).abs() < 1e-7 && b.normal[2].abs() > 0.99
                        })
                    })
                    .unwrap()
                    .id;
                pick(
                    app.world_mut(),
                    id,
                    FeaturePick::Plane(PlaneRef::PlanarFace { face_id: top }),
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
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
        assert_eq!(result.document.scene.bodies.len(), 2);
        if kind == SolidFormKind::OffsetPlane {
            assert!((maximum_z(&fixture) - 45.).abs() < 1e-4);
        }
        let edited = exported(&fixture);
        assert_ne!(created, edited);
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
