use super::*;
use bevy::ecs::schedule::Schedule;
use std::sync::Arc;

fn call(engine: &AppState, method: &str, arguments: Value) {
    crate::session_bridge::parse_engine_envelope(
        engine.engine_call(method, &arguments.to_string()),
    )
    .unwrap();
}

fn pixels(value: Val) -> f32 {
    match value {
        Val::Px(value) => value,
        other => panic!("Expected a fixed annotation bound, got {other:?}"),
    }
}

// Inspect the actual button bounds, transform and focus outline, independently
// of the allocator's reserved rectangles. These are also the click targets.
fn footprint(world: &World, entity: Entity, scale: f32) -> Rect {
    let node = world.get::<Node>(entity).unwrap();
    let size = Vec2::new(pixels(node.width), pixels(node.height));
    let center = Vec2::new(pixels(node.left), pixels(node.top)) + size * 0.5;
    let outline = world.get::<Outline>(entity).unwrap();
    let half = size * 0.5 + Vec2::splat(pixels(outline.width) + pixels(outline.offset));
    let rotation = world
        .get::<UiTransform>(entity)
        .map_or(Rot2::IDENTITY, |t| t.rotation);
    let corners = [
        Vec2::new(-half.x, -half.y),
        Vec2::new(half.x, -half.y),
        Vec2::new(half.x, half.y),
        Vec2::new(-half.x, half.y),
    ]
    .map(|corner| (center + rotation * corner) * scale);
    Rect::from_corners(
        corners
            .into_iter()
            .fold(Vec2::splat(f32::INFINITY), Vec2::min),
        corners
            .into_iter()
            .fold(Vec2::splat(f32::NEG_INFINITY), Vec2::max),
    )
}

#[test]
fn actual_dimension_and_constraint_controls_stay_disjoint_without_changing_saved_positions() {
    for style in ["iso", "aligned"] {
        let engine = Arc::new(AppState::new());
        call(
            &engine,
            "begin_sketch",
            json!({"type":"origin_plane","plane":"xy"}),
        );
        call(
            &engine,
            "add_line",
            json!({
                "from":{"x":0.,"y":0.}, "to_raw":{"x":20.,"y":0.}, "ctrl_held":true
            }),
        );
        let sketch = active(&engine).unwrap().unwrap();
        let line = sketch
            .entities
            .iter()
            .find(|entity| matches!(entity, EntityDto::Line { .. }))
            .unwrap()
            .id();
        call(
            &engine,
            "add_constraints",
            json!({"constraints":[{"type":"horizontal","entity":line}]}),
        );
        // The second measurement becomes a reference dimension. Both saved
        // anchors intentionally occupy the exact same position.
        for _ in 0..2 {
            call(
                &engine,
                "add_dimension",
                json!({"entities":[line], "text_pos":{"x":10.,"y":-4.}}),
            );
        }
        call(&engine, "set_dimension_style", json!({"style":style}));
        let sketch = active(&engine).unwrap().unwrap();
        assert_eq!(sketch.dimensions.len(), 2);
        assert_eq!(sketch.dimensions[0].text_pos, sketch.dimensions[1].text_pos);
        let model = engine.engine_call("project_export_model", "");
        let revision = engine.geometry_revision();
        let owner = DocumentContext {
            window_id: "main".into(),
            document_id: String::new(),
            epoch: 0,
        };
        let services = NativeServices {
            engine: engine.clone(),
            bridge: Arc::new(SessionBridgeState::default()),
        };
        let editor = Editor {
            stamp: Some(Stamp {
                owner: owner.clone(),
                revision,
                sketch: Some(sketch.name.clone()),
                basis: Some(sketch.basis),
            }),
            ..default()
        };
        let mut app = native_viewport::interface_scene_fixture();
        app.add_schedule(Schedule::new(Update))
            .init_resource::<Assets<Image>>();
        interface_shell::install(&mut app, NativeInterfaceHandle::new(|| {}), |_, _| {});
        let camera = app.world_mut().spawn(InterfaceCamera).id();
        let keys: Vec<_> = sketch
            .dimensions
            .iter()
            .map(|dim| format!("dim-{}", dim.constraint_id.0))
            .chain(
                sketch
                    .constraints
                    .iter()
                    .filter(|c| c.constraint.kind() == ConstraintKind::Geometric)
                    .map(|c| format!("constraint-{}", c.id.0)),
            )
            .collect();
        assert!(keys.len() >= 3);
        let mut retained = None;
        for ui_scale in [0.75, 1., 1.75] {
            let canvas = InterfaceRect {
                x: 240.,
                y: 120.,
                width: 1360. / f64::from(ui_scale) - 240.,
                height: 860. / f64::from(ui_scale) - 168.,
            };
            let scale = ui_scale * 2.; // Retina monitor plus interface preference.
            app.world_mut().insert_resource(bevy::ui::UiScale(ui_scale));
            native_viewport::apply_interface_viewport(app.world_mut(), canvas, scale).unwrap();
            for distance in [80., 200., 600.] {
                for up in [[0., 1., 0.], [1., 1., 0.], [1., 0., 0.]] {
                    let world = app.world_mut();
                    native_viewport::apply_interface_view(
                        world,
                        &owner.document_id,
                        Some(ViewportCamera {
                            position: [10., 0., distance],
                            target: [10., 0., 0.],
                            up,
                            vertical_fov_degrees: 45.,
                        }),
                        None,
                    )
                    .unwrap();
                    synchronize(world, camera, &services, &owner, &editor, canvas).unwrap();
                    world.run_schedule(Update);
                    let state = world.resource::<AnnotationState>();
                    let entities: Vec<_> = keys
                        .iter()
                        .map(|key| {
                            state
                                .widgets
                                .entity(key)
                                .expect("Every fixture annotation remains visible")
                        })
                        .collect();
                    if let Some(previous) = &retained {
                        assert_eq!(&entities, previous);
                    }
                    retained = Some(entities.clone());
                    for (i, entity) in entities.iter().enumerate() {
                        let control = world.get::<InterfaceControl>(*entity).unwrap();
                        assert!(control.visible && !control.disabled);
                        assert!(world
                            .get::<interface_shell::InterfacePointerPassthrough>(*entity)
                            .is_none());
                        let bounds = footprint(world, *entity, scale);
                        assert!(
                            bounds.min.x >= canvas.x as f32 * scale
                                && bounds.min.y >= canvas.y as f32 * scale
                        );
                        assert!(
                            bounds.max.x <= (canvas.x + canvas.width) as f32 * scale
                                && bounds.max.y <= (canvas.y + canvas.height) as f32 * scale
                        );
                        for other in &entities[i + 1..] {
                            let other = footprint(world, *other, scale);
                            assert!(bounds.max.x <= other.min.x || other.max.x <= bounds.min.x
                                || bounds.max.y <= other.min.y || other.max.y <= bounds.min.y,
                                "{style}, scale {ui_scale}, distance {distance}: {bounds:?} overlaps {other:?}");
                        }
                    }
                    assert_eq!(state.sketch.as_ref().unwrap().dimensions, sketch.dimensions);
                    assert_eq!(engine.geometry_revision(), revision);
                    assert_eq!(engine.engine_call("project_export_model", ""), model);
                }
            }
        }
    }
}
