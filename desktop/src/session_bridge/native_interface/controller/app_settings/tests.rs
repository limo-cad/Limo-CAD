use super::*;
use crate::session_bridge::{native_interface::tests::Fixture, parse_engine_envelope};
use bevy::text::{EditableText, TextCursorStyle};
use bevy::ui::ComputedStackIndex;

#[test]
fn settings_build_identity_wraps_without_overlapping_description_or_controls() {
    use bevy::camera::{ComputedCameraValues, RenderTargetInfo, Viewport};
    use bevy::text::TextLayoutInfo;

    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        bevy::asset::AssetPlugin::default(),
        bevy::text::TextPlugin,
        bevy::ui::UiPlugin,
    ))
    .init_resource::<Assets<Image>>()
    .init_resource::<Assets<bevy::image::TextureAtlasLayout>>()
    .init_resource::<ViewportUiAssets>()
    .init_resource::<bevy::input_focus::InputFocus>();
    let camera = app
        .world_mut()
        .spawn((
            Camera2d,
            Camera {
                computed: ComputedCameraValues {
                    target_info: Some(RenderTargetInfo {
                        physical_size: UVec2::new(1600, 1200),
                        scale_factor: 1.,
                    }),
                    ..default()
                },
                viewport: Some(Viewport {
                    physical_size: UVec2::new(1600, 1200),
                    ..default()
                }),
                ..default()
            },
        ))
        .id();
    let store = Store::at(
        std::path::PathBuf::from(std::env::var_os("LIMO_CAD_SESSION_DIR").unwrap())
            .join("settings-build-layout"),
    )
    .unwrap();
    let mut settings = Settings::new(Ok(store), Locale::En);
    let identity = format!(
        "0.2.2 · bevy-preview-{} · {} (modified)",
        "20261005.experimental.".repeat(8),
        "0123456789abcdef0123456789abcdef01234567"
    );
    for (width, height, scale, locale) in [
        (800., 860., 1., Locale::En),
        (272., 360., 1., Locale::De),
        (272., 360., 1.5, Locale::Es),
        (800., 860., 2., Locale::En),
    ] {
        settings.detected = locale;
        app.world_mut().resource_mut::<UiScale>().0 = scale;
        for _ in 0..2 {
            panel::paint(
                app.world_mut(),
                camera,
                &services,
                &mut settings,
                width,
                height,
            )
            .unwrap();
            let build = settings.widgets.entity("settings-build").unwrap();
            app.world_mut()
                .entity_mut(build)
                .insert(Text::new(identity.clone()));
            app.world_mut().run_schedule(PostUpdate);
        }
        let world = app.world();
        let bounds = |key: &str| {
            let entity = settings.widgets.entity(key).unwrap();
            let node = world.get::<ComputedNode>(entity).unwrap();
            let center = world.get::<UiGlobalTransform>(entity).unwrap().translation;
            (center.y - node.size.y / 2., center.y + node.size.y / 2.)
        };
        let build = settings.widgets.entity("settings-build").unwrap();
        let node = world.get::<ComputedNode>(build).unwrap();
        let text = world.get::<TextLayoutInfo>(build).unwrap();
        assert!(
            !text.glyphs.is_empty(),
            "The regression must measure actual shaped text"
        );
        assert!(
            text.size.y * node.inverse_scale_factor > 18.,
            "The build must wrap"
        );
        let drawn_right = text
            .glyphs
            .iter()
            .filter(|glyph| glyph.atlas_info.rect.width() > 0.)
            .map(|glyph| glyph.position.x + glyph.atlas_info.rect.width() / 2.)
            .fold(0., f32::max);
        assert!(
            drawn_right <= node.size.x + 1.,
            "width={width}, scale={scale}, drawn_right={drawn_right}, node={:?}",
            node.size
        );
        assert!(
            text.size.y <= node.size.y + 1.,
            "width={width}, scale={scale}, text={:?}, node={:?}",
            text.size,
            node.size
        );
        assert!(bounds("settings-build").1 <= bounds("settings-build-hint").0);
        assert!(bounds("settings-build-hint").1 <= bounds("settings-theme").0);
        let content = world
            .get::<ComputedNode>(settings.widgets.entity("settings-content").unwrap())
            .unwrap();
        let content_height = content.size.y * content.inverse_scale_factor;
        assert!(content_height > 654.);
        assert!(
            (settings.scroll_max - (content_height - settings.content.unwrap().height as f32))
                .abs()
                < 1.
        );

        settings.scroll = settings.scroll_max;
        panel::paint(
            app.world_mut(),
            camera,
            &services,
            &mut settings,
            width,
            height,
        )
        .unwrap();
        app.world_mut()
            .entity_mut(build)
            .insert(Text::new(identity.clone()));
        app.world_mut().run_schedule(PostUpdate);
        let world = app.world();
        let units = settings.widgets.entity("settings-units").unwrap();
        let clip = settings.widgets.entity("settings-clip").unwrap();
        let bottom = |entity| {
            world
                .get::<UiGlobalTransform>(entity)
                .unwrap()
                .translation
                .y
                + world.get::<ComputedNode>(entity).unwrap().size.y / 2.
        };
        assert!(
            bottom(units) <= bottom(clip),
            "The final setting must remain reachable"
        );
        let top = |entity| {
            world
                .get::<UiGlobalTransform>(entity)
                .unwrap()
                .translation
                .y
                - world.get::<ComputedNode>(entity).unwrap().size.y / 2.
        };
        assert!(top(units) >= top(clip));
        settings.scroll = 0.;
    }
}

