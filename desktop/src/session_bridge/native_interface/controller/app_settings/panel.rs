use super::*;
use preferences::locale::{code, native_name, translate, SUPPORTED};

const ABOUT_MIN_HEIGHT: f32 = 94.;
const OPTIONS_HEIGHT: f32 = 634.;

pub(super) fn paint(
    world: &mut World,
    camera: Entity,
    services: &NativeServices,
    settings: &mut Settings,
    width: f32,
    height: f32,
) -> Result<(), String> {
    let effective = settings.effective();
    let locale = effective.locale;
    let t = |key| translate(locale, key);
    let theme = ui::theme(world);
    let w = 480_f32.min((width - 32.).max(240.));
    let h = 700_f32.min((height - 48.).max(220.));
    let x = (width - w) * 0.5;
    let y = (height - h) * 0.5;
    settings.widgets.backdrop(
        (world, camera),
        "settings-shade",
        "app-settings",
        NativeCommand::AppSettings(Command::Close),
        rect(0., 0., width, height),
        70,
    )?;
    if let Some(shade) = settings.widgets.entity("settings-shade") {
        world
            .entity_mut(shade)
            .insert(BackgroundColor(Color::BLACK.with_alpha(0.3)));
    }
    workbench::card(
        (&mut settings.widgets, world, camera),
        "settings-card",
        rect(x, y, w, h),
        theme.panel.with_alpha(1.),
        6.,
        72,
    );
    settings.widgets.panel(
        world,
        camera,
        "settings-header",
        rect(x + 1., y + 1., w - 2., 43.),
        theme.header.with_alpha(1.),
        73,
    );
    settings.widgets.text(
        world,
        camera,
        "settings-title",
        rect(x + 16., y + 10., w - 64., 24.),
        t("appearance.title"),
        14.,
        74,
    );
    button(
        world,
        camera,
        settings,
        "settings-close",
        t("appearance.close"),
        Some("\u{00d7}"),
        Command::Close,
        rect(x + w - 40., y + 8., 28., 28.),
        None,
        false,
    )?;
    let body = InterfaceRect {
        x: (x + 1.) as f64,
        y: (y + 44.) as f64,
        width: (w - 2.) as f64,
        height: (h - if settings.error.is_some() { 141. } else { 85. }) as f64,
    };
    settings.content = Some(body);
    let content_height = settings
        .widgets
        .entity("settings-content")
        .and_then(|entity| world.get::<ComputedNode>(entity))
        .map_or(0., |node| node.size.y * node.inverse_scale_factor)
        .max(ABOUT_MIN_HEIGHT + OPTIONS_HEIGHT);
    settings.scroll_max = (content_height - body.height as f32).max(0.);
    settings.scroll = settings.scroll.clamp(0., settings.scroll_max);
    settings.widgets.panel(
        world,
        camera,
        "settings-clip",
        rect(
            body.x as f32,
            body.y as f32,
            body.width as f32,
            body.height as f32,
        ),
        Color::NONE,
        73,
    );
    let clip = settings.widgets.entity("settings-clip").unwrap();
    settings.widgets.panel(
        world,
        camera,
        "settings-content",
        Node {
            height: Val::Auto,
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            ..rect(0., -settings.scroll, w - 2., 0.)
        },
        Color::NONE,
        74,
    );
    settings.widgets.parent(world, "settings-content", clip);
    let content = settings.widgets.entity("settings-content").unwrap();
    let inner = w - 34.;
    settings.widgets.panel(
        world,
        camera,
        "settings-about-content",
        Node {
            min_height: px(ABOUT_MIN_HEIGHT),
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.,
            padding: UiRect {
                left: px(16.),
                right: px(16.),
                top: px(12.),
                bottom: px(4.),
            },
            row_gap: px(4.),
            ..default()
        },
        Color::NONE,
        74,
    );
    settings
        .widgets
        .parent(world, "settings-about-content", content);
    let about = settings.widgets.entity("settings-about-content").unwrap();
    let build = limo_cad_build_info::build_info();
    let identity = format!(
        "{} \u{00b7} {} \u{00b7} {}{}",
        build.version,
        build.channel,
        build.revision,
        if build.modified { " (modified)" } else { "" }
    );
    for (key, value, size) in [
        ("settings-about", t("appearance.about"), 12.),
        ("settings-build", identity.as_str(), 11.),
        (
            "settings-build-hint",
            t("appearance.buildIdentityHint"),
            10.,
        ),
    ] {
        settings.widgets.text(
            world,
            camera,
            key,
            Node {
                width: percent(100.),
                min_width: px(0.),
                flex_shrink: 0.,
                ..default()
            },
            value,
            size,
            75,
        );
        settings.widgets.parent(world, key, about);
        let entity = settings.widgets.entity(key).unwrap();
        let layout = TextLayout::new(Justify::Left, bevy::text::LineBreak::WordOrCharacter);
        if world.get::<TextLayout>(entity).is_none_or(|current| {
            current.justify != layout.justify || current.linebreak != layout.linebreak
        }) {
            world.entity_mut(entity).insert(layout);
        }
    }
    settings.widgets.panel(
        world,
        camera,
        "settings-options-content",
        Node {
            height: px(OPTIONS_HEIGHT),
            flex_shrink: 0.,
            ..default()
        },
        Color::NONE,
        74,
    );
    settings
        .widgets
        .parent(world, "settings-options-content", content);
    let content = settings.widgets.entity("settings-options-content").unwrap();
    let text = |world: &mut World,
                settings: &mut Settings,
                key: &str,
                top: f32,
                height: f32,
                value: &str,
                size: f32| {
        settings.widgets.text(
            world,
            camera,
            key,
            rect(16., top - ABOUT_MIN_HEIGHT, inner, height),
            value,
            size,
            75,
        );
        settings.widgets.parent(world, key, content);
    };
    text(
        world,
        settings,
        "settings-theme",
        94.,
        18.,
        t("appearance.theme"),
        11.,
    );
    let cell = (inner - 16.) / 3.;
    for (index, (value, key)) in [
        (ThemePreference::System, "system"),
        (ThemePreference::Light, "light"),
        (ThemePreference::Dark, "dark"),
    ]
    .into_iter()
    .enumerate()
    {
        let id = format!("settings-theme-{key}");
        let label_key = format!("appearance.{key}");
        let description_key = format!("appearance.{key}Description");
        let left = 16. + index as f32 * (cell + 8.);
        let entity = button(
            world,
            camera,
            settings,
            &id,
            translate(locale, &label_key),
            None,
            Command::Theme(value),
            rect(left, 118. - ABOUT_MIN_HEIGHT, cell, 82.),
            Some(effective.theme == value),
            true,
        )?;
        settings.widgets.parent(world, &id, content);
        interface_shell::compact_label(world, entity, 12.);
        interface_shell::caption_node(world, entity, rect(10., 10., cell - 20., 22.));
        let description_id = format!("{id}-description");
        settings.widgets.text(
            world,
            camera,
            &description_id,
            rect(left + 10., 158. - ABOUT_MIN_HEIGHT, cell - 20., 38.),
            translate(locale, &description_key),
            10.,
            77,
        );
        settings.widgets.parent(world, &description_id, content);
    }
    let resolved = effective.theme.resolve(system_dark(world));
    let current = t("appearance.current").replace(
        "{theme}",
        t(if resolved == preferences::ResolvedTheme::Light {
            "appearance.light"
        } else {
            "appearance.dark"
        }),
    );
    text(
        world,
        settings,
        "settings-resolved",
        208.,
        20.,
        &current,
        10.,
    );
    text(
        world,
        settings,
        "settings-navigation",
        242.,
        18.,
        t("appearance.navigation"),
        11.,
    );
    text(
        world,
        settings,
        "settings-speed-label",
        266.,
        20.,
        t("appearance.sixDofSpeed"),
        12.,
    );
    settings.widgets.text(
        world,
        camera,
        "settings-speed-value",
        rect(w - 96., 266. - ABOUT_MIN_HEIGHT, 62., 20.),
        &format!("{}%", (effective.six_dof_speed * 100.).round()),
        11.,
        75,
    );
    settings
        .widgets
        .parent(world, "settings-speed-value", content);
    let mut speed = InterfaceControl::button("document/appearance", t("appearance.sixDofSpeed"));
    speed.modal_scope = Some("app-settings".into());
    speed.field = Field::Range {
        value: effective.six_dof_speed,
        min: preferences::MIN_SIX_DOF_SPEED,
        max: preferences::MAX_SIX_DOF_SPEED,
        step: 0.05,
    };
    settings.widgets.button(
        world,
        camera,
        "settings-speed",
        speed,
        None,
        NativeCommand::AppSettings(Command::Speed),
        rect(16., 292. - ABOUT_MIN_HEIGHT, inner, 26.),
        None,
        76,
    )?;
    settings.widgets.parent(world, "settings-speed", content);
    settings.widgets.text(
        world,
        camera,
        "settings-speed-hint",
        rect(16., 324. - ABOUT_MIN_HEIGHT, inner - 82., 40.),
        t("appearance.sixDofSpeedDescription"),
        10.,
        75,
    );
    settings
        .widgets
        .parent(world, "settings-speed-hint", content);
    button(
        world,
        camera,
        settings,
        "settings-speed-reset",
        t("appearance.reset"),
        None,
        Command::ResetSpeed,
        rect(w - 98., 326. - ABOUT_MIN_HEIGHT, 64., 26.),
        None,
        false,
    )?;
    settings
        .widgets
        .parent(world, "settings-speed-reset", content);
    text(
        world,
        settings,
        "settings-language",
        382.,
        18.,
        t("appearance.language"),
        11.,
    );
    let cell = (inner - 18.) / 4.;
    for (index, value) in SUPPORTED.into_iter().enumerate() {
        let key = format!("settings-locale-{}", code(value));
        button(
            world,
            camera,
            settings,
            &key,
            native_name(value),
            None,
            Command::Language(value),
            rect(
                16. + index as f32 * (cell + 6.),
                408. - ABOUT_MIN_HEIGHT,
                cell,
                32.,
            ),
            Some(effective.locale == value),
            true,
        )?;
        settings.widgets.parent(world, &key, content);
    }
    text(
        world,
        settings,
        "settings-language-hint",
        446.,
        34.,
        t("appearance.languageHint"),
        10.,
    );
    text(
        world,
        settings,
        "settings-interface-size",
        488.,
        22.,
        t("appearance.uiScale"),
        12.,
    );
    let cell = (inner - 30.) / preferences::UI_SCALE_OPTIONS.len() as f32;
    for (index, scale) in preferences::UI_SCALE_OPTIONS.into_iter().enumerate() {
        let key = format!("settings-interface-size-{index}");
        button(
            world,
            camera,
            settings,
            &key,
            &format!("{}%", (scale * 100.).round() as u32),
            None,
            Command::InterfaceSize(index as u8),
            rect(
                16. + index as f32 * (cell + 6.),
                514. - ABOUT_MIN_HEIGHT,
                cell,
                32.,
            ),
            Some(effective.ui_scale == scale),
            true,
        )?;
        settings.widgets.parent(world, &key, content);
    }
    text(
        world,
        settings,
        "settings-interface-size-hint",
        552.,
        34.,
        t("appearance.uiScaleDescription"),
        10.,
    );
    let key = "settings-gpu-stock";
    button(
        world,
        camera,
        settings,
        key,
        t("appearance.gpuStockRemoval"),
        None,
        Command::GpuStock(!effective.gpu_stock_removal),
        rect(16., 600. - ABOUT_MIN_HEIGHT, inner, 32.),
        Some(effective.gpu_stock_removal),
        false,
    )?;
    settings.widgets.parent(world, key, content);
    text(
        world,
        settings,
        "settings-gpu-stock-hint",
        638.,
        34.,
        t("appearance.gpuStockRemovalDescription"),
        10.,
    );
    let units = match services.engine.document_units() {
        limo_cad_core::UnitSystem::Mm => "mm",
        limo_cad_core::UnitSystem::Cm => "cm",
        limo_cad_core::UnitSystem::In => "in",
    };
    text(
        world,
        settings,
        "settings-units",
        680.,
        22.,
        &t("appearance.documentUnits").replace("{unit}", units),
        11.,
    );
    if settings.error.is_some() {
        settings.widgets.panel(
            world,
            camera,
            "settings-error-footer",
            rect(x + 1., y + h - 97., w - 2., 56.),
            theme.header.with_alpha(1.),
            76,
        );
        settings.widgets.text(
            world,
            camera,
            "settings-error",
            rect(x + 12., y + h - 91., w - 150., 44.),
            t("appearance.preferenceError"),
            10.,
            77,
        );
        button(
            world,
            camera,
            settings,
            "settings-retry",
            t("appearance.retryPreferences"),
            None,
            Command::Retry,
            rect(x + w - 130., y + h - 86., 116., 32.),
            None,
            false,
        )?;
    }
    settings.widgets.panel(
        world,
        camera,
        "settings-scroll-footer",
        rect(x + 1., y + h - 41., w - 2., 40.),
        theme.header.with_alpha(1.),
        76,
    );
    settings.widgets.text(
        world,
        camera,
        "settings-scroll-hint",
        rect(x + 12., y + h - 36., w - 100., 30.),
        t("appearance.scrollHint"),
        10.,
        77,
    );
    for (key, label, caption, direction, left, disabled) in [
        (
            "settings-scroll-up",
            "appearance.scrollUp",
            "\u{2191}",
            -1,
            w - 78.,
            settings.scroll <= 0.,
        ),
        (
            "settings-scroll-down",
            "appearance.scrollDown",
            "\u{2193}",
            1,
            w - 42.,
            settings.scroll >= settings.scroll_max,
        ),
    ] {
        let mut control = InterfaceControl::button("document/appearance", t(label));
        control.modal_scope = Some("app-settings".into());
        control.disabled = disabled;
        let entity = settings.widgets.button(
            world,
            camera,
            key,
            control,
            Some(caption),
            NativeCommand::AppSettings(Command::Scroll(direction)),
            rect(x + left, y + h - 36., 30., 30.),
            None,
            78,
        )?;
        interface_shell::center_caption(world, entity);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn button(
    world: &mut World,
    camera: Entity,
    settings: &mut Settings,
    key: &str,
    label: &str,
    caption: Option<&str>,
    command: Command,
    bounds: Node,
    selected: Option<bool>,
    radio: bool,
) -> Result<Entity, String> {
    let mut control = InterfaceControl::button("document/appearance", label);
    control.modal_scope = Some("app-settings".into());
    control.selected = selected;
    if radio {
        control.role = "radio".into();
    }
    let entity = settings.widgets.button(
        world,
        camera,
        key,
        control,
        caption,
        NativeCommand::AppSettings(command),
        bounds,
        None,
        if command == Command::Retry { 78 } else { 76 },
    )?;
    interface_shell::center_caption(world, entity);
    interface_shell::caption_size(world, entity, 11.);
    Ok(entity)
}
