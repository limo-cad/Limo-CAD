use super::*;

fn bounds(body: &limo_cad_solid::BodyDto) -> [[f64; 3]; 2] {
    let mut result = [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]];
    for p in body.mesh.positions.as_chunks::<3>().0 {
        for i in 0..3 {
            result[0][i] = result[0][i].min(p[i] as f64);
            result[1][i] = result[1][i].max(p[i] as f64);
        }
    }
    result
}

#[test]
fn body_moves_preserve_identity_pivots_exact_cancel_and_atomic_history() {
    let _lock = super::super::super::super::tests::TEST_LOCK.lock().unwrap();
    use SolidField as F;
    for mode in ["free", "translate", "rotate", "point_to_point"] {
        for copy in [false, true] {
            let fixture = Fixture::new();
            let owner = fixture.owner();
            for (op, args) in [
                ("sketch_begin", json!({"type":"origin_plane","plane":"xy"})),
                (
                    "sketch_add_rectangle",
                    json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":20.,"y":10.},"ctrl_held":true}),
                ),
                ("sketch_finish", json!({})),
            ] {
                fixture
                    .bridge
                    .apply_native_mutation(&fixture.engine, &owner, op, &args, || Ok(()))
                    .unwrap();
            }
            fixture.bridge.apply_native_mutation(&fixture.engine,&owner,"solid_extrude",&json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":10.}}),||Ok(())).unwrap();
            let source = model_snapshot(&fixture.engine);
            let body = &source.document.scene.bodies[0];
            assert_eq!(bounds(body), [[0., 0., 0.], [20., 10., 10.]]);
            let mut app = scene(&fixture, &owner);
            let open = |world: &mut World, feature_id| {
                reduce(
                    &fixture.engine,
                    &fixture.bridge,
                    world,
                    &owner,
                    &FeatureCommand::Open {
                        kind: SolidFormKind::MoveCopy,
                        feature_id,
                    },
                    &ControlInput::Click,
                    || Ok(()),
                )
                .unwrap()["form_id"]
                    .as_u64()
                    .unwrap()
            };
            let before = exported(&fixture);
            let baseline_view = native_viewport::interface_view_snapshot(app.world()).2;
            let id = open(app.world_mut(), None);
            assert!(!panel(app.world()).unwrap().can_apply);
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
            field(&fixture, app.world_mut(), &owner, id, F::MoveMode, mode);
            if mode == "free" {
                let unmodified = native_viewport::interface_view_snapshot(app.world()).2;
                assert!(std::sync::Arc::ptr_eq(
                    &unmodified.body_poses,
                    &baseline_view.body_poses
                ));
                assert!(std::sync::Arc::ptr_eq(
                    &unmodified.instance_body_poses,
                    &baseline_view.instance_body_poses
                ));
            }
            field(
                &fixture,
                app.world_mut(),
                &owner,
                id,
                F::Copy,
                if copy { "true" } else { "false" },
            );
            let set =
                |world: &mut World, key, value| field(&fixture, world, &owner, id, key, value);
            let expected = match mode {
                "free" => {
                    set(app.world_mut(), F::TranslationX, "NaN");
                    assert!(!panel(app.world()).unwrap().can_apply);
                    set(app.world_mut(), F::TranslationX, "3 cm");
                    set(app.world_mut(), F::TranslationY, "2");
                    set(app.world_mut(), F::TranslationZ, "3");
                    set(app.world_mut(), F::RotationZ, "90 deg");
                    for key in [F::PivotX, F::PivotY, F::PivotZ] {
                        set(app.world_mut(), key, "0");
                    }
                    [[20., 2., 3.], [30., 22., 13.]]
                }
                "translate" => {
                    set(app.world_mut(), F::DirectionX, "0");
                    assert!(!panel(app.world()).unwrap().can_apply);
                    set(app.world_mut(), F::DirectionY, "2");
                    set(app.world_mut(), F::Distance, "30 mm");
                    [[0., 30., 0.], [20., 40., 10.]]
                }
                "rotate" => {
                    set(app.world_mut(), F::Angle, "90 deg");
                    for key in [F::PivotX, F::PivotY, F::PivotZ] {
                        set(app.world_mut(), key, "0");
                    }
                    [[-10., 0., 0.], [0., 20., 10.]]
                }
                _ => {
                    for (point, expected_translation) in [
                        ([0., 0., 10.], [0., 0., 0.]),
                        ([20., 0., 10.], [0., 0., 0.]),
                    ] {
                        action(
                            &fixture,
                            app.world_mut(),
                            &owner,
                            id,
                            FeatureControl::Pick(F::FromPoint),
                            ControlInput::Click,
                        )
                        .unwrap();
                        accept_pick(
                            &fixture.engine,
                            &fixture.bridge,
                            app.world_mut(),
                            &owner,
                            id,
                            FeaturePick::MovePoint(point),
                            || Ok(()),
                        )
                        .unwrap();
                        let request = {
                            let editor = app
                                .world()
                                .resource::<NativeFeature>()
                                .editor
                                .as_ref()
                                .unwrap();
                            editor
                                .form
                                .move_request(&editor.snapshot.model(None))
                                .unwrap()
                        };
                        assert_eq!(
                            [
                                request.translation.x,
                                request.translation.y,
                                request.translation.z
                            ],
                            expected_translation
                        );
                    }
                    for (key, value) in [
                        (F::FromX, "10"),
                        (F::FromY, "6"),
                        (F::FromZ, "5"),
                        (F::ToX, "50"),
                        (F::ToY, "20"),
                        (F::ToZ, "5"),
                    ] {
                        set(app.world_mut(), key, value);
                    }
                    [[40., 14., 0.], [60., 24., 10.]]
                }
            };
            assert!(
                panel(app.world()).unwrap().can_apply,
                "{mode}: {:?}",
                panel(app.world()).unwrap().fields
            );
            assert_eq!(
                exported(&fixture),
                before,
                "Previews must not mutate the document"
            );
            let preview = native_viewport::interface_view_snapshot(app.world()).2;
            if !copy {
                assert_ne!(preview.body_poses, baseline_view.body_poses);
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
            assert_eq!(result.document.scene.bodies.len(), if copy { 2 } else { 1 });
            let moved = if copy {
                result
                    .document
                    .scene
                    .bodies
                    .iter()
                    .find(|b| b.id != body.id)
                    .unwrap()
            } else {
                assert_eq!(result.document.scene.bodies[0].id, body.id);
                &result.document.scene.bodies[0]
            };
            let actual = bounds(moved);
            for (actual, expected) in actual
                .into_iter()
                .flatten()
                .zip(expected.into_iter().flatten())
            {
                assert!(
                    (actual - expected).abs() < 1e-4,
                    "{mode} copy {copy}: {actual}/{expected}"
                );
            }
            assert!((combine::volume(moved) - 2000.).abs() < 1e-3);
            if copy {
                assert_eq!(
                    bounds(
                        result
                            .document
                            .scene
                            .bodies
                            .iter()
                            .find(|b| b.id == body.id)
                            .unwrap()
                    ),
                    bounds(body)
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
            assert!(
                !panel(app.world())
                    .unwrap()
                    .fields
                    .iter()
                    .find(|f| f.field == F::Copy)
                    .unwrap()
                    .enabled
            );
            field(&fixture, app.world_mut(), &owner, id, F::TranslationX, "99");
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
            field(&fixture, app.world_mut(), &owner, id, F::TranslationX, "99");
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
            let undo = fixture
                .bridge
                .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
                .unwrap();
            assert_eq!(exported(&fixture), created);
            fixture
                .bridge
                .apply_native_history(&fixture.engine, &undo.context, true, || Ok(()))
                .unwrap();
            assert_eq!(exported(&fixture), edited);
        }
    }
}

#[test]
fn component_moves_and_copies_respect_nested_frames_and_reusable_source_history() {
    let _lock = super::super::super::super::tests::TEST_LOCK.lock().unwrap();
    use SolidField as F;
    for group in [false, true] {
        for copy in [false, true] {
            let fixture = Fixture::new();
            let owner = sketch(&fixture);
            let mutate = |op, args: Value| {
                fixture
                    .bridge
                    .apply_native_mutation(&fixture.engine, &owner, op, &args, || Ok(()))
                    .unwrap();
            };
            mutate(
                "solid_extrude",
                json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":10.}}),
            );
            let assembly = || -> limo_cad_sketch::AssemblyDocumentDto {
                serde_json::from_value(
                    parse_engine_envelope(fixture.engine.engine_call("assembly_document", ""))
                        .unwrap(),
                )
                .unwrap()
            };
            let solution = || -> limo_cad_sketch::AssemblySolutionDto {
                serde_json::from_value(
                    parse_engine_envelope(fixture.engine.engine_call("assembly_solution", ""))
                        .unwrap(),
                )
                .unwrap()
            };
            let child = assembly().component_structure.occurrences[0].id.0;
            mutate(
                "assembly_create_component",
                json!({"name":"Carrier","body_ids":[]}),
            );
            let component = assembly()
                .component_structure
                .definitions
                .last()
                .unwrap()
                .id
                .0;
            let r = std::f64::consts::FRAC_1_SQRT_2;
            mutate(
                "assembly_create_occurrence",
                json!({"component_id":component,"name":"Rotated parent","local_pose":{"translation":[100.,0.,0.],"rotation":[0.,0.,r,r]}}),
            );
            let parent = assembly()
                .component_structure
                .occurrences
                .last()
                .unwrap()
                .id
                .0;
            mutate(
                "assembly_update_occurrence",
                json!({"occurrence":{"id":child,"parent_occurrence_id":parent,"local_pose":{"translation":[10.,0.,0.],"rotation":[0.,0.,0.,1.]}}}),
            );
            let target = if group { parent } else { child };
            let original = exported(&fixture);
            let source = model_snapshot(&fixture.engine);
            let original_solid = serde_json::to_value(source.document.scene.as_ref()).unwrap();
            let original_features = fixture.engine.document_snapshot().features.len();
            let before_poses = std::sync::Arc::new(solution().instance_body_poses);
            let mut app = scene(&fixture, &owner);
            let mut view = native_viewport::interface_view_snapshot(app.world()).2;
            view.body_poses = source.body_poses.clone();
            view.instance_body_poses = before_poses.clone();
            native_viewport::apply_interface_view(
                app.world_mut(),
                &owner.document_id,
                None,
                Some(view),
            )
            .unwrap();
            let open = |world: &mut World| {
                reduce(
                    &fixture.engine,
                    &fixture.bridge,
                    world,
                    &owner,
                    &FeatureCommand::Open {
                        kind: SolidFormKind::MoveCopy,
                        feature_id: None,
                    },
                    &ControlInput::Click,
                    || Ok(()),
                )
                .unwrap()["form_id"]
                    .as_u64()
                    .unwrap()
            };
            for cancel in [true, false] {
                let id = open(app.world_mut());
                accept_pick(
                    &fixture.engine,
                    &fixture.bridge,
                    app.world_mut(),
                    &owner,
                    id,
                    FeaturePick::Occurrence(target),
                    || Ok(()),
                )
                .unwrap();
                for (key, value) in [
                    (F::TranslationX, "30"),
                    (F::TranslationY, "2"),
                    (F::TranslationZ, "3"),
                    (F::RotationZ, "90"),
                    (F::PivotX, "0"),
                    (F::PivotY, "0"),
                    (F::PivotZ, "0"),
                    (F::Copy, if copy { "true" } else { "false" }),
                ] {
                    field(&fixture, app.world_mut(), &owner, id, key, value);
                }
                assert!(panel(app.world()).unwrap().can_apply);
                assert_eq!(exported(&fixture), original, "preview is presentation only");
                let view = native_viewport::interface_view_snapshot(app.world()).2;
                if copy {
                    assert_eq!(view.instance_body_poses, before_poses);
                } else {
                    let p = view
                        .instance_body_poses
                        .iter()
                        .find(|p| p.occurrence_id.0 == child)
                        .unwrap();
                    for (actual, expected) in p.translation.into_iter().zip([20., 102., 3.]) {
                        assert!((actual - expected).abs() < 1e-8);
                    }
                }
                action(
                    &fixture,
                    app.world_mut(),
                    &owner,
                    id,
                    if cancel {
                        FeatureControl::Cancel
                    } else {
                        FeatureControl::Apply
                    },
                    ControlInput::Click,
                )
                .unwrap();
                if cancel {
                    assert_eq!(exported(&fixture), original);
                    assert_eq!(
                        native_viewport::interface_view_snapshot(app.world())
                            .2
                            .instance_body_poses,
                        before_poses
                    );
                }
            }
            let result = solution();
            assert!(result.solved);
            assert_eq!(
                serde_json::to_value(model_snapshot(&fixture.engine).document.scene.as_ref())
                    .unwrap(),
                original_solid,
                "moving a component must not alter source geometry"
            );
            assert_eq!(
                fixture.engine.document_snapshot().features.len(),
                original_features
            );
            let target_id = if copy {
                assembly()
                    .component_structure
                    .occurrences
                    .iter()
                    .filter(|o| {
                        o.component_id.0
                            == if group {
                                component
                            } else {
                                assembly()
                                    .component_structure
                                    .occurrences
                                    .iter()
                                    .find(|o| o.id.0 == child)
                                    .unwrap()
                                    .component_id
                                    .0
                            }
                    })
                    .map(|o| o.id.0)
                    .max()
                    .unwrap()
            } else {
                target
            };
            let p = result
                .occurrence_poses
                .iter()
                .find(|p| p.occurrence_id.0 == target_id)
                .unwrap();
            let expected = if group {
                [30., 102., 3.]
            } else {
                [20., 102., 3.]
            };
            for (a, b) in p.translation.into_iter().zip(expected) {
                assert!((a - b).abs() < 1e-8, "{a} vs {b}");
            }
            if copy {
                let original_pose = result
                    .instance_body_poses
                    .iter()
                    .find(|p| p.occurrence_id.0 == child)
                    .unwrap();
                assert_eq!(*original_pose, before_poses[0]);
                assert_eq!(result.instance_body_poses.len(), 2);
            }
            let after = exported(&fixture);
            let original_next_id = original["assembly"]["component_structure"]
                ["next_occurrence_id"]
                .as_u64()
                .unwrap();
            let next_id = after["assembly"]["component_structure"]["next_occurrence_id"]
                .as_u64()
                .unwrap();
            if copy {
                assert!(next_id > original_next_id);
            } else {
                assert_eq!(next_id, original_next_id);
            }
            let undone = fixture
                .bridge
                .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
                .unwrap();
            let mut restored = original.clone();
            restored["assembly"]["component_structure"]["next_occurrence_id"] =
                serde_json::json!(original_next_id.max(next_id));
            assert_eq!(exported(&fixture), restored);
            fixture
                .bridge
                .apply_native_history(&fixture.engine, &undone.context, true, || Ok(()))
                .unwrap();
            assert_eq!(exported(&fixture), after);
        }
    }
}