#[test]
fn interface_size_shortcuts_persist_and_respect_document_editor_and_modal_ownership() {
    use bevy::input::{
        keyboard::{Key, KeyCode, KeyboardInput},
        ButtonState,
    };
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let mut app = native_viewport::interface_scene_fixture();
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let handle = NativeInterfaceHandle::new(|| {});
    app.init_resource::<ViewportUiAssets>();
    let root = std::path::PathBuf::from(std::env::var_os("LIMO_CAD_SESSION_DIR").unwrap());
    let store = Store::at(root.join("interface-size-shortcuts")).unwrap();
    app.insert_resource(Settings::new(Ok(store.clone()), Locale::En));
    let bounds = InterfaceRect {
        x: 0.,
        y: 0.,
        width: 800.,
        height: 600.,
    };
    let mut frame = InterfaceFrame {
        context: fixture.owner(),
        client: bounds,
        surface: bounds,
        canvases: vec![Canvas {
            name: "viewport".into(),
            bounds,
        }],
        surfaces: vec![Surface {
            name: "Viewport".into(),
            text: None,
        }],
        modal_stack: vec![],
        document_visible: true,
    };
    handle.present(frame.clone()).unwrap();
    interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
    let window = app.world_mut().spawn_empty().id();
    let mut event = NativeHostInput {
        ui_scale: 1.,
        context: Some(fixture.owner()),
        cursor: None,
        modifiers: default(),
        event: WindowEvent::KeyboardInput(KeyboardInput {
            key_code: KeyCode::Equal,
            logical_key: Key::Character("=".into()),
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        }),
        consumed: false,
        actions: vec![],
    };
    event.modifiers.ctrl = true;
    let model = fixture.engine.engine_call("project_export_model", "");
    assert!(shortcut(app.world_mut(), &handle, &services, &event)
        .unwrap()
        .is_some());
    assert_eq!(store.read().unwrap().ui_scale, Some(1.1));
    event.modifiers.ctrl = false;
    event.modifiers.meta = true;
    assert!(shortcut(app.world_mut(), &handle, &services, &event)
        .unwrap()
        .is_some());
    assert_eq!(store.read().unwrap().ui_scale, Some(1.25));
    let WindowEvent::KeyboardInput(key) = &mut event.event else {
        unreachable!()
    };
    key.key_code = KeyCode::Digit0;
    key.logical_key = Key::Character("0".into());
    assert!(shortcut(app.world_mut(), &handle, &services, &event)
        .unwrap()
        .is_some());
    assert_eq!(store.read().unwrap().ui_scale, Some(1.));
    let WindowEvent::KeyboardInput(key) = &mut event.event else {
        unreachable!()
    };
    key.key_code = KeyCode::Equal;
    key.logical_key = Key::Character("+".into());
    event.context.as_mut().unwrap().epoch += 1;
    assert!(shortcut(app.world_mut(), &handle, &services, &event)
        .unwrap()
        .is_none());
    event.context = Some(fixture.owner());
    event.consumed = true;
    assert!(shortcut(app.world_mut(), &handle, &services, &event)
        .unwrap()
        .is_none());
    event.consumed = false;
    frame.modal_stack.push("app-settings".into());
    frame.surfaces.push(Surface {
        name: "app-settings".into(),
        text: None,
    });
    handle.present(frame.clone()).unwrap();
    interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
    assert!(shortcut(app.world_mut(), &handle, &services, &event)
        .unwrap()
        .is_none());
    frame.modal_stack.clear();
    handle.present(frame).unwrap();
    let mut editor = InterfaceControl::button("Viewport", "Draft text");
    editor.field = Field::Text {
        value: "uncommitted".into(),
        read_only: false,
        selection: None,
    };
    editor.text_editing = true;
    app.world_mut().spawn((
        editor,
        ComputedNode {
            size: Vec2::new(80., 24.),
            inverse_scale_factor: 1.,
            ..default()
        },
        UiGlobalTransform::from_translation(Vec2::new(60., 42.)),
        ComputedStackIndex(1),
        InheritedVisibility::VISIBLE,
    ));
    interface_shell::tests::publish_layout_once(app.world_mut(), handle.clone());
    assert!(handle.focus_next(false).unwrap());
    assert!(shortcut(app.world_mut(), &handle, &services, &event)
        .unwrap()
        .is_none());
    assert_eq!(store.read().unwrap().ui_scale, Some(1.));
    assert_eq!(
        fixture.engine.engine_call("project_export_model", ""),
        model
    );
}

