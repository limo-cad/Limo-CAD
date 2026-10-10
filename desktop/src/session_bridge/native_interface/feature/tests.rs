use super::super::tests::Fixture;
use super::*;
use crate::native_viewport::{ViewportLineLayer, ViewportPresentation};
mod body_planes;
mod combine;
mod holes;
mod move_copy;
mod parity;
mod patterns;
mod planes;
mod profiles;
mod threads;

fn sketch(fixture: &Fixture) -> DocumentContext {
    let owner = fixture.owner();
    for (operation, arguments) in [
        ("sketch_begin", json!({"type":"origin_plane","plane":"xy"})),
        (
            "sketch_add_rectangle",
            json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":20.,"y":12.},"ctrl_held":false}),
        ),
        ("sketch_finish", json!({})),
    ] {
        fixture
            .bridge
            .apply_native_mutation(&fixture.engine, &owner, operation, &arguments, || Ok(()))
            .unwrap();
    }
    owner
}

fn scene(fixture: &Fixture, owner: &DocumentContext) -> bevy::app::App {
    let mut app = native_viewport::interface_scene_fixture();
    native_viewport::apply_interface_model(app.world_mut(), model_snapshot(&fixture.engine))
        .unwrap();
    let presentation = ViewportPresentation {
        selected_profiles: vec![ProfileRefDto {
            sketch_name: "Sketch1".into(),
            profile_index: 0,
        }],
        ..Default::default()
    };
    native_viewport::apply_interface_view(
        app.world_mut(),
        &owner.document_id,
        None,
        Some(presentation),
    )
    .unwrap();
    app
}

fn exported(fixture: &Fixture) -> Value {
    let exported =
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap();
    serde_json::from_str(exported.as_str().unwrap()).unwrap()
}

fn open(
    fixture: &Fixture,
    world: &mut World,
    owner: &DocumentContext,
    feature_id: Option<u64>,
) -> u64 {
    reduce(
        &fixture.engine,
        &fixture.bridge,
        world,
        owner,
        &FeatureCommand::Open {
            kind: SolidFormKind::Extrude,
            feature_id,
        },
        &ControlInput::Click,
        || Ok(()),
    )
    .unwrap()["form_id"]
        .as_u64()
        .unwrap()
}

fn action(
    fixture: &Fixture,
    world: &mut World,
    owner: &DocumentContext,
    form_id: u64,
    action: FeatureControl,
    input: ControlInput,
) -> Result<Value, String> {
    reduce(
        &fixture.engine,
        &fixture.bridge,
        world,
        owner,
        &FeatureCommand::Control { form_id, action },
        &input,
        || Ok(()),
    )
}

fn field(
    fixture: &Fixture,
    world: &mut World,
    owner: &DocumentContext,
    id: u64,
    field: SolidField,
    value: &str,
) {
    action(
        fixture,
        world,
        owner,
        id,
        FeatureControl::Field(field),
        ControlInput::SetValue(value.into()),
    )
    .unwrap();
}

fn maximum_z(fixture: &Fixture) -> f32 {
    fixture
        .engine
        .viewport_snapshot()
        .2
        .bodies
        .iter()
        .flat_map(|body| {
            body.mesh
                .positions
                .as_chunks::<3>()
                .0
                .iter()
                .map(|point| point[2])
        })
        .fold(f32::NEG_INFINITY, f32::max)
}

