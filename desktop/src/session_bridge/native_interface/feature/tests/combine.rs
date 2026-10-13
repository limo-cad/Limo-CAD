use super::*;

pub(super) fn volume(body: &limo_cad_solid::BodyDto) -> f64 {
    body.mesh
        .indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|triangle| {
            let p: Vec<_> = triangle
                .iter()
                .map(|i| {
                    let k = *i as usize * 3;
                    bevy::math::DVec3::new(
                        body.mesh.positions[k] as f64,
                        body.mesh.positions[k + 1] as f64,
                        body.mesh.positions[k + 2] as f64,
                    )
                })
                .collect();
            p[0].dot(p[1].cross(p[2])) / 6.
        })
        .sum::<f64>()
        .abs()
}

#[test]
fn booleans_keep_distinct_sources_and_edit_with_exact_cancel_and_undo() {
    let _lock = super::super::super::super::tests::TEST_LOCK.lock().unwrap();
    for (operation, keep, expected) in [
        ("join", false, 6000.),
        ("cut", false, 2000.),
        ("intersect", false, 2000.),
        ("cut", true, 2000.),
    ] {
        let fixture = Fixture::new();
        let owner = fixture.owner();
        for (op, args) in [
            ("sketch_begin", json!({"type":"origin_plane","plane":"xy"})),
            (
                "sketch_add_rectangle",
                json!({"mode":"two_point","p1":{"x":0.,"y":0.},"p2":{"x":20.,"y":20.},"ctrl_held":true}),
            ),
            ("sketch_finish", json!({})),
            (
                "solid_extrude",
                json!({"sketch_name":"Sketch1","profile_indices":[0],"extent":{"type":"distance","distance":10.}}),
            ),
            ("sketch_begin", json!({"type":"origin_plane","plane":"xy"})),
            (
                "sketch_add_rectangle",
                json!({"mode":"two_point","p1":{"x":10.,"y":0.},"p2":{"x":30.,"y":20.},"ctrl_held":true}),
            ),
            ("sketch_finish", json!({})),
            (
                "solid_extrude",
                json!({"sketch_name":"Sketch2","profile_indices":[0],"extent":{"type":"distance","distance":10.}}),
            ),
        ] {
            fixture
                .bridge
                .apply_native_mutation(&fixture.engine, &owner, op, &args, || Ok(()))
                .unwrap();
        }
        let bodies = fixture.engine.viewport_snapshot().2.bodies;
        assert!(
            bodies.iter().all(|b| (volume(b) - 4000.).abs() < 1e-4),
            "Seed two exact 20 x 20 x 10 bodies before testing booleans"
        );
        let target = bodies[0].id;
        let tool = bodies[1].id;
        let mut app = scene(&fixture, &owner);
        let before = exported(&fixture);
        let open = |world: &mut World, id| {
            reduce(
                &fixture.engine,
                &fixture.bridge,
                world,
                &owner,
                &FeatureCommand::Open {
                    kind: SolidFormKind::Combine,
                    feature_id: id,
                },
                &ControlInput::Click,
                || Ok(()),
            )
            .unwrap()["form_id"]
                .as_u64()
                .unwrap()
        };
        let pick = |world: &mut World, id, field, bodies| {
            action(
                &fixture,
                world,
                &owner,
                id,
                FeatureControl::Pick(field),
                ControlInput::Click,
            )
            .unwrap();
            accept_pick(
                &fixture.engine,
                &fixture.bridge,
                world,
                &owner,
                id,
                FeaturePick::Bodies(bodies),
                || Ok(()),
            )
        };
        let id = open(app.world_mut(), None);
        assert!(!panel(app.world()).unwrap().can_apply);
        pick(app.world_mut(), id, SolidField::TargetBody, vec![target]).unwrap();
        assert!(pick(app.world_mut(), id, SolidField::ToolBodies, vec![target]).is_err());
        assert!(pick(
            app.world_mut(),
            id,
            SolidField::ToolBodies,
            vec![BodyId(u64::MAX)]
        )
        .is_err());
        assert_eq!(exported(&fixture), before);
        pick(app.world_mut(), id, SolidField::ToolBodies, vec![tool]).unwrap();
        field(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            SolidField::Operation,
            operation,
        );
        field(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            SolidField::KeepTools,
            if keep { "true" } else { "false" },
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
        let result = fixture.engine.viewport_snapshot().2;
        assert!(result.errors.is_empty());
        assert_eq!(result.bodies.len(), if keep { 2 } else { 1 });
        let actual = volume(result.bodies.iter().find(|b| b.id == target).unwrap());
        assert!(
            (actual - expected).abs() < 1e-4,
            "{operation} keep={keep}: expected {expected}, got {actual}"
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
        assert!(
            panel(app.world()).unwrap().can_apply,
            "Original tool body must exist in the staged input"
        );
        field(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            SolidField::Operation,
            "cut",
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
        assert_eq!(exported(&fixture), created);
        let id = open(app.world_mut(), Some(feature));
        let next = if operation == "join" { "cut" } else { "join" };
        field(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            SolidField::Operation,
            next,
        );
        field(
            &fixture,
            app.world_mut(),
            &owner,
            id,
            SolidField::KeepTools,
            "false",
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
        let result = fixture.engine.viewport_snapshot().2;
        assert!(result.errors.is_empty());
        assert_eq!(result.bodies.len(), 1);
        assert!(
            (volume(&result.bodies[0]) - if next == "join" { 6000. } else { 2000. }).abs() < 1e-4
        );
        let result = fixture
            .bridge
            .apply_native_history(&fixture.engine, &owner, false, || Ok(()))
            .unwrap();
        let mut restored = created.clone();
        restored["assembly"]["component_structure"]["next_occurrence_id"] = json!(created
            ["assembly"]["component_structure"]["next_occurrence_id"]
            .as_u64()
            .unwrap()
            .max(
                edited["assembly"]["component_structure"]["next_occurrence_id"]
                    .as_u64()
                    .unwrap()
            ));
        assert_eq!(exported(&fixture), restored);
        fixture
            .bridge
            .apply_native_history(&fixture.engine, &result.context, true, || Ok(()))
            .unwrap();
        assert_eq!(exported(&fixture), edited);
    }
}