#[test]
fn shared_theme_and_locale_repaint_retained_edits_without_document_history() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let model =
        || parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap();
    let before = model();
    let history = fixture
        .bridge
        .native_history_available(&fixture.engine, &fixture.owner())
        .unwrap();
    let root = std::path::PathBuf::from(std::env::var_os("LIMO_CAD_SESSION_DIR").unwrap())
        .join("appearance-test");
    let store = Store::at(root).unwrap();
    store
        .patch(Preferences {
            theme: Some(ThemePreference::Dark),
            ..default()
        })
        .unwrap();
    let mut app = native_viewport::interface_scene_fixture();
    app.init_resource::<ViewportUiAssets>();
    app.insert_resource(Settings::new(Ok(store.clone()), Locale::De));
    refresh(app.world_mut(), true);
    apply_scale(app.world_mut());
    assert!(
        native_viewport::interface_view(app.world())
            .2
            .cam_gpu_stock_removal
    );
    let camera = app.world_mut().spawn_empty().id();
    let mut widgets = Widgets::default();
    let mut control = InterfaceControl::button("test", "Draft name");
    control.field = Field::Text {
        value: "saved name".into(),
        read_only: false,
        selection: None,
    };
    let entity = widgets
        .button(
            app.world_mut(),
            camera,
            "name",
            control,
            None,
            NativeCommand::ClearSelection,
            rect(0., 0., 200., 28.),
            None,
            35,
        )
        .unwrap();
    app.world_mut()
        .get_mut::<EditableText>(entity)
        .unwrap()
        .editor
        .set_text("uncommitted \u{96f6}\u{4ef6}");
    let binding = app.world().get::<InterfaceControl>(entity).unwrap().clone();
    let first_revision = ui::appearance_revision(app.world());
    store
        .patch(Preferences {
            theme: Some(ThemePreference::Light),
            locale: Some(Locale::ZhCn),
            ui_scale: Some(1.5),
            gpu_stock_removal: Some(false),
            ..default()
        })
        .unwrap();
    refresh(app.world_mut(), true);
    let theme = ui::theme(app.world());
    assert!(
        !native_viewport::interface_view(app.world())
            .2
            .cam_gpu_stock_removal
    );
    assert_eq!(localization::locale(app.world()), Locale::ZhCn);
    assert_eq!(app.world().resource::<bevy::ui::UiScale>().0, 1.);
    apply_scale(app.world_mut());
    assert_eq!(app.world().resource::<bevy::ui::UiScale>().0, 1.5);
    assert!(ui::appearance_revision(app.world()) > first_revision);
    assert_eq!(
        app.world().get::<EditableText>(entity).unwrap().value(),
        "uncommitted \u{96f6}\u{4ef6}"
    );
    assert_eq!(
        app.world().get::<InterfaceControl>(entity).unwrap(),
        &binding
    );
    assert_eq!(app.world().get::<TextColor>(entity).unwrap().0, theme.ink);
    assert_eq!(
        app.world().get::<TextCursorStyle>(entity).unwrap().color,
        theme.ink
    );
    assert_eq!(
        app.world().get::<BackgroundColor>(entity).unwrap().0,
        theme.panel
    );
    assert_eq!(model(), before);
    assert_eq!(
        fixture
            .bridge
            .native_history_available(&fixture.engine, &fixture.owner())
            .unwrap(),
        history
    );

    let palette = ui::palette(app.world());
    let revision = ui::appearance_revision(app.world());
    std::fs::write(store.path(), b"invalid preferences").unwrap();
    refresh(app.world_mut(), true);
    assert!(app.world().resource::<Settings>().error.is_some());
    assert_eq!(ui::palette(app.world()), palette);
    assert_eq!(ui::appearance_revision(app.world()), revision);
    assert_eq!(localization::locale(app.world()), Locale::ZhCn);
    assert_eq!(std::fs::read(store.path()).unwrap(), b"invalid preferences");
}

