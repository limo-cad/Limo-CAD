//! CPU-only retained Settings controls. No OS window or live inbox is used.
use super::*;
use crate::app_preferences::navigation::{Action, Bindings};
use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
use bevy::ecs::system::RunSystemOnce;
use bevy::text::TextLayoutInfo;
use limo_cad_interface::ControlKey;

fn paint(app: &mut App, handle: &NativeInterfaceHandle, camera: Entity, services: &NativeServices) {
    for _ in 0..3 {
        synchronize(app.world_mut(), camera, services, 1400., 900.).unwrap();
        app.world_mut().run_schedule(PostUpdate);
    }
    let mut query = app
        .world_mut()
        .query_filtered::<Entity, With<InterfaceControl>>();
    let controls = query.iter(app.world()).collect::<Vec<_>>();
    for entity in controls {
        app.world_mut()
            .entity_mut(entity)
            .insert(InheritedVisibility::VISIBLE);
    }
    interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
}

fn activate(
    app: &mut App,
    handle: &NativeInterfaceHandle,
    camera: Entity,
    services: &NativeServices,
    key: &str,
    value: Option<&str>,
) -> Result<Value, String> {
    paint(app, handle, camera, services);
    let entity = app
        .world()
        .resource::<Settings>()
        .widgets
        .entity(key)
        .unwrap();
    let center = app
        .world()
        .get::<UiGlobalTransform>(entity)
        .unwrap()
        .translation
        .y;
    {
        let mut settings = app.world_mut().resource_mut::<Settings>();
        let body = settings.content.unwrap();
        settings.scroll = (settings.scroll + center - body.y as f32 - body.height as f32 * 0.5)
            .clamp(0., settings.scroll_max);
    }
    paint(app, handle, camera, services);
    let control = handle
        .read_surface(|_, frame| {
            frame
                .controls
                .iter()
                .find(|control| control.key == ControlKey(entity.to_bits()))
                .cloned()
        })?
        .expect("The scrolled Settings control must remain in the retained catalog");
    let body = app.world().resource::<Settings>().content.unwrap();
    assert!(
        control.visible && control.bounds.width > 0. && control.bounds.height > 0.,
        "{key} must be visible after scrolling: {control:?}"
    );
    assert!(
        control.bounds.x >= body.x - 1.
            && control.bounds.x + control.bounds.width <= body.x + body.width + 1.
            && control.bounds.y >= body.y - 1.
            && control.bounds.y + control.bounds.height <= body.y + body.height + 1.,
        "{key} must lie inside the actual Settings clip: {control:?}, body={body:?}"
    );
    if !control.disabled {
        assert_eq!(
            handle.hit_key([
                control.bounds.x + control.bounds.width / 2.,
                control.bounds.y + control.bounds.height / 2.
            ]),
            Some(control.key),
            "The scrolled control must own its actual hit bounds"
        );
    }
    let mut action = handle.resolve_retained(ControlKey(entity.to_bits()))?;
    if let Some(value) = value {
        action.control.input = ControlInput::SetValue(value.into());
    }
    let command = match &app
        .world()
        .get::<NativeCommandBinding>(entity)
        .unwrap()
        .command
    {
        NativeCommand::AppSettings(command) => *command,
        _ => panic!("Navigation control must dispatch through Settings"),
    };
    reduce(
        app.world_mut(),
        handle,
        &services.engine,
        &services.bridge,
        &action,
        command,
    )
}