#[test]
fn topology_features_stage_original_input_without_mutating_and_commit_one_undo_step() {
    let _lock = super::super::super::tests::TEST_LOCK.lock().unwrap();
    for kind in [
        SolidFormKind::Fillet,
        SolidFormKind::Chamfer,
        SolidFormKind::Shell,
    ] {
        let fixture = Fixture::new();
        let owner = sketch(&fixture);
        fixture.bridge.apply_native_mutation(&fixture.engine,&owner,"solid_extrude",
            &json!({"sketch_name":"Sketch1","profile_indices":[0],"operation":"new_body","extent":{"type":"distance","distance":10.}}),||Ok(())).unwrap();
        let mut app = scene(&fixture, &owner);
        let original = exported(&fixture);
        let open_edge = |world: &mut World, feature_id| {
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
        let id = open_edge(app.world_mut(), None);
        assert!(!panel(app.world()).unwrap().can_apply);
        let model = model_snapshot(&fixture.engine);
        let body = &model.document.scene.bodies[0];
        let edge = body.edges.iter().find(|e| e.refinable).unwrap().id;
        accept_pick(
            &fixture.engine,
            &fixture.bridge,
            app.world_mut(),
            &owner,
            id,
            if kind == SolidFormKind::Shell {
                FeaturePick::Faces {
                    body: Some(body.id),
                    faces: vec![body.faces[0].id],
                }
            } else {
                FeaturePick::Edges {
                    body: Some(body.id),
                    edges: vec![edge],
                }
            },
            || Ok(()),
        )
        .unwrap();
        assert!(accept_pick(
            &fixture.engine,
            &fixture.bridge,
            app.world_mut(),
            &owner,
            id,
            if kind == SolidFormKind::Shell {
                FeaturePick::Faces {
                    body: Some(body.id),
                    faces: vec![limo_cad_core::FaceId(u64::MAX)],
                }
            } else {
                FeaturePick::Edges {
                    body: Some(body.id),
                    edges: vec![limo_cad_core::EdgeId(u64::MAX)],
                }
            },
            || Ok(())
        )
        .is_err());
        let size = if kind == SolidFormKind::Fillet {
            SolidField::Radius
        } else if kind == SolidFormKind::Shell {
            SolidField::Thickness
        } else {
            SolidField::Distance
        };
        field(&fixture, app.world_mut(), &owner, id, size, "-1 mm");
        assert!(!panel(app.world()).unwrap().can_apply);
        assert_eq!(exported(&fixture), original);
        field(&fixture, app.world_mut(), &owner, id, size, "1 mm");
        action(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            FeatureControl::Apply,
            ControlInput::Click,
        )
        .unwrap();
        assert!(fixture.engine.viewport_snapshot().2.errors.is_empty());
        let created = exported(&fixture);
        let feature = fixture
            .engine
            .document_snapshot()
            .features
            .last()
            .unwrap()
            .id
            .0;
        let receipt = fixture
            .bridge
            .native_document_receipt(&fixture.engine, &owner)
            .unwrap();
        let id = open_edge(app.world_mut(), Some(feature));
        assert_eq!(
            fixture
                .bridge
                .native_document_receipt(&fixture.engine, &owner)
                .unwrap(),
            receipt
        );
        assert_eq!(
            exported(&fixture),
            created,
            "opening a feature must leave the live cursor and document untouched"
        );
        assert!(
            panel(app.world()).unwrap().can_apply,
            "the original edge must be selectable in the input model"
        );
        field(&fixture, app.world_mut(), &owner, id, size, "2 mm");
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
        let id = open_edge(app.world_mut(), Some(feature));
        field(&fixture, app.world_mut(), &owner, id, size, "1000 mm");
        assert!(action(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            FeatureControl::Apply,
            ControlInput::Click
        )
        .is_err());
        assert_eq!(
            exported(&fixture),
            created,
            "kernel failure must preserve the complete live document"
        );
        field(&fixture, app.world_mut(), &owner, id, size, "2 mm");
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
        assert_eq!(fixture.engine.document_snapshot().rollback_index, 3);
        let result = fixture
            .bridge
            .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
            .unwrap();
        assert_eq!(
            exported(&fixture),
            created,
            "one Undo must restore the pre-edit feature and original cursor"
        );
        fixture
            .bridge
            .apply_native_history(&fixture.engine, &result.context, true, || Ok(()))
            .unwrap();
        assert_eq!(exported(&fixture), edited);
        if kind == SolidFormKind::Shell {
            let scene = fixture.engine.viewport_snapshot().2;
            assert!(scene.bodies[0]
                .mesh
                .positions
                .as_chunks::<3>()
                .0
                .iter()
                .all(|p| p[0] >= -1e-4
                    && p[0] <= 20.0001
                    && p[1] >= -1e-4
                    && p[1] <= 12.0001
                    && p[2] >= -1e-4
                    && p[2] <= 10.0001));
            let current_owner = fixture.owner();
            native_viewport::apply_interface_model(
                app.world_mut(),
                model_snapshot(&fixture.engine),
            )
            .unwrap();
            let opened = reduce(
                &fixture.engine,
                &fixture.bridge,
                app.world_mut(),
                &current_owner,
                &FeatureCommand::Open {
                    kind,
                    feature_id: Some(feature),
                },
                &ControlInput::Click,
                || Ok(()),
            )
            .unwrap();
            let id = opened["form_id"].as_u64().unwrap();
            field(
                &fixture,
                app.world_mut(),
                &current_owner,
                id,
                SolidField::Inward,
                "false",
            );
            action(
                &fixture,
                app.world_mut(),
                &current_owner,
                id,
                FeatureControl::Apply,
                ControlInput::Click,
            )
            .unwrap();
            let scene = fixture.engine.viewport_snapshot().2;
            assert!(scene.errors.is_empty());
            assert!(scene.bodies[0]
                .mesh
                .positions
                .as_chunks::<3>()
                .0
                .iter()
                .any(|p| p[0] < -0.1
                    || p[0] > 20.1
                    || p[1] < -0.1
                    || p[1] > 12.1
                    || p[2] < -0.1
                    || p[2] > 10.1));
        }
    }
}

#[test]
fn rib_native_form_applies_edits_and_cancels_with_real_geometry() {
    let _lock = super::super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    for (op, args) in [
        ("sketch_begin", json!({"type":"origin_plane","plane":"xy"})),
        (
            "sketch_add_line",
            json!({"from":{"x":0.,"y":0.},"to_raw":{"x":40.,"y":0.},"ctrl_held":true}),
        ),
        ("sketch_finish", json!({})),
    ] {
        fixture
            .bridge
            .apply_native_mutation(&fixture.engine, &owner, op, &args, || Ok(()))
            .unwrap();
    }
    let mut app = scene(&fixture, &owner);
    let open_rib = |world: &mut World, feature_id| {
        reduce(
            &fixture.engine,
            &fixture.bridge,
            world,
            &owner,
            &FeatureCommand::Open {
                kind: SolidFormKind::Rib,
                feature_id,
            },
            &ControlInput::Click,
            || Ok(()),
        )
        .unwrap()["form_id"]
            .as_u64()
            .unwrap()
    };
    let id = open_rib(app.world_mut(), None);
    assert!(!panel(app.world()).unwrap().can_apply);
    let snapshot = model_snapshot(&fixture.engine);
    let curve = snapshot.document.profile_catalog[0].path_curves[0].entity_id();
    accept_pick(
        &fixture.engine,
        &fixture.bridge,
        app.world_mut(),
        &owner,
        id,
        FeaturePick::Path(PathRefDto {
            sketch_name: "Sketch1".into(),
            entity_ids: vec![curve],
        }),
        || Ok(()),
    )
    .unwrap();
    field(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        SolidField::Thickness,
        "3 mm",
    );
    field(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        SolidField::Distance,
        "1 cm",
    );
    let created = action(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        FeatureControl::Apply,
        ControlInput::Click,
    )
    .unwrap();
    assert!(
        created["form_error"].is_null() && created["render_error"].is_null(),
        "{created}"
    );
    assert_eq!(fixture.engine.viewport_snapshot().2.bodies.len(), 1);
    assert!((maximum_z(&fixture) - 10.).abs() < 1e-4);
    let defs = || parse_engine_envelope(fixture.engine.engine_call("rib_definitions", "")).unwrap();
    let original = defs();
    let feature = original[0]["feature_id"].as_u64().unwrap();
    let id = open_rib(app.world_mut(), Some(feature));
    field(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        SolidField::Distance,
        "15",
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
    assert_eq!(defs(), original);
    let id = open_rib(app.world_mut(), Some(feature));
    field(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        SolidField::Distance,
        "15",
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
    assert!((maximum_z(&fixture) - 15.).abs() < 1e-4);
    let undone = fixture
        .bridge
        .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
        .unwrap();
    assert!((maximum_z(&fixture) - 10.).abs() < 1e-4);
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &undone.context, true, || Ok(()))
        .unwrap();
    assert!((maximum_z(&fixture) - 15.).abs() < 1e-4);
    assert_eq!(defs()[0]["feature_id"], feature);
    assert_eq!(defs()[0]["thickness"], 3.);
}

#[test]
fn sweep_and_loft_native_forms_create_edit_cancel_and_undo_exact_solids() {
    let _lock = super::super::super::tests::TEST_LOCK.lock().unwrap();
    for kind in [SolidFormKind::Sweep, SolidFormKind::Loft] {
        let fixture = Fixture::new();
        let owner = sketch(&fixture);
        let mutate = |op, args| {
            fixture
                .bridge
                .apply_native_mutation(&fixture.engine, &owner, op, &args, || Ok(()))
                .unwrap()
        };
        let definition_method = if kind == SolidFormKind::Sweep {
            "sweep_definitions"
        } else {
            "loft_definitions"
        };
        if kind == SolidFormKind::Sweep {
            mutate("sketch_begin", json!({"type":"origin_plane","plane":"xz"}));
            mutate(
                "sketch_add_line",
                json!({"from":{"x":0.,"y":0.},"to_raw":{"x":0.,"y":30.},"ctrl_held":true}),
            );
        } else {
            mutate(
                "construction_plane_offset",
                json!({"name":"Top section","reference":{"type":"origin_plane","plane":"xy"},"distance":30.}),
            );
            let datum = fixture
                .engine
                .document_snapshot()
                .features
                .last()
                .unwrap()
                .id
                .0;
            let definitions =
                parse_engine_envelope(fixture.engine.engine_call("datum_plane_definitions", ""))
                    .unwrap();
            let datum_id = definitions
                .as_array()
                .unwrap()
                .iter()
                .find(|d| d["feature_id"] == datum)
                .unwrap()["datum_id"]
                .clone();
            mutate(
                "sketch_begin",
                json!({"type":"datum_plane","datum_id":datum_id}),
            );
            mutate(
                "sketch_add_rectangle",
                json!({"mode":"two_point","p1":{"x":2.,"y":2.},"p2":{"x":18.,"y":10.},"ctrl_held":true}),
            );
        }
        mutate("sketch_finish", json!({}));
        let mut app = scene(&fixture, &owner);
        let open_kind = |world: &mut World, feature_id| {
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
        let id = open_kind(app.world_mut(), None);
        assert!(!panel(app.world()).unwrap().can_apply);
        let before = exported(&fixture);
        let pick = if kind == SolidFormKind::Sweep {
            action(
                &fixture,
                app.world_mut(),
                &owner,
                id,
                FeatureControl::Pick(SolidField::Path),
                ControlInput::Click,
            )
            .unwrap();
            let snapshot = model_snapshot(&fixture.engine);
            let path = snapshot
                .document
                .profile_catalog
                .iter()
                .find(|s| s.sketch_name == "Sketch2")
                .unwrap();
            FeaturePick::Path(PathRefDto {
                sketch_name: path.sketch_name.clone(),
                entity_ids: vec![path.path_curves[0].entity_id()],
            })
        } else {
            FeaturePick::Profiles(vec![
                ProfileRefDto {
                    sketch_name: "Sketch1".into(),
                    profile_index: 0,
                },
                ProfileRefDto {
                    sketch_name: "Sketch2".into(),
                    profile_index: 0,
                },
            ])
        };
        accept_pick(
            &fixture.engine,
            &fixture.bridge,
            app.world_mut(),
            &owner,
            id,
            pick,
            || Ok(()),
        )
        .unwrap();
        assert!(
            panel(app.world()).unwrap().can_apply,
            "{} fields invalid",
            kind.label()
        );
        assert_eq!(
            exported(&fixture),
            before,
            "References must not mutate geometry"
        );
        let created = action(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            FeatureControl::Apply,
            ControlInput::Click,
        )
        .unwrap();
        assert!(
            created["form_error"].is_null() && created["render_error"].is_null(),
            "{created}"
        );
        assert_eq!(fixture.engine.viewport_snapshot().2.bodies.len(), 1);
        assert!(fixture.engine.viewport_snapshot().2.errors.is_empty());
        assert!((maximum_z(&fixture) - 30.).abs() < 1e-4);
        let feature = fixture
            .engine
            .document_snapshot()
            .features
            .last()
            .unwrap()
            .id
            .0;
        let definitions =
            || parse_engine_envelope(fixture.engine.engine_call(definition_method, "")).unwrap();
        let original = definitions();
        let edited_field = if kind == SolidFormKind::Sweep {
            SolidField::ForceC1
        } else {
            SolidField::Ruled
        };
        let prop = if kind == SolidFormKind::Sweep {
            "force_c1"
        } else {
            "ruled"
        };
        let id = open_kind(app.world_mut(), Some(feature));
        field(&fixture, app.world_mut(), &owner, id, edited_field, "true");
        action(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            FeatureControl::Cancel,
            ControlInput::Click,
        )
        .unwrap();
        assert_eq!(definitions(), original);
        let id = open_kind(app.world_mut(), Some(feature));
        field(&fixture, app.world_mut(), &owner, id, edited_field, "true");
        action(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            FeatureControl::Apply,
            ControlInput::Click,
        )
        .unwrap();
        assert_eq!(definitions()[0][prop], true);
        let undone = fixture
            .bridge
            .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
            .unwrap();
        assert_eq!(definitions()[0][prop], false);
        fixture
            .bridge
            .apply_native_history(&fixture.engine, &undone.context, true, || Ok(()))
            .unwrap();
        assert_eq!(definitions()[0][prop], true);
        assert_eq!(definitions()[0]["feature_id"], feature);
    }
}

#[test]
fn revolve_controls_commit_edit_cancel_and_undo_a_real_parametric_solid() {
    let _lock = super::super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = sketch(&fixture);
    let mut app = scene(&fixture, &owner);
    let open_revolve = |world: &mut World, feature_id| {
        reduce(
            &fixture.engine,
            &fixture.bridge,
            world,
            &owner,
            &FeatureCommand::Open {
                kind: SolidFormKind::Revolve,
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
    let id = open_revolve(app.world_mut(), None);
    assert!(!panel(app.world()).unwrap().can_apply);
    field(&fixture, app.world_mut(), &owner, id, SolidField::Axis, "x");
    field(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        SolidField::Angle,
        "180",
    );
    assert!(panel(app.world()).unwrap().can_apply);
    assert!(
        !native_viewport::interface_preview_snapshot(app.world()).lines[0]
            .segments
            .is_empty()
    );
    assert_eq!(exported(&fixture), before);
    let result = action(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        FeatureControl::Apply,
        ControlInput::Click,
    )
    .unwrap();
    assert_eq!(result["operation"], "solid_revolve");
    assert!(
        result["form_error"].is_null() && result["render_error"].is_null(),
        "{result}"
    );
    assert_eq!(fixture.engine.viewport_snapshot().2.bodies.len(), 1);
    assert!(fixture.engine.viewport_snapshot().2.errors.is_empty());
    let feature = fixture
        .engine
        .document_snapshot()
        .features
        .last()
        .unwrap()
        .id;
    let initial = exported(&fixture);
    let id = open_revolve(app.world_mut(), Some(feature.0));
    field(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        SolidField::Angle,
        "270",
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
    assert_eq!(exported(&fixture), initial);
    let id = open_revolve(app.world_mut(), Some(feature.0));
    field(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        SolidField::Angle,
        "270",
    );
    let result = action(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        FeatureControl::Apply,
        ControlInput::Click,
    )
    .unwrap();
    assert_eq!(result["operation"], "solid_edit_revolve");
    assert_eq!(
        fixture
            .engine
            .document_snapshot()
            .features
            .last()
            .unwrap()
            .id,
        feature
    );
    let definitions =
        || parse_engine_envelope(fixture.engine.engine_call("revolve_definitions", "")).unwrap();
    assert_eq!(definitions()[0]["angle_deg"], 270.);
    let undone = fixture
        .bridge
        .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
        .unwrap();
    assert_eq!(definitions()[0]["angle_deg"], 180.);
    fixture
        .bridge
        .apply_native_history(&fixture.engine, &undone.context, true, || Ok(()))
        .unwrap();
    assert_eq!(definitions()[0]["angle_deg"], 270.);
}

#[test]
fn actual_preview_and_invalid_fields_never_mutate_the_model_and_cancel_restores_prior_preview() {
    let _lock = super::super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = sketch(&fixture);
    let before = exported(&fixture);
    let mut app = scene(&fixture, &owner);
    let prior = ViewportPreview {
        lines: vec![ViewportLineLayer {
            color: [1., 0., 0., 1.],
            width: 2.,
            segments: vec![0., 0., 0., 2., 3., 4.].into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    native_viewport::apply_interface_preview(app.world_mut(), &owner.document_id, prior.clone())
        .unwrap();
    let retained = native_viewport::interface_preview_snapshot(app.world());
    let id = open(&fixture, app.world_mut(), &owner, None);
    assert!(Arc::ptr_eq(
        &retained,
        &app.world()
            .resource::<NativeFeature>()
            .editor
            .as_ref()
            .unwrap()
            .previous_preview
    ));
    assert!(panel(app.world()).unwrap().can_apply);
    let visible = native_viewport::interface_preview_snapshot(app.world());
    assert!(
        visible.lines[0].segments.len() >= 24 && visible.triangles[0].positions.len() >= 108,
        "The renderer receives the selected source outline and shaded extrusion volume"
    );
    assert_eq!(exported(&fixture), before);
    field(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        SolidField::Distance,
        "2 + (",
    );
    let panel = panel(app.world()).unwrap();
    assert!(!panel.can_apply);
    let distance = panel
        .fields
        .iter()
        .find(|field| field.field == SolidField::Distance)
        .unwrap();
    assert!(distance.error.is_some());
    assert!(
        matches!(&distance.value, limo_cad_interface::Field::Text {value,..} if value == "2 + (")
    );
    let invalid = native_viewport::interface_preview_snapshot(app.world());
    assert!(
        invalid.arrows.is_empty(),
        "An invalid distance must not show a tool volume"
    );
    assert!(
        invalid
            .triangles
            .iter()
            .flat_map(|t| t.positions.as_chunks::<3>().0.iter())
            .all(|p| p[2] == 0.),
        "Only the accepted source region remains highlighted while a number is invalid"
    );
    assert!(action(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        FeatureControl::Apply,
        ControlInput::Click
    )
    .is_err());
    assert_eq!(exported(&fixture), before);
    let cancelled = action(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        FeatureControl::Cancel,
        ControlInput::Click,
    )
    .unwrap();
    assert_eq!(cancelled["preview_restored"], true);
    assert!(super::panel(app.world()).is_none());
    assert!(Arc::ptr_eq(
        &retained,
        &native_viewport::interface_preview_snapshot(app.world())
    ));
    assert_eq!(
        native_viewport::interface_preview_snapshot(app.world()).lines[0].segments,
        prior.lines[0].segments
    );
    assert_eq!(exported(&fixture), before);
    let next = open(&fixture, app.world_mut(), &owner, None);
    assert_ne!(next, id);
    assert!(action(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        FeatureControl::Apply,
        ControlInput::Click
    )
    .is_err());
    assert_eq!(exported(&fixture), before);
}

#[test]
fn native_apply_and_edit_recompute_the_real_parametric_extrusion_and_cancel_preserves_it() {
    let _lock = super::super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = sketch(&fixture);
    let mut app = scene(&fixture, &owner);
    let id = open(&fixture, app.world_mut(), &owner, None);
    field(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        SolidField::Distance,
        "=(2 + 3) * 5 mm",
    );
    let result = action(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        FeatureControl::Apply,
        ControlInput::Click,
    )
    .unwrap();
    assert!(result["render_error"].is_null(), "{result}");
    assert!(result["form_error"].is_null(), "{result}");
    assert!(panel(app.world()).is_none());
    assert_eq!(fixture.engine.document_snapshot().features.len(), 2);
    assert_eq!(fixture.engine.viewport_snapshot().2.bodies.len(), 1);
    assert!((maximum_z(&fixture) - 25.).abs() < 1e-4);
    let feature_id = fixture
        .engine
        .document_snapshot()
        .features
        .last()
        .unwrap()
        .id
        .0;
    let before = exported(&fixture);
    let edit_id = open(&fixture, app.world_mut(), &owner, Some(feature_id));
    field(
        &fixture,
        app.world_mut(),
        &owner,
        edit_id,
        SolidField::Distance,
        "35 mm",
    );
    assert_eq!(
        exported(&fixture),
        before,
        "An edit preview must not stage changes in the live history"
    );
    action(
        &fixture,
        app.world_mut(),
        &owner,
        edit_id,
        FeatureControl::Cancel,
        ControlInput::Click,
    )
    .unwrap();
    assert_eq!(exported(&fixture), before);
    let edit_id = open(&fixture, app.world_mut(), &owner, Some(feature_id));
    field(
        &fixture,
        app.world_mut(),
        &owner,
        edit_id,
        SolidField::Distance,
        "35 mm",
    );
    let result = action(
        &fixture,
        app.world_mut(),
        &owner,
        edit_id,
        FeatureControl::Apply,
        ControlInput::Click,
    )
    .unwrap();
    assert_eq!(result["operation"], "solid_edit_extrude");
    assert!(result["render_error"].is_null(), "{result}");
    assert_eq!(fixture.engine.document_snapshot().features.len(), 2);
    assert_eq!(
        fixture
            .engine
            .document_snapshot()
            .features
            .last()
            .unwrap()
            .id
            .0,
        feature_id
    );
    assert!((maximum_z(&fixture) - 35.).abs() < 1e-4);
    assert_ne!(exported(&fixture), before);
    assert!(fixture.engine.viewport_snapshot().2.errors.is_empty());
}

#[test]
fn stale_owner_revision_and_cancelled_activation_cannot_commit_or_repaint_old_work() {
    let _lock = super::super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = sketch(&fixture);
    let mut app = scene(&fixture, &owner);
    let id = open(&fixture, app.world_mut(), &owner, None);
    let before = exported(&fixture);
    let rejected = reduce(
        &fixture.engine,
        &fixture.bridge,
        app.world_mut(),
        &owner,
        &FeatureCommand::Control {
            form_id: id,
            action: FeatureControl::Apply,
        },
        &ControlInput::Click,
        || Err("Control binding changed".into()),
    );
    assert!(rejected.is_err());
    assert_eq!(exported(&fixture), before);
    assert!(
        panel(app.world()).unwrap().can_apply,
        "Unchanged failed Apply retains a corrected, retryable draft"
    );
    fixture.rename(&owner, "External edit").unwrap();
    let changed = exported(&fixture);
    assert!(action(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        FeatureControl::Apply,
        ControlInput::Click
    )
    .is_err());
    assert_eq!(exported(&fixture), changed);
    native_viewport::apply_interface_model(app.world_mut(), model_snapshot(&fixture.engine))
        .unwrap();
    synchronize(&fixture.engine, &fixture.bridge, app.world_mut(), &owner).unwrap();
    assert!(panel(app.world()).is_none());
    assert!(native_viewport::interface_preview_snapshot(app.world())
        .lines
        .is_empty());
    let id = open(&fixture, app.world_mut(), &owner, None);
    let replacement = fixture
        .bridge
        .apply_native_mutation(
            &fixture.engine,
            &owner,
            "cad_new_project",
            &json!({}),
            || Ok(()),
        )
        .unwrap();
    native_viewport::apply_interface_model(app.world_mut(), model_snapshot(&fixture.engine))
        .unwrap();
    synchronize(
        &fixture.engine,
        &fixture.bridge,
        app.world_mut(),
        &replacement.context,
    )
    .unwrap();
    assert!(panel(app.world()).is_none());
    assert!(action(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        FeatureControl::Cancel,
        ControlInput::Click
    )
    .is_err());
    assert!(fixture.engine.document_snapshot().features.is_empty());
}

#[test]
fn focused_escape_closes_its_form_without_overwriting_a_newer_preview() {
    let _lock = super::super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = sketch(&fixture);
    let before = exported(&fixture);
    let mut app = scene(&fixture, &owner);
    let id = open(&fixture, app.world_mut(), &owner, None);
    let newer = ViewportPreview {
        lines: vec![ViewportLineLayer {
            color: [0., 1., 0., 1.],
            width: 2.,
            segments: vec![4., 5., 6., 7., 8., 9.].into(),
            ..Default::default()
        }],
        ..Default::default()
    };
    native_viewport::apply_interface_preview(app.world_mut(), &owner.document_id, newer.clone())
        .unwrap();
    let cancelled = action(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        FeatureControl::Field(SolidField::Distance),
        ControlInput::Key(limo_cad_interface::KeyChord::plain("Escape")),
    )
    .unwrap();
    assert_eq!(cancelled["cancelled"], true);
    assert_eq!(cancelled["preview_restored"], false);
    assert!(panel(app.world()).is_none());
    assert_eq!(
        native_viewport::interface_preview_snapshot(app.world()).lines[0].segments,
        newer.lines[0].segments
    );
    assert_eq!(exported(&fixture), before);
}

#[test]
fn native_apply_enqueues_once_and_completes_the_original_form_with_real_geometry() {
    use crate::native_viewport::interface_shell::NativeInterfaceHandle;
    use crate::session_bridge::native_interface::controller::{worker, NativeServices};
    let _lock = super::super::super::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = sketch(&fixture);
    let mut app = scene(&fixture, &owner);
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    worker::install(
        app.world_mut(),
        services.clone(),
        NativeInterfaceHandle::new(|| {}),
    )
    .unwrap();
    let id = open(&fixture, app.world_mut(), &owner, None);
    field(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        SolidField::Distance,
        "25 mm",
    );
    let pending = action(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        FeatureControl::Apply,
        ControlInput::Click,
    )
    .unwrap();
    assert_eq!(pending["mutation_pending"], true);
    assert!(worker::busy(app.world()));
    assert!(panel(app.world()).unwrap().busy);
    assert!(!panel(app.world()).unwrap().can_apply);
    assert!(action(
        &fixture,
        app.world_mut(),
        &owner,
        id,
        FeatureControl::Apply,
        ControlInput::Click
    )
    .unwrap_err()
    .contains("still applying"));
    let (_, mut camera, _, _) = native_viewport::interface_view_snapshot(app.world());
    camera.position[0] += 5.;
    native_viewport::apply_interface_view(app.world_mut(), &owner.document_id, Some(camera), None)
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let outcome = loop {
        if let Some(outcome) = worker::poll(app.world_mut(), &services) {
            break outcome;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "Native extrusion worker did not complete"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    let completed = outcome.value.unwrap();
    assert_eq!(completed["operation"], "solid_extrude");
    assert!(completed["render_error"].is_null(), "{completed}");
    assert!(!worker::busy(app.world()));
    assert!(panel(app.world()).is_none());
    assert_eq!(fixture.engine.document_snapshot().features.len(), 2);
    assert_eq!(fixture.engine.viewport_snapshot().2.bodies.len(), 1);
    assert!((maximum_z(&fixture) - 25.).abs() < 1e-4);
}