#[test]
fn a_watcher_notification_bypasses_the_recent_ui_read_without_an_extra_input_frame() {
    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let _fixture = Fixture::new();
    let root = std::path::PathBuf::from(std::env::var_os("LIMO_CAD_SESSION_DIR").unwrap())
        .join("appearance-wake-test");
    let store = Store::at(root).unwrap();
    store
        .patch(Preferences {
            theme: Some(ThemePreference::Dark),
            ..default()
        })
        .unwrap();
    let mut app = native_viewport::interface_scene_fixture();
    app.insert_resource(Settings::new(Ok(store.clone()), Locale::En));
    let wake = install(app.world_mut());
    refresh(app.world_mut(), true);
    std::fs::write(
        store.path(),
        br#"{"schema_version":1,"theme":"light","locale":"es"}"#,
    )
    .unwrap();
    assert_eq!(
        app.world().resource::<Settings>().effective().theme,
        ThemePreference::Dark
    );
    wake.changed();
    refresh(app.world_mut(), false);
    assert_eq!(
        app.world().resource::<Settings>().effective().theme,
        ThemePreference::Light
    );
    assert_eq!(localization::locale(app.world()), Locale::Es);
}

#[test]
fn absent_preferences_keep_os_language_and_do_not_persist_detected_defaults() {
    let root = std::env::temp_dir().join(format!("native-settings-{}", uuid::Uuid::new_v4()));
    let store = Store::at(root).unwrap();
    let mut settings = Settings::new(Ok(store.clone()), Locale::Es);
    settings.poll(true);
    assert_eq!(settings.effective().locale, Locale::Es);
    assert_eq!(settings.effective().theme, ThemePreference::System);
    assert_eq!(settings.effective().six_dof_speed, 1.5);
    assert!(!store.path().exists());
}