#[test]
fn navigation_controls_stage_validate_apply_cancel_and_keep_failed_changes_unapplied() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let (_, handle, _, _) = interface_shell::tests::fixture();
    let owner = fixture.owner();
    let before = fixture.engine.engine_call("project_export_model", "");
    let mut frame = handle.frame().unwrap();
    frame.context = owner.clone();
    frame.client.width = 1400.;
    frame.client.height = 900.;
    frame.surface = frame.client;
    frame.surfaces.push(Surface {
        name: "document/appearance".into(),
        text: None,
    });
    handle.present(frame.clone()).unwrap();
    let store = Store::at(
        std::path::PathBuf::from(std::env::var_os("LIMO_CAD_SESSION_DIR").unwrap())
            .join("navigation-settings"),
    )
    .unwrap();
    let mut app = native_viewport::interface_scene_fixture();
    app.add_plugins((
        bevy::time::TimePlugin,
        bevy::text::TextPlugin,
        bevy::ui::UiPlugin,
    ))
    .init_resource::<Assets<Image>>()
    .init_resource::<Assets<bevy::image::TextureAtlasLayout>>()
    .init_resource::<bevy::input_focus::InputFocus>()
    .insert_resource(services.clone())
    .insert_resource(Settings::new(Ok(store.clone()), Locale::En));
    let fonts_before = app.world().resource::<Assets<Font>>().iter().count();
    app.world_mut()
        .run_system_once(ui::load_system_font)
        .unwrap();
    assert!(
        app.world().resource::<Assets<Font>>().iter().count() > fonts_before,
        "Measured wrapping requires an available real system UI font"
    );
    native_viewport::apply_interface_model(app.world_mut(), model_snapshot(&fixture.engine))
        .unwrap();
    refresh(app.world_mut(), true);
    let camera = app
        .world_mut()
        .spawn((
            Camera2d,
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: UVec2::new(1400, 900),
                        scale_factor: 1.,
                    }),
                    ..default()
                },
                viewport: Some(Viewport {
                    physical_size: UVec2::new(1400, 900),
                    ..default()
                }),
                ..default()
            },
        ))
        .id();
    files::initialize(
        app.world_mut(),
        Arc::new(Mutex::new(workspace::DocumentWorkspace::default())),
    );
    let mut opener = Widgets::default();
    let button = opener
        .button(
            app.world_mut(),
            camera,
            "settings-opener",
            InterfaceControl::button("document/appearance", "Settings"),
            None,
            NativeCommand::File(files::FileCommand::ShowSettings),
            rect(0., 0., 80., 28.),
            None,
            30,
        )
        .unwrap();
    app.world_mut().run_schedule(PostUpdate);
    app.world_mut()
        .entity_mut(button)
        .insert(InheritedVisibility::VISIBLE);
    interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
    let action = handle
        .resolve_retained(ControlKey(button.to_bits()))
        .unwrap();
    files::reduce(
        app.world_mut(),
        &handle,
        &fixture.engine,
        &fixture.bridge,
        &action,
        &files::FileCommand::ShowSettings,
    )
    .unwrap();
    frame.modal_stack = vec!["app-settings".into()];
    frame.surfaces.push(Surface {
        name: "app-settings".into(),
        text: None,
    });
    handle.present(frame.clone()).unwrap();
    let run = |app: &mut App, key, value| activate(app, &handle, camera, &services, key, value);
    run(&mut app, "nav-drag-Pan-button", Some("right")).unwrap();
    paint(&mut app, &handle, camera, &services);
    assert!(!store.path().exists());
    let apply = app
        .world()
        .resource::<Settings>()
        .widgets
        .entity("nav-apply")
        .unwrap();
    assert!(
        app.world().get::<InterfaceControl>(apply).unwrap().disabled,
        "The conflict must disable Apply rather than merely clip its bounds"
    );
    assert_eq!(
        app.world().resource::<Settings>().effective().navigation,
        Bindings::default()
    );
    assert!(
        run(&mut app, "nav-apply", None).is_err(),
        "A conflicting draft cannot activate Apply"
    );
    let diagnostic: Value = serde_json::from_str(&caption(app.world()).unwrap()).unwrap();
    assert!(diagnostic["navigation_conflicts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|error| error.as_str().unwrap().contains("both use")));
    assert!(!store.path().exists());
    paint(&mut app, &handle, camera, &services);
    let baseline_height = navigation::height(app.world(), app.world().resource::<Settings>());
    app.world_mut().resource_mut::<Settings>().detected = Locale::De;
    refresh(app.world_mut(), true);
    run(&mut app, "nav-drag-Orbit-button", Some("right")).unwrap();
    run(&mut app, "nav-drag-Zoom-button", Some("right")).unwrap();
    assert!(run(&mut app, "nav-apply", None).is_err());
    let status = app
        .world()
        .resource::<Settings>()
        .widgets
        .entity("nav-status")
        .unwrap();
    assert!(
        app.world().get::<ComputedNode>(status).unwrap().size.y > 40.,
        "Translated multi-conflict text must wrap into multiple lines"
    );
    assert!(
        !app.world()
            .get::<TextLayoutInfo>(status)
            .unwrap()
            .glyphs
            .is_empty(),
        "Reachability must measure actually shaped German conflict text"
    );
    assert!(
        navigation::height(app.world(), app.world().resource::<Settings>()) > baseline_height,
        "Long conflict text must expand the panel and scrolling range"
    );
    run(&mut app, "nav-drag-Pan-modifiers", Some("none")).unwrap();
    run(
        &mut app,
        "nav-drag-OrbitAlternative-button",
        Some("disabled"),
    )
    .unwrap();
    run(&mut app, "nav-drag-Zoom-button", Some("right")).unwrap();
    run(&mut app, "nav-drag-Zoom-modifiers", Some("ctrl")).unwrap();
    run(&mut app, "nav-scroll-Wheel", Some("pan")).unwrap();
    assert!(!store.path().exists());
    run(&mut app, "nav-apply", None).unwrap();
    let applied = store.read().unwrap().navigation.unwrap();
    assert_eq!(
        app.world().resource::<Settings>().effective().navigation,
        applied
    );
    assert_eq!(applied.wheel, Action::Pan);
    run(&mut app, "nav-defaults", None).unwrap();
    assert_eq!(store.read().unwrap().navigation, Some(applied));
    run(&mut app, "nav-cancel", None).unwrap();
    assert_eq!(
        app.world()
            .resource::<Settings>()
            .navigation
            .as_ref()
            .unwrap()
            .value,
        applied
    );
    run(&mut app, "nav-defaults", None).unwrap();
    run(&mut app, "nav-apply", None).unwrap();
    let restored_bytes = std::fs::read(store.path()).unwrap();
    std::fs::write(store.path(), b"broken owned test preferences").unwrap();
    run(&mut app, "nav-scroll-Wheel", Some("pan")).unwrap();
    assert!(run(&mut app, "nav-apply", None).is_err());
    assert_eq!(
        std::fs::read(store.path()).unwrap(),
        b"broken owned test preferences"
    );
    assert_eq!(
        app.world().resource::<Settings>().effective().navigation,
        Bindings::default()
    );
    std::fs::write(store.path(), restored_bytes).unwrap();
    run(&mut app, "nav-apply", None).unwrap();
    let live = store.read().unwrap().navigation.unwrap();
    run(&mut app, "nav-scroll-Wheel", Some("orbit")).unwrap();
    let external = Bindings {
        trackpad: Action::Zoom,
        ..live
    };
    store.patch_navigation(live, external).unwrap();
    refresh(app.world_mut(), true);
    let external_bytes = std::fs::read(store.path()).unwrap();
    assert!(run(&mut app, "nav-apply", None)
        .unwrap_err()
        .contains("another window"));
    assert_eq!(std::fs::read(store.path()).unwrap(), external_bytes);
    run(&mut app, "nav-cancel", None).unwrap();
    assert_eq!(
        app.world()
            .resource::<Settings>()
            .navigation
            .as_ref()
            .unwrap()
            .value,
        external
    );
    run(&mut app, "nav-scroll-Wheel", Some("orbit")).unwrap();
    files::close_settings(app.world_mut());
    assert!(app.world().resource::<Settings>().navigation.is_none());
    frame.modal_stack.clear();
    handle.present(frame).unwrap();
    synchronize(app.world_mut(), camera, &services, 1400., 900.).unwrap();
    interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
    let original_camera = native_viewport::interface_camera_snapshot(app.world()).1;
    let event = crate::native_viewport::winit_host::NativeHostInput {
        ui_scale: 1.,
        context: Some(owner),
        cursor: Some(Vec2::new(300., 300.)),
        modifiers: default(),
        event: WindowEvent::MouseWheel(bevy::input::mouse::MouseWheel {
            unit: bevy::input::mouse::MouseScrollUnit::Line,
            x: 0.,
            y: 2.,
            window: Entity::PLACEHOLDER,
            phase: bevy::input::touch::TouchPhase::Moved,
        }),
        consumed: false,
        actions: vec![],
    };
    assert!(view::navigate(app.world_mut(), &handle, &event).unwrap());
    let moved = native_viewport::interface_camera_snapshot(app.world()).1;
    assert_ne!(
        moved.target, original_camera.target,
        "Applied scroll binding must drive the actual camera router"
    );
    assert!(
        fixture.engine.engine_call("project_export_model", "") == before,
        "Navigation must preserve document geometry/model"
    );
    assert_eq!(store.read().unwrap().navigation, Some(external));
}
