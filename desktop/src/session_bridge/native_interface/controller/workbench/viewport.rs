use super::*;
use crate::native_viewport::ui::{HudAxisLabel, HudAxisMark};
use crate::{app_preferences::locale as dictionary, native_viewport::localization};

#[derive(Component)]
struct DialTick;
#[derive(Component)]
struct DialCenter;

pub(super) fn synchronize(
    world: &mut World,
    camera: Entity,
    controls: &HashMap<String, Entity>,
    width: f32,
    height: f32,
    side: f32,
    state: &mut Workbench,
) -> Result<(), String> {
    let theme = crate::native_viewport::ui::theme(world);
    let locale = localization::locale(world);
    let assets = world.resource::<ViewportUiAssets>().clone();
    if let Some(bounds) = world
        .get_resource::<NavigationRectangle>()
        .and_then(|r| r.0)
    {
        card(
            (&mut state.widgets, world, camera),
            "zoom-window",
            rect(
                bounds.x as f32,
                bounds.y as f32,
                bounds.width as f32,
                bounds.height as f32,
            ),
            theme.accent.with_alpha(0.08),
            0.,
            40,
        );
        world
            .entity_mut(state.widgets.entity("zoom-window").unwrap())
            .insert(BorderColor::all(theme.accent));
    }
    if state.sketch {
        state.dial = None;
        if let Some(entity) = state.axes.take() {
            world.despawn(entity);
        }
        for entity in controls.values() {
            if matches!(
                world
                    .get::<NativeCommandBinding>(*entity)
                    .map(|b| &b.command),
                Some(NativeCommand::Orient(_) | NativeCommand::ClearSelection)
            ) {
                world.get_mut::<InterfaceControl>(*entity).unwrap().visible = false;
            }
        }
    } else {
        let x = width - 144.;
        let y = 132.;
        card(
            (&mut state.widgets, world, camera),
            "dial-card",
            rect(x, y, 132., 174.),
            theme.panel,
            12.,
            25,
        );
        let card_entity = state.widgets.entity("dial-card").unwrap();
        world
            .entity_mut(card_entity)
            .insert(bevy::ui::BoxShadow::new(
                theme.shadow,
                px(0),
                px(6),
                px(0),
                px(16),
            ));
        for (key, text, top, font) in [
            (
                "dial-title",
                dictionary::translate(locale, "orientationDial.label"),
                y + 7.,
                8.,
            ),
            (
                "dial-hint",
                dictionary::translate(locale, "orientationDial.orbit"),
                y + 154.,
                8.,
            ),
        ] {
            state.widgets.text(
                world,
                camera,
                key,
                rect(x + 4., top, 124., 14.),
                text,
                font,
                27,
            );
            world
                .entity_mut(state.widgets.entity(key).unwrap())
                .insert((TextLayout::justify(Justify::Center), TextColor(theme.mute)));
        }
        let orbit = rect(x + 28., y + 32., 76., 76.);
        state.dial = Some(InterfaceRect {
            x: (x + 28.) as f64,
            y: (y + 32.) as f64,
            width: 76.,
            height: 76.,
        });
        let axis_root = if let Some(entity) = state.axes {
            entity
        } else {
            let mut node = orbit.clone();
            node.border_radius = BorderRadius::MAX;
            node.border = UiRect::all(px(1.));
            let root = world
                .spawn((
                    node,
                    UiTargetCamera(camera),
                    BackgroundColor(theme.viewport),
                    BorderColor::all(theme.edge),
                    ZIndex(26),
                ))
                .id();
            for index in 0..20 {
                let angle = index as f32 * std::f32::consts::TAU / 20.;
                let mut node = rect(37. + angle.cos() * 31., 37. + angle.sin() * 31., 2., 2.);
                node.border_radius = BorderRadius::MAX;
                let e = world
                    .spawn((node, BackgroundColor(theme.edge), DialTick))
                    .id();
                world.entity_mut(root).add_child(e);
            }
            for (axis, label, color) in [
                (Vec3::X, "X", Color::srgb_u8(225, 91, 100)),
                (Vec3::Y, "Y", Color::srgb_u8(88, 173, 114)),
                (Vec3::Z, "Z", Color::srgb_u8(66, 165, 232)),
            ] {
                for index in 1..=12 {
                    let radius = if index == 12 { 2.5 } else { 1.15 };
                    let mut node = rect(0., 0., radius * 2., radius * 2.);
                    node.border_radius = BorderRadius::MAX;
                    let e = world
                        .spawn((
                            node,
                            HudAxisMark {
                                axis,
                                fraction: index as f32 / 12.,
                                radius,
                            },
                            BackgroundColor(color),
                        ))
                        .id();
                    world.entity_mut(root).add_child(e);
                }
                let e = world
                    .spawn((
                        HudAxisLabel { axis },
                        Node {
                            position_type: PositionType::Absolute,
                            ..default()
                        },
                        Text::new(label),
                        theme.text(&assets, 8., FontWeight::BOLD),
                        TextColor(color),
                    ))
                    .id();
                world.entity_mut(root).add_child(e);
            }
            let mut node = rect(35.5, 35.5, 5., 5.);
            node.border_radius = BorderRadius::MAX;
            let e = world
                .spawn((node, BackgroundColor(theme.ink), DialCenter))
                .id();
            world.entity_mut(root).add_child(e);
            world.entity_mut(root).insert(InterfaceControl::button(
                "document/session",
                dictionary::translate(locale, "navbar.orbit"),
            ));
            bind_command(
                world,
                root,
                NativeCommand::Workbench(Command::Navigation(NavigationTool::Orbit)),
            )?;
            state.axes = Some(root);
            root
        };
        if let Some(mut control) = world.get_mut::<InterfaceControl>(axis_root) {
            let orbit_label = dictionary::translate(locale, "navbar.orbit");
            if control.label != orbit_label {
                control.label = orbit_label.into();
            }
        }
        if world.get::<BackgroundColor>(axis_root) != Some(&BackgroundColor(theme.viewport)) {
            world
                .entity_mut(axis_root)
                .insert(BackgroundColor(theme.viewport));
        }
        let border = BorderColor::all(theme.edge);
        if world.get::<BorderColor>(axis_root) != Some(&border) {
            world.entity_mut(axis_root).insert(border);
        }
        for (mut color, center) in world.query_filtered::<(&mut BackgroundColor,Option<&DialCenter>),Or<(With<DialTick>,With<DialCenter>)>>().iter_mut(world) {
        let next=if center.is_some() {theme.ink} else {theme.edge};
        if color.0!=next {color.0=next;}
    }
        {
            let mut node = world.get_mut::<Node>(axis_root).unwrap();
            node.left = px(x + 28.);
            node.top = px(y + 32.);
        }
        for (key, label_key, caption, direction, left, top, w) in [
            (
                "front",
                "orientationDial.front",
                "F",
                ViewDirection::Front,
                52.,
                18.,
                28.,
            ),
            (
                "right",
                "orientationDial.right",
                "R",
                ViewDirection::Right,
                90.,
                60.,
                28.,
            ),
            (
                "back",
                "orientationDial.back",
                "B",
                ViewDirection::Back,
                52.,
                102.,
                28.,
            ),
            (
                "left",
                "orientationDial.left",
                "L",
                ViewDirection::Left,
                14.,
                60.,
                28.,
            ),
            (
                "top",
                "orientationDial.top",
                "+Z",
                ViewDirection::Top,
                8.,
                130.,
                36.,
            ),
            (
                "iso",
                "orientationDial.axonometric",
                "ISO",
                ViewDirection::Isometric,
                48.,
                130.,
                36.,
            ),
            (
                "bottom",
                "orientationDial.bottom",
                "−Z",
                ViewDirection::Bottom,
                88.,
                130.,
                36.,
            ),
        ] {
            let label = dictionary::translate(locale, label_key);
            let mut bounds = rect(x + left, y + top, w, 20.);
            bounds.border = UiRect::all(px(1.));
            bounds.border_radius = BorderRadius::all(px(if top < 130. { 10. } else { 4. }));
            let original = match key {
                "iso" => controls.get("isometric"),
                "front" | "top" => controls.get(key),
                _ => None,
            };
            let e = if let Some(&e) = original {
                bounds.justify_content = JustifyContent::Center;
                world
                    .entity_mut(e)
                    .remove::<interface_shell::InterfaceFlat>()
                    .insert((
                        bounds,
                        interface_shell::InterfaceCaption(caption.into()),
                        ZIndex(28),
                    ));
                world.get_mut::<InterfaceControl>(e).unwrap().visible = true;
                interface_shell::center_caption(world, e);
                e
            } else {
                centered_button(
                    (&mut state.widgets, world, camera),
                    (&format!("dial-{key}"), label, caption),
                    NativeCommand::Orient(direction),
                    bounds,
                    None,
                    false,
                    28,
                )?
            };
            interface_shell::caption_size(world, e, 9.);
        }
    }
    let nav_width = 336.;
    let nav_x = side + (width - side - nav_width) / 2.;
    let nav_y = navigation_top(height);
    card(
        (&mut state.widgets, world, camera),
        "navigation",
        rect(nav_x, nav_y, nav_width, 34.),
        theme.header,
        5.,
        25,
    );
    for (i, (key, icon)) in [("undo", Icon::Undo), ("redo", Icon::Redo)]
        .into_iter()
        .enumerate()
    {
        if let Some(&e) = controls.get(key) {
            let mut bounds = rect(nav_x + 6. + i as f32 * 26., nav_y + 5., 24., 24.);
            bounds.justify_content = JustifyContent::Center;
            world.entity_mut(e).insert((
                bounds,
                interface_shell::InterfaceFlat,
                interface_shell::InterfaceCaption(String::new()),
            ));
            {
                let tint = if world.get::<InterfaceControl>(e).unwrap().disabled {
                    theme.edge
                } else {
                    theme.mute
                };
                state.widgets.glyph(
                    (world, camera),
                    &format!("nav-{key}-glyph"),
                    rect(nav_x + 10. + i as f32 * 26., nav_y + 9., 16., 16.),
                    icon,
                    tint,
                    31,
                )
            };
        }
    }
    state.widgets.panel(
        world,
        camera,
        "nav-divider",
        rect(nav_x + 60., nav_y + 8., 1., 18.),
        theme.edge,
        26,
    );
    for (i, (id, label_key, tool, icon)) in [
        ("orbit", "navbar.orbit", NavigationTool::Orbit, Icon::Orbit),
        ("pan", "navbar.pan", NavigationTool::Pan, Icon::Pan),
        ("zoom", "navbar.zoom", NavigationTool::Zoom, Icon::Zoom),
        (
            "zoom-window",
            "navbar.zoomWindow",
            NavigationTool::ZoomWindow,
            Icon::ZoomWindow,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let left = nav_x + 66. + i as f32 * 28.;
        let label = dictionary::translate(locale, label_key);
        centered_button(
            (&mut state.widgets, world, camera),
            (&format!("nav-{id}"), label, ""),
            NativeCommand::Workbench(Command::Navigation(tool)),
            rect(left, nav_y + 5., 26., 24.),
            Some(state.navigation == tool),
            false,
            30,
        )?;
        state.widgets.glyph(
            (world, camera),
            &format!("nav-{id}-glyph"),
            rect(left + 5., nav_y + 9., 16., 16.),
            icon,
            if state.navigation == tool {
                theme.accent
            } else {
                theme.mute
            },
            31,
        );
    }
    if let Some(&e) = controls.get("fit") {
        world.entity_mut(e).insert((
            rect(nav_x + 178., nav_y + 5., 26., 24.),
            interface_shell::InterfaceFlat,
            interface_shell::InterfaceCaption(String::new()),
        ));
        state.widgets.glyph(
            (world, camera),
            "fit-glyph",
            rect(nav_x + 183., nav_y + 9., 16., 16.),
            Icon::Fit,
            theme.mute,
            31,
        );
    }
    for (i, (id, label_key, icon)) in [
        ("display", "navbar.displaySettings", Icon::Monitor),
        ("grid", "navbar.gridSettings", Icon::Grid),
    ]
    .into_iter()
    .enumerate()
    {
        let left = nav_x + 210. + i as f32 * 28.;
        let label = dictionary::translate(locale, label_key);
        centered_button(
            (&mut state.widgets, world, camera),
            (&format!("nav-{id}"), label, ""),
            NativeCommand::Workbench(Command::Menu("display".into())),
            rect(left, nav_y + 5., 26., 24.),
            None,
            true,
            30,
        )?;
        state.widgets.glyph(
            (world, camera),
            &format!("nav-{id}-glyph"),
            rect(left + 5., nav_y + 9., 16., 16.),
            icon,
            theme.edge,
            31,
        );
    }
    centered_button(
        (&mut state.widgets, world, camera),
        (
            "nav-select",
            dictionary::translate(locale, "ribbon.solid.select"),
            "",
        ),
        NativeCommand::Workbench(Command::Navigation(NavigationTool::Select)),
        rect(nav_x + 270., nav_y + 5., 26., 24.),
        Some(state.navigation == NavigationTool::Select),
        false,
        30,
    )?;
    state.widgets.glyph(
        (world, camera),
        "nav-select-glyph",
        rect(nav_x + 275., nav_y + 9., 16., 16.),
        Icon::Select,
        theme.mute,
        31,
    );
    let status = super::super::six_dof::status(world);
    let button = centered_button(
        (&mut state.widgets, world, camera),
        ("nav-3d-mouse", &status.message, ""),
        NativeCommand::SixDof(super::super::six_dof::command(&status)),
        rect(nav_x + 302., nav_y + 5., 26., 24.),
        Some(status.state == "connected"),
        super::super::six_dof::disabled(&status),
        30,
    )?;
    world
        .entity_mut(button)
        .insert(super::super::six_dof::ConnectionButton);
    state.widgets.glyph(
        (world, camera),
        "nav-3d-mouse-glyph",
        rect(nav_x + 307., nav_y + 9., 16., 16.),
        Icon::Gamepad,
        theme.mute,
        31,
    );
    let mut dot = rect(nav_x + 323., nav_y + 6., 5., 5.);
    dot.border_radius = BorderRadius::MAX;
    let color = super::super::six_dof::color(world, &status);
    state
        .widgets
        .panel(world, camera, "nav-3d-mouse-dot", dot, color, 32);
    if let Some(entity) = state.widgets.entity("nav-3d-mouse-dot") {
        world
            .entity_mut(entity)
            .insert(super::super::six_dof::ConnectionDot);
        world
            .entity_mut(entity)
            .remove::<interface_shell::InterfaceOccluder>();
    }
    Ok(())
}