#[test]
fn failed_save_keeps_live_choice_and_error_until_retry_merges_fresh_other_fields() {
    let root = std::env::temp_dir().join(format!("native-settings-retry-{}", uuid::Uuid::new_v4()));
    let store = Store::at(root.clone()).unwrap();
    store
        .patch(Preferences {
            theme: Some(ThemePreference::Light),
            locale: Some(Locale::En),
            ..default()
        })
        .unwrap();
    let mut settings = Settings::new(Ok(store.clone()), Locale::De);
    settings.poll(true);
    std::fs::write(store.path(), b"broken external settings").unwrap();
    assert!(settings
        .patch(Preferences {
            theme: Some(ThemePreference::Dark),
            ..default()
        })
        .is_err());
    assert_eq!(settings.effective().theme, ThemePreference::Dark);
    settings.poll(true);
    assert!(settings.error.is_some());
    assert_eq!(settings.effective().theme, ThemePreference::Dark);
    assert_eq!(
        std::fs::read(store.path()).unwrap(),
        b"broken external settings"
    );
    std::fs::write(
        store.path(),
        br#"{"schema_version":1,"theme":"light","locale":"es","six_dof_speed":2.25}"#,
    )
    .unwrap();
    settings.poll(true);
    assert_eq!(settings.effective().theme, ThemePreference::Dark);
    assert_eq!(settings.effective().locale, Locale::Es);
    assert!(
        settings.error.is_some(),
        "A successful poll does not acknowledge a failed save"
    );
    settings.patch(Preferences::default()).unwrap();
    assert!(settings.error.is_none());
    let saved = store.read().unwrap();
    assert_eq!(saved.theme, Some(ThemePreference::Dark));
    assert_eq!(saved.locale, Some(Locale::Es));
    assert_eq!(saved.six_dof_speed, Some(2.25));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn retry_is_reachable_by_the_shell_pointer_above_the_retained_error_footer() {
    use crate::native_viewport::interface_shell::{InterfaceOccluder, PointerButton, PointerPhase};
    use bevy::ui::{ComputedStackIndex, UiGlobalTransform};
    use limo_cad_interface::ControlKey;

    fn publish(world: &mut World, handle: &NativeInterfaceHandle) {
        let px_value = |value: Val| match value {
            Val::Px(value) => value,
            other => panic!("Expected Settings pixel bounds, got {other:?}"),
        };
        let layout: Vec<_> = world
            .query_filtered::<(Entity, &Node, &ZIndex), (
                Without<ChildOf>,
                Or<(With<InterfaceControl>, With<InterfaceOccluder>)>,
            )>()
            .iter(world)
            .map(|(entity, node, z)| {
                let size = Vec2::new(px_value(node.width), px_value(node.height));
                (
                    entity,
                    size,
                    Vec2::new(px_value(node.left), px_value(node.top)) + size / 2.,
                    z.0,
                )
            })
            .collect();
        for (entity, size, center, z) in layout {
            world.entity_mut(entity).insert((
                ComputedNode {
                    size,
                    inverse_scale_factor: 1.,
                    ..default()
                },
                UiGlobalTransform::from_translation(center),
                ComputedStackIndex(z as u32),
                InheritedVisibility::VISIBLE,
            ));
        }
        interface_shell::tests::publish_layout_once(world, handle.clone());
    }

    let _lock = crate::session_bridge::tests::TEST_LOCK.lock().unwrap();
    let fixture = Fixture::new();
    let owner = fixture.owner();
    let before =
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap();
    let store = Store::at(
        std::path::PathBuf::from(std::env::var_os("LIMO_CAD_SESSION_DIR").unwrap())
            .join("appearance-pointer-retry"),
    )
    .unwrap();
    store
        .patch(Preferences {
            theme: Some(ThemePreference::Light),
            locale: Some(Locale::De),
            ..default()
        })
        .unwrap();
    let mut settings = Settings::new(Ok(store.clone()), Locale::En);
    settings.poll(true);
    std::fs::write(store.path(), b"broken external settings").unwrap();
    assert!(settings
        .patch(Preferences {
            theme: Some(ThemePreference::Dark),
            ..default()
        })
        .is_err());
    let services = NativeServices {
        engine: fixture.engine.clone(),
        bridge: fixture.bridge.clone(),
    };
    let handle = NativeInterfaceHandle::new(|| {});
    let mut app = native_viewport::interface_scene_fixture();
    app.init_resource::<ViewportUiAssets>()
        .insert_resource(services.clone())
        .insert_resource(settings);
    files::initialize(
        app.world_mut(),
        Arc::new(Mutex::new(workspace::DocumentWorkspace::default())),
    );
    let camera = app.world_mut().spawn(InterfaceCamera).id();
    let mut opener_widgets = Widgets::default();
    let opener = opener_widgets
        .button(
            app.world_mut(),
            camera,
            "settings-opener",
            InterfaceControl::button("document/session", "Settings"),
            None,
            NativeCommand::File(files::FileCommand::ShowSettings),
            rect(0., 0., 80., 28.),
            None,
            30,
        )
        .unwrap();
    let bounds = InterfaceRect {
        x: 0.,
        y: 0.,
        width: 1360.,
        height: 860.,
    };
    let mut frame = InterfaceFrame {
        context: owner.clone(),
        client: bounds,
        surface: bounds,
        canvases: vec![],
        surfaces: vec![Surface {
            name: "document/appearance".into(),
            text: None,
        }],
        modal_stack: vec![],
        document_visible: true,
    };
    handle.present(frame.clone()).unwrap();
    publish(app.world_mut(), &handle);
    let action = handle
        .resolve_retained(ControlKey(opener.to_bits()))
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
    assert!(files::settings_open(app.world()));
    frame.modal_stack = vec!["app-settings".into()];
    frame.surfaces.push(Surface {
        name: "app-settings".into(),
        text: None,
    });
    handle.present(frame).unwrap();

    let mut retained_retry = None;
    for _ in 0..3 {
        synchronize(app.world_mut(), camera, &services, 1360., 860.).unwrap();
        let retry = app
            .world()
            .resource::<Settings>()
            .widgets
            .entity("settings-retry")
            .unwrap();
        if let Some(previous) = retained_retry {
            assert_eq!(retry, previous);
        }
        retained_retry = Some(retry);
        publish(app.world_mut(), &handle);
    }
    let retry = retained_retry.unwrap();
    let center = app
        .world()
        .get::<UiGlobalTransform>(retry)
        .unwrap()
        .translation;
    std::fs::write(
        store.path(),
        br#"{"schema_version":1,"theme":"light","locale":"de","six_dof_speed":2.25}"#,
    )
    .unwrap();
    for phase in [PointerPhase::Down, PointerPhase::Up] {
        assert!(handle
            .pointer(
                phase,
                [center.x as f64, center.y as f64],
                PointerButton::Primary
            )
            .unwrap());
    }
    let actions = handle.take_actions().unwrap();
    assert_eq!(
        actions.len(),
        1,
        "The footer must not swallow Retry or dispatch Close"
    );
    assert_eq!(actions[0].control.key, ControlKey(retry.to_bits()));
    let outcome = super::super::reduce_control_input(
        &fixture.engine,
        &fixture.bridge,
        app.world_mut(),
        &handle,
        &actions[0],
    )
    .unwrap();
    assert_eq!(outcome["persisted"], true);
    assert!(app.world().resource::<Settings>().error.is_none());
    assert_eq!(
        app.world().resource::<Settings>().pending,
        Preferences::default()
    );
    assert_eq!(store.read().unwrap().theme, Some(ThemePreference::Dark));
    assert_eq!(store.read().unwrap().six_dof_speed, Some(2.25));
    assert_eq!(
        parse_engine_envelope(fixture.engine.engine_call("project_export_model", "")).unwrap(),
        before
    );
}
