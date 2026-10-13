use super::*;
use limo_cad_solid::SketchPointKindDto;

#[test]
fn holes_keep_associative_positions_validate_styles_and_edit_original_input_atomically() {
    let _lock = super::super::super::super::tests::TEST_LOCK.lock().unwrap();
    use SolidField as F;
    for mode in [
        "simple",
        "counterbore",
        "countersink",
        "iso_metric",
        "unified_inch",
        "custom_trapezoidal",
    ] {
        let fixture = Fixture::new();
        let owner = fixture.owner();
        for (op, args) in [
            ("sketch_begin", json!({"type":"origin_plane","plane":"xy"})),
            (
                "sketch_add_rectangle",
                json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":60.,"y":40.},"ctrl_held":true}),
            ),
            (
                "sketch_add_point",
                json!({"position":{"x":15.,"y":20.},"ctrl_held":true}),
            ),
            (
                "sketch_add_point",
                json!({"position":{"x":45.,"y":20.},"ctrl_held":true}),
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
        let top = body
            .faces
            .iter()
            .find(|f| {
                f.plane
                    .is_some_and(|p| p.normal[2] > 0.9 && p.origin[2] > 19.9)
            })
            .unwrap();
        let points: Vec<_> = source.document.profile_catalog[0]
            .reference_points
            .iter()
            .filter(|p| {
                p.point == SketchPointKindDto::Point
                    && (p.position.y - 20.).abs() < 1e-8
                    && p.position.x > 10.
                    && p.position.x < 50.
            })
            .collect();
        assert_eq!(points.len(), 2);
        let mut app = scene(&fixture, &owner);
        let open = |world: &mut World, feature_id| {
            reduce(
                &fixture.engine,
                &fixture.bridge,
                world,
                &owner,
                &FeatureCommand::Open {
                    kind: SolidFormKind::Hole,
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
        let id = open(app.world_mut(), None);
        assert!(!panel(app.world()).unwrap().can_apply);
        accept_pick(
            &fixture.engine,
            &fixture.bridge,
            app.world_mut(),
            &owner,
            id,
            FeaturePick::HoleSupport {
                face: PlanarFaceSourceDto {
                    body_id: body.id,
                    face_id: top.id,
                },
                point: Some([15., 20., 20.]),
            },
            || Ok(()),
        )
        .unwrap();
        action(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            FeatureControl::Clear(F::HolePositions),
            ControlInput::Click,
        )
        .unwrap();
        for p in &points {
            accept_pick(
                &fixture.engine,
                &fixture.bridge,
                app.world_mut(),
                &owner,
                id,
                FeaturePick::HolePosition {
                    point: [p.position.x, p.position.y, 0.],
                    reference: Some(limo_cad_solid::SketchPointRefDto {
                        sketch_name: "Sketch1".into(),
                        entity_id: p.entity_id,
                        point: p.point.clone(),
                    }),
                },
                || Ok(()),
            )
            .unwrap();
        }
        app.world_mut()
            .resource_mut::<NativeFeature>()
            .editor
            .as_mut()
            .unwrap()
            .hovered_point = Some([45., 20., 0.]);
        for invalid in ["-1", "NaN", "2 rad"] {
            field(
                &fixture,
                app.world_mut(),
                &owner,
                id,
                F::HoleDiameter,
                invalid,
            );
            assert!(!panel(app.world()).unwrap().can_apply);
            assert!(app
                .world()
                .resource::<NativeFeature>()
                .editor
                .as_ref()
                .unwrap()
                .hovered_point
                .is_none());
            assert_eq!(exported(&fixture), before);
        }
        field(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            F::HoleDiameter,
            "0.5 cm",
        );
        if mode != "simple" {
            field(&fixture, app.world_mut(), &owner, id, F::Extent, "distance");
            assert_eq!(
                panel(app.world()).unwrap().pick_target,
                Some(F::HolePositions)
            );
            field(&fixture, app.world_mut(), &owner, id, F::HoleDepth, "8 mm");
            field(
                &fixture,
                app.world_mut(),
                &owner,
                id,
                F::BottomStyle,
                "flat",
            );
        }
        if mode == "counterbore" || mode == "countersink" {
            field(&fixture, app.world_mut(), &owner, id, F::HoleStyle, mode);
            let size = if mode == "counterbore" {
                F::CounterboreDiameter
            } else {
                F::CountersinkDiameter
            };
            field(&fixture, app.world_mut(), &owner, id, size, "4");
            assert!(!panel(app.world()).unwrap().can_apply);
            field(&fixture, app.world_mut(), &owner, id, size, "9");
        } else if mode != "simple" {
            field(&fixture, app.world_mut(), &owner, id, F::Threaded, "true");
            if mode == "unified_inch" {
                field(
                    &fixture,
                    app.world_mut(),
                    &owner,
                    id,
                    F::ThreadStandard,
                    mode,
                );
                field(
                    &fixture,
                    app.world_mut(),
                    &owner,
                    id,
                    F::ThreadPreset,
                    "unc-1/4-20",
                );
            } else if mode == "custom_trapezoidal" {
                field(
                    &fixture,
                    app.world_mut(),
                    &owner,
                    id,
                    F::ThreadStandard,
                    mode,
                );
            }
            field(
                &fixture,
                app.world_mut(),
                &owner,
                id,
                F::FullThread,
                "false",
            );
            field(&fixture, app.world_mut(), &owner, id, F::Distance, "9");
            assert!(!panel(app.world()).unwrap().can_apply);
            field(&fixture, app.world_mut(), &owner, id, F::Distance, "4");
        }
        assert!(
            panel(app.world()).unwrap().can_apply,
            "{mode}: {:?}",
            panel(app.world()).unwrap().fields
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
        let result = model_snapshot(&fixture.engine);
        assert!(result.document.scene.errors.is_empty(), "{mode}");
        let volume = combine::volume(&result.document.scene.bodies[0]);
        let removed = combine::volume(body) - volume;
        assert!(
            removed > 50. && removed < 3000.,
            "{mode}: removed {removed}"
        );
        if mode == "simple" {
            let expected = 2. * std::f64::consts::PI * 2.5_f64.powi(2) * 20.;
            assert!(
                (removed - expected).abs() / expected < 0.025,
                "through-hole volume: {removed}/{expected}"
            );
        }
        let definitions =
            parse_engine_envelope(fixture.engine.engine_call("hole_definitions", "")).unwrap();
        assert_eq!(definitions[0]["positions"].as_array().unwrap().len(), 2);
        assert!(definitions[0]["positions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["position_reference"]["sketch_name"] == "Sketch1"));
        let created = exported(&fixture);
        let feature = definitions[0]["feature_id"].as_u64().unwrap();
        let id = open(app.world_mut(), Some(feature));
        let changed = if mode == "simple" {
            F::HoleDiameter
        } else {
            F::HoleDepth
        };
        field(&fixture, app.world_mut(), &owner, id, changed, "6");
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
        field(&fixture, app.world_mut(), &owner, id, changed, "6");
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
        assert!(
            fixture.engine.export_3mf("{}").unwrap().starts_with(b"PK"),
            "{mode} must export a closed mesh"
        );
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

#[test]
fn hole_snap_is_visible_and_associative_on_the_first_support_click() {
    use crate::native_viewport::ViewportCamera;
    use crate::session_bridge::native_interface::controller::NativeServices;
    let _lock = super::super::super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = sketch(&fixture);
    for (op, args) in [
        ("sketch_edit", json!({"name":"Sketch1"})),
        (
            "sketch_add_point",
            json!({"position":{"x":4.,"y":4.},"ctrl_held":true}),
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
    let before = exported(&fixture);
    let mut app = scene(&fixture, &owner);
    native_viewport::apply_interface_viewport(
        app.world_mut(),
        limo_cad_interface::Rect {
            x: 0.,
            y: 0.,
            width: 1000.,
            height: 700.,
        },
        1.,
    )
    .unwrap();
    native_viewport::apply_interface_view(
        app.world_mut(),
        &owner.document_id,
        Some(ViewportCamera {
            position: [10., 6., 100.],
            target: [10., 6., 0.],
            up: [0., 1., 0.],
            ..Default::default()
        }),
        Some(ViewportPresentation::default()),
    )
    .unwrap();
    let id = reduce(
        &fixture.engine,
        &fixture.bridge,
        app.world_mut(),
        &owner,
        &FeatureCommand::Open {
            kind: SolidFormKind::Hole,
            feature_id: None,
        },
        &ControlInput::Click,
        || Ok(()),
    )
    .unwrap()["form_id"]
        .as_u64()
        .unwrap();
    let cursor =
        native_viewport::interface_world_point(app.world(), &owner.document_id, [4., 4., 10.])
            .unwrap()
            .unwrap();
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    hover_references(app.world_mut(), &services, &owner, Some(cursor)).unwrap();
    let editor = app
        .world()
        .resource::<NativeFeature>()
        .editor
        .as_ref()
        .unwrap();
    assert_eq!(
        editor.hovered_point,
        Some([4., 4., 10.]),
        "The base sketch's point projects onto the support face before acceptance"
    );
    assert!(editor.form.hole_support().is_none());
    assert!(!native_viewport::interface_preview_snapshot(app.world())
        .points
        .is_empty());
    handle_canvas_pick(app.world_mut(), &services, &owner, cursor).unwrap();
    let editor = app
        .world()
        .resource::<NativeFeature>()
        .editor
        .as_ref()
        .unwrap();
    assert_eq!(editor.pick_target, Some(SolidField::HolePositions));
    let model = editor.snapshot.model(editor.form.parameter_sketch());
    let (request, _, _) = editor.form.hole_guide(&model).unwrap();
    assert!(
        request.positions.iter().any(|position| position
            .position_reference
            .as_ref()
            .is_some_and(|reference| reference.sketch_name == "Sketch1")),
        "The very first snap must retain its sketch association"
    );
    assert!(panel(app.world()).unwrap().can_apply);
    assert_eq!(
        exported(&fixture),
        before,
        "Hover and selection do not mutate the model"
    );
    action(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        FeatureControl::Cancel,
        ControlInput::Click,
    )
    .unwrap();
}
