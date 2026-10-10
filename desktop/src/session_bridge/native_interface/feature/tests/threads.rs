use super::*;

#[test]
fn external_threads_reject_wrong_faces_validate_fits_and_edit_original_cylinder_atomically() {
    super::super::super::super::tests::with_isolated_session_test(external_threads_case);
}

fn external_threads_case() {
    use SolidField as F;
    for rounded in [false, true] {
        let fixture = Fixture::new();
        let owner = fixture.owner();
        for (op, args) in [
            ("sketch_begin", json!({"type":"origin_plane","plane":"xy"})),
            (
                "sketch_add_circle",
                json!({"mode":"center_diameter","p1":{"x":0.,"y":0.},"p2":{"x":10.,"y":0.},"ctrl_held":true}),
            ),
            (
                "sketch_add_circle",
                json!({"mode":"center_diameter","p1":{"x":0.,"y":0.},"p2":{"x":5.,"y":0.},"ctrl_held":true}),
            ),
            ("sketch_finish", json!({})),
            (
                "solid_extrude",
                json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":20.}}),
            ),
        ] {
            fixture
                .bridge
                .apply_native_mutation(&fixture.engine, &owner, op, &args, || Ok(()))
                .unwrap();
        }
        let source = model_snapshot(&fixture.engine);
        let body = &source.document.scene.bodies[0];
        let outer = body
            .faces
            .iter()
            .filter(|f| f.cylinder.is_some())
            .max_by(|a, b| {
                a.cylinder
                    .unwrap()
                    .radius
                    .total_cmp(&b.cylinder.unwrap().radius)
            })
            .unwrap();
        let inner = body
            .faces
            .iter()
            .filter(|f| f.cylinder.is_some())
            .min_by(|a, b| {
                a.cylinder
                    .unwrap()
                    .radius
                    .total_cmp(&b.cylinder.unwrap().radius)
            })
            .unwrap();
        assert_ne!(
            outer.id, inner.id,
            "Fixture must have both a shaft and an internal wall"
        );
        let mut app = scene(&fixture, &owner);
        let open = |world: &mut World, feature_id| {
            reduce(
                &fixture.engine,
                &fixture.bridge,
                world,
                &owner,
                &FeatureCommand::Open {
                    kind: SolidFormKind::ExternalThread,
                    feature_id,
                },
                &ControlInput::Click,
                || Ok(()),
            )
            .unwrap()["form_id"]
                .as_u64()
                .unwrap()
        };
        let pick = |world: &mut World, id, face_id| {
            accept_pick(
                &fixture.engine,
                &fixture.bridge,
                world,
                &owner,
                id,
                FeaturePick::Face(PlanarFaceSourceDto {
                    body_id: body.id,
                    face_id,
                }),
                || Ok(()),
            )
        };
        let original = exported(&fixture);
        let id = open(app.world_mut(), None);
        assert!(!panel(app.world()).unwrap().can_apply);
        assert!(pick(
            app.world_mut(),
            id,
            body.faces.iter().find(|f| f.cylinder.is_none()).unwrap().id
        )
        .unwrap_err()
        .contains("cylindrical"));
        assert!(pick(app.world_mut(), id, inner.id)
            .unwrap_err()
            .contains("internal hole"));
        pick(app.world_mut(), id, outer.id).unwrap();
        assert!(panel(app.world()).unwrap().can_apply);
        let rows = panel(app.world()).unwrap().fields;
        assert!(rows.iter().any(|r|r.field == F::ThreadPreset && matches!(&r.value,limo_cad_interface::Field::Choice{value,..} if value == "metric_coarse-20-2.5")));
        field(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            F::ThreadStandard,
            "unified_inch",
        );
        field(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            F::ThreadPreset,
            "unc-3/4-10",
        );
        assert!(
            !panel(app.world()).unwrap().can_apply,
            "A mismatched nominal diameter must not apply"
        );
        field(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            F::ThreadStandard,
            if rounded {
                "custom_trapezoidal"
            } else {
                "iso_metric"
            },
        );
        field(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            F::ThreadPreset,
            "custom",
        );
        for invalid in ["0", "NaN", "3 deg"] {
            field(&fixture, app.world_mut(), &owner, id, F::Pitch, invalid);
            assert!(!panel(app.world()).unwrap().can_apply);
            assert_eq!(exported(&fixture), original);
        }
        field(&fixture, app.world_mut(), &owner, id, F::Pitch, "2.5 mm");
        field(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            F::FullThread,
            "false",
        );
        field(&fixture, app.world_mut(), &owner, id, F::Distance, "21");
        assert!(!panel(app.world()).unwrap().can_apply);
        field(&fixture, app.world_mut(), &owner, id, F::Distance, "0.8 cm");
        field(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            F::ThreadHand,
            if rounded { "left" } else { "right" },
        );
        field(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            F::Representation,
            "modeled",
        );
        field(&fixture, app.world_mut(), &owner, id, F::Flip, "true");
        if rounded {
            field(
                &fixture,
                app.world_mut(),
                &owner,
                id,
                F::CornerRadius,
                "100",
            );
            assert!(!panel(app.world()).unwrap().can_apply);
            field(
                &fixture,
                app.world_mut(),
                &owner,
                id,
                F::CornerRadius,
                "0.1875 mm",
            );
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
        assert_eq!(result.document.scene.bodies.len(), 1);
        let volume = combine::volume(&result.document.scene.bodies[0]);
        let before_volume = combine::volume(body);
        assert!(
            volume > before_volume * 0.8 && volume < before_volume * 0.999,
            "Modeled thread must remove a finite groove: {volume}/{before_volume}"
        );
        for payload in [
            "{}",
            r#"{"linear_deflection":0.0375,"angular_deflection":0.175}"#,
        ] {
            let archive = fixture.engine.export_3mf(payload).unwrap();
            assert!(archive.starts_with(b"PK"));
        }
        assert_eq!(
            model_snapshot(&fixture.engine).document.scene,
            result.document.scene
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
        field(&fixture, app.world_mut(), &owner, id, F::Distance, "6");
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
        field(&fixture, app.world_mut(), &owner, id, F::Distance, "5");
        field(&fixture, app.world_mut(), &owner, id, F::Flip, "false");
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
