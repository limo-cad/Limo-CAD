use super::*;
use crate::{
    app_preferences::{palette::viewport_palette, ResolvedTheme},
    native_viewport::ui::{Appearance, ViewportUiTheme},
};
use bevy::ecs::schedule::Schedule;
use std::sync::Arc;

fn call(engine: &AppState, method: &str, arguments: Value) -> Value {
    crate::session_bridge::parse_engine_envelope(engine.engine_call(method, &arguments.to_string()))
        .unwrap()
}

#[test]
fn cached_dimension_labels_restyle_without_refetching_sketch_or_rebinding_controls() {
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
        "add_dimension",
        json!({"entities":[line], "text_pos":{"x":10.,"y":-4.}}),
    );
    let sketch = active(&engine).unwrap().unwrap();
    assert_eq!(sketch.dimensions.len(), 1);
    let dimension_key = format!("dim-{}", sketch.dimensions[0].constraint_id.0);
    let model = engine.engine_call("project_export_model", "");
    let revision = engine.geometry_revision();
    let services = NativeServices {
        engine: engine.clone(),
        bridge: Arc::new(SessionBridgeState::default()),
    };
    let owner = DocumentContext {
        window_id: "main".into(),
        document_id: String::new(),
        epoch: 0,
    };
    let mut editor = Editor {
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
    let canvas = InterfaceRect {
        x: 0.,
        y: 0.,
        width: 1360.,
        height: 860.,
    };
    native_viewport::apply_interface_viewport(app.world_mut(), canvas, 1.).unwrap();
    let mut retained = None;
    let mut cache_allocation = None;
    let mut previous_key = None;
    for (appearance_revision, theme) in [
        ResolvedTheme::Dark,
        ResolvedTheme::Light,
        ResolvedTheme::Dark,
    ]
    .into_iter()
    .enumerate()
    {
        let palette = *viewport_palette(theme);
        let ui_theme = ViewportUiTheme::from_palette(&palette);
        let world = app.world_mut();
        world.insert_resource(Appearance {
            palette,
            theme: ui_theme,
            revision: appearance_revision as u64 + 1,
        });
        native_viewport::apply_interface_palette(world, palette);
        interface_shell::refresh_theme(world, ui_theme);
        synchronize(world, camera, &services, &owner, &editor, canvas).unwrap();
        world.run_schedule(Update);
        let state = world.resource::<AnnotationState>();
        let entity = state
            .widgets
            .entity(&dimension_key)
            .expect("Visible projected dimension");
        let binding = world.get::<InterfaceControl>(entity).unwrap().binding;
        let allocation = state.sketch.as_ref().unwrap().entities.as_ptr() as usize;
        if let Some((prior_entity, prior_binding)) = &retained {
            assert_eq!(entity, *prior_entity);
            assert_eq!(binding, *prior_binding);
            assert_eq!(
                Some(allocation),
                cache_allocation,
                "Appearance must not query/replace the cached engine sketch"
            );
            assert_ne!(previous_key.as_ref(), Some(&state.view));
        }
        retained = Some((entity, binding));
        cache_allocation = Some(allocation);
        previous_key = Some(state.view.clone());
        let caption_color = world
            .get::<Children>(entity)
            .unwrap()
            .iter()
            .find_map(|child| world.get::<TextColor>(child))
            .unwrap()
            .0;
        assert_eq!(
            caption_color,
            Color::srgb(
                palette.dimension[0],
                palette.dimension[1],
                palette.dimension[2]
            )
        );
        assert_eq!(state.stamp, editor.stamp);
        assert_eq!(engine.geometry_revision(), revision);
        assert_eq!(engine.engine_call("project_export_model", ""), model);
    }
    // Opening a dimension makes other annotations passthrough, but the active
    // label must still catch the second click of a double-click. Changing the
    // active dimension must invalidate that routing even with the same camera.
    editor.interaction.dimension = Some("20 + 1".into());
    for (id, blocked) in [
        (Some(sketch.dimensions[0].constraint_id), false),
        (None, true),
        (Some(sketch.dimensions[0].constraint_id), false),
    ] {
        editor.interaction.dimension_id = id;
        let world = app.world_mut();
        synchronize(world, camera, &services, &owner, &editor, canvas).unwrap();
        let entity = world
            .resource::<AnnotationState>()
            .widgets
            .entity(&dimension_key)
            .unwrap();
        assert_eq!(
            world.get::<InterfaceControl>(entity).unwrap().disabled,
            blocked
        );
        assert_eq!(
            world
                .get::<interface_shell::InterfacePointerPassthrough>(entity)
                .is_some(),
            blocked
        );
        assert_eq!(
            Some(
                world
                    .resource::<AnnotationState>()
                    .sketch
                    .as_ref()
                    .unwrap()
                    .entities
                    .as_ptr() as usize
            ),
            cache_allocation
        );
        assert_eq!(engine.geometry_revision(), revision);
        assert_eq!(engine.engine_call("project_export_model", ""), model);
    }
}
